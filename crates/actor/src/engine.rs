use std::sync::{Arc, Mutex};
use std::ffi::{CStr, CString};
use crate::pipeline::PipelineOutput;
use crate::asset_manager::StyleVector;
use crate::tflite;

const MATCHA_CFM_TEMPERATURE: f32 = 0.667;
const STYLETTS2_HOP_SIZE: f64 = 512.0;
const DEFAULT_TOKEN_DURATION: i32 = 8;
const DEFAULT_VAT: [f32; 3] = [0.5, 0.5, 0.5];

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

    pub fn process_and_synthesize(&self, span: stage::prosody_payload::ProsodySpan) -> Result<ActorEngineOutput, SpeechEngineError> {
        let is_matcha = self.speech_engine.is_matcha();
        let pipeline_out = self.pipeline.process_span(span.clone());

        let vat = Some(vec![
            span.emotion.valence as f32,
            span.emotion.arousal as f32,
            span.emotion.tension as f32,
        ]);

        let duration_scales: Option<Vec<f32>> = span.acoustics.as_ref().and_then(|a| {
            a.token_duration_scales.as_ref().map(|v| v.iter().map(|&x| x as f32).collect())
        });

        let f0_bias: Option<Vec<f32>> = span.acoustics.as_ref().and_then(|a| {
            a.token_f0_biases.as_ref().map(|v| v.iter().map(|&x| x as f32).collect())
        });

        // Group the span's G2P tokens into chunks that fit the engine's static
        // token limit (the e2e TFLite export bakes a [1, 50] phoneme input, so a
        // typical sentence overflows a single forward pass). Each chunk is
        // synthesized separately and the audio concatenated. A limit of 0 means
        // unbounded.
        let token_limit = self.speech_engine.get_token_limit().max(0) as usize;
        let mut chunks: Vec<String> = Vec::new();
        let mut current = String::new();
        for tp in &pipeline_out.phonemes {
            let mut candidate = current.clone();
            candidate.push_str(&tp.phonemes);
            candidate.push_str(&tp.whitespace);
            let over_limit = token_limit > 0
                && !current.trim().is_empty()
                && self
                    .pipeline
                    .tokenize_phonemes(candidate.trim().to_string(), is_matcha)
                    .len()
                    > token_limit;
            if over_limit {
                chunks.push(current.trim().to_string());
                current = String::new();
                current.push_str(&tp.phonemes);
                current.push_str(&tp.whitespace);
            } else {
                current = candidate;
            }
        }
        if !current.trim().is_empty() {
            chunks.push(current.trim().to_string());
        }

        if chunks.is_empty() {
            return Ok(ActorEngineOutput { audio: Vec::new(), pred_dur: Vec::new() });
        }

        // Per-token duration/F0 arrays are indexed against the whole span's id
        // sequence; chunk-splitting would misalign them, so they only pass
        // through on single-chunk spans. (Today's Matcha e2e graph exposes no
        // such tensors, so nothing is lost when a span splits.)
        let single_chunk = chunks.len() == 1;
        let mut audio: Vec<f32> = Vec::new();
        let mut pred_dur: Vec<i32> = Vec::new();
        for chunk in chunks {
            let phoneme_ids = self.pipeline.tokenize_phonemes(chunk, is_matcha);
            let out = self.speech_engine.forward(
                phoneme_ids,
                pipeline_out.style.clone(),
                pipeline_out.speed_multiplier as f32,
                vat.clone(),
                if single_chunk { duration_scales.clone() } else { None },
                if single_chunk { f0_bias.clone() } else { None },
            )?;
            audio.extend(out.audio);
            pred_dur.extend(out.pred_dur);
        }
        Ok(ActorEngineOutput { audio, pred_dur })
    }

    pub fn reclaim_memory(&self) {
        self.speech_engine.reclaim_memory();
    }
}

