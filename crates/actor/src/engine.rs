use std::sync::{Arc, Mutex};
use std::ffi::{CStr, CString};
use crate::pipeline::PipelineOutput;
use crate::asset_manager::StyleVector;
use crate::tflite;

const MATCHA_CFM_TEMPERATURE: f32 = 0.667;
const STYLETTS2_HOP_SIZE: f64 = 512.0;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActorEngineOutput {
    pub audio: Vec<f32>,
    pub pred_dur: Vec<i32>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SpeechEngineError {
    #[error("inference error: {msg}")]
    Inference { msg: String },
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaSpeechEngine: Send + Sync {
    fn synthesize(&self, input: PipelineOutput) -> ActorEngineOutput;
    
    fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        vat: Option<Vec<f32>>,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError>;
    
    fn reclaim_memory(&self);

    fn is_matcha(&self) -> bool {
        false
    }

    fn get_token_limit(&self) -> i32 {
        510
    }
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
        let is_matcha = self.speech_engine.is_matcha();
        let pipeline_out = self.pipeline.process_span(span.clone());
        
        let mut chunk_phonemes = String::new();
        for tp in &pipeline_out.phonemes {
            chunk_phonemes.push_str(&tp.phonemes);
            chunk_phonemes.push_str(&tp.whitespace);
        }
        let trimmed_phonemes = chunk_phonemes.trim().to_string();
        let phoneme_ids = self.pipeline.tokenize_phonemes(trimmed_phonemes, is_matcha);
        
        let vat = Some(vec![
            span.emotion.valence as f32,
            span.emotion.arousal as f32,
            span.emotion.tension as f32,
        ]);
        
        let duration_scales = span.acoustics.as_ref().and_then(|a| {
            a.token_duration_scales.as_ref().map(|v| v.iter().map(|&x| x as f32).collect())
        });
        
        let f0_bias = span.acoustics.as_ref().and_then(|a| {
            a.token_f0_biases.as_ref().map(|v| v.iter().map(|&x| x as f32).collect())
        });
        
        self.speech_engine.forward(
            phoneme_ids,
            pipeline_out.style,
            pipeline_out.speed_multiplier as f32,
            vat,
            duration_scales,
            f0_bias,
        ).unwrap_or_else(|_e| {
            ActorEngineOutput {
                audio: Vec::new(),
                pred_dur: Vec::new(),
            }
        })
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
    is_matcha: bool,
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
                .map_err(|e| SpeechEngineError::Inference { msg: format!("Invalid model path: {}", e) })?;

            let model = tflite::TfLiteModelCreateFromFile(model_path_c.as_ptr());
            if model.is_null() {
                return Err(SpeechEngineError::Inference { msg: format!("Failed to load model from {}", self.model_path) });
            }

            let options = tflite::TfLiteInterpreterOptionsCreate();
            if options.is_null() {
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { msg: "Failed to create interpreter options".to_string() });
            }

            tflite::TfLiteInterpreterOptionsSetNumThreads(options, 4);

            let interpreter = tflite::TfLiteInterpreterCreate(model, options);
            if interpreter.is_null() {
                tflite::TfLiteInterpreterOptionsDelete(options);
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { msg: "Failed to create interpreter".to_string() });
            }

            let status = tflite::TfLiteInterpreterAllocateTensors(interpreter);
            if status != 0 {
                tflite::TfLiteInterpreterDelete(interpreter);
                tflite::TfLiteInterpreterOptionsDelete(options);
                tflite::TfLiteModelDelete(model);
                return Err(SpeechEngineError::Inference { msg: format!("Failed to allocate tensors (status {})", status) });
            }

            // Detect if this is a Matcha model by checking for input names
            let input_count = tflite::TfLiteInterpreterGetInputTensorCount(interpreter);
            let mut has_x = false;
            let mut has_x_lengths = false;
            let mut has_scales = false;

            for i in 0..input_count {
                let tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, i);
                if !tensor.is_null() {
                    let name_ptr = tflite::TfLiteTensorName(tensor);
                    if !name_ptr.is_null() {
                        let name = CStr::from_ptr(name_ptr).to_string_lossy().to_lowercase();
                        if name == "x" {
                            has_x = true;
                        } else if name.contains("x_lengths") {
                            has_x_lengths = true;
                        } else if name == "scales" {
                            has_scales = true;
                        }
                    }
                }
            }

            let is_matcha = has_x && has_x_lengths && has_scales;

            *wrapper_lock = Some(InterpreterWrapper {
                model,
                options,
                interpreter,
                last_phoneme_length: 0,
                is_matcha,
            });

            Ok(wrapper_lock)
        }
    }

    fn forward_impl(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        vat: Option<Vec<f32>>,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        let mut wrapper_guard = self.get_or_init_interpreter()?;
        let wrapper = wrapper_guard.as_mut().unwrap();

        unsafe {
            let interpreter = wrapper.interpreter;
            let token_count = phoneme_ids.len();
            let is_matcha = wrapper.is_matcha;

            // 1. Identify input tensor indices by matching names
            let input_count = tflite::TfLiteInterpreterGetInputTensorCount(interpreter);
            let mut phonemes_index: i32 = -1;
            let mut style_index: i32 = -1;
            let mut speed_index: i32 = -1;
            let mut vat_index: i32 = -1;
            let mut x_lengths_index: i32 = -1;
            let mut scales_index: i32 = -1;
            let mut duration_scales_index: i32 = -1;
            let mut f0_bias_index: i32 = -1;

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

                if name == "x" {
                    phonemes_index = i;
                } else if name.contains("x_lengths") {
                    x_lengths_index = i;
                } else if name == "scales" {
                    scales_index = i;
                } else if name.contains("phone") || name.contains("input_ids") || name.contains("text") {
                    phonemes_index = i;
                } else if name.contains("style") || name.contains("ref") {
                    style_index = i;
                } else if name.contains("speed") || name.contains("tempo") {
                    if !name.contains("vat") {
                        speed_index = i;
                    }
                } else if name.contains("vat") || name.contains("emotion") || name.contains("control") {
                    vat_index = i;
                } else if name.contains("duration_scale") || name.contains("dur_scale") {
                    duration_scales_index = i;
                } else if name.contains("f0_bias") || name.contains("pitch_bias") {
                    f0_bias_index = i;
                }
            }

            if phonemes_index == -1 {
                return Err(SpeechEngineError::Inference {
                    msg: "LiteRT actor model lacks expected phonemes/x input tensor.".to_string(),
                });
            }

            // 2. Handle input tensor sizing
            if is_matcha {
                // Matcha uses static compiled size, we pad rather than resize.
                let phonemes_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, phonemes_index);
                if phonemes_tensor.is_null() {
                    return Err(SpeechEngineError::Inference {
                        msg: "Failed to get phonemes tensor".to_string(),
                    });
                }
                let byte_size = tflite::TfLiteTensorByteSize(phonemes_tensor);
                let dtype = tflite::TfLiteTensorType(phonemes_tensor);
                let element_size = if dtype == tflite::kTfLiteInt64 { 8 } else { 4 };
                let static_limit = byte_size / element_size;

                if token_count > static_limit {
                    return Err(SpeechEngineError::Inference {
                        msg: format!(
                            "Input token count ({}) exceeds the model's static limit ({})",
                            token_count, static_limit
                        ),
                    });
                }

                if element_size == 8 {
                    let mut phoneme_ids_i64 = vec![0i64; static_limit];
                    for j in 0..token_count {
                        phoneme_ids_i64[j] = phoneme_ids[j] as i64;
                    }
                    let status = tflite::TfLiteTensorCopyFromBuffer(
                        phonemes_tensor,
                        phoneme_ids_i64.as_ptr() as *const std::ffi::c_void,
                        byte_size,
                    );
                    if status != 0 {
                        return Err(SpeechEngineError::Inference {
                            msg: format!("Failed to copy phoneme IDs to TFLite input (status: {})", status),
                        });
                    }
                } else {
                    let mut phoneme_ids_i32 = vec![0i32; static_limit];
                    for j in 0..token_count {
                        phoneme_ids_i32[j] = phoneme_ids[j];
                    }
                    let status = tflite::TfLiteTensorCopyFromBuffer(
                        phonemes_tensor,
                        phoneme_ids_i32.as_ptr() as *const std::ffi::c_void,
                        byte_size,
                    );
                    if status != 0 {
                        return Err(SpeechEngineError::Inference {
                            msg: format!("Failed to copy phoneme IDs to TFLite input (status: {})", status),
                        });
                    }
                }

                // Copy x_lengths
                if x_lengths_index != -1 {
                    let lengths_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, x_lengths_index);
                    if !lengths_tensor.is_null() {
                        let byte_size = tflite::TfLiteTensorByteSize(lengths_tensor);
                        if byte_size == 8 {
                            let val = [token_count as i64];
                            tflite::TfLiteTensorCopyFromBuffer(
                                lengths_tensor,
                                val.as_ptr() as *const std::ffi::c_void,
                                8,
                            );
                        } else {
                            let val = [token_count as i32];
                            tflite::TfLiteTensorCopyFromBuffer(
                                lengths_tensor,
                                val.as_ptr() as *const std::ffi::c_void,
                                4,
                            );
                        }
                    }
                }

                // Copy scales
                if scales_index != -1 {
                    let scales_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, scales_index);
                    if !scales_tensor.is_null() {
                        let byte_size = tflite::TfLiteTensorByteSize(scales_tensor);
                        let count = byte_size / std::mem::size_of::<f32>();
                        let mut scale_vals = vec![MATCHA_CFM_TEMPERATURE; count];
                        if count >= 2 {
                            scale_vals[1] = 1.0 / speed;
                        }
                        tflite::TfLiteTensorCopyFromBuffer(
                            scales_tensor,
                            scale_vals.as_ptr() as *const std::ffi::c_void,
                            byte_size,
                        );
                    }
                }
            } else {
                // StyleTTS2 supports dynamic resizing
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
                            msg: format!("Failed to resize TFLite phoneme tensor to {} (status: {})", token_count, status),
                        });
                    }
                    if duration_scales_index != -1 {
                        tflite::TfLiteInterpreterResizeInputTensor(
                            interpreter,
                            duration_scales_index,
                            dims.as_ptr(),
                            2,
                        );
                    }
                    if f0_bias_index != -1 {
                        tflite::TfLiteInterpreterResizeInputTensor(
                            interpreter,
                            f0_bias_index,
                            dims.as_ptr(),
                            2,
                        );
                    }
                    let alloc_status = tflite::TfLiteInterpreterAllocateTensors(interpreter);
                    if alloc_status != 0 {
                        return Err(SpeechEngineError::Inference {
                            msg: format!("Failed to re-allocate TFLite tensors after resize (status: {})", alloc_status),
                        });
                    }
                    wrapper.last_phoneme_length = token_count;
                }

                // Copy phoneme IDs
                let phonemes_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, phonemes_index);
                if !phonemes_tensor.is_null() {
                    let byte_size = tflite::TfLiteTensorByteSize(phonemes_tensor);
                    let status = tflite::TfLiteTensorCopyFromBuffer(
                        phonemes_tensor,
                        phoneme_ids.as_ptr() as *const std::ffi::c_void,
                        byte_size,
                    );
                    if status != 0 {
                        return Err(SpeechEngineError::Inference {
                            msg: format!("Failed to copy phoneme IDs to TFLite input (status: {})", status),
                        });
                    }
                }

                // Copy Style Vectors
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

                // Copy Speed
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

                // Copy Emotion VAT
                if vat_index != -1 {
                    let vat_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, vat_index);
                    if !vat_tensor.is_null() {
                        let vat_data = match vat {
                            Some(ref v) if v.len() == 3 => [v[0], v[1], v[2]],
                            _ => [0.5, 0.5, 0.5],
                        };
                        tflite::TfLiteTensorCopyFromBuffer(
                            vat_tensor,
                            vat_data.as_ptr() as *const std::ffi::c_void,
                            vat_data.len() * std::mem::size_of::<f32>(),
                        );
                    }
                }

                // Copy duration scales
                if duration_scales_index != -1 {
                    let tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, duration_scales_index);
                    if !tensor.is_null() {
                        let mut data = vec![1.0f32; token_count];
                        if let Some(ref ds) = duration_scales {
                            for (j, &val) in ds.iter().enumerate().take(token_count) {
                                data[j] = val;
                            }
                        }
                        let byte_size = tflite::TfLiteTensorByteSize(tensor);
                        tflite::TfLiteTensorCopyFromBuffer(
                            tensor,
                            data.as_ptr() as *const std::ffi::c_void,
                            byte_size,
                        );
                    }
                }

                // Copy F0 bias
                if f0_bias_index != -1 {
                    let tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, f0_bias_index);
                    if !tensor.is_null() {
                        let mut data = vec![0.0f32; token_count];
                        if let Some(ref fb) = f0_bias {
                            for (j, &val) in fb.iter().enumerate().take(token_count) {
                                data[j] = val;
                            }
                        }
                        let byte_size = tflite::TfLiteTensorByteSize(tensor);
                        tflite::TfLiteTensorCopyFromBuffer(
                            tensor,
                            data.as_ptr() as *const std::ffi::c_void,
                            byte_size,
                        );
                    }
                }
            }

            // 3. Invoke Inference
            let invoke_status = tflite::TfLiteInterpreterInvoke(interpreter);
            if invoke_status != 0 {
                return Err(SpeechEngineError::Inference {
                    msg: format!("TFLite interpreter execution failed (status: {})", invoke_status),
                });
            }

            // 4. Extract output buffer PCM floats
            let output_count = tflite::TfLiteInterpreterGetOutputTensorCount(interpreter);
            if output_count == 0 {
                return Err(SpeechEngineError::Inference {
                    msg: "LiteRT model returned no output tensors.".to_string(),
                });
            }

            let mut actual_len = 0usize;
            let mut has_actual_len = false;

            if is_matcha && output_count >= 2 {
                let len_tensor = tflite::TfLiteInterpreterGetOutputTensor(interpreter, 1);
                if !len_tensor.is_null() {
                    let byte_size = tflite::TfLiteTensorByteSize(len_tensor);
                    if byte_size == 8 {
                        let mut len_val = 0i64;
                        let copy_status = tflite::TfLiteTensorCopyToBuffer(
                            len_tensor,
                            &mut len_val as *mut i64 as *mut std::ffi::c_void,
                            8,
                        );
                        if copy_status == 0 {
                            actual_len = len_val as usize;
                            has_actual_len = true;
                        }
                    } else if byte_size == 4 {
                        let mut len_val = 0i32;
                        let copy_status = tflite::TfLiteTensorCopyToBuffer(
                            len_tensor,
                            &mut len_val as *mut i32 as *mut std::ffi::c_void,
                            4,
                        );
                        if copy_status == 0 {
                            actual_len = len_val as usize;
                            has_actual_len = true;
                        }
                    }
                }
            }

            let out_tensor = tflite::TfLiteInterpreterGetOutputTensor(interpreter, 0);
            if out_tensor.is_null() {
                return Err(SpeechEngineError::Inference {
                    msg: "Failed to get output tensor 0.".to_string(),
                });
            }

            let byte_size = tflite::TfLiteTensorByteSize(out_tensor);
            let total_elements = byte_size / std::mem::size_of::<f32>();

            let element_count = if has_actual_len {
                actual_len.min(total_elements)
            } else {
                total_elements
            };

            let mut output_pcm = vec![0.0f32; total_elements];
            let copy_status = tflite::TfLiteTensorCopyToBuffer(
                out_tensor,
                output_pcm.as_mut_ptr() as *mut std::ffi::c_void,
                byte_size,
            );
            if copy_status != 0 {
                return Err(SpeechEngineError::Inference {
                    msg: format!("Failed to copy PCM data out of TFLite output tensor (status: {})", copy_status),
                });
            }

            output_pcm.truncate(element_count);

            let mut pred_dur = vec![8i32; token_count];
            if is_matcha {
                // Resample from 22050 to 24000
                output_pcm = resample_linear(output_pcm, 22050.0, 24000.0);

                // Distribute total frames evenly across phonemes
                let total_frames = (output_pcm.len() as f64 / STYLETTS2_HOP_SIZE) as i32;
                let avg_dur = (total_frames as f32 / token_count as f32).round() as i32;
                pred_dur = vec![avg_dur.max(1); token_count];
            }

            if let Some(ref scales) = duration_scales {
                for (j, &scale) in scales.iter().enumerate().take(pred_dur.len()) {
                    pred_dur[j] = ((pred_dur[j] as f32 * scale).round() as i32).max(1);
                }
            }

            Ok(ActorEngineOutput {
                audio: output_pcm,
                pred_dur,
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
        vat: Option<Vec<f32>>,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, vat, duration_scales, f0_bias)
    }

    pub fn reclaim_memory(&self) {
        self.reclaim_memory_impl();
    }

    pub fn get_token_limit(&self) -> i32 {
        <Self as ProsodiaSpeechEngine>::get_token_limit(self)
    }

    pub fn is_matcha(&self) -> bool {
        <Self as ProsodiaSpeechEngine>::is_matcha(self)
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
        vat: Option<Vec<f32>>,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, vat, duration_scales, f0_bias)
    }

    fn reclaim_memory(&self) {
        self.reclaim_memory_impl();
    }

    fn is_matcha(&self) -> bool {
        if let Ok(guard) = self.get_or_init_interpreter() {
            guard.as_ref().map(|w| w.is_matcha).unwrap_or(false)
        } else {
            false
        }
    }

    fn get_token_limit(&self) -> i32 {
        if let Ok(guard) = self.get_or_init_interpreter() {
            if let Some(ref wrapper) = *guard {
                if wrapper.is_matcha {
                    unsafe {
                        let interpreter = wrapper.interpreter;
                        let input_count = tflite::TfLiteInterpreterGetInputTensorCount(interpreter);
                        let mut phonemes_index: i32 = -1;
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
                            if name == "x" || name.contains("phone") || name.contains("input_ids") || name.contains("text") {
                                phonemes_index = i;
                                break;
                            }
                        }
                        if phonemes_index != -1 {
                            let phonemes_tensor = tflite::TfLiteInterpreterGetInputTensor(interpreter, phonemes_index);
                            if !phonemes_tensor.is_null() {
                                let byte_size = tflite::TfLiteTensorByteSize(phonemes_tensor);
                                let dtype = tflite::TfLiteTensorType(phonemes_tensor);
                                let element_size = if dtype == tflite::kTfLiteInt64 { 8 } else { 4 };
                                let limit = byte_size / element_size;
                                return (limit.saturating_sub(2)) as i32;
                            }
                        }
                    }
                }
            }
        }
        510
    }
}

