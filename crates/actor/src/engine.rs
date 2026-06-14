use std::sync::{Arc, Mutex};
use std::ffi::{CStr, CString};
use crate::pipeline::PipelineOutput;
use crate::asset_manager::StyleVector;
use crate::tflite;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActorEngineOutput {
    pub audio: Vec<f32>,
    pub pred_dur: Vec<i32>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SpeechEngineError {
    #[error("inference error: {message}")]
    Inference { message: String },
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaSpeechEngine: Send + Sync {
    fn synthesize(&self, input: PipelineOutput) -> ActorEngineOutput;
    
    fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError>;
    
    fn reclaim_memory(&self);
}

#[uniffi::export(callback_interface)]
pub trait AudioSink: Send + Sync {
    fn schedule_audio(&self, audio: Vec<f32>, sample_rate: u32);
}

#[derive(uniffi::Object)]
pub struct ProsodiaActorEngine {
    pub pipeline: Arc<crate::pipeline::ProsodiaActorPipeline>,
    pub speech_engine: Box<dyn ProsodiaSpeechEngine>,
}

#[uniffi::export]
impl ProsodiaActorEngine {
    #[uniffi::constructor]
    pub fn new(pipeline: Arc<crate::pipeline::ProsodiaActorPipeline>, speech_engine: Box<dyn ProsodiaSpeechEngine>) -> Arc<Self> {
        Arc::new(Self { pipeline, speech_engine })
    }

    pub fn process_and_synthesize(&self, span: stage::prosody_payload::ProsodySpan) -> ActorEngineOutput {
        let pipeline_out = self.pipeline.process_span(span);
        self.speech_engine.synthesize(pipeline_out)
    }

    pub fn reclaim_memory(&self) {
        self.speech_engine.reclaim_memory();
    }
}

struct InterpreterWrapper {
    model: *mut tflite::TfLiteModel,
    options: *mut tflite::TfLiteInterpreterOptions,
    interpreter: *mut tflite::TfLiteInterpreter,
    last_phoneme_length: usize,
}

unsafe impl Send for InterpreterWrapper {}
unsafe impl Sync for InterpreterWrapper {}

impl Drop for InterpreterWrapper {
    fn drop(&mut self) {
        unsafe {
            if !self.interpreter.is_null() {
                tflite::TfLiteInterpreterDelete(self.interpreter);
            }
            if !self.options.is_null() {
                tflite::TfLiteInterpreterOptionsDelete(self.options);
            }
            if !self.model.is_null() {
                tflite::TfLiteModelDelete(self.model);
            }
        }
    }
}

/// A ``ProsodiaSpeechEngine`` powered by the Google LiteRT (TensorFlow Lite) runtime.
#[derive(uniffi::Object)]
pub struct LiteRtActorEngine {
    model_path: String,
    inner: Mutex<Option<InterpreterWrapper>>,
}

#[uniffi::export]
impl LiteRtActorEngine {
    #[uniffi::constructor]
    pub fn new(model_path: String) -> Arc<Self> {
        Arc::new(Self {
            model_path,
            inner: Mutex::new(None),
        })
    }
}

impl LiteRtActorEngine {
    fn get_or_init_interpreter(&self) -> Result<std::sync::MutexGuard<'_, Option<InterpreterWrapper>>, SpeechEngineError> {
        let mut wrapper_lock = self.inner.lock().unwrap();
        if wrapper_lock.is_some() {
            return Ok(wrapper_lock);
        }

        unsafe {
            let model_path_c = CString::new(self.model_path.as_str())
                .map_err(|e| SpeechEngineError::Inference { message: format!("Invalid model path: {}", e) })?;

            let model = tflite::TfLiteModelCreateFromFile(model_path_c.as_ptr());
            if model.is_null() {
                return Err(SpeechEngineError::Inference { message: format!("Failed to load model from {}", self.model_path) });
            }

            let options = tflite::TfLiteInterpreterOptionsCreate();
            if options.is_null() {
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { message: "Failed to create interpreter options".to_string() });
            }

            tflite::TfLiteInterpreterOptionsSetNumThreads(options, 4);

            let interpreter = tflite::TfLiteInterpreterCreate(model, options);
            if interpreter.is_null() {
                tflite::TfLiteInterpreterOptionsDelete(options);
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { message: "Failed to create interpreter".to_string() });
            }

            let status = tflite::TfLiteInterpreterAllocateTensors(interpreter);
            if status != 0 {
                tflite::TfLiteInterpreterDelete(interpreter);
                tflite::TfLiteInterpreterOptionsDelete(options);
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { message: format!("Failed to allocate tensors (status {})", status) });
            }

            *wrapper_lock = Some(InterpreterWrapper {
                model,
                options,
                interpreter,
                last_phoneme_length: 0,
            });

            Ok(wrapper_lock)
        }
    }

    fn forward_impl(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        _duration_scales: Option<Vec<f32>>,
        _f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        let mut wrapper_guard = self.get_or_init_interpreter()?;
        let wrapper = wrapper_guard.as_mut().unwrap();

        unsafe {
            let interpreter = wrapper.interpreter;
            let token_count = phoneme_ids.len();

            // 1. Identify input tensor indices by matching names
            let input_count = tflite::TfLiteInterpreterGetInputTensorCount(interpreter);
            let mut phonemes_index: i32 = -1;
            let mut style_index: i32 = -1;
            let mut speed_index: i32 = -1;
            let mut vat_index: i32 = -1;

            for i in 0..input_count {
                let tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, i);
                if tensor.is_null() {
                    continue;
                }
                let name_ptr = tflite::TfLiteTensorName(tensor);
                if name_ptr.is_null() {
                    continue;
                }
                let name = CStr::from_ptr(name_ptr).to_string_lossy().to_lowercase();

                if name.contains("phone") || name.contains("input_ids") || name.contains("text") {
                    phonemes_index = i;
                } else if name.contains("style") || name.contains("ref") {
                    style_index = i;
                } else if name.contains("speed") || name.contains("tempo") {
                    if !name.contains("vat") {
                        speed_index = i;
                    }
                } else if name.contains("vat") || name.contains("emotion") || name.contains("control") {
                    vat_index = i;
                }
            }

            if phonemes_index == -1 {
                return Err(SpeechEngineError::Inference {
                    message: "LiteRT actor model lacks expected phonemes input tensor.".to_string(),
                });
            }

            // 2. Dynamically resize phonemes tensor if text length changed
            if token_count != wrapper.last_phoneme_length {
                let dims = [1, token_count as i32];
                let status = tflite::TfLiteInterpreterResizeInputTensor(
                    interpreter,
                    phonemes_index,
                    dims.as_ptr(),
                    2,
                );
                if status != 0 {
                    return Err(SpeechEngineError::Inference {
                        message: format!("Failed to resize TFLite phoneme tensor to {} (status: {})", token_count, status),
                    });
                }
                let alloc_status = tflite::TfLiteInterpreterAllocateTensors(interpreter);
                if alloc_status != 0 {
                    return Err(SpeechEngineError::Inference {
                        message: format!("Failed to re-allocate TFLite tensors after resize (status: {})", alloc_status),
                    });
                }
                wrapper.last_phoneme_length = token_count;
            }

            // 3. Copy data into input tensors
            // A. Phoneme IDs
            let phonemes_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, phonemes_index);
            if !phonemes_tensor.is_null() {
                let size = phoneme_ids.len() * std::mem::size_of::<i32>();
                let status = tflite::TfLiteTensorCopyFromBuffer(
                    phonemes_tensor,
                    phoneme_ids.as_ptr() as *const std::ffi::c_void,
                    size,
                );
                if status != 0 {
                    return Err(SpeechEngineError::Inference {
                        message: format!("Failed to copy phoneme IDs to TFLite input (status: {})", status),
                    });
                }
            }

            // B. Style Vectors
            if style_index != -1 {
                let style_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, style_index);
                if !style_tensor.is_null() {
                    let size = style.data.len() * std::mem::size_of::<f32>();
                    tflite::TfLiteTensorCopyFromBuffer(
                        style_tensor,
                        style.data.as_ptr() as *const std::ffi::c_void,
                        size,
                    );
                }
            }

            // C. Speed
            if speed_index != -1 {
                let speed_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, speed_index);
                if !speed_tensor.is_null() {
                    let speed_val = speed;
                    tflite::TfLiteTensorCopyFromBuffer(
                        speed_tensor,
                        &speed_val as *const f32 as *const std::ffi::c_void,
                        std::mem::size_of::<f32>(),
                    );
                }
            }

            // D. Emotion VAT
            if vat_index != -1 {
                let vat_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, vat_index);
                if !vat_tensor.is_null() {
                    let vat_data: [f32; 3] = [0.5, 0.5, 0.5];
                    tflite::TfLiteTensorCopyFromBuffer(
                        vat_tensor,
                        vat_data.as_ptr() as *const std::ffi::c_void,
                        vat_data.len() * std::mem::size_of::<f32>(),
                    );
                }
            }

            // 4. Invoke Inference
            let invoke_status = tflite::TfLiteInterpreterInvoke(interpreter);
            if invoke_status != 0 {
                return Err(SpeechEngineError::Inference {
                    message: format!("TFLite interpreter execution failed (status: {})", invoke_status),
                });
            }

            // 5. Extract output buffer PCM floats
            let output_count = tflite::TfLiteInterpreterGetOutputTensorCount(interpreter);
            if output_count == 0 {
                return Err(SpeechEngineError::Inference {
                    message: "LiteRT model returned no output tensors.".to_string(),
                });
            }
            let out_tensor = tflite::TfLiteInterpreterGetOutputTensor(interpreter, 0);
            if out_tensor.is_null() {
                return Err(SpeechEngineError::Inference {
                    message: "Failed to get output tensor 0.".to_string(),
                });
            }

            let byte_size = tflite::TfLiteTensorByteSize(out_tensor);
            let element_count = byte_size / std::mem::size_of::<f32>();
            let mut output_pcm = vec![0.0f32; element_count];

            let copy_status = tflite::TfLiteTensorCopyToBuffer(
                out_tensor,
                output_pcm.as_mut_ptr() as *mut std::ffi::c_void,
                byte_size,
            );
            if copy_status != 0 {
                return Err(SpeechEngineError::Inference {
                    message: format!("Failed to copy PCM data out of TFLite output tensor (status: {})", copy_status),
                });
            }

            let dummy_durations = vec![8i32; token_count];
            Ok(ActorEngineOutput {
                audio: output_pcm,
                pred_dur: dummy_durations,
            })
        }
    }

    fn reclaim_memory_impl(&self) {
        *self.inner.lock().unwrap() = None;
    }
}

#[uniffi::export]
impl LiteRtActorEngine {
    pub fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, duration_scales, f0_bias)
    }

    pub fn reclaim_memory(&self) {
        self.reclaim_memory_impl();
    }
}

impl ProsodiaSpeechEngine for LiteRtActorEngine {
    fn synthesize(&self, _input: PipelineOutput) -> ActorEngineOutput {
        panic!("synthesize(input:) is deprecated, use forward instead");
    }

    fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, duration_scales, f0_bias)
    }

    fn reclaim_memory(&self) {
        self.reclaim_memory_impl();
    }
}
