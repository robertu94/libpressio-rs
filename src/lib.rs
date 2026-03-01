#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_uchar, c_void},
    ptr::NonNull,
};

use ndarray::{Array, ArrayView, Dimension, IxDyn};
use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct PressioError {
    pub error_code: i32,
    pub message: String,
}

impl From<std::str::Utf8Error> for PressioError {
    fn from(_: std::str::Utf8Error) -> Self {
        PressioError {
            error_code: 2,
            message: String::from("utf8 error"),
        }
    }
}

impl From<std::ffi::NulError> for PressioError {
    fn from(_: std::ffi::NulError) -> Self {
        PressioError {
            error_code: 1,
            message: String::from("nul error"),
        }
    }
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
                message: "failed to init library".to_string(),
            }),
        }
    }

    pub fn get_compressor<S: AsRef<str>>(
        &mut self,
        id: S,
    ) -> Result<PressioCompressor, PressioError> {
        let id = CString::new(id.as_ref())?;
        let ptr =
            unsafe { libpressio_sys::pressio_get_compressor(self.library.as_ptr(), id.as_ptr()) };
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(self.get_error());
        };
        let options =
            unsafe { libpressio_sys::pressio_compressor_get_options(ptr.as_ptr().cast_const()) };
        let Some(options) = NonNull::new(options) else {
            unsafe { libpressio_sys::pressio_compressor_release(ptr.as_ptr()) };
            return Err(unsafe { PressioCompressor::get_error_from_raw(ptr) });
        };
        let mut thread_safe = libpressio_sys::pressio_thread_safety_pressio_thread_safety_single;
        let status = unsafe {
            libpressio_sys::pressio_options_get_threadsafety(
                options.as_ptr(),
                c"pressio:thread_safe".as_ptr(),
                &raw mut thread_safe,
            )
        };
        unsafe { libpressio_sys::pressio_options_free(options.as_ptr()) };
        if status != libpressio_sys::pressio_options_key_status_pressio_options_key_set {
            unsafe { libpressio_sys::pressio_compressor_release(ptr.as_ptr()) };
            return Err(PressioError {
                error_code: 1,
                message: String::from("compressor does not expose a `pressio:thread_safe` option"),
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

    pub fn supported_compressors(&self) -> Result<Vec<String>, PressioError> {
        // Safety:
        // - pressio_supported_compressors is safe to call
        // - the returned pointer's ownership is questionable, so we copy it immediately
        let supported_compressors =
            unsafe { CStr::from_ptr(libpressio_sys::pressio_supported_compressors()).to_owned() };

        Ok(supported_compressors
            .into_string()
            .map_err(|err| err.utf8_error())?
            .split(' ')
            .filter(|x| !x.trim().is_empty())
            .map(String::from)
            .collect())
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
                message: message.to_string(),
            },
            Err(e) => e.into(),
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
        compressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_compress(
                self.ptr.as_ptr(),
                input_data.data,
                compressed_data.data,
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
        decompressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_decompress(
                self.ptr.as_ptr(),
                compressed_data.data,
                decompressed_data.data,
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
            libpressio_sys::pressio_compressor_set_options(self.ptr.as_ptr(), options.ptr)
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(self.get_error())
        }
    }

    pub fn get_options(&self) -> Result<PressioOptions, PressioError> {
        let options = unsafe {
            libpressio_sys::pressio_compressor_get_options(self.ptr.as_ptr().cast_const())
        };
        if !options.is_null() {
            Ok(PressioOptions::from_raw(options))
        } else {
            Err(self.get_error())
        }
    }

    pub fn get_metric_results(&self) -> Result<PressioOptions, PressioError> {
        let ptr = unsafe {
            libpressio_sys::pressio_compressor_get_metrics_results(self.ptr.as_ptr().cast_const())
        };
        if !ptr.is_null() {
            Ok(PressioOptions::from_raw(ptr))
        } else {
            Err(self.get_error())
        }
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
                message: message.to_string(),
            },
            Err(e) => e.into(),
        }
    }
}

