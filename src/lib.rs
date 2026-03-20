#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::{
    borrow::Borrow,
    ffi::{CStr, CString, c_char, c_int, c_uchar, c_void},
    iter::FusedIterator,
    marker::PhantomData,
    ptr::NonNull,
};

use libpressio_sys::{
    pressio_thread_safety_pressio_thread_safety_multiple,
    pressio_thread_safety_pressio_thread_safety_serialized,
    pressio_thread_safety_pressio_thread_safety_single,
};
use ndarray::{Array, ArrayBase, ArrayView, CowArray, Data, Dimension, IxDyn};
use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct PressioError {
    pub error_code: i32,
    pub message: String,
}

impl PressioError {
    fn utf8_error(_err: std::str::Utf8Error, context: &str) -> Self {
        Self {
            error_code: 2,
            message: format!("invalid UTF-8 in {context}"),
        }
    }

    fn alloc_error(context: &str) -> Self {
        PressioError {
            error_code: 1,
            message: format!("failed to allocate {context}"),
        }
    }

    fn null_error(_err: std::ffi::NulError, context: &str) -> Self {
        PressioError {
            error_code: 1,
            message: format!("invalid null byte in {context}"),
        }
    }
}

pub fn major_version() -> u32 {
    unsafe { libpressio_sys::pressio_major_version() }
}

pub fn minor_version() -> u32 {
    unsafe { libpressio_sys::pressio_minor_version() }
}

pub fn patch_version() -> u32 {
    unsafe { libpressio_sys::pressio_patch_version() }
}

pub fn supported_compressors() -> Result<Vec<&'static str>, PressioError> {
    // Safety:
    // - pressio_supported_compressors is safe to call
    // - the returned pointer has 'static lifetime
    let supported_compressors =
        unsafe { CStr::from_ptr(libpressio_sys::pressio_supported_compressors()) };

    Ok(supported_compressors
        .to_str()
        .map_err(|err| PressioError::utf8_error(err, "compressor id"))?
        .split(' ')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect())
}

pub fn supported_io_modules() -> Result<Vec<&'static str>, PressioError> {
    // Safety:
    // - pressio_supported_io_modules is safe to call
    // - the returned pointer has 'static lifetime
    let supported_io = unsafe { CStr::from_ptr(libpressio_sys::pressio_supported_io_modules()) };

    Ok(supported_io
        .to_str()
        .map_err(|err| PressioError::utf8_error(err, "io module id"))?
        .split(' ')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect())
}

pub fn supported_metrics() -> Result<Vec<&'static str>, PressioError> {
    // Safety:
    // - pressio_supported_metrics is safe to call
    // - the returned pointer has 'static lifetime
    let supported_metrics = unsafe { CStr::from_ptr(libpressio_sys::pressio_supported_metrics()) };

    Ok(supported_metrics
        .to_str()
        .map_err(|err| PressioError::utf8_error(err, "metric id"))?
        .split(' ')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect())
}

pub fn features() -> Result<Vec<&'static str>, PressioError> {
    // Safety:
    // - pressio_features is safe to call
    // - the returned pointer has 'static lifetime
    let features = unsafe { CStr::from_ptr(libpressio_sys::pressio_features()) };

    Ok(features
        .to_str()
        .map_err(|err| PressioError::utf8_error(err, "feature"))?
        .split(' ')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect())
}

pub struct Pressio {
    // pressio is Send but !Sync
    // - impl Send below
    // - impl !Sync from NonNull
    library: NonNull<libpressio_sys::pressio>,
}

unsafe impl Send for Pressio {}

impl Pressio {
    pub fn new() -> Result<Pressio, PressioError> {
        let library: *mut libpressio_sys::pressio;
        unsafe {
            library = libpressio_sys::pressio_instance();
        }
        match NonNull::new(library) {
            Some(library) => Ok(Pressio { library }),
            None => Err(PressioError {
                error_code: 1,
                message: String::from("failed to initialize libpressio"),
            }),
        }
    }

    pub fn get_compressor<S: AsRef<str>>(
        &mut self,
        id: S,
    ) -> Result<PressioCompressor, PressioError> {
        let id = CString::new(id.as_ref())
            .map_err(|err| PressioError::null_error(err, "compressor id"))?;
        let ptr =
            unsafe { libpressio_sys::pressio_get_compressor(self.library.as_ptr(), id.as_ptr()) };
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(self.get_error());
        };
        let config = unsafe {
            libpressio_sys::pressio_compressor_get_configuration(ptr.as_ptr().cast_const())
        };
        let Some(config) = NonNull::new(config) else {
            unsafe { libpressio_sys::pressio_compressor_release(ptr.as_ptr()) };
            return Err(unsafe { PressioCompressor::get_error_from_raw(ptr) });
        };
        let mut thread_safe = libpressio_sys::pressio_thread_safety_pressio_thread_safety_single;
        let status = unsafe {
            libpressio_sys::pressio_options_get_threadsafety(
                config.as_ptr(),
                c"pressio:thread_safe".as_ptr(),
                &raw mut thread_safe,
            )
        };
        unsafe { libpressio_sys::pressio_options_free(config.as_ptr()) };
        if status != libpressio_sys::pressio_options_key_status_pressio_options_key_set {
            unsafe { libpressio_sys::pressio_compressor_release(ptr.as_ptr()) };
            return Err(PressioError {
                error_code: 1,
                message: String::from("compressor does not expose a `pressio:thread_safe` config"),
            });
        };
        if thread_safe < libpressio_sys::pressio_thread_safety_pressio_thread_safety_multiple {
            unsafe { libpressio_sys::pressio_compressor_release(ptr.as_ptr()) };
            return Err(PressioError {
                error_code: 1,
                message: String::from("compressor cannot be sent across threads"),
            });
        }
        Ok(PressioCompressor { ptr })
    }

    fn get_error(&mut self) -> PressioError {
        let error_code = unsafe { libpressio_sys::pressio_error_code(self.library.as_ptr()) };
        let message = unsafe {
            let message = libpressio_sys::pressio_error_msg(self.library.as_ptr());
            CStr::from_ptr(message).to_str()
        };
        match message {
            Ok(message) => PressioError {
                error_code,
                message: String::from(message),
            },
            Err(err) => PressioError::utf8_error(err, "pressio error message"),
        }
    }
}

impl Drop for Pressio {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_release(self.library.as_ptr());
        }
    }
}

pub struct PressioCompressor {
    // pressio_compressor (generally) is Send but !Sync
    // - impl Send below
    // - impl !Sync from NonNull
    // - check at runtime that no compressor can be instantiated that would
    //   violate these properties
    ptr: NonNull<libpressio_sys::pressio_compressor>,
}

unsafe impl Send for PressioCompressor {}