fn resample_linear(input: Vec<f32>, from_rate: f32, to_rate: f32) -> Vec<f32> {
    if input.is_empty() || (from_rate - to_rate).abs() < 1e-3 {
        return input;
    }
    
    let ratio = from_rate / to_rate;
    let input_len = input.len();
    let output_len = (input_len as f32 * to_rate / from_rate).round() as usize;
    if output_len == 0 {
        return Vec::new();
    }
    
    let mut output = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let t = i as f32 * ratio;
        let t_floor = t.floor() as usize;
        let t_fract = t - t_floor as f32;
        
        if t_floor + 1 < input_len {
            let sample = (1.0 - t_fract) * input[t_floor] + t_fract * input[t_floor + 1];
            output.push(sample);
        } else if t_floor < input_len {
            output.push(input[t_floor]);
        } else {
            output.push(0.0);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_matcha_stock_forward() {
        let model_path = "../../Models/matcha_stock.tflite";
        if !Path::new(model_path).exists() {
            println!("Skipping test: {} not found", model_path);
            return;
        }

        let engine = LiteRtActorEngine::new(model_path.to_string());
        assert!(engine.is_matcha(), "Expected loaded model to be detected as Matcha");

        let phoneme_ids = vec![12, 15, 18, 5, 9];
        let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };
        
        let output = engine.forward(
            phoneme_ids.clone(),
            style,
            1.0,
            None,
            None,
            None,
        ).expect("Forward execution failed");

        assert!(!output.audio.is_empty(), "Expected non-empty output audio");
        assert_eq!(output.pred_dur.len(), phoneme_ids.len(), "Expected pred_dur to match phoneme count");
    }

    #[test]
    fn test_resample_linear() {
        let input = vec![0.0; 100];
        let output = resample_linear(input.clone(), 22050.0, 24000.0);
        assert!(!output.is_empty());
        assert_eq!(output.len(), 109);
    }
}