impl Drop for PressioCompressor {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_compressor_release(self.ptr.as_ptr());
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

impl PressioDtype {
    const fn to_dtype(self) -> libpressio_sys::pressio_dtype {
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
    data: *mut libpressio_sys::pressio_data,
}

impl PressioData {
    pub fn new_empty<D: AsRef<[usize]>>(dtype: PressioDtype, shape: D) -> PressioData {
        let shape = shape.as_ref();
        let data = unsafe {
            libpressio_sys::pressio_data_new_empty(dtype.to_dtype(), shape.len(), shape.as_ptr())
        };
        PressioData { data }
    }

    pub fn new<T: PressioElement, D: Dimension>(x: Array<T, D>) -> Self {
        Self::new_inner(x, <T as sealed::PressioElement>::DTYPE)
    }

    pub fn new_bytes<D: Dimension>(x: Array<c_uchar, D>) -> Self {
        Self::new_inner(x, libpressio_sys::pressio_dtype_pressio_byte_dtype)
    }

    fn new_inner<T: Copy, D: Dimension>(
        x: Array<T, D>,
        dtype: libpressio_sys::pressio_dtype,
    ) -> Self {
        let shape = x.shape().to_vec();

        let data = if x.is_standard_layout() {
            let mut x = x;
            let data_ptr = x.as_mut_ptr();
            let box_ptr = Box::into_raw(Box::new(x));
            let deleter: Box<Box<dyn FnOnce()>> = Box::new(Box::new(|| {
                let abox: Box<Array<T, D>> = unsafe { Box::from_raw(box_ptr) };
                std::mem::drop(abox);
            }));
            unsafe {
                libpressio_sys::pressio_data_new_move(
                    dtype,
                    data_ptr.cast(),
                    shape.len(),
                    shape.as_ptr(),
                    Some(deleter_trampoline),
                    Box::into_raw(deleter).cast(),
                )
            }
        } else {
            let mut x: Vec<T> = x.into_iter().collect();
            let data_ptr = x.as_mut_ptr();
            let box_ptr = Box::into_raw(Box::new(x));
            let deleter: Box<Box<dyn FnOnce()>> = Box::new(Box::new(|| {
                let vbox: Box<Vec<T>> = unsafe { Box::from_raw(box_ptr) };
                std::mem::drop(vbox);
            }));
            unsafe {
                libpressio_sys::pressio_data_new_move(
                    dtype,
                    data_ptr.cast(),
                    shape.len(),
                    shape.as_ptr(),
                    Some(deleter_trampoline),
                    Box::into_raw(deleter).cast(),
                )
            }
        };
        PressioData { data }
    }

    pub fn new_copied<T: PressioElement, D: Dimension>(x: ArrayView<T, D>) -> Self {
        Self::new_copied_inner(x, <T as sealed::PressioElement>::DTYPE)
    }

    pub fn new_bytes_copied<D: Dimension>(x: ArrayView<c_uchar, D>) -> Self {
        Self::new_copied_inner(x, libpressio_sys::pressio_dtype_pressio_byte_dtype)
    }

    fn new_copied_inner<T: Copy, D: Dimension>(
        x: ArrayView<T, D>,
        dtype: libpressio_sys::pressio_dtype,
    ) -> Self {
        let shape = x.shape().to_vec();

        let data = if x.is_standard_layout() {
            let data_ptr = x.as_ptr();
            unsafe {
                libpressio_sys::pressio_data_new_copy(
                    dtype,
                    data_ptr.cast_mut().cast(), // FIXME: why cast mut?
                    shape.len(),
                    shape.as_ptr(),
                )
            }
        } else {
            let mut x: Vec<T> = x.iter().copied().collect();
            let data_ptr = x.as_mut_ptr();
            let box_ptr = Box::into_raw(Box::new(x));
            let deleter: Box<Box<dyn FnOnce()>> = Box::new(Box::new(|| {
                let vbox: Box<Vec<T>> = unsafe { Box::from_raw(box_ptr) };
                std::mem::drop(vbox);
            }));
            unsafe {
                libpressio_sys::pressio_data_new_move(
                    dtype,
                    data_ptr.cast(),
                    shape.len(),
                    shape.as_ptr(),
                    Some(deleter_trampoline),
                    Box::into_raw(deleter).cast(),
                )
            }
        };
        PressioData { data }
    }