struct InterpreterWrapper {
    model: *mut tflite::TfLiteModel,
    options: *mut tflite::TfLiteInterpreterOptions,
    interpreter: *mut tflite::TfLiteInterpreter,
    /// XNNPACK delegate (Apple builds only; null when unavailable).
    /// Must be deleted only after the interpreter.
    delegate: *mut tflite::TfLiteDelegate,
    last_phoneme_length: usize,
    is_matcha: bool,
    sample_rate: u32,
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
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if !self.delegate.is_null() {
                tflite::TfLiteXNNPackDelegateDelete(self.delegate);
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

            // XNNPACK delegate (Apple): optimized f32 kernels — measured ~5x
            // faster per forward than the builtin reference kernels on the
            // Matcha e2e graph (M1 Max). Falls back to the plain interpreter
            // when unavailable; the Linux TFLite build has XNNPACK off.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            let delegate = {
                // TfLiteXNNPackDelegateOptions with num_threads = 4. The struct
                // is version-dependent, but its first field has always been
                // `int32_t num_threads`, and zero is the benign default for every
                // later field — so a zeroed, oversized buffer is a safe stand-in
                // (the delegate reads only its true struct size).
                let mut opts_buf = [0u8; 256];
                opts_buf[..4].copy_from_slice(&4i32.to_ne_bytes());
                let delegate = tflite::TfLiteXNNPackDelegateCreate(
                    opts_buf.as_ptr() as *const std::os::raw::c_void,
                );
                if !delegate.is_null() {
                    tflite::TfLiteInterpreterOptionsAddDelegate(options, delegate);
                }
                delegate
            };
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            let delegate: *mut tflite::TfLiteDelegate = std::ptr::null_mut();

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
            let sample_rate = get_model_sample_rate(&self.model_path, is_matcha);