impl PressioCompressor {
    pub fn compress(
        &mut self,
        input_data: &PressioData,
        mut compressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_compress(
                self.as_raw_mut(),
                input_data.as_raw(),
                compressed_data.as_raw_mut(),
            )
        };
        if rc == 0 {
            Ok(compressed_data)
        } else {
            Err(self.get_error())
        }
    }

    pub fn decompress(
        &mut self,
        compressed_data: &PressioData,
        mut decompressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_decompress(
                self.as_raw_mut(),
                compressed_data.as_raw(),
                decompressed_data.as_raw_mut(),
            )
        };
        if rc == 0 {
            Ok(decompressed_data)
        } else {
            Err(self.get_error())
        }
    }

    pub fn set_options(&mut self, options: &PressioOptions) -> Result<(), PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_set_options(
                self.as_raw_mut(),
                options.ptr.as_ptr().cast_const(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(self.get_error())
        }
    }

    pub fn get_configuration(&self) -> Result<PressioOptions, PressioError> {
        let config = unsafe { libpressio_sys::pressio_compressor_get_configuration(self.as_raw()) };
        match NonNull::new(config) {
            Some(ptr) => Ok(PressioOptions { ptr }),
            None => Err(self.get_error()),
        }
    }

    pub fn get_documentation(&self) -> Result<PressioOptions, PressioError> {
        let docs = unsafe { libpressio_sys::pressio_compressor_get_documentation(self.as_raw()) };
        match NonNull::new(docs) {
            Some(ptr) => Ok(PressioOptions { ptr }),
            None => Err(self.get_error()),
        }
    }

    pub fn get_options(&self) -> Result<PressioOptions, PressioError> {
        let options = unsafe { libpressio_sys::pressio_compressor_get_options(self.as_raw()) };
        match NonNull::new(options) {
            Some(ptr) => Ok(PressioOptions { ptr }),
            None => Err(self.get_error()),
        }
    }

    pub fn get_metrics_options(&self) -> Result<PressioOptions, PressioError> {
        let options =
            unsafe { libpressio_sys::pressio_compressor_metrics_get_options(self.as_raw()) };
        match NonNull::new(options) {
            Some(ptr) => Ok(PressioOptions { ptr }),
            None => Err(self.get_error()),
        }
    }

    pub fn set_metrics_options(&mut self, options: &PressioOptions) -> Result<(), PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_metrics_set_options(
                self.as_raw_mut(),
                options.ptr.as_ptr().cast_const(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(self.get_error())
        }
    }

    pub fn get_metric_results(&self) -> Result<PressioOptions, PressioError> {
        let ptr = unsafe { libpressio_sys::pressio_compressor_get_metrics_results(self.as_raw()) };
        match NonNull::new(ptr) {
            Some(ptr) => Ok(PressioOptions { ptr }),
            None => Err(self.get_error()),
        }
    }

    pub fn get_name(&self) -> Result<&str, PressioError> {
        let name_ptr = unsafe { libpressio_sys::pressio_compressor_get_name(self.as_raw()) };
        let name = unsafe { CStr::from_ptr(name_ptr) };
        name.to_str()
            .map_err(|err| PressioError::utf8_error(err, "compressor name"))
    }

    pub fn set_name(&mut self, name: impl AsRef<str>) -> Result<(), PressioError> {
        let name = CString::new(name.as_ref())
            .map_err(|err| PressioError::null_error(err, "compressor name"))?;
        unsafe {
            libpressio_sys::pressio_compressor_set_name(self.as_raw_mut(), name.as_ptr());
        }
        Ok(())
    }

    pub fn get_prefix(&self) -> Result<&str, PressioError> {
        let prefix_ptr = unsafe { libpressio_sys::pressio_compressor_get_prefix(self.as_raw()) };
        let prefix = unsafe { CStr::from_ptr(prefix_ptr) };
        prefix
            .to_str()
            .map_err(|err| PressioError::utf8_error(err, "compressor id"))
    }

    pub fn major_version(&self) -> c_int {
        unsafe { libpressio_sys::pressio_compressor_major_version(self.as_raw()) }
    }

    pub fn minor_version(&self) -> c_int {
        unsafe { libpressio_sys::pressio_compressor_minor_version(self.as_raw()) }
    }

    pub fn patch_version(&self) -> c_int {
        unsafe { libpressio_sys::pressio_compressor_patch_version(self.as_raw()) }
    }

    pub fn get_version(&self) -> Result<&str, PressioError> {
        let version_ptr = unsafe { libpressio_sys::pressio_compressor_version(self.as_raw()) };
        let version = unsafe { CStr::from_ptr(version_ptr) };
        version
            .to_str()
            .map_err(|err| PressioError::utf8_error(err, "compressor version"))
    }

    fn as_raw(&self) -> *const libpressio_sys::pressio_compressor {
        self.ptr.as_ptr().cast_const()
    }

    fn as_raw_mut(&mut self) -> *mut libpressio_sys::pressio_compressor {
        self.ptr.as_ptr()
    }

    fn get_error(&self) -> PressioError {
        unsafe { Self::get_error_from_raw(self.ptr) }
    }

    unsafe fn get_error_from_raw(ptr: NonNull<libpressio_sys::pressio_compressor>) -> PressioError {
        let error_code =
            unsafe { libpressio_sys::pressio_compressor_error_code(ptr.as_ptr().cast_const()) };
        let message = unsafe {
            let message = libpressio_sys::pressio_compressor_error_msg(ptr.as_ptr().cast_const());
            CStr::from_ptr(message).to_str()
        };
        match message {
            Ok(message) => PressioError {
                error_code,
                message: String::from(message),
            },
            Err(err) => PressioError::utf8_error(err, "compressor error message"),
        }
    }
}

impl Drop for PressioCompressor {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_compressor_release(self.as_raw_mut());
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PressioDtype {
    Byte,
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl PressioDtype {
    pub fn is_floating(self) -> bool {
        unsafe { libpressio_sys::pressio_dtype_is_floating(self.into_raw()) != 0 }
    }

    pub fn is_numeric(self) -> bool {
        unsafe { libpressio_sys::pressio_dtype_is_numeric(self.into_raw()) != 0 }
    }

    pub fn is_signed(self) -> bool {
        unsafe { libpressio_sys::pressio_dtype_is_signed(self.into_raw()) != 0 }
    }

    fn from_raw(dtype: libpressio_sys::pressio_dtype) -> Option<Self> {
        match dtype {
            libpressio_sys::pressio_dtype_pressio_byte_dtype => Some(PressioDtype::Byte),
            libpressio_sys::pressio_dtype_pressio_bool_dtype => Some(PressioDtype::Bool),
            libpressio_sys::pressio_dtype_pressio_uint8_dtype => Some(PressioDtype::U8),
            libpressio_sys::pressio_dtype_pressio_uint16_dtype => Some(PressioDtype::U16),
            libpressio_sys::pressio_dtype_pressio_uint32_dtype => Some(PressioDtype::U32),
            libpressio_sys::pressio_dtype_pressio_uint64_dtype => Some(PressioDtype::U64),
            libpressio_sys::pressio_dtype_pressio_int8_dtype => Some(PressioDtype::I8),
            libpressio_sys::pressio_dtype_pressio_int16_dtype => Some(PressioDtype::I16),
            libpressio_sys::pressio_dtype_pressio_int32_dtype => Some(PressioDtype::I32),
            libpressio_sys::pressio_dtype_pressio_int64_dtype => Some(PressioDtype::I64),
            libpressio_sys::pressio_dtype_pressio_float_dtype => Some(PressioDtype::F32),
            libpressio_sys::pressio_dtype_pressio_double_dtype => Some(PressioDtype::F64),
            _ => None,
        }
    }

    const fn into_raw(self) -> libpressio_sys::pressio_dtype {
        match self {
            Self::Bool => libpressio_sys::pressio_dtype_pressio_bool_dtype,
            Self::U8 => libpressio_sys::pressio_dtype_pressio_uint8_dtype,
            Self::U16 => libpressio_sys::pressio_dtype_pressio_uint16_dtype,
            Self::U32 => libpressio_sys::pressio_dtype_pressio_uint32_dtype,
            Self::U64 => libpressio_sys::pressio_dtype_pressio_uint64_dtype,
            Self::I8 => libpressio_sys::pressio_dtype_pressio_int8_dtype,
            Self::I16 => libpressio_sys::pressio_dtype_pressio_int16_dtype,
            Self::I32 => libpressio_sys::pressio_dtype_pressio_int32_dtype,
            Self::I64 => libpressio_sys::pressio_dtype_pressio_int64_dtype,
            Self::F32 => libpressio_sys::pressio_dtype_pressio_float_dtype,
            Self::F64 => libpressio_sys::pressio_dtype_pressio_double_dtype,
            Self::Byte => libpressio_sys::pressio_dtype_pressio_byte_dtype,
        }
    }
}

impl std::fmt::Display for PressioDtype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Bool => "bool",
            Self::Byte => "byte",
            Self::U8 => "uint8",
            Self::U16 => "uint16",
            Self::U32 => "uint32",
            Self::U64 => "uint64",
            Self::I8 => "int8",
            Self::I16 => "int16",
            Self::I32 => "int32",
            Self::I64 => "int64",
            Self::F32 => "float",
            Self::F64 => "double",
        })
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum PressioArray {
    Byte(Array<c_uchar, IxDyn>),
    Bool(Array<bool, IxDyn>),
    U8(Array<u8, IxDyn>),
    U16(Array<u16, IxDyn>),
    U32(Array<u32, IxDyn>),
    U64(Array<u64, IxDyn>),
    I8(Array<i8, IxDyn>),
    I16(Array<i16, IxDyn>),
    I32(Array<i32, IxDyn>),
    I64(Array<i64, IxDyn>),
    F32(Array<f32, IxDyn>),
    F64(Array<f64, IxDyn>),
}

