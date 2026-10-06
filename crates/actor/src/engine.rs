use std::sync::{Arc, Mutex};
use std::ffi::{CStr, CString};
use crate::pipeline::PipelineOutput;
use crate::asset_manager::StyleVector;
use crate::tflite;

const MATCHA_CFM_TEMPERATURE: f32 = 0.667;
const STYLETTS2_HOP_SIZE: f64 = 512.0;
const DEFAULT_TOKEN_DURATION: i32 = 8;
pub(crate) const DEFAULT_VAT: [f32; 3] = [0.5, 0.5, 0.5];

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

/// Per-call synthesis controls, passed through `forward`. The Swift and
/// Kotlin bridges pass the record through without reading it, so a new field
/// changes the record and the bindings, not the bridges.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SynthesisControls {
    /// Row of the model's speaker table; `None` means the role's default.
    pub speaker: Option<u32>,
    /// [valence, arousal, tension] for the whole call.
    pub vat: Option<Vec<f32>>,
    /// Gain in dB for the whole call; `None` means 0 dB.
    pub gain_db: Option<f32>,
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaSpeechEngine: Send + Sync {
    fn synthesize(&self, input: PipelineOutput) -> ActorEngineOutput;
    
    fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        controls: SynthesisControls,
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
    /// The speaker row the app selected; `None` is the role's default. Read
    /// once at the start of each span.
    speaker: Mutex<Option<u32>>,
}

#[uniffi::export]
impl ProsodiaActorEngine {
    #[uniffi::constructor]
    pub fn new(pipeline: Arc<crate::pipeline::ProsodiaActorPipeline>, speech_engine: Box<dyn ProsodiaSpeechEngine>) -> Arc<Self> {
        Arc::new(Self { pipeline, speech_engine, speaker: Mutex::new(None) })
    }

    /// Selects the speaker row for the spans that start after this call;
    /// `None` restores the role's default.
    pub fn set_speaker(&self, row: Option<u32>) {
        *self.speaker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = row;
    }

    pub fn process_and_synthesize(&self, span: stage::prosody_payload::ProsodySpan) -> Result<ActorEngineOutput, SpeechEngineError> {
        // One set of controls per span, built before any work: the speaker the
        // app selected, the span's VAT, and its `G:` in dB (`GB:` stays
        // dropped). G is read from the span, not from
        // `PipelineOutput.gain_multiplier`, whose default of 1.0 hides an
        // absent `G:`.
        let gain_db = match span.acoustics.as_ref().and_then(|a| a.gain_multiplier) {
            Some(g) => Some(
                crate::controls::gain_db_from_multiplier(g).map_err(|msg| SpeechEngineError::Inference { msg })?,
            ),
            None => None,
        };
        let controls = SynthesisControls {
            speaker: *self.speaker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
            vat: Some(vec![
                span.emotion.valence as f32,
                span.emotion.arousal as f32,
                span.emotion.tension as f32,
            ]),
            gain_db,
        };

        let is_matcha = self.speech_engine.is_matcha();
        let pipeline_out = self.pipeline.process_span(span.clone());

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
                controls.clone(),
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
    /// Lazily-built multi-graph runtime when `model_path` is a split-model
    /// directory (textenc/decoder/vocoder graphs + emb.bin + config.json)
    /// instead of a single .tflite file.
    split: Mutex<Option<crate::split_engine::SplitGraphEngine>>,
    /// The role's conditioning facts, checked against the graphs at every
    /// split load. `None` treats no VAT channel as trained and defaults to
    /// speaker row 0 (fail closed).
    conditioning: Option<crate::controls::RoleConditioning>,
    /// The controls `forward_split` last resolved, for the seam tests.
    #[cfg(test)]
    last_resolved: Mutex<Option<crate::controls::ResolvedControls>>,
}

#[uniffi::export]
impl LiteRtActorEngine {
    #[uniffi::constructor]
    pub fn new(model_path: String) -> Arc<Self> {
        Self::build(model_path, None)
    }