    pub fn from_array(a: PressioArray) -> Self {
        match a {
            PressioArray::Byte(a) => Self::new_bytes(a),
            PressioArray::Bool(a) => Self::new(a),
            PressioArray::U8(a) => Self::new(a),
            PressioArray::U16(a) => Self::new(a),
            PressioArray::U32(a) => Self::new(a),
            PressioArray::U64(a) => Self::new(a),
            PressioArray::I8(a) => Self::new(a),
            PressioArray::I16(a) => Self::new(a),
            PressioArray::I32(a) => Self::new(a),
            PressioArray::I64(a) => Self::new(a),
            PressioArray::F32(a) => Self::new(a),
            PressioArray::F64(a) => Self::new(a),
        }
    }

    pub fn clone_into_array(&self) -> Option<PressioArray> {
        fn clone_into_array_typed<T: Copy>(ptr: *const T, shape: &[usize]) -> Array<T, IxDyn> {
            unsafe { ArrayView::from_shape_ptr(shape, ptr) }.to_owned()
        }

        if !self.has_data() {
            return None;
        }

        let dtype = self.dtype()?;

        let shape = (0..self.ndim())
            .map(|i| unsafe { libpressio_sys::pressio_data_get_dimension(self.data, i) })
            .collect::<Vec<_>>();

        let mut num_bytes = 0;
        let ptr =
            unsafe { libpressio_sys::pressio_data_ptr(self.data, &raw mut num_bytes) }.cast_const();

        match dtype {
            PressioDtype::Bool => Some(PressioArray::Bool(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::U8 => Some(PressioArray::U8(clone_into_array_typed(ptr.cast(), &shape))),
            PressioDtype::U16 => Some(PressioArray::U16(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::U32 => Some(PressioArray::U32(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::U64 => Some(PressioArray::U64(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::I8 => Some(PressioArray::I8(clone_into_array_typed(ptr.cast(), &shape))),
            PressioDtype::I16 => Some(PressioArray::I16(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::I32 => Some(PressioArray::I32(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::I64 => Some(PressioArray::I64(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::F32 => Some(PressioArray::F32(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::F64 => Some(PressioArray::F64(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
            PressioDtype::Byte => Some(PressioArray::Byte(clone_into_array_typed(
                ptr.cast(),
                &shape,
            ))),
        }
    }

    pub fn dtype(&self) -> Option<PressioDtype> {
        let dtype = unsafe { libpressio_sys::pressio_data_dtype(self.data) };

        match dtype {
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

    pub fn has_data(&self) -> bool {
        unsafe { libpressio_sys::pressio_data_has_data(self.data) }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_num_elements(self.data) }
    }

    pub fn ndim(&self) -> usize {
        unsafe { libpressio_sys::pressio_data_num_dimensions(self.data) }
    }
}

unsafe extern "C" fn deleter_trampoline(_data_ptr: *mut c_void, fn_ptr: *mut c_void) {
    let deleter: Box<Box<dyn FnOnce()>> = unsafe { Box::from_raw(fn_ptr.cast()) };
    deleter()
}

impl Clone for PressioData {
    fn clone(&self) -> PressioData {
        PressioData {
            data: unsafe { libpressio_sys::pressio_data_new_clone(self.data) },
        }
    }
}

impl Drop for PressioData {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_data_free(self.data);
        }
    }
}

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
    unset,
}

pub struct PressioOptions {
    ptr: *mut libpressio_sys::pressio_options,
}

impl std::fmt::Display for PressioOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = unsafe {
            let ptr = libpressio_sys::pressio_options_to_string(self.ptr);
            let s = CStr::from_ptr(ptr).to_str().unwrap().to_string();
            libc::free(ptr as *mut c_void);
            s
        };
        write!(f, "{}", msg)
    }
}

impl PressioOptions {
    pub fn new() -> Result<PressioOptions, PressioError> {
        let ptr = unsafe { libpressio_sys::pressio_options_new() };
        if !ptr.is_null() {
            Ok(PressioOptions { ptr })
        } else {
            Err(PressioError {
                message: "failed to allocate options".to_string(),
                error_code: 1,
            })
        }
    }
    fn from_raw(ptr: *mut libpressio_sys::pressio_options) -> PressioOptions {
        PressioOptions { ptr }
    }

    pub fn set<S: AsRef<str>>(
        self,
        option_name: S,
        option: PressioOption,
    ) -> Result<PressioOptions, PressioError> {
        let option_name = option_name.as_ref();
        let option_name = CString::new(option_name)?;
        let option_name = option_name.as_ptr();

        unsafe {
            match option {
                PressioOption::bool(Some(x)) => {
                    libpressio_sys::pressio_options_set_bool(self.ptr, option_name, x)
                }
                PressioOption::int8(Some(x)) => {
                    libpressio_sys::pressio_options_set_integer8(self.ptr, option_name, x)
                }
                PressioOption::int16(Some(x)) => {
                    libpressio_sys::pressio_options_set_integer16(self.ptr, option_name, x)
                }
                PressioOption::int32(Some(x)) => {
                    libpressio_sys::pressio_options_set_integer(self.ptr, option_name, x)
                }
                PressioOption::int64(Some(x)) => {
                    libpressio_sys::pressio_options_set_integer64(self.ptr, option_name, x)
                }
                PressioOption::uint8(Some(x)) => {
                    libpressio_sys::pressio_options_set_uinteger8(self.ptr, option_name, x)
                }
                PressioOption::uint16(Some(x)) => {
                    libpressio_sys::pressio_options_set_uinteger16(self.ptr, option_name, x)
                }
                PressioOption::uint32(Some(x)) => {
                    libpressio_sys::pressio_options_set_uinteger(self.ptr, option_name, x)
                }
                PressioOption::uint64(Some(x)) => {
                    libpressio_sys::pressio_options_set_uinteger64(self.ptr, option_name, x)
                }
                PressioOption::float32(Some(x)) => {
                    libpressio_sys::pressio_options_set_float(self.ptr, option_name, x)
                }
                PressioOption::float64(Some(x)) => {
                    libpressio_sys::pressio_options_set_double(self.ptr, option_name, x)
                }
                PressioOption::string(Some(x)) => {
                    let option_value = CString::new(x)?;
                    let option_ptr = option_value.as_ptr();
                    libpressio_sys::pressio_options_set_string(self.ptr, option_name, option_ptr)
                }
                PressioOption::vec_string(Some(x)) => {
                    let option_value = x
                        .iter()
                        .map(|val: &String| CString::new(val.clone()))
                        .collect::<Result<Vec<CString>, _>>()?;
                    let option_value_cptr: Vec<*const i8> =
                        option_value.iter().map(|val| val.as_ptr()).collect();
                    libpressio_sys::pressio_options_set_strings(
                        self.ptr,
                        option_name,
                        option_value_cptr.len(),
                        option_value_cptr.as_ptr(),
                    );
                }
                PressioOption::data(Some(x)) => {
                    libpressio_sys::pressio_options_set_data(self.ptr, option_name, x.data);
                }
                PressioOption::user_ptr(Some(x)) => {
                    libpressio_sys::pressio_options_set_userptr(self.ptr, option_name, x);
                }
                PressioOption::unset => {}
                PressioOption::bool(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_option_type_pressio_option_bool_type,
                    );
                }
                PressioOption::int8(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_option_type_pressio_option_int8_type,
                    );
                }
                PressioOption::int16(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_int16_dtype,
                    );
                }
                PressioOption::int32(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_int32_dtype,
                    );
                }
                PressioOption::int64(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_int64_dtype,
                    );
                }
                PressioOption::uint8(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint8_dtype,
                    );
                }
                PressioOption::uint16(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint16_dtype,
                    );
                }
                PressioOption::uint32(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint32_dtype,
                    );
                }
                PressioOption::uint64(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint64_dtype,
                    );
                }
                PressioOption::float32(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_float_dtype,
                    );
                }
                PressioOption::float64(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_double_dtype,
                    );
                }
                PressioOption::string(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint64_dtype,
                    );
                }
                PressioOption::vec_string(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint64_dtype,
                    );
                }
                PressioOption::data(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint64_dtype,
                    );
                }
                PressioOption::user_ptr(None) => {
                    libpressio_sys::pressio_options_set_type(
                        self.ptr,
                        option_name,
                        libpressio_sys::pressio_dtype_pressio_uint64_dtype,
                    );
                }
            }
        }
        Ok(self)
    }