impl PressioArray {
    pub const fn dtype(&self) -> PressioDtype {
        match self {
            Self::Byte(_) => PressioDtype::Byte,
            Self::Bool(_) => PressioDtype::Bool,
            Self::U8(_) => PressioDtype::U8,
            Self::U16(_) => PressioDtype::U16,
            Self::U32(_) => PressioDtype::U32,
            Self::U64(_) => PressioDtype::U64,
            Self::I8(_) => PressioDtype::I8,
            Self::I16(_) => PressioDtype::I16,
            Self::I32(_) => PressioDtype::I32,
            Self::I64(_) => PressioDtype::I64,
            Self::F32(_) => PressioDtype::F32,
            Self::F64(_) => PressioDtype::F64,
        }
    }
}

pub trait PressioElement: sealed::PressioElement {
    const DTYPE: PressioDtype;
}

mod sealed {
    pub trait PressioElement: Copy {
        const DTYPE: libpressio_sys::pressio_dtype;
    }
}

macro_rules! impl_pressio_element {
    ($($variant:ident($ty:ty) => $impl:ident),*) => {
        $(
            impl sealed::PressioElement for $ty {
                const DTYPE: libpressio_sys::pressio_dtype = libpressio_sys::$impl;
            }

            impl PressioElement for $ty {
                const DTYPE: PressioDtype = PressioDtype::$variant;
            }
        )*
    };
}

impl_pressio_element! {
    Bool(bool) => pressio_dtype_pressio_bool_dtype,
    U8(u8) => pressio_dtype_pressio_uint8_dtype,
    U16(u16) => pressio_dtype_pressio_uint16_dtype,
    U32(u32) => pressio_dtype_pressio_uint32_dtype,
    U64(u64) => pressio_dtype_pressio_uint64_dtype,
    I8(i8) => pressio_dtype_pressio_int8_dtype,
    I16(i16) => pressio_dtype_pressio_int16_dtype,
    I32(i32) => pressio_dtype_pressio_int32_dtype,
    I64(i64) => pressio_dtype_pressio_int64_dtype,
    F32(f32) => pressio_dtype_pressio_float_dtype,
    F64(f64) => pressio_dtype_pressio_double_dtype
}

pub struct PressioData {
    // pressio_data is Send but !Sync
    // - impl Send below
    // - impl !Sync from NonNull
    data: NonNull<libpressio_sys::pressio_data>,
}

unsafe impl Send for PressioData {}

impl PressioData {
    pub fn new_empty<D: AsRef<[usize]>>(dtype: PressioDtype, shape: D) -> PressioData {
        let shape = shape.as_ref();
        let data = unsafe {
            libpressio_sys::pressio_data_new_empty(dtype.into_raw(), shape.len(), shape.as_ptr())
        };
        let data = NonNull::new(data).expect("pressio_data_new_empty must not return null");
        PressioData { data }
    }

    pub fn new_copied<T: PressioElement, S: Data<Elem = T>, D: Dimension>(
        x: impl Borrow<ArrayBase<S, D>>,
    ) -> Self {
        Self::new_copied_inner(x.borrow(), <T as sealed::PressioElement>::DTYPE)
    }

    pub fn new_bytes_copied<S: Data<Elem = c_uchar>, D: Dimension>(
        x: impl Borrow<ArrayBase<S, D>>,
    ) -> Self {
        Self::new_copied_inner(x.borrow(), libpressio_sys::pressio_dtype_pressio_byte_dtype)
    }

    fn new_copied_inner<T: Copy, S: Data<Elem = T>, D: Dimension>(
        x: &ArrayBase<S, D>,
        dtype: libpressio_sys::pressio_dtype,
    ) -> Self {
        let shape = x.shape().to_vec();

        let data = if x.is_standard_layout() {
            unsafe {
                libpressio_sys::pressio_data_new_copy(
                    dtype,
                    x.as_ptr().cast(),
                    shape.len(),
                    shape.as_ptr(),
                )
            }
        } else {
            let x_vec: Vec<T> = x.iter().copied().collect();
            unsafe {
                libpressio_sys::pressio_data_new_copy(
                    dtype,
                    x_vec.as_ptr().cast(),
                    shape.len(),
                    shape.as_ptr(),
                )
            }
        };
        let data = NonNull::new(data).expect("pressio_data_new_copy must not return null");
        PressioData { data }
    }