    /// An engine with its role's conditioning facts (`parse_role_conditioning`).
    /// With a block, `model_path` must be a split-model directory, and the
    /// graphs load now, so a block that does not fit them is refused here
    /// rather than at the first render. `None` is the same as `new`.
    #[uniffi::constructor]
    pub fn new_with_conditioning(
        model_path: String,
        conditioning: Option<crate::controls::RoleConditioning>,
    ) -> Result<Arc<Self>, SpeechEngineError> {
        let engine = Self::build(model_path, conditioning);
        if engine.conditioning.is_some() {
            if !engine.is_split() {
                return Err(SpeechEngineError::Inference {
                    msg: format!(
                        "conditioning refused: {} is not a split-model directory, and only split roles take a conditioning block",
                        engine.model_path
                    ),
                });
            }
            drop(engine.get_or_init_split()?);
        }
        Ok(engine)
    }
}

impl LiteRtActorEngine {
    fn build(model_path: String, conditioning: Option<crate::controls::RoleConditioning>) -> Arc<Self> {
        Arc::new(Self {
            model_path,
            inner: Mutex::new(None),
            split: Mutex::new(None),
            conditioning,
            #[cfg(test)]
            last_resolved: Mutex::new(None),
        })
    }

    fn is_split(&self) -> bool {
        crate::split_engine::is_split_model_dir(std::path::Path::new(&self.model_path))
    }

    fn get_or_init_split(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<crate::split_engine::SplitGraphEngine>>, SpeechEngineError>
    {
        let mut guard = self.split.lock().map_err(|_| SpeechEngineError::Inference {
            msg: "split engine mutex poisoned".to_string(),
        })?;
        if guard.is_none() {
            let engine =
                crate::split_engine::SplitGraphEngine::new(std::path::Path::new(&self.model_path))
                    .map_err(|msg| SpeechEngineError::Inference { msg })?;
            if let Some(conditioning) = &self.conditioning {
                crate::controls::validate_conditioning(conditioning, engine.model_facts()).map_err(|msg| {
                    SpeechEngineError::Inference { msg: format!("{}: {msg}", self.model_path) }
                })?;
            }
            *guard = Some(engine);
        }
        Ok(guard)
    }

    fn forward_split(
        &self,
        phoneme_ids: Vec<i32>,
        speed: f32,
        controls: &SynthesisControls,
        duration_scales: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        let guard = self.get_or_init_split()?;
        let engine = guard.as_ref().unwrap();
        // Speaker, VAT and gain, checked against the role's conditioning and
        // the graphs; refused, never adjusted. The gain is Volume — a constant
        // mel-domain envelope in dB — not the trained energy channel vat[1].
        let resolved =
            crate::controls::resolve_controls(controls, self.conditioning.as_ref(), engine.model_facts())
                .map_err(|msg| SpeechEngineError::Inference { msg })?;
        #[cfg(test)]
        {
            *self.last_resolved.lock().unwrap() = Some(resolved.clone());
        }
        let out = engine
            .forward(
                &phoneme_ids,
                speed,
                duration_scales.as_deref(),
                resolved.mel_gain_db.as_deref(),
                MATCHA_CFM_TEMPERATURE,
                None,
                crate::split_engine::Conditioning { speaker: resolved.speaker, vat: resolved.vat.as_deref() },
            )
            .map_err(|msg| SpeechEngineError::Inference { msg })?;
        // Same output contract as the monolithic Matcha path: resample the
        // model-native rate to the stage's 24 kHz.
        let audio = resample_linear(out.audio, engine.cfg.sample_rate as f32, 24000.0);
        Ok(ActorEngineOutput { audio, pred_dur: out.pred_dur })
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
        controls: SynthesisControls,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        if self.is_split() {
            // Multi-graph Plan A path: speaker, VAT and gain (Volume) are
            // resolved against the role's conditioning in `forward_split`;
            // duration_scales apply host-side on the realized durations. The
            // split graphs take no style or F0 input.
            let _ = (&style, &f0_bias);
            return self.forward_split(phoneme_ids, speed, &controls, duration_scales);
        }
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

            // Controls on a monolith: no speaker table, VAT only as the graph's
            // own input takes it, gain on the PCM after the graph.
            let mono = crate::controls::resolve_monolith_controls(&controls, vat_index != -1)
                .map_err(|msg| SpeechEngineError::Inference { msg })?;

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
                        let vat_data = mono.vat.unwrap_or(DEFAULT_VAT);
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

            crate::controls::apply_pcm_gain(&mut output_pcm, mono.gain);

            Ok(ActorEngineOutput {
                audio: output_pcm,
                pred_dur,
            })
        }
    }