            *wrapper_lock = Some(InterpreterWrapper {
                model,
                options,
                interpreter,
                delegate,
                last_phoneme_length: 0,
                is_matcha,
                sample_rate,
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
                            _ => DEFAULT_VAT,
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

            let mut pred_dur = vec![DEFAULT_TOKEN_DURATION; token_count];
            if is_matcha {
                let model_sr = wrapper.sample_rate;
                output_pcm = resample_linear(output_pcm, model_sr as f32, 24000.0);

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

fn get_model_sample_rate(model_path: &str, is_matcha: bool) -> u32 {
    let path = std::path::Path::new(model_path);
    if let Some(parent) = path.parent() {
        let config_path = parent.join("config.json");
        if config_path.exists() {
            if let Ok(config_str) = std::fs::read_to_string(config_path) {
                if let Ok(config_json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                    if let Some(sr) = config_json.get("sample_rate").and_then(|v| v.as_u64()) {
                        return sr as u32;
                    }
                }
            }
        }
    }
    if is_matcha {
        22050
    } else {
        24000
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
        let model_path = "../../../Reference/models/matcha_stock.tflite";
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

    /// Regression test for the 50-token static-limit overflow (2026-07-11): a
    /// typical sentence tokenizes past the e2e export's [1, 50] phoneme input,
    /// and `process_and_synthesize` must chunk rather than fail. Runs the real
    /// app render path: G2P → process_span → chunked forward.
    #[test]
    fn test_span_render_chunks_past_static_limit() {
        let model_path = "../../../Reference/models/sonora.tflite";
        let config_path = "../../../Reference/models/config.json";
        if !Path::new(model_path).exists() || !Path::new(config_path).exists() {
            println!("Skipping test: staged model/config not found");
            return;
        }

        struct NoVoices;
        impl crate::voice_loader::VoiceAssetProvider for NoVoices {
            fn load_voice_bytes(&self, _voice_name: String) -> Option<Vec<u8>> { None }
        }

        struct G2pWrap(std::sync::Arc<crate::g2p::ProsodiaSpeech>);
        impl crate::g2p::ProsodiaG2PProcessor for G2pWrap {
            fn process(&self, text: String) -> Vec<crate::g2p::MToken> { self.0.process(text) }
        }

        struct EngineWrap(std::sync::Arc<LiteRtActorEngine>);
        impl ProsodiaSpeechEngine for EngineWrap {
            fn synthesize(&self, input: PipelineOutput) -> ActorEngineOutput {
                <LiteRtActorEngine as ProsodiaSpeechEngine>::synthesize(&self.0, input)
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
                <LiteRtActorEngine as ProsodiaSpeechEngine>::forward(&self.0, phoneme_ids, style, speed, vat, duration_scales, f0_bias)
            }
            fn reclaim_memory(&self) {
                <LiteRtActorEngine as ProsodiaSpeechEngine>::reclaim_memory(&self.0)
            }
            fn is_matcha(&self) -> bool {
                <LiteRtActorEngine as ProsodiaSpeechEngine>::is_matcha(&self.0)
            }
            fn get_token_limit(&self) -> i32 {
                <LiteRtActorEngine as ProsodiaSpeechEngine>::get_token_limit(&self.0)
            }
        }

        let config_json = std::fs::read_to_string(config_path).unwrap();
        let pipeline = crate::pipeline::ProsodiaActorPipeline::new(
            Box::new(G2pWrap(crate::g2p::ProsodiaSpeech::new())),
            crate::voice_loader::VoiceLoader::new(Box::new(NoVoices)),
            config_json,
            24000,
            "en-us".to_string(),
        ).expect("pipeline construction failed");

        let engine = ProsodiaActorEngine {
            pipeline,
            speech_engine: Box::new(EngineWrap(LiteRtActorEngine::new(model_path.to_string()))),
        };

        let span = stage::prosody_payload::ProsodySpan {
            text: "The morning light spilled across the quiet kitchen table.".to_string(),
            emotion: stage::prosody::EmotionVector { valence: 0.0, arousal: 0.0, tension: 0.0 },
            leading_pause: 0.0,
            acoustics: None,
        };

        let out = engine
            .process_and_synthesize(span)
            .expect("span render failed — static-limit chunking regressed?");
        let peak = out.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "app path: {} samples ({:.2}s @24kHz), peak {:.6}",
            out.audio.len(),
            out.audio.len() as f32 / 24000.0,
            peak
        );
        assert!(!out.audio.is_empty(), "expected non-empty audio");
        assert!(peak > 0.01, "output audio is near-silent (peak {})", peak);

        // Listenable artifact for manual audition (gitignored target/ dir):
        // minimal 32-bit-float mono WAV, header written by hand to avoid a
        // dev-dependency for a debug artifact.
        let sr: u32 = 24000;
        let data_len = (out.audio.len() * 4) as u32;
        let mut wav: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(3u16.to_le_bytes()); // IEEE float
        wav.extend(1u16.to_le_bytes()); // mono
        wav.extend(sr.to_le_bytes());
        wav.extend((sr * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(32u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for s in &out.audio {
            wav.extend(s.to_le_bytes());
        }
        if std::fs::write("../../target/span_render_test.wav", &wav).is_ok() {
            println!("wrote ../../target/span_render_test.wav");
        }
    }

    /// TEMP diagnostic: same phrase as the reference-ids test, but through our
    /// Rust G2P + IPA mapping — prints phonemes/ids for diffing and writes
    /// audio for A/B audition against ref_render_test.wav.
    #[test]
    fn test_our_g2p_render_tmp() {
        let model_path = "../../../Reference/models/sonora.tflite";
        let config_path = "../../../Reference/models/config.json";
        if !Path::new(model_path).exists() || !Path::new(config_path).exists() {
            println!("Skipping: staged model/config not found");
            return;
        }
        struct NoVoices;
        impl crate::voice_loader::VoiceAssetProvider for NoVoices {
            fn load_voice_bytes(&self, _voice_name: String) -> Option<Vec<u8>> { None }
        }
        struct G2pWrap(std::sync::Arc<crate::g2p::ProsodiaSpeech>);
        impl crate::g2p::ProsodiaG2PProcessor for G2pWrap {
            fn process(&self, text: String) -> Vec<crate::g2p::MToken> { self.0.process(text) }
        }
        let config_json = std::fs::read_to_string(config_path).unwrap();
        let pipeline = crate::pipeline::ProsodiaActorPipeline::new(
            Box::new(G2pWrap(crate::g2p::ProsodiaSpeech::new())),
            crate::voice_loader::VoiceLoader::new(Box::new(NoVoices)),
            config_json,
            24000,
            "en-us".to_string(),
        ).unwrap();

        let span = stage::prosody_payload::ProsodySpan {
            text: "The morning light.".to_string(),
            emotion: stage::prosody::EmotionVector { valence: 0.0, arousal: 0.0, tension: 0.0 },
            leading_pause: 0.0,
            acoustics: None,
        };
        let out_pipe = pipeline.process_span(span);
        let mut phonemes = String::new();
        for tp in &out_pipe.phonemes {
            phonemes.push_str(&tp.phonemes);
            phonemes.push_str(&tp.whitespace);
        }
        let trimmed = phonemes.trim().to_string();
        println!("our raw phonemes: {:?}", trimmed);
        println!("our mapped IPA:   {:?}", crate::pipeline::map_styletts2_to_matcha_ipa(&trimmed));
        let ids = pipeline.tokenize_phonemes(trimmed, true);
        println!("our ids: {:?}", ids);

        let engine = LiteRtActorEngine::new(model_path.to_string());
        let out = engine.forward(ids, out_pipe.style, 1.0, None, None, None).expect("forward failed");
        let peak = out.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!("our render: {} samples ({:.2}s), peak {:.4}", out.audio.len(), out.audio.len() as f32 / 24000.0, peak);
        let sr: u32 = 24000;
        let data_len = (out.audio.len() * 4) as u32;
        let mut wav: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(3u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(sr.to_le_bytes());
        wav.extend((sr * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(32u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for s in &out.audio {
            wav.extend(s.to_le_bytes());
        }
        std::fs::write("../../target/g2p_render_test.wav", &wav).unwrap();
        println!("wrote ../../target/g2p_render_test.wav");
    }

    /// TEMP diagnostic: forward pre-built espeak-IPA reference ids (written by
    /// a Python helper to target/ref_ids.json) and write the audio for manual
    /// audition — isolates the G2P frontend from the model.
    #[test]
    fn test_reference_ids_render_tmp() {
        let model_path = "../../../Reference/models/sonora.tflite";
        let ids_path = "../../target/ref_ids.json";
        if !Path::new(model_path).exists() || !Path::new(ids_path).exists() {
            println!("Skipping: model or ref_ids.json missing");
            return;
        }
        let ids: Vec<i32> = serde_json::from_str(&std::fs::read_to_string(ids_path).unwrap()).unwrap();
        println!("reference ids: {:?}", ids);
        for (model, out_name) in [
            (model_path, "../../target/ref_render_test.wav"),
            ("../../../Reference/models/matcha_stock.tflite", "../../target/ref_render_stock.wav"),
        ] {
            if !Path::new(model).exists() {
                println!("skipping {model} (not found)");
                continue;
            }
            let engine = LiteRtActorEngine::new(model.to_string());
            let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };
            let out = engine.forward(ids.clone(), style, 1.0, None, None, None).expect("forward failed");
            let peak = out.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!("{model}: {} samples ({:.2}s), peak {:.4}", out.audio.len(), out.audio.len() as f32 / 24000.0, peak);
            let sr: u32 = 24000;
            let data_len = (out.audio.len() * 4) as u32;
            let mut wav: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
            wav.extend(b"RIFF");
            wav.extend((36 + data_len).to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(3u16.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(sr.to_le_bytes());
            wav.extend((sr * 4).to_le_bytes());
            wav.extend(4u16.to_le_bytes());
            wav.extend(32u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend(data_len.to_le_bytes());
            for s in &out.audio {
                wav.extend(s.to_le_bytes());
            }
            std::fs::write(out_name, &wav).unwrap();
            println!("wrote {out_name}");
        }
    }

    /// Direct engine forward against the staged Sonora e2e export (skips when
    /// the gitignored `Reference/models/sonora.tflite` is absent).
    #[test]
    fn test_sonora_e2e_forward() {
        let model_path = "../../../Reference/models/sonora.tflite";
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

        let peak = output.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "sonora e2e: {} samples, peak amplitude {:.6}, pred_dur len {}",
            output.audio.len(), peak, output.pred_dur.len()
        );
        assert!(!output.audio.is_empty(), "Expected non-empty output audio");
        assert!(peak > 0.001, "Output audio is silent (peak {})", peak);
    }

    #[test]
    fn test_resample_linear() {
        let input = vec![0.0; 100];
        let output = resample_linear(input.clone(), 22050.0, 24000.0);
        assert!(!output.is_empty());
        assert_eq!(output.len(), 109);
    }

    #[test]
    fn test_get_model_sample_rate_fallback() {
        assert_eq!(get_model_sample_rate("nonexistent/model.tflite", true), 22050);
        assert_eq!(get_model_sample_rate("nonexistent/model.tflite", false), 24000);
    }

    #[test]
    fn test_get_model_sample_rate_from_config() {
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("temp_model.tflite");
        let config_path = temp_dir.join("config.json");
        
        let config_data = r#"{"sample_rate": 16000}"#;
        std::fs::write(&config_path, config_data).unwrap();
        
        let rate = get_model_sample_rate(model_path.to_str().unwrap(), true);
        assert_eq!(rate, 16000);
        
        let _ = std::fs::remove_file(config_path);
    }
}