    pub fn new_with_shared<T: PressioElement, S: Data<Elem = T>, D: Dimension, O>(
        x: impl Borrow<ArrayBase<S, D>>,
        with: impl for<'a> FnOnce(&'a Self) -> O,
    ) -> O {
        Self::new_with_shared_inner(x.borrow(), <T as sealed::PressioElement>::DTYPE, with)
    }

    pub fn new_with_bytes_shared<S: Data<Elem = c_uchar>, D: Dimension, O>(
        x: impl Borrow<ArrayBase<S, D>>,
        with: impl for<'a> FnOnce(&'a Self) -> O,
    ) -> O {
        Self::new_with_shared_inner(
            x.borrow(),
            libpressio_sys::pressio_dtype_pressio_byte_dtype,
            with,
        )
    }

    fn new_with_shared_inner<T: Copy, S: Data<Elem = T>, D: Dimension, O>(
        x: &ArrayBase<S, D>,
        dtype: libpressio_sys::pressio_dtype,
        with: impl for<'a> FnOnce(&'a Self) -> O,
    ) -> O {
        if x.is_standard_layout() {
            let data = unsafe {
                libpressio_sys::pressio_data_new_nonowning(
                    dtype,
                    // SAFETY: we only give access to &PressioData, which does
                    //         not expose mutating access, so we can cast a
                    //         const ptr to a mut ptr here
                    x.as_ptr().cast_mut().cast(),
                    x.ndim(),
                    x.shape().as_ptr(),
                )
            };
            let data = NonNull::new(data).expect("pressio_data_new_nonowning must not return null");
            with(&Self { data })
        } else {
            let x_vec: Vec<T> = x.iter().copied().collect();
            let data = unsafe {
                libpressio_sys::pressio_data_new_nonowning(
                    dtype,
                    // SAFETY: we only give access to &PressioData, which does
                    //         not expose mutating access, so we can cast a
                    //         const ptr to a mut ptr here
                    x_vec.as_ptr().cast_mut().cast(),
                    x.ndim(),
                    x.shape().as_ptr(),
                )
            };
            let data = NonNull::new(data).expect("pressio_data_new_nonowning must not return null");
            let result = with(&Self { data });
            std::mem::drop(x_vec);
            result
        }
    }

    pub fn copied_from_array(a: impl AsRef<PressioArray>) -> Self {
        fn copied_from_array_ref(a: &PressioArray) -> PressioData {
            match a {
                PressioArray::Byte(a) => PressioData::new_bytes_copied(a),
                PressioArray::Bool(a) => PressioData::new_copied(a),
                PressioArray::U8(a) => PressioData::new_copied(a),
                PressioArray::U16(a) => PressioData::new_copied(a),
                PressioArray::U32(a) => PressioData::new_copied(a),
                PressioArray::U64(a) => PressioData::new_copied(a),
                PressioArray::I8(a) => PressioData::new_copied(a),
                PressioArray::I16(a) => PressioData::new_copied(a),
                PressioArray::I32(a) => PressioData::new_copied(a),
                PressioArray::I64(a) => PressioData::new_copied(a),
                PressioArray::F32(a) => PressioData::new_copied(a),
                PressioArray::F64(a) => PressioData::new_copied(a),
            }
        }

        copied_from_array_ref(a.as_ref())
    }

    pub fn with_shared<T: PressioElement, D: Dimension, O>(
        &self,
        shape: impl Into<D>,
        with: impl for<'a> FnOnce(CowArray<'a, T, D>) -> O,
    ) -> Option<O> {
        self.with_shared_inner(shape, with, <T as sealed::PressioElement>::DTYPE)
    }

    pub fn with_shared_bytes<D: Dimension, O>(
        &self,
        shape: impl Into<D>,
        with: impl for<'a> FnOnce(CowArray<'a, c_uchar, D>) -> O,
    ) -> Option<O> {
        self.with_shared_inner(
            shape,
            with,
            libpressio_sys::pressio_dtype_pressio_byte_dtype,
        )
    }

    fn with_shared_inner<T: Copy, D: Dimension, O>(
        &self,
        shape_out: impl Into<D>,
        with: impl for<'a> FnOnce(CowArray<'a, T, D>) -> O,
        dtype_out: libpressio_sys::pressio_dtype,
    ) -> Option<O> {
        if !self.has_data() {
            return None;
        }

        let dtype = self.dtype()?;

        if dtype.into_raw() != dtype_out {
            return None;
        }

        let shape_out = shape_out.into();
        let shape = self.shape();

        if ArrayView::from(shape.as_slice()) != shape_out.as_array_view() {
            return None;
        }

        let mut num_bytes = 0;
        let ptr = unsafe { libpressio_sys::pressio_data_ptr(self.as_raw(), &raw mut num_bytes) }
            .cast_const()
            .cast::<T>();

        if ptr.is_aligned() {
            // SAFETY: the data is aligned
            let data = unsafe { ArrayView::from_shape_ptr(shape_out, ptr) };
            return Some(with(CowArray::from(data)));
        }

        // copy the data into a new vector, ensuring that our copy is
        // properly aligned, no matter the alignment in libpressio
        let data = unsafe {
            let mut data = Vec::<T>::with_capacity(shape_out.size());
            std::ptr::copy_nonoverlapping::<u8>(
                ptr.cast(),
                data.as_mut_ptr().cast(),
                std::mem::size_of::<T>() * shape_out.size(),
            );
            data.set_len(shape_out.size());
            Array::from_shape_vec_unchecked(shape_out, data)
        };
        Some(with(CowArray::from(data)))
    }

    pub fn clone_into_array(&self) -> Option<PressioArray> {
        fn clone_into_array_typed<T: Copy>(ptr: *const c_void, shape: &[usize]) -> Array<T, IxDyn> {
            let size: usize = shape.iter().product();
            // copy the data into a new vector, ensuring that our copy is
            // properly aligned, no matter the alignment in libpressio
            let data = unsafe {
                let mut data = Vec::<T>::with_capacity(size);
                std::ptr::copy_nonoverlapping::<u8>(
                    ptr.cast(),
                    data.as_mut_ptr().cast(),
                    std::mem::size_of::<T>() * size,
                );
                data.set_len(size);
                data
            };
            unsafe { Array::from_shape_vec_unchecked(shape, data) }
        }

        if !self.has_data() {
            return None;
        }

        let dtype = self.dtype()?;
        let shape = self.shape();

        let mut num_bytes = 0;
        let ptr = unsafe { libpressio_sys::pressio_data_ptr(self.as_raw(), &raw mut num_bytes) }
            .cast_const();

        match dtype {
            PressioDtype::Bool => Some(PressioArray::Bool(clone_into_array_typed(ptr, &shape))),
            PressioDtype::U8 => Some(PressioArray::U8(clone_into_array_typed(ptr.cast(), &shape))),
            PressioDtype::U16 => Some(PressioArray::U16(clone_into_array_typed(ptr, &shape))),
            PressioDtype::U32 => Some(PressioArray::U32(clone_into_array_typed(ptr, &shape))),
            PressioDtype::U64 => Some(PressioArray::U64(clone_into_array_typed(ptr, &shape))),
            PressioDtype::I8 => Some(PressioArray::I8(clone_into_array_typed(ptr, &shape))),
            PressioDtype::I16 => Some(PressioArray::I16(clone_into_array_typed(ptr, &shape))),
            PressioDtype::I32 => Some(PressioArray::I32(clone_into_array_typed(ptr, &shape))),
            PressioDtype::I64 => Some(PressioArray::I64(clone_into_array_typed(ptr, &shape))),
            PressioDtype::F32 => Some(PressioArray::F32(clone_into_array_typed(ptr, &shape))),
            PressioDtype::F64 => Some(PressioArray::F64(clone_into_array_typed(ptr, &shape))),
            PressioDtype::Byte => Some(PressioArray::Byte(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
        }
    }

    pub fn dtype(&self) -> Option<PressioDtype> {
        let dtype = unsafe { libpressio_sys::pressio_data_dtype(self.as_raw()) };
        PressioDtype::from_raw(dtype)
    }

    pub fn has_data(&self) -> bool {
        unsafe { libpressio_sys::pressio_data_has_data(self.as_raw()) }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_num_elements(self.as_raw()) }
    }

    pub fn ndim(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_num_dimensions(self.as_raw()) }
    }

    pub fn shape(&self) -> Vec<usize> {
        (0..self.ndim())
            .map(|i| unsafe { libpressio_sys::pressio_data_get_dimension(self.as_raw(), i) })
            .collect::<Vec<_>>()
    }

    pub fn cast(&self, dtype: PressioDtype) -> Self {
        let data_ptr =
            unsafe { libpressio_sys::pressio_data_cast(self.as_raw(), dtype.into_raw()) };
        let data = NonNull::new(data_ptr).expect("pressio_data_cast must not return null");
        Self { data }
    }

    pub fn get_domain_id(&self) -> Result<&str, PressioError> {
        let domain_id_ptr = unsafe { libpressio_sys::pressio_data_domain_id(self.as_raw()) };
        let domain_id = unsafe { CStr::from_ptr(domain_id_ptr) };
        domain_id
            .to_str()
            .map_err(|err| PressioError::utf8_error(err, "data domain id"))
    }

    pub fn num_bytes(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_get_bytes(self.as_raw()) }
    }

    pub fn capacity_in_bytes(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_get_capacity_in_bytes(self.as_raw()) }
    }

    pub fn reshape(&mut self, shape: &[usize]) -> Result<(), PressioError> {
        let status = unsafe {
            libpressio_sys::pressio_data_reshape(self.as_raw_mut(), shape.len(), shape.as_ptr())
        };

        if status == 0 {
            Ok(())
        } else {
            Err(PressioError {
                error_code: status,
                message: String::from("failed to reshape data"),
            })
        }
    }

    fn as_raw(&self) -> *const libpressio_sys::pressio_data {
        self.data.as_ptr().cast_const()
    }

    fn as_raw_mut(&mut self) -> *mut libpressio_sys::pressio_data {
        self.data.as_ptr()
    }
}