    fn reclaim_memory_impl(&self) {
        *self.inner.lock().unwrap() = None;
        if let Ok(mut split) = self.split.lock() {
            *split = None;
        }
    }
}

#[uniffi::export]
impl LiteRtActorEngine {
    pub fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        style: StyleVector,
        speed: f32,
        controls: SynthesisControls,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, controls, duration_scales, f0_bias)
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

/// Test-only access, outside every exported `impl`, so test builds export no
/// extra symbol.
#[cfg(test)]
impl LiteRtActorEngine {
    /// The controls `forward_split` last resolved.
    fn last_resolved_controls(&self) -> Option<crate::controls::ResolvedControls> {
        self.last_resolved.lock().unwrap().clone()
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
        controls: SynthesisControls,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.forward_impl(phoneme_ids, style, speed, controls, duration_scales, f0_bias)
    }

    fn reclaim_memory(&self) {
        self.reclaim_memory_impl();
    }

    fn is_matcha(&self) -> bool {
        if self.is_split() {
            // The split recipe is Matcha by construction (blank-interspersed
            // ids, Matcha symbol inventory).
            return true;
        }
        if let Ok(guard) = self.get_or_init_interpreter() {
            guard.as_ref().map(|w| w.is_matcha).unwrap_or(false)
        } else {
            false
        }
    }