    pub fn get_options(&mut self) -> Result<BTreeMap<String, PressioOption>, PressioError> {
        // Safety:
        // - self.ptr is a valid pointer to options
        let options_size = unsafe { libpressio_sys::pressio_options_size(self.ptr) };

        // Safety:
        // - self.ptr is a valid pointer to options
        // - we hold a mutable reference to ensure the iterator is not
        //   invalidated while we iterate
        let options_iter = unsafe { libpressio_sys::pressio_options_get_iter(self.ptr) };

        let mut options = Vec::with_capacity(options_size);

        let mut first = true;
        for _ in 0..options_size {
            if !first {
                // Safety:
                // - options_iter is a valid options iterator
                // - we only advance when there are more options
                unsafe { libpressio_sys::pressio_options_iter_next(options_iter) };
            }
            first = false;
            // Safety:
            // - options_iter is a valid options iterator
            // - the returned cstr is non-owned to so we copy it immediately
            let option_key = unsafe {
                CStr::from_ptr(libpressio_sys::pressio_options_iter_get_key(options_iter))
                    .to_owned()
            };
            // Safety:
            // - self.ptr is a valid pointer to options
            // - option_key is a valid options key
            let option_ptr =
                unsafe { libpressio_sys::pressio_options_get(self.ptr, option_key.as_ptr()) };
            let option_key = option_key.into_string().unwrap_or(String::from("<error>"));

            // Safety: option_ptr is a valid pointer to an option
            let option_type = unsafe { libpressio_sys::pressio_option_get_type(option_ptr) };
            let option_has_value = unsafe { libpressio_sys::pressio_option_has_value(option_ptr) };

            let option = match option_type {
                libpressio_sys::pressio_option_type_pressio_option_unset_type => {
                    Some(PressioOption::unset)
                }
                libpressio_sys::pressio_option_type_pressio_option_bool_type => {
                    Some(PressioOption::bool(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_bool(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_int8_type => {
                    Some(PressioOption::int8(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_integer8(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_int16_type => {
                    Some(PressioOption::int16(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_integer16(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_int32_type => {
                    Some(PressioOption::int32(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_integer(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_int64_type => {
                    Some(PressioOption::int64(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_integer64(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_uint8_type => {
                    Some(PressioOption::uint8(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_uinteger8(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_uint16_type => {
                    Some(PressioOption::uint16(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_uinteger16(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_uint32_type => {
                    Some(PressioOption::uint32(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_uinteger(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_uint64_type => {
                    Some(PressioOption::uint64(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_uinteger64(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_float_type => {
                    Some(PressioOption::float32(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_float(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_double_type => {
                    Some(PressioOption::float64(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_double(option_ptr) })
                    } else {
                        None
                    }))
                }
                libpressio_sys::pressio_option_type_pressio_option_charptr_type => {
                    Some(PressioOption::string(if option_has_value {
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
                    Some(PressioOption::string(if option_has_value {
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
                    Some(PressioOption::user_ptr(if option_has_value {
                        Some(unsafe { libpressio_sys::pressio_option_get_userptr(option_ptr) })
                    } else {
                        None
                    }))
                }
                // FIXME: skip unsupported types
                _ => None,
            };

            // Safety: option_ptr is a valid pointer to an option
            unsafe { libpressio_sys::pressio_option_free(option_ptr) };

            if let Some(option_value) = option {
                options.push((option_key, option_value));
            }
        }

        // Safety: options_iter is a valid options iterator
        unsafe { libpressio_sys::pressio_options_iter_free(options_iter) };

        Ok(BTreeMap::from_iter(options))
    }
}
impl Drop for PressioOptions {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_options_free(self.ptr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_data() -> ndarray::ArrayD<f32> {
        let data = unsafe {
            let mut data = ndarray::Array2::<f32>::uninit([30, 30]);
            for ((x, y), elm) in data.indexed_iter_mut() {
                *elm = std::mem::MaybeUninit::new((x + y) as f32);
            }
            data.assume_init()
        };
        data.into_dyn()
    }

    #[test]
    fn safe_works() -> Result<(), crate::PressioError> {
        let mut lib = Pressio::new().expect("failed to create library");
        eprintln!("supported compressors: {:}", unsafe {
            CStr::from_ptr(libpressio_sys::pressio_supported_compressors()).to_str()?
        });
        let mut compressor = lib.get_compressor("pressio").expect("expected compressor");

        let options = PressioOptions::new()?
            .set("pressio:lossless", PressioOption::int32(Some(1)))?
            .set(
                "pressio:metric",
                PressioOption::string(Some("size".to_string())),
            )?;

        let input_pdata = PressioData::new(input_data());
        let compressed_data = PressioData::new_empty(PressioDtype::Byte, []);
        let decompressed_data = input_pdata.clone();

        compressor.set_options(&options).unwrap();
        let options = compressor.get_options().unwrap();
        println!("{}", options);

        let compressed_data = compressor
            .compress(&input_pdata, compressed_data)
            .expect("compression failed");
        let _decompressed_data = compressor
            .decompress(&compressed_data, decompressed_data)
            .expect("decompressed_data failed");

        let metric_results = compressor.get_metric_results()?;
        println!("{}", metric_results);

        Ok(())
    }

    #[test]
    fn unsafe_works() {
        use std::ptr;

        use libpressio_sys::*;

        unsafe {
            let library = pressio_instance();
            let compressor_id = CString::new("sz").unwrap();
            let compressor = pressio_get_compressor(library, compressor_id.as_ptr());
            assert_ne!(compressor, ptr::null_mut::<pressio_compressor>());

            let mut input_array = input_data();
            let input_pdata = pressio_data_new_copy(
                pressio_dtype_pressio_float_dtype,
                input_array.as_mut_ptr() as *mut c_void,
                input_array.ndim(),
                input_array.shape().as_ptr(),
            );
            assert_ne!(input_pdata, ptr::null_mut::<pressio_data>());

            let compressed_pdata =
                pressio_data_new_empty(pressio_dtype_pressio_byte_dtype, 0, ptr::null());
            let output_pdata = pressio_data_new_clone(input_pdata);
            assert_ne!(output_pdata, ptr::null_mut::<pressio_data>());

            let pressio_options = pressio_options_new();
            let pressio_metric = c"pressio:metric";
            let pressio_metric_value = c"size";
            let pressio_lossless = c"pressio:lossless";
            pressio_options_set_string(
                pressio_options,
                pressio_metric.as_ptr(),
                pressio_metric_value.as_ptr(),
            );
            pressio_options_set_integer(pressio_options, pressio_lossless.as_ptr(), 1);
            let ec = pressio_compressor_set_options(compressor, pressio_options);
            assert_eq!(ec, 0);

            let ec = pressio_compressor_compress(compressor, input_pdata, compressed_pdata);
            assert_eq!(ec, 0);

            let ec = pressio_compressor_decompress(compressor, compressed_pdata, output_pdata);
            assert_eq!(ec, 0);

            let metrics_results = pressio_compressor_get_metrics_results(compressor);
            assert!(pressio_options_size(metrics_results) > 0);
            let metrics_ptr = pressio_options_to_string(metrics_results);
            let metrics_cstr = CStr::from_ptr(metrics_ptr).to_str().unwrap();
            println!("{}", metrics_cstr);

            pressio_compressor_release(compressor);
            pressio_options_free(metrics_results);
            pressio_options_free(pressio_options);
            pressio_data_free(input_pdata);
            pressio_data_free(compressed_pdata);
            pressio_data_free(output_pdata);
            pressio_release(library);
            libc::free(metrics_ptr as *mut c_void);
        }
    }
}