impl Clone for PressioData {
    fn clone(&self) -> PressioData {
        let data = unsafe { libpressio_sys::pressio_data_new_clone(self.as_raw()) };
        let data = NonNull::new(data).expect("pressio_data_new_clone must not return null");
        PressioData { data }
    }
}

impl Drop for PressioData {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_data_free(self.as_raw_mut());
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressioThreadSafety {
    Single,
    Serialized,
    Multiple,
}

impl PressioThreadSafety {
    fn into_raw(self) -> libpressio_sys::pressio_thread_safety {
        match self {
            Self::Single => pressio_thread_safety_pressio_thread_safety_single,
            Self::Serialized => pressio_thread_safety_pressio_thread_safety_serialized,
            Self::Multiple => pressio_thread_safety_pressio_thread_safety_multiple,
        }
    }

    fn from_raw(safety: libpressio_sys::pressio_thread_safety) -> Option<Self> {
        match safety {
            pressio_thread_safety_pressio_thread_safety_single => Some(Self::Single),
            pressio_thread_safety_pressio_thread_safety_serialized => Some(Self::Serialized),
            pressio_thread_safety_pressio_thread_safety_multiple => Some(Self::Multiple),
            _ => None,
        }
    }
}

impl std::fmt::Display for PressioThreadSafety {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Single => "single",
            Self::Multiple => "multiple",
            Self::Serialized => "serialized",
        })
    }
}

#[derive(Clone)]
#[non_exhaustive]
pub enum PressioOption {
    bool(Option<bool>),
    int8(Option<i8>),
    int16(Option<i16>),
    int32(Option<i32>),
    int64(Option<i64>),
    uint8(Option<u8>),
    uint16(Option<u16>),
    uint32(Option<u32>),
    uint64(Option<u64>),
    float32(Option<f32>),
    float64(Option<f64>),
    string(Option<String>),
    vec_string(Option<Vec<String>>),
    data(Option<PressioData>),
    user_ptr(Option<*mut c_void>),
    dtype(Option<PressioDtype>),
    thread_safety(Option<PressioThreadSafety>),
    unset,
}

impl PressioOption {
    pub fn copy_type_only(&self) -> Self {
        match self {
            Self::bool(_) => Self::bool(None),
            Self::int8(_) => Self::int8(None),
            Self::int16(_) => Self::int16(None),
            Self::int32(_) => Self::int32(None),
            Self::int64(_) => Self::int64(None),
            Self::uint8(_) => Self::uint8(None),
            Self::uint16(_) => Self::uint16(None),
            Self::uint32(_) => Self::uint32(None),
            Self::uint64(_) => Self::uint64(None),
            Self::float32(_) => Self::float32(None),
            Self::float64(_) => Self::float64(None),
            Self::string(_) => Self::string(None),
            Self::vec_string(_) => Self::vec_string(None),
            Self::data(_) => Self::data(None),
            Self::user_ptr(_) => Self::user_ptr(None),
            Self::dtype(_) => Self::dtype(None),
            Self::thread_safety(_) => Self::thread_safety(None),
            Self::unset => Self::unset,
        }
    }