    fn get_token_limit(&self) -> i32 {
        if self.is_split() {
            // The fixed-shape textenc pads to MAX_TEXT interspersed tokens.
            return match self.get_or_init_split() {
                Ok(guard) => guard.as_ref().map(|e| e.cfg.max_text as i32).unwrap_or(50),
                Err(_) => 50,
            };
        }
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

    /// The staged e2e actor model and its config (the `actor` role in
    /// `prosodia_models.json`, pinned by sha256), or `None` with a message
    /// when they are absent. They are looked up in `PROSODIA_MODELS_DIR` when
    /// set (on ai-lab-0, `/data/models`), else in the workspace's `models/`.
    /// `PROSODIA_REQUIRE_PINNED_MODELS=1` turns the skip into a failure.
    fn staged_actor() -> Option<(String, String)> {
        let dir = std::env::var("PROSODIA_MODELS_DIR").unwrap_or_else(|_| "../../../models".to_string());
        let (model, config) = (format!("{dir}/sonora.tflite"), format!("{dir}/config.json"));
        if Path::new(&model).exists() && Path::new(&config).exists() {
            return Some((model, config));
        }
        if std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() == Ok("1") {
            panic!("PROSODIA_REQUIRE_PINNED_MODELS=1 but {model} or {config} not found");
        }
        println!("Skipping: staged model/config not found");
        None
    }

    /// Regression test for the 50-token static-limit overflow (2026-07-11): a
    /// typical sentence tokenizes past the e2e export's [1, 50] phoneme input,
    /// and `process_and_synthesize` must chunk rather than fail. Runs the real
    /// app render path: G2P → process_span → chunked forward.
    #[test]
    fn test_span_render_chunks_past_static_limit() {
        let Some((model_path, config_path)) = staged_actor() else { return };

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
                controls: SynthesisControls,
                duration_scales: Option<Vec<f32>>,
                f0_bias: Option<Vec<f32>>,
            ) -> Result<ActorEngineOutput, SpeechEngineError> {
                <LiteRtActorEngine as ProsodiaSpeechEngine>::forward(&self.0, phoneme_ids, style, speed, controls, duration_scales, f0_bias)
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
            speaker: Mutex::new(None),
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

    /// Diagnostic, not a check: same phrase as the reference-ids render, but
    /// through our Rust G2P + IPA mapping — prints phonemes/ids for diffing and
    /// writes audio for A/B audition against ref_render_test.wav. Run with
    /// `cargo test -- --ignored test_our_g2p_render_tmp`.
    #[test]
    #[ignore = "diagnostic render for manual audition; writes target/g2p_render_test.wav"]
    fn test_our_g2p_render_tmp() {
        let Some((model_path, config_path)) = staged_actor() else { return };
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
        let out = engine.forward(ids, out_pipe.style, 1.0, SynthesisControls::default(), None, None).expect("forward failed");
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

    /// Diagnostic, not a check: forward pre-built espeak-IPA reference ids
    /// (written by a Python helper to target/ref_ids.json) and write the audio
    /// for manual audition — isolates the G2P frontend from the model. Run with
    /// `cargo test -- --ignored test_reference_ids_render_tmp`.
    #[test]
    #[ignore = "diagnostic render for manual audition; needs target/ref_ids.json"]
    fn test_reference_ids_render_tmp() {
        let Some((model_path, _)) = staged_actor() else { return };
        let ids_path = "../../target/ref_ids.json";
        let ids: Vec<i32> = serde_json::from_str(
            &std::fs::read_to_string(ids_path).expect("target/ref_ids.json (written by a Python helper)"),
        )
        .unwrap();
        println!("reference ids: {:?}", ids);
        for (model, out_name) in [(model_path, "../../target/ref_render_test.wav")] {
            let engine = LiteRtActorEngine::new(model.to_string());
            let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };
            let out = engine.forward(ids.clone(), style, 1.0, SynthesisControls::default(), None, None).expect("forward failed");
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
    /// the workspace `../models/sonora.tflite` is absent).
    #[test]
    fn test_sonora_e2e_forward() {
        let Some((model_path, _)) = staged_actor() else { return };

        let engine = LiteRtActorEngine::new(model_path.to_string());
        assert!(engine.is_matcha(), "Expected loaded model to be detected as Matcha");

        let phoneme_ids = vec![12, 15, 18, 5, 9];
        let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };

        let output = engine.forward(
            phoneme_ids.clone(),
            style,
            1.0,
            SynthesisControls::default(),
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
        assert_eq!(output.pred_dur.len(), phoneme_ids.len(), "Expected one duration per phoneme id");
    }

    /// RMS of `a` in dB.
    fn rms_db(a: &[f32]) -> f64 {
        let ms = a.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / a.len().max(1) as f64;
        10.0 * ms.max(1e-20).log10()
    }

    /// The monolith path on the staged Matcha e2e graph: a speaker is
    /// refused, and +6 dB raises RMS with every sample inside ±1. The graph
    /// samples its own noise, so the audio check is direction only, on means
    /// of several renders; the exact gain is pinned by the unit tests of
    /// `resolve_monolith_controls` and `apply_pcm_gain`.
    #[test]
    fn monolith_refuses_a_speaker_and_boosts_into_the_clip() {
        let Some((model_path, _)) = staged_actor() else { return };
        let engine = LiteRtActorEngine::new(model_path);
        let ids = vec![12, 15, 18, 5, 9];
        let style = || StyleVector { data: vec![0.0; 64], shape: vec![64] };
        let err = engine
            .forward(ids.clone(), style(), 1.0, SynthesisControls { speaker: Some(1), ..Default::default() }, None, None)
            .err()
            .expect("speaker 1 on a model without a speaker table must be refused");
        assert!(err.to_string().contains("speaker 1"), "{err}");
        let renders = |gain_db: Option<f32>| -> Vec<Vec<f32>> {
            (0..8)
                .map(|_| {
                    engine
                        .forward(ids.clone(), style(), 1.0, SynthesisControls { gain_db, ..Default::default() }, None, None)
                        .expect("monolith forward")
                        .audio
                })
                .collect()
        };
        let mean_db = |r: &[Vec<f32>]| r.iter().map(|a| rms_db(a)).sum::<f64>() / r.len() as f64;
        let (base, boosted) = (renders(None), renders(Some(6.0)));
        let delta = mean_db(&boosted) - mean_db(&base);
        println!("monolith gain: requested +6 dB, mean RMS moved {delta:.2} dB");
        assert!(boosted.iter().flatten().all(|s| s.abs() <= 1.0), "a boosted sample escaped the ±1 clip");
        assert!(delta > 0.0, "+6 dB did not raise mean RMS: it moved {delta:.2} dB");
    }

    /// The split-model directory `dir`, or `None` with a message when this
    /// machine has no Sonora registry checkout beside the repo.
    /// `PROSODIA_REQUIRE_PINNED_MODELS=1` turns the skip into a failure.
    fn split_dir(dir: &'static str) -> Option<&'static str> {
        if crate::split_engine::is_split_model_dir(Path::new(dir)) {
            return Some(dir);
        }
        if std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() == Ok("1") {
            panic!("PROSODIA_REQUIRE_PINNED_MODELS=1 but split model dir {dir} not found");
        }
        println!("Skipping: split model dir {dir} not found");
        None
    }

    /// One G2P token per word; each word is its own phoneme string.
    struct WordG2p;
    impl crate::g2p::ProsodiaG2PProcessor for WordG2p {
        fn process(&self, text: String) -> Vec<crate::g2p::MToken> {
            text.split_whitespace()
                .map(|w| crate::g2p::MToken {
                    text: w.to_string(),
                    tag: String::new(),
                    whitespace: " ".to_string(),
                    phonemes: Some(w.to_string()),
                })
                .collect()
        }
    }

    struct NoVoicePacks;
    impl crate::voice_loader::VoiceAssetProvider for NoVoicePacks {
        fn load_voice_bytes(&self, _voice_name: String) -> Option<Vec<u8>> {
            None
        }
    }

    /// A speech engine that renders nothing and records the controls of
    /// every forward. `on_forward` runs inside each forward, before it
    /// returns; `limit` is the reported token limit (0 = unbounded).
    struct ControlsRecorder {
        calls: Arc<Mutex<Vec<SynthesisControls>>>,
        limit: i32,
        on_forward: Box<dyn Fn() + Send + Sync>,
    }

    impl ProsodiaSpeechEngine for ControlsRecorder {
        fn synthesize(&self, _input: PipelineOutput) -> ActorEngineOutput {
            ActorEngineOutput { audio: Vec::new(), pred_dur: Vec::new() }
        }

        fn forward(
            &self,
            phoneme_ids: Vec<i32>,
            _style: StyleVector,
            _speed: f32,
            controls: SynthesisControls,
            _duration_scales: Option<Vec<f32>>,
            _f0_bias: Option<Vec<f32>>,
        ) -> Result<ActorEngineOutput, SpeechEngineError> {
            self.calls.lock().unwrap().push(controls);
            (self.on_forward)();
            Ok(ActorEngineOutput { audio: vec![0.0; 4], pred_dur: vec![1; phoneme_ids.len()] })
        }

        fn reclaim_memory(&self) {}

        fn get_token_limit(&self) -> i32 {
            self.limit
        }
    }

    /// A `ProsodiaActorEngine` over a `ControlsRecorder`, with a letters-and-
    /// space vocab, and the recorder's call log.
    fn recording_engine(
        limit: i32,
        on_forward: Box<dyn Fn() + Send + Sync>,
    ) -> (Arc<ProsodiaActorEngine>, Arc<Mutex<Vec<SynthesisControls>>>) {
        let vocab: std::collections::HashMap<String, i32> = " abcdefghijklmnopqrstuvwxyz"
            .chars()
            .enumerate()
            .map(|(i, c)| (c.to_string(), i as i32 + 1))
            .collect();
        let pipeline = crate::pipeline::ProsodiaActorPipeline::new(
            Box::new(WordG2p),
            crate::voice_loader::VoiceLoader::new(Box::new(NoVoicePacks)),
            serde_json::json!({ "vocab": vocab }).to_string(),
            24000,
            "en-us".to_string(),
        )
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let engine = ProsodiaActorEngine::new(
            pipeline,
            Box::new(ControlsRecorder { calls: calls.clone(), limit, on_forward }),
        );
        (engine, calls)
    }

    fn span(
        text: &str,
        vat: (f64, f64, f64),
        acoustics: Option<stage::prosody::ProsodyAcoustics>,
    ) -> stage::prosody_payload::ProsodySpan {
        stage::prosody_payload::ProsodySpan {
            text: text.to_string(),
            emotion: stage::prosody::EmotionVector { valence: vat.0, arousal: vat.1, tension: vat.2 },
            leading_pause: 0.0,
            acoustics,
        }
    }

    /// The span's VAT moves into the record; the app has set no speaker and
    /// the payload carries no `G:`, so neither reaches the engine.
    #[test]
    fn process_and_synthesize_sends_the_span_vat_without_speaker_or_gain() {
        let (engine, calls) = recording_engine(0, Box::new(|| {}));
        engine
            .process_and_synthesize(span("the quick brown fox", (0.25, -0.5, 0.75), None))
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![SynthesisControls { speaker: None, vat: Some(vec![0.25, -0.5, 0.75]), gain_db: None }]
        );
    }

    const LONG_TEXT: &str = "the quick brown fox jumps over the lazy dog while the cat sleeps by the warm fire tonight";

    fn acoustics(gain: Option<f64>, gain_bias: Option<f64>, speaker_lock: Option<&str>) -> stage::prosody::ProsodyAcoustics {
        stage::prosody::ProsodyAcoustics {
            speed_multiplier: Some(1.1),
            speed_bias: None,
            gain_multiplier: gain,
            gain_bias,
            casting_profile: None,
            speaker_lock: speaker_lock.map(str::to_string),
            pause_multiplier: None,
            pronunciation_override: None,
            pitch: None,
            token_duration_scales: None,
            token_f0_biases: None,
        }
    }

    #[test]
    fn process_and_synthesize_converts_g_to_db() {
        let (engine, calls) = recording_engine(0, Box::new(|| {}));
        engine
            .process_and_synthesize(span("hello there", (0.0, 0.0, 0.0), Some(acoustics(Some(0.5), None, None))))
            .unwrap();
        let db = calls.lock().unwrap()[0].gain_db.expect("G: 0.5 is a gain");
        assert!((db + 6.0206).abs() < 1e-3, "G 0.5 became {db} dB");
    }

    #[test]
    fn process_and_synthesize_refuses_a_g_that_is_not_positive_and_finite() {
        for g in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let (engine, calls) = recording_engine(0, Box::new(|| {}));
            let err = engine
                .process_and_synthesize(span("hello there", (0.0, 0.0, 0.0), Some(acoustics(Some(g), None, None))))
                .err()
                .expect("a G that is not positive and finite must be refused");
            assert!(err.to_string().contains("gain multiplier G"), "G {g}: {err}");
            assert!(calls.lock().unwrap().is_empty(), "G {g}: the engine ran");
        }
    }

    /// `GB:`, `LK:` and acoustics without `G:` become no gain and no speaker.
    /// A 0 dB gain is not harmless: on the monolith it engages the +/-1 clip.
    /// The speaker comes from the app only.
    #[test]
    fn gain_bias_speaker_lock_and_other_acoustics_send_no_gain_or_speaker() {
        let (engine, calls) = recording_engine(0, Box::new(|| {}));
        engine
            .process_and_synthesize(span("hello there", (0.0, 0.0, 0.0), Some(acoustics(None, Some(0.2), Some("229")))))
            .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!((calls[0].speaker, calls[0].gain_db), (None, None));
    }

    #[test]
    fn every_chunk_of_a_span_gets_the_same_controls() {
        let (engine, calls) = recording_engine(12, Box::new(|| {}));
        engine.set_speaker(Some(3));
        engine
            .process_and_synthesize(span(LONG_TEXT, (0.0, 0.5, 0.0), Some(acoustics(Some(1.2), None, None))))
            .unwrap();
        let calls = calls.lock().unwrap();
        assert!(calls.len() > 1, "expected the span to be chunked, got {} forward(s)", calls.len());
        assert!(calls.iter().all(|c| *c == calls[0]), "controls differ across chunks: {calls:?}");
        assert_eq!(calls[0].speaker, Some(3));
        assert_eq!(calls[0].vat, Some(vec![0.0, 0.5, 0.0]));
        assert!((calls[0].gain_db.unwrap() - 1.5836).abs() < 1e-3);
    }

    #[test]
    fn set_speaker_applies_from_the_next_span_and_none_restores_the_default() {
        let (engine, calls) = recording_engine(0, Box::new(|| {}));
        let speak = || engine.process_and_synthesize(span("hello there", (0.0, 0.0, 0.0), None)).unwrap();
        speak();
        engine.set_speaker(Some(7));
        speak();
        engine.set_speaker(None);
        speak();
        let speakers: Vec<Option<u32>> = calls.lock().unwrap().iter().map(|c| c.speaker).collect();
        assert_eq!(speakers, vec![None, Some(7), None]);
    }

    /// A speaker change while a multi-chunk span renders applies from the next
    /// span; the span in flight keeps one speaker.
    #[test]
    fn a_speaker_change_during_a_span_waits_for_the_next_span() {
        let slot: Arc<std::sync::OnceLock<std::sync::Weak<ProsodiaActorEngine>>> = Arc::new(std::sync::OnceLock::new());
        let hook = slot.clone();
        let (engine, calls) = recording_engine(
            12,
            Box::new(move || {
                if let Some(engine) = hook.get().and_then(|weak| weak.upgrade()) {
                    engine.set_speaker(Some(9));
                }
            }),
        );
        slot.set(Arc::downgrade(&engine)).unwrap();
        engine.set_speaker(Some(3));
        engine.process_and_synthesize(span(LONG_TEXT, (0.0, 0.0, 0.0), None)).unwrap();
        let first: Vec<SynthesisControls> = calls.lock().unwrap().drain(..).collect();
        assert!(first.len() > 1, "expected the span to be chunked");
        assert!(first.iter().all(|c| c.speaker == Some(3)), "the speaker changed inside a span: {first:?}");
        engine.process_and_synthesize(span("next", (0.0, 0.0, 0.0), None)).unwrap();
        assert_eq!(calls.lock().unwrap()[0].speaker, Some(9));
    }

    /// End-to-end dispatch through LiteRtActorEngine with a split-model
    /// DIRECTORY path: detection, token limit, forward, 24 kHz resample.
    /// Skips when the `Sonora/huggingface` registry checkout is absent.
    #[test]
    fn test_split_dispatch_through_engine() {
        let Some(dir) = split_dir("../../../Sonora/huggingface/baseline-ljspeech-22k/litert-split") else { return };
        let engine = LiteRtActorEngine::new(dir.to_string());
        assert!(engine.is_matcha(), "split dir must report matcha");
        assert_eq!(engine.get_token_limit(), 256, "split token limit = MAX_TEXT");

        let ids = vec![0, 12, 0, 15, 0, 18, 0, 5, 0, 9, 0];
        let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };
        let out = engine
            .forward(ids.clone(), style, 1.0, SynthesisControls::default(), None, None)
            .expect("split dispatch forward");
        let peak = out.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "split dispatch: {} samples @24k, peak {:.4}, pred_dur len {}",
            out.audio.len(),
            peak,
            out.pred_dur.len()
        );
        assert!(!out.audio.is_empty());
        assert!(peak > 0.001, "silent output (peak {peak})");
        assert_eq!(out.pred_dur.len(), ids.len(), "real per-token durations");
    }

    /// The multi-speaker 24 kHz split export through `LiteRtActorEngine`:
    /// native-rate output passes through without resampling, so the sample
    /// count stays a whole number of 256-sample hops.
    #[test]
    fn test_split_dispatch_24k_conditioned_model() {
        let Some(dir) = split_dir("../../../Sonora/huggingface/derisk-energy-24k/litert-split") else { return };
        let engine = LiteRtActorEngine::new(dir.to_string());
        assert!(engine.is_matcha());
        assert_eq!(engine.get_token_limit(), 256);
        let ids = vec![0, 12, 0, 15, 0, 18, 0, 5, 0, 9, 0];
        let style = StyleVector { data: vec![0.0; 64], shape: vec![64] };
        let out = engine
            .forward(ids.clone(), style, 1.0, SynthesisControls::default(), None, None)
            .expect("24 kHz split dispatch forward");
        let peak = out.audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!("24k split dispatch: {} samples, peak {peak:.4}", out.audio.len());
        assert!(!out.audio.is_empty() && peak > 0.001, "silent output (peak {peak})");
        assert_eq!(out.audio.len() % 256, 0, "24 kHz output was resampled");
        assert_eq!(out.pred_dur.len(), ids.len());
    }

    const DERISK_SPLIT: &str = "../../../Sonora/huggingface/derisk-energy-24k/litert-split";

    /// The committed `prosodia_models.json` block for `role`.
    fn committed_conditioning(role: &str) -> Option<crate::controls::RoleConditioning> {
        crate::controls::parse_role_conditioning(crate::controls::committed_models_json(), role.to_string())
            .expect("the committed block parses")
    }

    fn short_ids() -> Vec<i32> {
        vec![0, 12, 0, 15, 0, 18, 0, 5, 0, 9, 0]
    }

    fn zero_style() -> StyleVector {
        StyleVector { data: vec![0.0; 64], shape: vec![64] }
    }

    #[test]
    fn conditioning_constructor_refuses_a_non_split_path() {
        let block = crate::controls::RoleConditioning {
            trained_vat: vec![1],
            default_speaker: 0,
            speaker_labels: Vec::new(),
            evidence: "test".to_string(),
        };
        let err = LiteRtActorEngine::new_with_conditioning("/nonexistent/sonora.tflite".to_string(), Some(block))
            .err()
            .expect("a block on a non-split path must be refused");
        assert!(err.to_string().contains("not a split-model directory"), "{err}");
        assert!(
            LiteRtActorEngine::new_with_conditioning("/nonexistent/sonora.tflite".to_string(), None).is_ok(),
            "no block is accepted on every path"
        );
    }

    /// The committed `actor-split-24k` block passes load validation on the
    /// real derisk graphs, and a block that does not fit them is refused when
    /// the engine is built.
    #[test]
    fn conditioning_block_is_validated_against_the_derisk_graphs() {
        let Some(dir) = split_dir(DERISK_SPLIT) else { return };
        let block = committed_conditioning("actor-split-24k").expect("actor-split-24k has a block");
        assert!(LiteRtActorEngine::new_with_conditioning(dir.to_string(), Some(block.clone())).is_ok());
        for (bad, fact) in [
            (crate::controls::RoleConditioning { default_speaker: 247, ..block.clone() }, "defaultSpeaker"),
            (crate::controls::RoleConditioning { trained_vat: vec![3], ..block.clone() }, "trainedVat"),
            (crate::controls::RoleConditioning { speaker_labels: vec!["19".to_string()], ..block.clone() }, "speaker labels"),
        ] {
            let err = LiteRtActorEngine::new_with_conditioning(dir.to_string(), Some(bad))
                .err()
                .expect("a block that does not fit the graphs must be refused");
            assert!(err.to_string().contains(fact), "{fact}: {err}");
        }
    }

    /// Through the conditioning constructor and `forward`: `speaker: None`
    /// resolves to the role's default (row 22), an explicit row is used, and
    /// a nonzero valence is refused.
    #[test]
    fn conditioned_split_defaults_the_speaker_and_refuses_untrained_vat() {
        let Some(dir) = split_dir(DERISK_SPLIT) else { return };
        let engine =
            LiteRtActorEngine::new_with_conditioning(dir.to_string(), committed_conditioning("actor-split-24k")).unwrap();
        let render = |controls: SynthesisControls| engine.forward(short_ids(), zero_style(), 1.0, controls, None, None);
        render(SynthesisControls { vat: Some(vec![0.0, 0.5, 0.0]), ..Default::default() }).expect("energy is trained");
        assert_eq!(engine.last_resolved_controls().unwrap().speaker, 22);
        render(SynthesisControls { speaker: Some(100), ..Default::default() }).expect("an explicit speaker");
        assert_eq!(engine.last_resolved_controls().unwrap().speaker, 100);
        let err = render(SynthesisControls { vat: Some(vec![0.3, 0.0, 0.0]), ..Default::default() })
            .err()
            .expect("valence is untrained on derisk-energy-24k");
        assert!(err.to_string().contains("vat[0] (valence)"), "{err}");
    }

    /// Volume through the seam: the exact −6 dB envelope reaches the graph,
    /// and it lowers the output. The noise on this path is random, so the
    /// audio check is direction only, on means of several renders; the exact
    /// dB is pinned by the fixed-noise `SplitGraphEngine` test.
    #[test]
    fn conditioned_split_volume_lowers_rms() {
        let Some(dir) = split_dir(DERISK_SPLIT) else { return };
        let engine =
            LiteRtActorEngine::new_with_conditioning(dir.to_string(), committed_conditioning("actor-split-24k")).unwrap();
        let render = |gain_db: Option<f32>| -> f64 {
            let controls = SynthesisControls { gain_db, ..Default::default() };
            rms_db(&engine.forward(short_ids(), zero_style(), 1.0, controls, None, None).expect("split forward").audio)
        };
        render(Some(-6.0));
        let max_mel = engine.split.lock().unwrap().as_ref().expect("graphs loaded").model_facts().max_mel;
        assert_eq!(engine.last_resolved_controls().unwrap().mel_gain_db, Some(vec![-6.0; max_mel]));
        let mean_db = |gain_db: Option<f32>| -> f64 { (0..6).map(|_| render(gain_db)).sum::<f64>() / 6.0 };
        let (quiet, base) = (mean_db(Some(-6.0)), mean_db(None));
        println!("seam volume: requested -6 dB, mean RMS moved {:.2} dB", quiet - base);
        assert!(quiet < base, "-6 dB did not lower mean RMS: {quiet:.2} dB vs {base:.2} dB at 0 dB");
    }

    /// `reclaim_memory` drops the graphs, not the role: after it, the role's
    /// default speaker (row 22) and its refusals still apply.
    #[test]
    fn conditioning_survives_reclaim_memory() {
        let Some(dir) = split_dir(DERISK_SPLIT) else { return };
        let engine =
            LiteRtActorEngine::new_with_conditioning(dir.to_string(), committed_conditioning("actor-split-24k")).unwrap();
        engine.reclaim_memory();
        engine
            .forward(short_ids(), zero_style(), 1.0, SynthesisControls::default(), None, None)
            .expect("reload and render");
        assert_eq!(engine.last_resolved_controls().unwrap().speaker, 22);
        let err = engine
            .forward(short_ids(), zero_style(), 1.0, SynthesisControls { vat: Some(vec![0.0, 0.0, 0.2]), ..Default::default() }, None, None)
            .err()
            .expect("tension stays refused after a reload");
        assert!(err.to_string().contains("vat[2] (tension)"), "{err}");
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
