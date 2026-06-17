use std::os::raw::{c_char, c_int};

// Opaque types
#[repr(C)]
pub struct TfLiteModel { _unused: [u8; 0] }
#[repr(C)]
pub struct TfLiteInterpreterOptions { _unused: [u8; 0] }
#[repr(C)]
pub struct TfLiteInterpreter { _unused: [u8; 0] }
#[repr(C)]
pub struct TfLiteTensor { _unused: [u8; 0] }

unsafe extern "C" {
    pub fn TfLiteModelCreateFromFile(model_path: *const c_char) -> *mut TfLiteModel;
    pub fn TfLiteModelDelete(model: *mut TfLiteModel);

    pub fn TfLiteInterpreterOptionsCreate() -> *mut TfLiteInterpreterOptions;
    pub fn TfLiteInterpreterOptionsDelete(options: *mut TfLiteInterpreterOptions);
    pub fn TfLiteInterpreterOptionsSetNumThreads(options: *mut TfLiteInterpreterOptions, num_threads: i32);

    pub fn TfLiteInterpreterCreate(model: *const TfLiteModel, options: *const TfLiteInterpreterOptions) -> *mut TfLiteInterpreter;
    pub fn TfLiteInterpreterDelete(interpreter: *mut TfLiteInterpreter);
    pub fn TfLiteInterpreterAllocateTensors(interpreter: *mut TfLiteInterpreter) -> i32;
    pub fn TfLiteInterpreterInvoke(interpreter: *mut TfLiteInterpreter) -> i32;

    pub fn TfLiteInterpreterGetInputTensorCount(interpreter: *const TfLiteInterpreter) -> i32;
    pub fn TfLiteInterpreterGetInputTensor(interpreter: *const TfLiteInterpreter, input_index: i32) -> *mut TfLiteTensor;

    pub fn TfLiteInterpreterGetOutputTensorCount(interpreter: *const TfLiteInterpreter) -> i32;
    pub fn TfLiteInterpreterGetOutputTensor(interpreter: *const TfLiteInterpreter, output_index: i32) -> *const TfLiteTensor;

    pub fn TfLiteInterpreterResizeInputTensor(
        interpreter: *mut TfLiteInterpreter,
        input_index: i32,
        dims: *const i32,
        dims_count: i32,
    ) -> i32;

    pub fn TfLiteTensorCopyFromBuffer(
        tensor: *mut TfLiteTensor,
        input_data: *const std::ffi::c_void,
        input_data_size: usize,
    ) -> i32;

    pub fn TfLiteTensorCopyToBuffer(
        tensor: *const TfLiteTensor,
        output_data: *mut std::ffi::c_void,
        output_data_size: usize,
    ) -> i32;

    pub fn TfLiteTensorByteSize(tensor: *const TfLiteTensor) -> usize;
    pub fn TfLiteTensorName(tensor: *const TfLiteTensor) -> *const c_char;
    pub fn TfLiteTensorType(tensor: *const TfLiteTensor) -> i32;
}

#[allow(non_upper_case_globals)]
pub const kTfLiteInt32: i32 = 2;
#[allow(non_upper_case_globals)]
pub const kTfLiteInt64: i32 = 4;