    fn into_raw(self) -> Result<NonNull<libpressio_sys::pressio_option>, PressioError> {
        struct OptionDrop(NonNull<libpressio_sys::pressio_option>);

        impl Drop for OptionDrop {
            fn drop(&mut self) {
                unsafe {
                    libpressio_sys::pressio_option_free(self.0.as_ptr());
                }
            }
        }

        let option = unsafe { libpressio_sys::pressio_option_new() };
        let Some(option) = NonNull::new(option) else {
            return Err(PressioError::alloc_error("option"));
        };

        let guard = OptionDrop(option);

        unsafe {
            match self {
                Self::bool(Some(x)) => libpressio_sys::pressio_option_set_bool(option.as_ptr(), x),
                Self::int8(Some(x)) => {
                    libpressio_sys::pressio_option_set_integer8(option.as_ptr(), x)
                }
                Self::int16(Some(x)) => {
                    libpressio_sys::pressio_option_set_integer16(option.as_ptr(), x)
                }
                Self::int32(Some(x)) => {
                    libpressio_sys::pressio_option_set_integer(option.as_ptr(), x)
                }
                Self::int64(Some(x)) => {
                    libpressio_sys::pressio_option_set_integer64(option.as_ptr(), x)
                }
                Self::uint8(Some(x)) => {
                    libpressio_sys::pressio_option_set_uinteger8(option.as_ptr(), x)
                }
                Self::uint16(Some(x)) => {
                    libpressio_sys::pressio_option_set_uinteger16(option.as_ptr(), x)
                }
                Self::uint32(Some(x)) => {
                    libpressio_sys::pressio_option_set_uinteger(option.as_ptr(), x)
                }
                Self::uint64(Some(x)) => {
                    libpressio_sys::pressio_option_set_uinteger64(option.as_ptr(), x)
                }
                Self::float32(Some(x)) => {
                    libpressio_sys::pressio_option_set_float(option.as_ptr(), x)
                }
                Self::float64(Some(x)) => {
                    libpressio_sys::pressio_option_set_double(option.as_ptr(), x)
                }
                Self::string(Some(x)) => {
                    let option_value = CString::new(x)
                        .map_err(|err| PressioError::null_error(err, "string option"))?;
                    let option_ptr = option_value.as_ptr();
                    libpressio_sys::pressio_option_set_string(option.as_ptr(), option_ptr)
                }
                Self::vec_string(Some(x)) => {
                    let option_value = x
                        .into_iter()
                        .map(CString::new)
                        .collect::<Result<Vec<CString>, _>>()
                        .map_err(|err| PressioError::null_error(err, "string array option"))?;
                    let mut option_value_cptr: Vec<*const i8> =
                        option_value.iter().map(|val| val.as_ptr()).collect();
                    libpressio_sys::pressio_option_set_strings(
                        option.as_ptr(),
                        option_value_cptr.as_mut_ptr(),
                        option_value_cptr.len(),
                    );
                }
                Self::data(Some(mut x)) => {
                    let data_ptr = x.as_raw_mut();
                    std::mem::forget(x);
                    libpressio_sys::pressio_option_set_data(option.as_ptr(), data_ptr);
                }
                Self::user_ptr(Some(x)) => {
                    libpressio_sys::pressio_option_set_userptr(option.as_ptr(), x);
                }
                Self::dtype(Some(x)) => {
                    libpressio_sys::pressio_option_set_dtype(option.as_ptr(), x.into_raw());
                }
                Self::thread_safety(Some(x)) => {
                    libpressio_sys::pressio_option_set_threadsafety(option.as_ptr(), x.into_raw());
                }
                Self::unset => {}
                Self::bool(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_bool_type,
                    );
                }
                Self::int8(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_int8_type,
                    );
                }
                Self::int16(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_int16_type,
                    );
                }
                Self::int32(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_int32_type,
                    );
                }
                Self::int64(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_int64_type,
                    );
                }
                Self::uint8(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_uint8_type,
                    );
                }
                Self::uint16(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_uint16_type,
                    );
                }
                Self::uint32(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_uint32_type,
                    );
                }
                Self::uint64(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_uint64_type,
                    );
                }
                Self::float32(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_float_type,
                    );
                }
                Self::float64(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_double_type,
                    );
                }
                Self::string(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_charptr_type,
                    );
                }
                Self::vec_string(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_charptr_array_type,
                    );
                }
                Self::data(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_data_type,
                    );
                }
                Self::user_ptr(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_userptr_type,
                    );
                }
                Self::dtype(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_dtype_type,
                    );
                }
                Self::thread_safety(None) => {
                    libpressio_sys::pressio_option_set_type(
                        option.as_ptr(),
                        libpressio_sys::pressio_option_type_pressio_option_threadsafety_type,
                    );
                }
            }
        };

        std::mem::forget(guard);

        Ok(option)
    }

    fn from_raw(option_ptr: *const libpressio_sys::pressio_option) -> Option<Self> {
        // Safety: option_ptr is a valid pointer to an option
        let option_type = unsafe { libpressio_sys::pressio_option_get_type(option_ptr) };
        let option_has_value = unsafe { libpressio_sys::pressio_option_has_value(option_ptr) };

        match option_type {
            libpressio_sys::pressio_option_type_pressio_option_unset_type => Some(Self::unset),
            libpressio_sys::pressio_option_type_pressio_option_bool_type => {
                Some(Self::bool(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_bool(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_int8_type => {
                Some(Self::int8(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_integer8(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_int16_type => {
                Some(Self::int16(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_integer16(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_int32_type => {
                Some(Self::int32(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_integer(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_int64_type => {
                Some(Self::int64(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_integer64(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_uint8_type => {
                Some(Self::uint8(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_uinteger8(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_uint16_type => {
                Some(Self::uint16(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_uinteger16(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_uint32_type => {
                Some(Self::uint32(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_uinteger(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_uint64_type => {
                Some(Self::uint64(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_uinteger64(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_float_type => {
                Some(Self::float32(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_float(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_double_type => {
                Some(Self::float64(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_double(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_charptr_type => {
                Some(Self::string(if option_has_value {
                    Some(unsafe {
                        CStr::from_ptr(libpressio_sys::pressio_option_get_string(option_ptr))
                            .to_owned()
                            .into_string()
                            .unwrap_or(String::from("<error>"))
                    })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_charptr_array_type => {
                Some(Self::vec_string(if option_has_value {
                    let mut len = 0;
                    let ptr = unsafe {
                        libpressio_sys::pressio_option_get_strings(option_ptr, &raw mut len)
                    };
                    let array = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
                    let strings = array
                        .iter()
                        .copied()
                        .map(|ptr| unsafe {
                            CStr::from_ptr(ptr)
                                .to_owned()
                                .into_string()
                                .unwrap_or(String::from("<error>"))
                        })
                        .collect();
                    unsafe {
                        libc::free(ptr.cast());
                    }
                    Some(strings)
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_userptr_type => {
                Some(Self::user_ptr(if option_has_value {
                    Some(unsafe { libpressio_sys::pressio_option_get_userptr(option_ptr) })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_data_type => {
                Some(Self::data(if option_has_value {
                    let data_ptr = unsafe { libpressio_sys::pressio_option_get_data(option_ptr) };
                    let data = NonNull::new(data_ptr)
                        .expect("pressio_option_get_data must not return null");
                    Some(PressioData { data })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_dtype_type => {
                Some(Self::dtype(if option_has_value {
                    PressioDtype::from_raw(unsafe {
                        libpressio_sys::pressio_option_get_dtype(option_ptr)
                    })
                } else {
                    None
                }))
            }
            libpressio_sys::pressio_option_type_pressio_option_threadsafety_type => {
                Some(Self::thread_safety(if option_has_value {
                    PressioThreadSafety::from_raw(unsafe {
                        libpressio_sys::pressio_option_get_threadsafety(option_ptr)
                    })
                } else {
                    None
                }))
            }
            // FIXME: skip unsupported types
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressioConversionSafety {
    Implicit,
    Explicit,
    Special,
}

impl PressioConversionSafety {
    const fn into_raw(self) -> libpressio_sys::pressio_conversion_safety {
        match self {
            Self::Implicit => libpressio_sys::pressio_conversion_safety_pressio_conversion_implicit,
            Self::Explicit => libpressio_sys::pressio_conversion_safety_pressio_conversion_explicit,
            Self::Special => libpressio_sys::pressio_conversion_safety_pressio_conversion_special,
        }
    }
}

pub struct PressioOptions {
    // pressio_options is Send but !Sync
    // - impl Send below
    // - impl !Sync from NonNull
    ptr: NonNull<libpressio_sys::pressio_options>,
}

unsafe impl Send for PressioOptions {}

impl PressioOptions {
    pub fn new() -> Result<PressioOptions, PressioError> {
        let ptr = unsafe { libpressio_sys::pressio_options_new() };
        match NonNull::new(ptr) {
            Some(ptr) => Ok(Self { ptr }),
            None => Err(PressioError::alloc_error("options")),
        }
    }

    pub fn merge(&self, extra: &Self) -> Self {
        let ptr = unsafe { libpressio_sys::pressio_options_merge(self.as_raw(), extra.as_raw()) };
        let ptr = NonNull::new(ptr).expect("pressio_options_merge must not return null");
        Self { ptr }
    }

    pub fn len(&self) -> usize {
        unsafe { libpressio_sys::pressio_options_size(self.as_raw()) }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn num_set(&self) -> usize {
        unsafe { libpressio_sys::pressio_options_num_set(self.as_raw()) }
    }

    pub fn has_option<S: AsRef<str>>(&self, option_name: S) -> Result<bool, PressioError> {
        let option_name = option_name.as_ref();
        let option_name = CString::new(option_name)
            .map_err(|err| PressioError::null_error(err, "option name"))?;
        let option_name_ptr = option_name.as_ptr();

        let status =
            unsafe { libpressio_sys::pressio_options_exists(self.as_raw(), option_name_ptr) };

        std::mem::drop(option_name);

        #[expect(clippy::wildcard_in_or_patterns)]
        Ok(match status {
            libpressio_sys::pressio_options_key_status_pressio_options_key_set
            | libpressio_sys::pressio_options_key_status_pressio_options_key_exists => true,
            libpressio_sys::pressio_options_key_status_pressio_options_key_does_not_exist | _ => {
                false
            }
        })
    }

    pub fn is_option_set<S: AsRef<str>>(&self, option_name: S) -> Result<bool, PressioError> {
        let option_name = option_name.as_ref();
        let option_name = CString::new(option_name)
            .map_err(|err| PressioError::null_error(err, "option name"))?;
        let option_name_ptr = option_name.as_ptr();

        let status =
            unsafe { libpressio_sys::pressio_options_exists(self.as_raw(), option_name_ptr) };

        std::mem::drop(option_name);

        #[expect(clippy::wildcard_in_or_patterns)]
        Ok(match status {
            libpressio_sys::pressio_options_key_status_pressio_options_key_set => true,
            libpressio_sys::pressio_options_key_status_pressio_options_key_exists
            | libpressio_sys::pressio_options_key_status_pressio_options_key_does_not_exist
            | _ => false,
        })
    }

    pub fn set<S: AsRef<str>>(
        &mut self,
        option_name: S,
        option: PressioOption,
    ) -> Result<(), PressioError> {
        let option_name = option_name.as_ref();
        let option_name = CString::new(option_name)
            .map_err(|err| PressioError::null_error(err, "option name"))?;
        let option_name_ptr = option_name.as_ptr();

        let option = option.into_raw()?;

        unsafe {
            libpressio_sys::pressio_options_set(
                self.as_raw_mut(),
                option_name_ptr,
                option.as_ptr(),
            );
        }

        unsafe {
            libpressio_sys::pressio_option_free(option.as_ptr());
        }

        std::mem::drop(option_name);
        Ok(())
    }

    pub fn set_with_cast<S: AsRef<str>>(
        &mut self,
        option_name: S,
        option: PressioOption,
        safety: PressioConversionSafety,
    ) -> Result<(), PressioError> {
        let option_name = option_name.as_ref();
        let option_name_cstr = CString::new(option_name)
            .map_err(|err| PressioError::null_error(err, "option name"))?;
        let option_name_ptr = option_name_cstr.as_ptr();

        let option = option.into_raw()?;

        let status = unsafe {
            libpressio_sys::pressio_options_cast_set(
                self.as_raw_mut(),
                option_name_ptr,
                option.as_ptr(),
                safety.into_raw(),
            )
        };

        unsafe {
            libpressio_sys::pressio_option_free(option.as_ptr());
        }

        if status != libpressio_sys::pressio_options_key_status_pressio_options_key_set {
            return Err(PressioError { error_code: status as i32, message: match status {
                libpressio_sys::pressio_options_key_status_pressio_options_key_exists => format!("failed to cast option: {option_name:?}"),
                libpressio_sys::pressio_options_key_status_pressio_options_key_does_not_exist => format!("no such option: {option_name:?}"),
                _ => String::from("<unknown>"),
            } });
        }

        std::mem::drop(option_name_cstr);
        Ok(())
    }

    pub fn get<S: AsRef<str>>(
        &self,
        option_name: S,
    ) -> Result<Option<PressioOption>, PressioError> {
        let option_name = option_name.as_ref();
        let option_name = CString::new(option_name)
            .map_err(|err| PressioError::null_error(err, "option name"))?;
        let option_name_ptr = option_name.as_ptr();

        let status =
            unsafe { libpressio_sys::pressio_options_exists(self.as_raw(), option_name_ptr) };

        #[expect(clippy::wildcard_in_or_patterns)]
        let has_option = match status {
            libpressio_sys::pressio_options_key_status_pressio_options_key_set
            | libpressio_sys::pressio_options_key_status_pressio_options_key_exists => true,
            libpressio_sys::pressio_options_key_status_pressio_options_key_does_not_exist | _ => {
                false
            }
        };

        if !has_option {
            return Ok(None);
        }

        let option_ptr =
            unsafe { libpressio_sys::pressio_options_get(self.as_raw(), option_name_ptr) };
        std::mem::drop(option_name);

        let option = PressioOption::from_raw(option_ptr);

        unsafe {
            libpressio_sys::pressio_option_free(option_ptr);
        }

        Ok(option)
    }

    pub fn iter(&self) -> impl FusedIterator<Item = (Option<String>, Option<PressioOption>)> + '_ {
        // Safety:
        // - self.ptr is a valid pointer to options
        // - we hold an immutable reference to ensure the iterator is not
        //   invalidated while we iterate
        let options_iter = unsafe { libpressio_sys::pressio_options_get_iter(self.as_raw()) };

        let ptr =
            NonNull::new(options_iter).expect("pressio_options_get_iter must not return null");

        PressioOptionsIter {
            ptr,
            _marker: PhantomData,
        }
    }

    fn as_raw(&self) -> *const libpressio_sys::pressio_options {
        self.ptr.as_ptr().cast_const()
    }

    fn as_raw_mut(&mut self) -> *mut libpressio_sys::pressio_options {
        self.ptr.as_ptr()
    }
}

impl Drop for PressioOptions {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_options_free(self.as_raw_mut());
        }
    }
}

impl Clone for PressioOptions {
    fn clone(&self) -> Self {
        let ptr = unsafe { libpressio_sys::pressio_options_copy(self.as_raw()) };
        let ptr = NonNull::new(ptr).expect("pressio_options_copy must not return null");
        Self { ptr }
    }
}

impl std::fmt::Display for PressioOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = unsafe {
            let ptr: *mut c_char = libpressio_sys::pressio_options_to_string(self.as_raw());
            let s = CStr::from_ptr(ptr).to_str().unwrap().to_string();
            libc::free(ptr.cast());
            s
        };
        write!(f, "{}", msg)
    }
}

struct PressioOptionsIter<'a> {
    // pressio_options_iter is Send but !Sync
    // - impl Send below
    // - impl !Sync from NonNull
    ptr: NonNull<libpressio_sys::pressio_options_iter>,
    _marker: PhantomData<&'a PressioOptions>,
}

impl<'a> PressioOptionsIter<'a> {
    fn as_raw_mut(&mut self) -> *mut libpressio_sys::pressio_options_iter {
        self.ptr.as_ptr()
    }
}

impl<'a> Iterator for PressioOptionsIter<'a> {
    type Item = (Option<String>, Option<PressioOption>);

    fn next(&mut self) -> Option<Self::Item> {
        if !unsafe { libpressio_sys::pressio_options_iter_has_value(self.as_raw_mut()) } {
            return None;
        }

        // Safety:
        // - options_iter is a valid options iterator
        // - the returned cstr is non-owned to so we copy it immediately
        let option_key = unsafe {
            CStr::from_ptr(libpressio_sys::pressio_options_iter_get_key(
                self.as_raw_mut(),
            ))
            .to_owned()
        };

        let option_ptr =
            unsafe { libpressio_sys::pressio_options_iter_get_value(self.as_raw_mut()) };
        let option_key = option_key.into_string().ok();

        // Safety: option_ptr is a valid pointer to an option
        let option = PressioOption::from_raw(option_ptr.cast_const());

        // Safety: option_ptr is a valid pointer to an option
        unsafe { libpressio_sys::pressio_option_free(option_ptr) };

        // Safety: options_iter is a valid options iterator
        unsafe {
            libpressio_sys::pressio_options_iter_next(self.as_raw_mut());
        }

        Some((option_key, option))
    }
}

impl<'a> FusedIterator for PressioOptionsIter<'a> {}

impl<'a> Drop for PressioOptionsIter<'a> {
    fn drop(&mut self) {
        unsafe { libpressio_sys::pressio_options_iter_free(self.as_raw_mut()) };
    }
}

unsafe impl<'a> Send for PressioOptionsIter<'a> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_data() -> ndarray::ArrayD<f32> {
        ndarray::Array2::from_shape_fn((30, 30), |(x, y)| (x + y) as f32).into_dyn()
    }

    fn safe_works(
        ndarray_to_data: impl Fn(
            ndarray::ArrayD<f32>,
            &mut PressioCompressor,
            Box<
                dyn FnOnce(
                    &PressioData,
                    &mut PressioCompressor,
                ) -> Result<(PressioData, PressioData), PressioError>,
            >,
        ) -> Result<(PressioData, PressioData), PressioError>,
    ) -> Result<(), PressioError> {
        let mut lib = Pressio::new()?;
        eprintln!("supported compressors: {:?}", supported_compressors());
        let mut compressor = lib.get_compressor("pressio")?;

        let mut options = PressioOptions::new()?;
        options.set("pressio:lossless", PressioOption::int32(Some(1)))?;
        options.set(
            "pressio:metric",
            PressioOption::string(Some(String::from("size"))),
        )?;

        compressor.set_options(&options)?;
        let options = compressor.get_options()?;
        println!("{}", options);

        let (compressed_data, decompressed_data) = ndarray_to_data(
            input_data(),
            &mut compressor,
            Box::new(|input_pdata, compressor| {
                let decompressed_data = input_pdata.clone();
                let compressed_data = PressioData::new_empty(PressioDtype::Byte, []);
                let compressed_data = compressor.compress(input_pdata, compressed_data)?;
                Ok((compressed_data, decompressed_data))
            }),
        )?;

        let _decompressed_data = compressor.decompress(&compressed_data, decompressed_data)?;

        let metric_results = compressor.get_metric_results()?;
        println!("{}", metric_results);

        Ok(())
    }

    #[test]
    fn safe_works_copied() -> Result<(), PressioError> {
        safe_works(|x, compressor, with| with(&PressioData::new_copied(x), compressor))
    }

    #[test]
    fn safe_works_with_shared() -> Result<(), PressioError> {
        safe_works(|x, compressor, with| PressioData::new_with_shared(x, |x| with(x, compressor)))
    }

    #[test]
    fn compress_decompress_noop_has_data() -> Result<(), PressioError> {
        let mut lib = Pressio::new()?;
        let mut compressor = lib.get_compressor("noop")?;

        let data = PressioData::new_copied(ndarray::array![1_i64, 2, 3, 4, 5]);
        assert!(data.has_data());
        assert_eq!(data.dtype(), Some(PressioDtype::I64));
        assert_eq!(data.len(), 5);
        assert_eq!(data.ndim(), 1);
        assert_eq!(
            data.clone_into_array(),
            Some(PressioArray::I64(ndarray::array![1, 2, 3, 4, 5].into_dyn()))
        );

        let compressed = PressioData::new_empty(PressioDtype::Byte, []);
        assert!(!compressed.has_data());
        assert_eq!(compressed.dtype(), Some(PressioDtype::Byte));
        assert_eq!(compressed.len(), 0);
        assert_eq!(compressed.ndim(), 0);
        assert_eq!(compressed.clone_into_array(), None);

        let compressed = compressor.compress(&data, compressed)?;
        assert!(compressed.has_data());
        assert_eq!(compressed.dtype(), Some(PressioDtype::I64));
        assert_eq!(compressed.len(), 5);
        assert_eq!(compressed.ndim(), 1);

        // FIXME: this should fail since we read uninit data
        let decompressed = PressioData::new_empty(PressioDtype::I64, [10]);
        assert!(!decompressed.has_data());
        assert_eq!(decompressed.dtype(), Some(PressioDtype::I64));
        assert_eq!(decompressed.len(), 10);
        assert_eq!(decompressed.ndim(), 1);

        let decompressed = compressor.decompress(&compressed, decompressed)?;
        assert!(decompressed.has_data());
        assert_eq!(decompressed.dtype(), Some(PressioDtype::I64));
        assert_eq!(decompressed.len(), 10);
        assert_eq!(decompressed.ndim(), 1);

        // FIXME: assert_eq!(
        //     decompressed.clone_into_array(),
        //     Some(PressioArray::I64(ndarray::array![1, 2, 3, 4, 5].into_dyn()))
        // );

        Ok(())
    }

    // #[test]
    // fn unsafe_works() {
    //     use std::ptr;

    //     use libpressio_sys::*;

    //     unsafe {
    //         let library = pressio_instance();
    //         let compressor_id = CString::new("sz").unwrap();
    //         let compressor = pressio_get_compressor(library, compressor_id.as_ptr());
    //         assert_ne!(compressor, ptr::null_mut::<pressio_compressor>());

    //         let input_array = input_data();
    //         let input_pdata = pressio_data_new_copy(
    //             pressio_dtype_pressio_float_dtype,
    //             input_array.as_ptr().cast(),
    //             input_array.ndim(),
    //             input_array.shape().as_ptr(),
    //         );
    //         assert_ne!(input_pdata, ptr::null_mut::<pressio_data>());

    //         let compressed_pdata =
    //             pressio_data_new_empty(pressio_dtype_pressio_byte_dtype, 0, ptr::null());
    //         let output_pdata = pressio_data_new_clone(input_pdata);
    //         assert_ne!(output_pdata, ptr::null_mut::<pressio_data>());

    //         let pressio_options = pressio_options_new();
    //         let pressio_metric = c"pressio:metric";
    //         let pressio_metric_value = c"size";
    //         let pressio_lossless = c"pressio:lossless";
    //         pressio_options_set_string(
    //             pressio_options,
    //             pressio_metric.as_ptr(),
    //             pressio_metric_value.as_ptr(),
    //         );
    //         pressio_options_set_integer(pressio_options, pressio_lossless.as_ptr(), 1);
    //         let ec = pressio_compressor_set_options(compressor, pressio_options);
    //         assert_eq!(ec, 0);

    //         let ec = pressio_compressor_compress(compressor, input_pdata, compressed_pdata);
    //         assert_eq!(ec, 0);

    //         let ec = pressio_compressor_decompress(compressor, compressed_pdata, output_pdata);
    //         assert_eq!(ec, 0);

    //         let metrics_results = pressio_compressor_get_metrics_results(compressor);
    //         assert!(pressio_options_size(metrics_results) > 0);
    //         let metrics_ptr = pressio_options_to_string(metrics_results);
    //         let metrics_cstr = CStr::from_ptr(metrics_ptr).to_str().unwrap();
    //         println!("{}", metrics_cstr);

    //         pressio_compressor_release(compressor);
    //         pressio_options_free(metrics_results);
    //         pressio_options_free(pressio_options);
    //         pressio_data_free(input_pdata);
    //         pressio_data_free(compressed_pdata);
    //         pressio_data_free(output_pdata);
    //         pressio_release(library);
    //         libc::free(metrics_ptr.cast());
    //     }
    // }
}
