#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::ffi::{CStr, CString, c_void};

#[derive(Debug, Clone)]
pub struct PressioError {
    pub error_code: i32,
    pub message: String,
}

impl From<&Pressio> for PressioError {
    fn from(library: &Pressio) -> Self {
        let error_code = unsafe { libpressio_sys::pressio_error_code(library.library) };
        let message = unsafe {
            let message = libpressio_sys::pressio_error_msg(library.library);
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

impl From<PressioCompressor> for PressioError {
    fn from(library: PressioCompressor) -> Self {
        let error_code = unsafe { libpressio_sys::pressio_compressor_error_code(library.ptr) };
        let message = unsafe {
            let message = libpressio_sys::pressio_compressor_error_msg(library.ptr);
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
impl From<&PressioCompressor> for PressioError {
    fn from(library: &PressioCompressor) -> Self {
        let error_code = unsafe { libpressio_sys::pressio_compressor_error_code(library.ptr) };
        let message = unsafe {
            let message = libpressio_sys::pressio_compressor_error_msg(library.ptr);
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

impl From<std::str::Utf8Error> for PressioError {
    fn from(_: std::str::Utf8Error) -> Self {
        PressioError {
            error_code: 2,
            message: "utf8 error".to_string(),
        }
    }
}

impl From<std::ffi::NulError> for PressioError {
    fn from(_: std::ffi::NulError) -> Self {
        PressioError {
            error_code: 1,
            message: "nul error".to_string(),
        }
    }
}

pub struct Pressio {
    library: *mut libpressio_sys::pressio,
}

pub struct PressioCompressor {
    ptr: *mut libpressio_sys::pressio_compressor,
}
impl PressioCompressor {
    pub fn compress(
        &self,
        input_data: &PressioData,
        compressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_compress(
                self.ptr,
                input_data.data,
                compressed_data.data,
            )
        };
        if rc == 0 {
            Ok(compressed_data)
        } else {
            Err(self.into())
        }
    }

    pub fn decompress(
        &self,
        compressed_data: &PressioData,
        decompressed_data: PressioData,
    ) -> Result<PressioData, PressioError> {
        let rc = unsafe {
            libpressio_sys::pressio_compressor_decompress(
                self.ptr,
                compressed_data.data,
                decompressed_data.data,
            )
        };
        if rc == 0 {
            Ok(decompressed_data)
        } else {
            Err(self.into())
        }
    }

    pub fn set_options(&self, options: &PressioOptions) -> Result<(), PressioError> {
        let rc = unsafe { libpressio_sys::pressio_compressor_set_options(self.ptr, options.ptr) };
        if rc == 0 { Ok(()) } else { Err(self.into()) }
    }

    pub fn get_options(&self) -> Result<PressioOptions, PressioError> {
        let options = unsafe { libpressio_sys::pressio_compressor_get_options(self.ptr) };
        if !options.is_null() {
            Ok(PressioOptions::from_raw(options))
        } else {
            Err(self.into())
        }
    }

    pub fn get_metric_results(&self) -> Result<PressioOptions, PressioError> {
        let ptr = unsafe { libpressio_sys::pressio_compressor_get_metrics_results(self.ptr) };
        if !ptr.is_null() {
            Ok(PressioOptions::from_raw(ptr))
        } else {
            Err(self.into())
        }
    }
}

impl Drop for PressioCompressor {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_compressor_release(self.ptr);
        }
    }
}

impl Drop for Pressio {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_release(self.library);
        }
    }
}
impl Pressio {
    pub fn new() -> Result<Pressio, PressioError> {
        let library: *mut libpressio_sys::pressio;
        unsafe {
            library = libpressio_sys::pressio_instance();
        }
        if !library.is_null() {
            Ok(Pressio { library })
        } else {
            Err(PressioError {
                error_code: 1,
                message: "failed to init library".to_string(),
            })
        }
    }

    pub fn get_compressor<S: AsRef<str>>(&self, id: S) -> Result<PressioCompressor, PressioError> {
        let id = CString::new(id.as_ref())?;
        let ptr = unsafe { libpressio_sys::pressio_get_compressor(self.library, id.as_ptr()) };
        if !ptr.is_null() {
            Ok(PressioCompressor { ptr })
        } else {
            Err(self.into())
        }
    }
}

pub struct PressioData {
    data: *mut libpressio_sys::pressio_data,
}

impl PressioData {
    pub fn new_empty<D: AsRef<[usize]>>(
        dtype: libpressio_sys::pressio_dtype,
        dims: D,
    ) -> PressioData {
        let dim_arr = dims.as_ref();
        let data = unsafe {
            libpressio_sys::pressio_data_new_empty(dtype, dim_arr.len(), dim_arr.as_ptr())
        };
        PressioData { data }
    }
}
impl Clone for PressioData {
    fn clone(&self) -> PressioData {
        PressioData {
            data: unsafe { libpressio_sys::pressio_data_new_clone(self.data) },
        }
    }
}
impl From<ndarray::ArrayD<f32>> for PressioData {
    fn from(mut input_array: ndarray::ArrayD<f32>) -> Self {
        let data = unsafe {
            libpressio_sys::pressio_data_new_copy(
                libpressio_sys::pressio_dtype_pressio_float_dtype,
                input_array.as_mut_ptr() as *mut c_void,
                input_array.ndim(),
                input_array.shape().as_ptr(),
            )
        };
        PressioData { data }
    }
}
impl Drop for PressioData {
    fn drop(&mut self) {
        unsafe {
            libpressio_sys::pressio_data_free(self.data);
        }
    }
}

pub enum PressioOption {
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
        let lib = Pressio::new().expect("failed to create library");
        eprintln!("supported compressors: {:}", unsafe {CStr::from_ptr(libpressio_sys::pressio_supported_compressors()).to_str()?});
        let compressor = lib.get_compressor("pressio").expect("expected compressor");

        let options = PressioOptions::new()?
            .set("pressio:lossless", PressioOption::int32(Some(1)))?
            .set("pressio:metric", PressioOption::string(Some("size".to_string())))?;

        let input_pdata: PressioData = input_data().into();
        let compressed_data =
            PressioData::new_empty(libpressio_sys::pressio_dtype_pressio_byte_dtype, []);
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
            pressio_options_set_string(pressio_options, pressio_metric.as_ptr(), pressio_metric_value.as_ptr());
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
