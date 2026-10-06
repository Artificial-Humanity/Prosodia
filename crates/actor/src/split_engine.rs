//! Multi-graph (split) Matcha runtime — the Plan A path.
//!
//! Orchestrates three fixed-shape TFLite graphs (text encoder, CFM decoder,
//! vocoder) with the sampling loop on the host, mirroring the litert-samples
//! Matcha recipe (`e2e_masked.py` in the litert-conversion harness is the
//! numerical reference):
//!
//!   ids → [host] embedding lookup + pad + text mask
//!       → textenc → (mu, logw)
//!       → [host] durations = ceil(exp(logw)) · length_scale (per-token
//!         `duration_scales` dictation applies here), length-regulate mu
//!       → decoder ×N (host Euler ODE over the CFM vector field; host-side
//!         sinusoidal time embedding)
//!       → [host] denormalize mel
//!       → vocoder → waveform (clip, trim to Σdurations · hop)
//!
//! Unlike the monolithic e2e export, compute is proportional to the actual
//! frame count only in the trim — the graphs themselves are fixed-shape
//! (MAX_TEXT × MAX_MEL) — but the host-visible `logw` finally makes per-token
//! duration dictation real, and per-module graphs enable delegate placement
//! and future streaming.
//!
//! A "model" for this engine is a **directory** containing the three graphs
//! (`matcha_{textenc,decoder,vocoder}*.tflite`, or `sonora_*` in the
//! multi-speaker exports), `emb.bin` (n_vocab × n_channels f32, host embedding
//! table), and `config.json` (shapes, mel stats, symbols).
//!
//! Multi-speaker, VAT-conditioned exports (Sonora `derisk-energy-24k` and
//! later) add `spk` and `vat` inputs to the text encoder and `spk` and `vat_y`
//! inputs to the decoder, plus `spk_emb.bin` (n_spks × spk_dim f32). The
//! engine reads which inputs exist, and their widths, from the graphs
//! themselves: the published `config.json` files carry a wrong
//! `time_embed_dim`, and the graph is the contract.

use crate::tflite;
use std::ffi::CString;
use std::path::Path;

/// `config.json` shipped beside the split graphs (registry `litert-split/`).
#[derive(serde::Deserialize)]
pub struct SplitConfig {
    pub n_vocab: usize,
    pub n_channels: usize,
    pub n_feats: usize,
    #[serde(rename = "MAX_TEXT")]
    pub max_text: usize,
    #[serde(rename = "MAX_MEL")]
    pub max_mel: usize,
    pub mel_mean: f32,
    pub mel_std: f32,
    pub hop: usize,
    pub sample_rate: u32,
    /// Baseline length scale baked into the recipe (0.95 for baseline-ljspeech-22k).
    pub length_scale: f32,
    #[serde(rename = "n_timesteps_default", default = "default_timesteps")]
    pub n_timesteps: usize,
    /// Speaker count, when the export is multi-speaker. Cross-checked against
    /// `spk_emb.bin`.
    #[serde(default)]
    pub n_spks: Option<usize>,
    /// Speaker vector width, when the export is multi-speaker. Cross-checked
    /// against the graphs.
    #[serde(default)]
    pub spk_emb_dim: Option<usize>,
    /// VAT channel count, when the export is VAT-conditioned. Cross-checked
    /// against the graphs.
    #[serde(default)]
    pub vat_dim: Option<usize>,
    /// Present on direction-contract-v2 exports, which this runtime does not
    /// support yet (see `SplitGraphEngine::new`).
    #[serde(default)]
    contract_version: Option<u32>,
    #[serde(default)]
    control: Option<serde_json::Value>,
    #[serde(default)]
    g2p: Option<serde_json::Value>,
}

/// Per-utterance conditioning for a split forward.
#[derive(Clone, Copy, Debug)]
pub struct Conditioning<'a> {
    /// Row of `spk_emb.bin`. Must be 0 for a single-speaker model.
    pub speaker: usize,
    /// One value per VAT channel (valence, arousal/energy, tension), applied to
    /// every token. Values outside [-1, 1] are refused. `None` is all zeros —
    /// the trained neutral (conditioning dropout). Must be `None` for a model
    /// without a `vat` input.
    ///
    /// Which channels a checkpoint was trained on is model-specific and not
    /// recorded in its export: `derisk-energy-24k` trained only channel 1
    /// (energy), so nonzero valence or tension is untrained input for it.
    pub vat: Option<&'a [f32]>,
}

impl Conditioning<'static> {
    /// Speaker 0, neutral VAT.
    pub const NEUTRAL: Conditioning<'static> = Conditioning { speaker: 0, vat: None };
}

fn default_timesteps() -> usize {
    10
}

/// Matcha's fixed time scale for the sinusoidal embedding.
const TIME_EMB_SCALE: f32 = 1000.0;

/// One TFLite graph with positional (index-based) f32 I/O.
///
/// The split graphs expose anonymous `serving_default_args_N` tensor names, so
/// the monolithic engine's name-substring binding does not apply — argument
/// order is part of the recipe's contract instead.
pub(crate) struct GraphRunner {
    model: *mut tflite::TfLiteModel,
    options: *mut tflite::TfLiteInterpreterOptions,
    interpreter: *mut tflite::TfLiteInterpreter,
    delegate: *mut tflite::TfLiteDelegate,
}

unsafe impl Send for GraphRunner {}
unsafe impl Sync for GraphRunner {}

impl Drop for GraphRunner {
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

impl GraphRunner {
    pub(crate) fn new(path: &Path) -> Result<Self, String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| format!("non-UTF8 model path: {}", path.display()))?;
        let c_path =
            CString::new(path_str).map_err(|e| format!("invalid model path: {e}"))?;
        unsafe {
            let model = tflite::TfLiteModelCreateFromFile(c_path.as_ptr());
            if model.is_null() {
                return Err(format!("failed to load graph {}", path.display()));
            }
            let options = tflite::TfLiteInterpreterOptionsCreate();
            if options.is_null() {
                tflite::TfLiteModelDelete(model);
                return Err("failed to create interpreter options".to_string());
            }
            tflite::TfLiteInterpreterOptionsSetNumThreads(options, 4);

            // Same XNNPACK setup as the monolithic engine (see engine.rs):
            // zeroed oversized options buffer, num_threads at field 0.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            let delegate = {
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
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                if !delegate.is_null() {
                    tflite::TfLiteXNNPackDelegateDelete(delegate);
                }
                return Err(format!("failed to create interpreter for {}", path.display()));
            }
            let status = tflite::TfLiteInterpreterAllocateTensors(interpreter);
            if status != 0 {
                let runner = GraphRunner { model, options, interpreter, delegate };
                drop(runner);
                return Err(format!(
                    "failed to allocate tensors for {} (status {status})",
                    path.display()
                ));
            }
            Ok(GraphRunner { model, options, interpreter, delegate })
        }
    }

    pub(crate) fn set_input(&self, index: i32, data: &[f32]) -> Result<(), String> {
        unsafe {
            let tensor = tflite::TfLiteInterpreterGetInputTensor(self.interpreter, index);
            if tensor.is_null() {
                return Err(format!("missing input tensor {index}"));
            }
            let byte_size = tflite::TfLiteTensorByteSize(tensor);
            let expected = data.len() * std::mem::size_of::<f32>();
            if byte_size != expected {
                return Err(format!(
                    "input {index} size mismatch: tensor {byte_size} B, host {expected} B"
                ));
            }
            let status = tflite::TfLiteTensorCopyFromBuffer(
                tensor,
                data.as_ptr() as *const std::ffi::c_void,
                byte_size,
            );
            if status != 0 {
                return Err(format!("failed to copy input {index} (status {status})"));
            }
            Ok(())
        }
    }

    pub(crate) fn invoke(&self) -> Result<(), String> {
        unsafe {
            let status = tflite::TfLiteInterpreterInvoke(self.interpreter);
            if status != 0 {
                return Err(format!("invoke failed (status {status})"));
            }
            Ok(())
        }
    }

    pub(crate) fn read_output(&self, index: i32, out: &mut Vec<f32>) -> Result<(), String> {
        unsafe {
            let tensor = tflite::TfLiteInterpreterGetOutputTensor(self.interpreter, index);
            if tensor.is_null() {
                return Err(format!("missing output tensor {index}"));
            }
            let byte_size = tflite::TfLiteTensorByteSize(tensor);
            out.resize(byte_size / std::mem::size_of::<f32>(), 0.0);
            let status = tflite::TfLiteTensorCopyToBuffer(
                tensor,
                out.as_mut_ptr() as *mut std::ffi::c_void,
                byte_size,
            );
            if status != 0 {
                return Err(format!("failed to copy output {index} (status {status})"));
            }
            Ok(())
        }
    }

    /// Second dimension of output tensor `index` (0 when unavailable) —
    /// used to tell mu `[1, n_feats, T]` from logw `[1, 1, T]` by shape,
    /// mirroring the Python reference.
    fn output_dim1(&self, index: i32) -> i32 {
        unsafe {
            let tensor = tflite::TfLiteInterpreterGetOutputTensor(self.interpreter, index);
            if tensor.is_null() || tflite::TfLiteTensorNumDims(tensor) < 2 {
                return 0;
            }
            tflite::TfLiteTensorDim(tensor, 1)
        }
    }

    fn input_count(&self) -> i32 {
        unsafe { tflite::TfLiteInterpreterGetInputTensorCount(self.interpreter) }
    }

    /// Shape of input tensor `index` (empty when unavailable).
    fn input_dims(&self, index: i32) -> Vec<i32> {
        unsafe {
            let tensor = tflite::TfLiteInterpreterGetInputTensor(self.interpreter, index);
            if tensor.is_null() {
                return Vec::new();
            }
            (0..tflite::TfLiteTensorNumDims(tensor))
                .map(|d| tflite::TfLiteTensorDim(tensor, d))
                .collect()
        }
    }
}

/// Output of one split-graph forward: mono PCM at the model's native sample
/// rate (already trimmed + clipped) and the realized per-token frame counts.
pub struct SplitForwardOutput {
    pub audio: Vec<f32>,
    pub pred_dur: Vec<i32>,
}

pub struct SplitGraphEngine {
    textenc: GraphRunner,
    decoder: GraphRunner,
    vocoder: GraphRunner,
    /// Host embedding table, n_vocab × n_channels, row-major.
    emb: Vec<f32>,
    pub cfg: SplitConfig,
    mu_output: i32,
    logw_output: i32,
    /// Width of the decoder's time-embedding input, read from the graph.
    time_emb_dim: usize,
    /// Host speaker table (n_spks × spk_dim), when the graphs take `spk`.
    speakers: Option<SpeakerTable>,
    /// VAT channel count, 0 when the graphs take no `vat`.
    vat_dim: usize,
}

struct SpeakerTable {
    dim: usize,
    vectors: Vec<f32>,
}

impl SpeakerTable {
    fn count(&self) -> usize {
        self.vectors.len() / self.dim
    }

    fn row(&self, speaker: usize) -> &[f32] {
        &self.vectors[speaker * self.dim..(speaker + 1) * self.dim]
    }
}

/// Graph-name prefixes per role: the LJSpeech recipe's names, then the
/// multi-speaker exports'.
const TEXTENC_PREFIXES: &[&str] = &["matcha_textenc", "sonora_textenc"];
const DECODER_PREFIXES: &[&str] = &["matcha_decoder", "sonora_decoder"];
const VOCODER_PREFIXES: &[&str] = &["matcha_vocoder", "sonora_vocoder"];

fn find_graph_any(dir: &Path, prefixes: &[&str]) -> Result<std::path::PathBuf, String> {
    prefixes
        .iter()
        .find_map(|p| find_graph(dir, p).ok())
        .ok_or_else(|| format!("no {}*.tflite in {}", prefixes.join("*/"), dir.display()))
}

fn read_f32_le(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() % 4 != 0 {
        return Err(format!("{} is not a whole number of f32s", path.display()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

fn find_graph(dir: &Path, prefix: &str) -> Result<std::path::PathBuf, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read model dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(prefix) && name.ends_with(".tflite") {
            return Ok(entry.path());
        }
    }
    Err(format!("no {prefix}*.tflite in {}", dir.display()))
}

/// A split-graph model home: a directory holding the three graphs + assets.
pub fn is_split_model_dir(path: &Path) -> bool {
    path.is_dir()
        && find_graph_any(path, TEXTENC_PREFIXES).is_ok()
        && path.join("config.json").exists()
        && path.join("emb.bin").exists()
}

impl SplitGraphEngine {
    pub fn new(dir: &Path) -> Result<Self, String> {
        let cfg_str = std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("cannot read split config.json: {e}"))?;
        let cfg: SplitConfig = serde_json::from_str(&cfg_str)
            .map_err(|e| format!("cannot parse split config.json: {e}"))?;
        // Contract v2 makes VAT channels 3.. a one-hot delivery block that must
        // not be interpolated, and declares the G2P front end the graphs expect.
        // Loading one here would treat those channels as continuous and ignore
        // the front end, so refuse it until v2 support is written deliberately.
        if cfg.contract_version.is_some_and(|v| v >= 2) || cfg.control.is_some() || cfg.g2p.is_some() {
            return Err(format!(
                "{} is a direction-contract-v2 export (contract_version/control/g2p in config.json), \
                 which this runtime does not support yet",
                dir.display()
            ));
        }

        let emb = read_f32_le(&dir.join("emb.bin"))?;
        if emb.len() != cfg.n_vocab * cfg.n_channels {
            return Err(format!(
                "emb.bin holds {} floats, n_vocab×n_channels = {}",
                emb.len(),
                cfg.n_vocab * cfg.n_channels
            ));
        }

        let textenc = GraphRunner::new(&find_graph_any(dir, TEXTENC_PREFIXES)?)?;
        let decoder = GraphRunner::new(&find_graph_any(dir, DECODER_PREFIXES)?)?;
        let vocoder = GraphRunner::new(&find_graph_any(dir, VOCODER_PREFIXES)?)?;

        // The conditioning contract, read from the graphs. Unconditioned:
        //   textenc (emb, tmask)             decoder (x, mu, t_emb, ymask)
        // Conditioned:
        //   textenc (emb, tmask, spk, vat)   decoder (x, mu, t_emb, ymask, spk, vat_y)
        let time_emb_dim = match decoder.input_dims(2).as_slice() {
            [1, d] if *d > 0 && d % 2 == 0 => *d as usize,
            other => return Err(format!("decoder input 2 (t_emb) has shape {other:?}")),
        };
        let (speakers, vat_dim) = match (textenc.input_count(), decoder.input_count()) {
            (2, 4) => (None, 0),
            (4, 6) => {
                let spk_dim = match textenc.input_dims(2).as_slice() {
                    [1, d] if *d > 0 => *d as usize,
                    other => return Err(format!("textenc input 2 (spk) has shape {other:?}")),
                };
                let vat_dim = match textenc.input_dims(3).as_slice() {
                    [1, v, t] if *v > 0 && *t as usize == cfg.max_text => *v as usize,
                    other => return Err(format!("textenc input 3 (vat) has shape {other:?}")),
                };
                // Contract v1 is three continuous channels (V, A, T). Any other
                // width is a layout this runtime cannot interpret.
                if vat_dim != 3 {
                    return Err(format!(
                        "vat is {vat_dim} channels wide; this runtime supports the 3-channel V/A/T layout only"
                    ));
                }
                if decoder.input_dims(4) != [1, spk_dim as i32]
                    || decoder.input_dims(5) != [1, vat_dim as i32, cfg.max_mel as i32]
                {
                    return Err(format!(
                        "decoder spk/vat_y inputs {:?} / {:?} disagree with the text encoder's",
                        decoder.input_dims(4),
                        decoder.input_dims(5)
                    ));
                }
                for (name, declared, found) in
                    [("spk_emb_dim", cfg.spk_emb_dim, spk_dim), ("vat_dim", cfg.vat_dim, vat_dim)]
                {
                    if declared.is_some_and(|d| d != found) {
                        return Err(format!(
                            "config.json {name} {declared:?} disagrees with the graphs ({found})"
                        ));
                    }
                }
                let vectors = read_f32_le(&dir.join("spk_emb.bin"))?;
                if vectors.is_empty() || vectors.len() % spk_dim != 0 {
                    return Err(format!(
                        "spk_emb.bin holds {} floats, not a whole number of {spk_dim}-wide rows",
                        vectors.len()
                    ));
                }
                let table = SpeakerTable { dim: spk_dim, vectors };
                if cfg.n_spks.is_some_and(|n| n != table.count()) {
                    return Err(format!(
                        "config.json n_spks {:?} disagrees with spk_emb.bin ({} rows)",
                        cfg.n_spks,
                        table.count()
                    ));
                }
                (Some(table), vat_dim)
            }
            (te, de) => {
                return Err(format!(
                    "unrecognized split contract: textenc has {te} inputs, decoder {de}"
                ))
            }
        };

        // Identify textenc outputs by shape: mu is [1, n_feats, T], logw [1, 1, T].
        let (mu_output, logw_output) = if textenc.output_dim1(0) == cfg.n_feats as i32 {
            (0, 1)
        } else {
            (1, 0)
        };

        Ok(Self {
            textenc,
            decoder,
            vocoder,
            emb,
            cfg,
            mu_output,
            logw_output,
            time_emb_dim,
            speakers,
            vat_dim,
        })
    }

    /// Number of speakers, or `None` for a single-speaker model.
    pub fn speaker_count(&self) -> Option<usize> {
        self.speakers.as_ref().map(SpeakerTable::count)
    }

    /// VAT channel count; 0 when the model takes no VAT conditioning.
    pub fn vat_dim(&self) -> usize {
        self.vat_dim
    }

    /// Width of the decoder's time-embedding input, as read from the graph.
    pub fn time_emb_dim(&self) -> usize {
        self.time_emb_dim
    }

    /// The facts `controls::resolve_controls` checks a call against: speaker
    /// count (1 for a single-speaker model), VAT width and `MAX_MEL`.
    pub(crate) fn model_facts(&self) -> crate::controls::ModelFacts {
        crate::controls::ModelFacts {
            speaker_count: self.speaker_count().unwrap_or(1),
            vat_dim: self.vat_dim,
            max_mel: self.cfg.max_mel,
        }
    }

    /// Validates `cond` against this model and returns the speaker vector and
    /// the per-utterance VAT values (zeros when `cond.vat` is `None`).
    fn resolve_conditioning(&self, cond: &Conditioning) -> Result<(Option<&[f32]>, Vec<f32>), String> {
        let spk = match &self.speakers {
            None if cond.speaker != 0 => {
                return Err(format!("speaker {} requested, but this model has one speaker", cond.speaker))
            }
            None => None,
            Some(table) if cond.speaker >= table.count() => {
                return Err(format!(
                    "speaker {} out of range: this model has {} speakers",
                    cond.speaker,
                    table.count()
                ))
            }
            Some(table) => Some(table.row(cond.speaker)),
        };
        let vat = match cond.vat {
            None => vec![0.0; self.vat_dim],
            Some(_) if self.vat_dim == 0 => {
                return Err("vat given, but this model has no vat input".to_string())
            }
            Some(v) if v.len() != self.vat_dim => {
                return Err(format!("vat has {} values, this model takes {}", v.len(), self.vat_dim))
            }
            Some(v) => {
                // Out-of-range control is refused, not clamped (the contract's
                // `clamp: "reject"`).
                if let Some(bad) = v.iter().find(|x| !x.is_finite() || x.abs() > 1.0) {
                    return Err(format!("vat value {bad} is outside [-1, 1]"));
                }
                v.to_vec()
            }
        };
        Ok((spk, vat))
    }

    /// Host sinusoidal time embedding (matcha `SinusoidalPosEmb`, weight-free),
    /// `dim` wide.
    fn time_embedding(t: f32, dim: usize) -> Vec<f32> {
        let half = dim / 2;
        let mut out = vec![0.0f32; dim];
        let log_base = (10000.0f32).ln();
        for k in 0..half {
            let freq = (-(log_base) * k as f32 / (half as f32 - 1.0)).exp();
            let angle = TIME_EMB_SCALE * t * freq;
            out[k] = angle.sin();
            out[half + k] = angle.cos();
        }
        out
    }

    /// Full host-orchestrated forward.
    ///
    /// * `speed` — the payload speed multiplier; applied as `1/speed` on top of
    ///   the recipe's baseline `length_scale` (same semantics as the monolithic
    ///   engine's `scales[1]`).
    /// * `duration_scales` — per-token multiplicative dictation, applied to the
    ///   realized (post-ceil) durations. This is where the control contract's
    ///   `DS:` channel finally reaches a model.
    /// * `mel_gain_db` — optional per-frame dB envelope added to the
    ///   denormalized log-mel between the decoder and vocoder graphs: the
    ///   **Volume** control (loudness), not the trained energy channel
    ///   `vat[1]`, which changes the voice. Measured linear on the 22 kHz
    ///   graphs (2026-07-14, 0 to −12 dB, within 0.1 dB) and on
    ///   `derisk-energy-24k` (2026-10-04, −12 to +6 dB, worst 0.37 dB, WER
    ///   flat). `LiteRtActorEngine` sends one constant envelope per call; a
    ///   varying envelope should ramp its edges (e.g. raised-cosine over ~8
    ///   frames) to avoid clicks. Indexed in frames; entries beyond the
    ///   envelope default to 0 dB.
    /// * `noise` — optional pre-scaled initial state x₀ (n_feats × MAX_MEL),
    ///   for reproducible runs and reference-parity tests; when absent, x₀ is
    ///   sampled N(0, temperature²).
    /// * `cond` — speaker and per-utterance VAT. Refused, not adjusted, when it
    ///   does not fit the model (see [`Conditioning`]).
    pub fn forward(
        &self,
        phoneme_ids: &[i32],
        speed: f32,
        duration_scales: Option<&[f32]>,
        mel_gain_db: Option<&[f32]>,
        temperature: f32,
        noise: Option<&[f32]>,
        cond: Conditioning,
    ) -> Result<SplitForwardOutput, String> {
        let cfg = &self.cfg;
        let t_x = phoneme_ids.len();
        if t_x == 0 {
            return Err("empty phoneme sequence".to_string());
        }
        if t_x > cfg.max_text {
            return Err(format!(
                "phoneme sequence length {t_x} exceeds the graph's MAX_TEXT {}",
                cfg.max_text
            ));
        }
        if speed <= 0.0 {
            return Err(format!("invalid speed {speed}"));
        }
        let (spk, vat) = self.resolve_conditioning(&cond)?;

        // 1. Embedding lookup + pad; text mask.
        let mut emb_x = vec![0.0f32; cfg.max_text * cfg.n_channels];
        for (t, &id) in phoneme_ids.iter().enumerate() {
            let id = id.clamp(0, cfg.n_vocab as i32 - 1) as usize;
            let src = &self.emb[id * cfg.n_channels..(id + 1) * cfg.n_channels];
            emb_x[t * cfg.n_channels..(t + 1) * cfg.n_channels].copy_from_slice(src);
        }
        let mut tmask = vec![0.0f32; cfg.max_text];
        tmask[..t_x].fill(1.0);

        // Per-token VAT [vat_dim × MAX_TEXT]: the utterance's values on every
        // real token, zero on padding (the reference's `vat_tok`).
        let mut vat_tok = vec![0.0f32; self.vat_dim * cfg.max_text];
        for (c, &value) in vat.iter().enumerate() {
            vat_tok[c * cfg.max_text..c * cfg.max_text + t_x].fill(value);
        }

        // 2. Text encoder → mu [n_feats × MAX_TEXT], logw [MAX_TEXT].
        self.textenc.set_input(0, &emb_x)?;
        self.textenc.set_input(1, &tmask)?;
        if let Some(spk) = spk {
            self.textenc.set_input(2, spk)?;
            self.textenc.set_input(3, &vat_tok)?;
        }
        self.textenc.invoke()?;
        let mut mu_x = Vec::new();
        let mut logw = Vec::new();
        self.textenc.read_output(self.mu_output, &mut mu_x)?;
        self.textenc.read_output(self.logw_output, &mut logw)?;

        // 3. Durations: w_ceil = ceil(exp(logw)) · length_scale · (1/speed),
        //    then per-token dictation. (ceil-then-scale matches the recipe.)
        let length_scale = cfg.length_scale / speed;
        let mut w_ceil = vec![0.0f32; t_x];
        for i in 0..t_x {
            let mut w = (logw[i].exp() * tmask[i]).ceil() * length_scale;
            if let Some(ds) = duration_scales {
                if let Some(&s) = ds.get(i) {
                    if s.is_finite() && s > 0.0 {
                        w *= s;
                    }
                }
            }
            w_ceil[i] = w;
        }
        let y_len_f: f32 = w_ceil.iter().sum();
        let mut y_lengths = (y_len_f as i64).max(1) as usize;
        if y_lengths > cfg.max_mel {
            eprintln!(
                "split engine: {y_lengths} frames exceed MAX_MEL {}; truncating audio",
                cfg.max_mel
            );
            y_lengths = cfg.max_mel;
        }

        // 4. Length-regulate mu (and VAT, through the same alignment): frame f
        //    belongs to the first token whose cumulative duration exceeds f
        //    (float compare, per the reference's sequence_mask-based
        //    generate_path).
        let mut ymask = vec![0.0f32; cfg.max_mel];
        ymask[..y_lengths].fill(1.0);
        let mut mu_y = vec![0.0f32; cfg.n_feats * cfg.max_mel];
        let mut vat_y = vec![0.0f32; self.vat_dim * cfg.max_mel];
        {
            let mut token = 0usize;
            let mut cum = w_ceil[0];
            for f in 0..y_lengths {
                while (f as f32) >= cum && token + 1 < t_x {
                    token += 1;
                    cum += w_ceil[token];
                }
                for c in 0..cfg.n_feats {
                    mu_y[c * cfg.max_mel + f] = mu_x[c * cfg.max_text + token];
                }
                for c in 0..self.vat_dim {
                    vat_y[c * cfg.max_mel + f] = vat_tok[c * cfg.max_text + token];
                }
            }
        }

        // 5. Initial state x₀ (masked), then the host Euler ODE over the CFM
        //    vector field. Decoder inputs are positional: (x, mu, t_emb, ymask),
        //    then (spk, vat_y) on a conditioned model.
        let state_len = cfg.n_feats * cfg.max_mel;
        let mut x = match noise {
            Some(z) => {
                if z.len() != state_len {
                    return Err(format!(
                        "noise length {} != n_feats×MAX_MEL = {state_len}",
                        z.len()
                    ));
                }
                z.to_vec()
            }
            None => {
                let mut rng = GaussianRng::from_entropy();
                (0..state_len).map(|_| rng.next_gaussian() * temperature).collect()
            }
        };
        for c in 0..cfg.n_feats {
            for f in y_lengths..cfg.max_mel {
                x[c * cfg.max_mel + f] = 0.0;
            }
        }

        let n_steps = cfg.n_timesteps.max(1);
        let dt = 1.0f32 / n_steps as f32;
        let mut t = 0.0f32;
        let mut v = Vec::new();
        for _ in 0..n_steps {
            let t_emb = Self::time_embedding(t, self.time_emb_dim);
            self.decoder.set_input(0, &x)?;
            self.decoder.set_input(1, &mu_y)?;
            self.decoder.set_input(2, &t_emb)?;
            self.decoder.set_input(3, &ymask)?;
            if let Some(spk) = spk {
                self.decoder.set_input(4, spk)?;
                self.decoder.set_input(5, &vat_y)?;
            }
            self.decoder.invoke()?;
            self.decoder.read_output(0, &mut v)?;
            for (xi, vi) in x.iter_mut().zip(v.iter()) {
                *xi += dt * vi;
            }
            t += dt;
        }

        // 6. Denormalize mel (masked), apply the per-frame Volume envelope
        //    (dB → natural-log mel units), vocode, clip (a guard: the 24 kHz
        //    vocoder ends in tanh), trim.
        const DB_TO_LN: f32 = 0.115_129_255; // ln(10)/20
        let mut mel = vec![0.0f32; state_len];
        for c in 0..cfg.n_feats {
            for f in 0..cfg.max_mel {
                let mut m = x[c * cfg.max_mel + f] * cfg.mel_std + cfg.mel_mean;
                if let Some(env) = mel_gain_db {
                    if let Some(&db) = env.get(f) {
                        m += db * DB_TO_LN;
                    }
                }
                mel[c * cfg.max_mel + f] = m * ymask[f];
            }
        }
        self.vocoder.set_input(0, &mel)?;
        self.vocoder.invoke()?;
        let mut wav = Vec::new();
        self.vocoder.read_output(0, &mut wav)?;
        wav.truncate(y_lengths * cfg.hop);
        for s in wav.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }

        let pred_dur: Vec<i32> = w_ceil.iter().map(|&w| (w.round() as i32).max(1)).collect();
        Ok(SplitForwardOutput { audio: wav, pred_dur })
    }
}

/// Minimal Gaussian sampler (xorshift64* + Box–Muller) — keeps the crate free
/// of an RNG dependency; reference-parity tests inject explicit noise instead.
struct GaussianRng {
    state: u64,
    spare: Option<f32>,
}

impl GaussianRng {
    fn from_entropy() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            | 1;
        Self { state: seed, spare: None }
    }

    /// Deterministic stream, for paired renders in tests.
    #[cfg(test)]
    fn from_seed(seed: u64) -> Self {
        Self { state: seed | 1, spare: None }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn next_uniform(&mut self) -> f32 {
        // (0, 1] to keep ln() finite.
        (((self.next_u64() >> 40) + 1) as f32) / ((1u64 << 24) as f32)
    }

    fn next_gaussian(&mut self) -> f32 {
        if let Some(s) = self.spare.take() {
            return s;
        }
        let u1 = self.next_uniform();
        let u2 = self.next_uniform();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * std::f32::consts::PI * u2;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SynthesisControls;

    const MODEL_DIR: &str = "../../../Sonora/huggingface/baseline-ljspeech-22k/litert-split";
    const DERISK_DIR: &str = "../../../Sonora/huggingface/derisk-energy-24k/litert-split";
    const FIXTURES: &str = "tests/fixtures/split_parity";

    /// Loads a split engine, or `None` (with a message) when the registry
    /// checkout is absent on this machine.
    fn load(dir: &str) -> Option<SplitGraphEngine> {
        if !is_split_model_dir(Path::new(dir)) {
            if std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() == Ok("1") {
                panic!("PROSODIA_REQUIRE_PINNED_MODELS=1 but split model dir {dir} not found");
            }
            println!("Skipping: split model dir {dir} not found");
            return None;
        }
        Some(SplitGraphEngine::new(Path::new(dir)).expect("split engine init"))
    }

    /// Blank-interspersed ids for an IPA string, from the model's own symbol
    /// table (`config.json` `symbols`), as the Matcha front end produces them.
    fn ids_for(dir: &str, ipa: &str) -> Vec<i32> {
        let cfg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(Path::new(dir).join("config.json")).unwrap(),
        )
        .unwrap();
        let symbols: Vec<String> = cfg["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let mut ids = vec![0];
        for ch in ipa.chars() {
            let id = symbols
                .iter()
                .position(|s| s == &ch.to_string())
                .unwrap_or_else(|| panic!("symbol {ch:?} not in vocab"));
            ids.push(id as i32);
            ids.push(0);
        }
        ids
    }

    /// Fixed initial state x₀ at the given temperature, so paired renders
    /// differ only in the input under test.
    fn fixed_noise(engine: &SplitGraphEngine, temperature: f32) -> Vec<f32> {
        let mut rng = GaussianRng::from_seed(0x5EED_1234);
        (0..engine.cfg.n_feats * engine.cfg.max_mel)
            .map(|_| rng.next_gaussian() * temperature)
            .collect()
    }

    fn rms_db(a: &[f32]) -> f64 {
        let ms = a.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / a.len() as f64;
        10.0 * ms.max(1e-20).log10()
    }

    const PHRASE: &str = "ðə kwˈɪk bɹˈaʊn fˈɑːks dʒˈʌmps ˌoʊvɚ ðə lˈeɪzi dˈɑːɡ.";

    #[test]
    fn baseline_split_is_unconditioned() {
        let Some(engine) = load(MODEL_DIR) else { return };
        assert_eq!(engine.speaker_count(), None);
        assert_eq!(engine.vat_dim(), 0);
        assert_eq!(engine.time_emb_dim(), 160);
        assert_eq!(engine.cfg.sample_rate, 22050);
        let ids = ids_for(MODEL_DIR, PHRASE);
        let err = engine
            .forward(&ids, 1.0, None, None, 0.667, None, Conditioning { speaker: 1, vat: None })
            .err()
            .expect("a speaker on a single-speaker model must be refused");
        assert!(err.contains("speaker"), "{err}");
        let err = engine
            .forward(&ids, 1.0, None, None, 0.667, None, Conditioning { speaker: 0, vat: Some(&[0.0, 0.5, 0.0]) })
            .err()
            .expect("vat on a model without a vat input must be refused");
        assert!(err.contains("vat"), "{err}");
    }

    #[test]
    fn derisk_split_reads_its_contract_from_the_graphs() {
        let Some(engine) = load(DERISK_DIR) else { return };
        assert_eq!(engine.speaker_count(), Some(247));
        assert_eq!(engine.vat_dim(), 3);
        // The published config.json says time_embed_dim 1024; the decoder
        // graph takes 224 (= in_channels), and the graph is what counts.
        assert_eq!(engine.time_emb_dim(), 224);
        assert_eq!(engine.cfg.sample_rate, 24000);
    }

    #[test]
    fn derisk_refuses_bad_conditioning() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let render = |speaker: usize, vat: Option<&[f32]>| {
            engine.forward(&ids, 1.0, None, None, 0.667, None, Conditioning { speaker, vat })
        };
        let err = render(247, None).err().expect("speaker 247 is out of range");
        assert!(err.contains("speaker"), "{err}");
        let err = render(0, Some(&[0.0, 0.5])).err().expect("vat must be 3 wide");
        assert!(err.contains("vat"), "{err}");
        let err = render(0, Some(&[0.0, 1.5, 0.0])).err().expect("vat outside [-1, 1]");
        assert!(err.contains("vat"), "{err}");
        let err = render(0, Some(&[0.0, f32::NAN, 0.0])).err().expect("non-finite vat");
        assert!(err.contains("vat"), "{err}");
    }

    /// The energy channel (vat index 1) must move loudness monotonically
    /// through the full TFLite pipeline, as Sonora measured at export
    /// (≈ −21 → −18 → −15 dB RMS across e = −1, 0, +1).
    #[test]
    fn derisk_energy_channel_is_monotonic() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let z = fixed_noise(&engine, 0.667);
        let render = |e: f32| {
            let vat = [0.0, e, 0.0];
            engine
                .forward(&ids, 1.0, None, None, 0.667, Some(&z), Conditioning { speaker: 0, vat: Some(&vat) })
                .expect("derisk forward")
                .audio
        };
        let (lo, mid, hi) = (rms_db(&render(-1.0)), rms_db(&render(0.0)), rms_db(&render(1.0)));
        println!("derisk energy sweep: e=-1 {lo:.2} dB, e=0 {mid:.2} dB, e=+1 {hi:.2} dB");
        assert!(lo < mid && mid < hi, "energy not monotonic: {lo:.2} / {mid:.2} / {hi:.2} dB");
        assert!(hi - lo > 3.0, "energy span only {:.2} dB across [-1, 1]", hi - lo);
    }

    #[test]
    fn derisk_speakers_differ_and_neutral_is_the_default() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let z = fixed_noise(&engine, 0.667);
        let render = |speaker: usize, vat: Option<&[f32]>| {
            engine
                .forward(&ids, 1.0, None, None, 0.667, Some(&z), Conditioning { speaker, vat })
                .expect("derisk forward")
        };
        let a = render(0, None);
        let b = render(100, None);
        assert!(!a.audio.is_empty() && !b.audio.is_empty());
        // `vat: None` is the trained neutral, all zeros.
        let zeros = render(0, Some(&[0.0, 0.0, 0.0]));
        assert_eq!(a.audio, zeros.audio, "vat None must equal explicit zeros");
        // Same text, same noise, different speaker vector: different audio.
        let n = a.audio.len().min(b.audio.len());
        let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (x, y) = (a.audio[i] as f64, b.audio[i] as f64);
            dot += x * y;
            na += x * x;
            nb += y * y;
        }
        let cosine = dot / (na.sqrt() * nb.sqrt()).max(1e-12);
        println!("derisk speakers 0 vs 100: cosine {cosine:.4}, frames {} vs {}", a.pred_dur.iter().sum::<i32>(), b.pred_dur.iter().sum::<i32>());
        assert!(cosine < 0.95, "speaker vector had no effect (cosine {cosine:.4})");
    }

    fn cosine(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (x, y) = (a[i] as f64, b[i] as f64);
            dot += x * y;
            na += x * x;
            nb += y * y;
        }
        dot / (na.sqrt() * nb.sqrt()).max(1e-12)
    }

    /// Renders every case in `FIXTURES/<name>/meta.json` and compares it with
    /// the independent NumPy reference (`FIXTURES/generate.py`): identical
    /// frame count, cosine > 0.999. The noise is regenerated from the
    /// fixture's seed, so a case differs from the reference only in the host
    /// pipeline under test.
    fn assert_parity_with_reference(dir: &str, name: &str) -> Option<(SplitGraphEngine, Vec<i32>, Vec<f32>)> {
        let engine = load(dir)?;
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{FIXTURES}/{name}/meta.json")).expect("fixture meta.json"),
        )
        .unwrap();
        let ids: Vec<i32> = meta["ids"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect();
        let temperature = meta["temperature"].as_f64().unwrap() as f32;
        let mut rng = GaussianRng::from_seed(meta["seed"].as_u64().unwrap());
        let z: Vec<f32> = (0..engine.cfg.n_feats * engine.cfg.max_mel)
            .map(|_| rng.next_gaussian() * temperature)
            .collect();
        let cases = meta["cases"].as_array().unwrap();
        assert!(!cases.is_empty(), "fixture {name} has no cases");
        for case in cases {
            let speaker = case["speaker"].as_u64().unwrap() as usize;
            let vat: Option<Vec<f32>> = case["vat"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_f64().unwrap() as f32).collect());
            let reference: Vec<f32> = std::fs::read(format!("{FIXTURES}/{name}/{}", case["pcm"].as_str().unwrap()))
                .unwrap()
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32767.0)
                .collect();
            let out = engine
                .forward(&ids, 1.0, None, None, temperature, Some(&z), Conditioning { speaker, vat: vat.as_deref() })
                .expect("split forward");
            let frames = case["y_lengths"].as_u64().unwrap() as usize;
            assert_eq!(
                out.audio.len(),
                frames * engine.cfg.hop,
                "{name} speaker {speaker}: frame count differs from the reference"
            );
            let c = cosine(&out.audio, &reference);
            println!("{name} parity: speaker {speaker} vat {vat:?}: {frames} frames, cosine {c:.6}");
            assert!(c > 0.999, "{name} speaker {speaker} vat {vat:?}: cosine {c:.6} vs the reference");
        }
        Some((engine, ids, z))
    }

    #[test]
    fn derisk_matches_the_reference_pipeline() {
        assert_parity_with_reference(DERISK_DIR, "derisk-energy-24k");
    }

    /// Baseline parity, then the host hooks on the same render: per-token
    /// duration dictation and the per-frame mel-gain (Volume) envelope.
    #[test]
    fn baseline_matches_the_reference_pipeline() {
        let Some((engine, ids, z)) = assert_parity_with_reference(MODEL_DIR, "baseline-ljspeech-22k") else {
            return;
        };
        let render = |ds: Option<&[f32]>, env: Option<&[f32]>| {
            engine
                .forward(&ids, 1.0, ds, env, 0.667, Some(&z), Conditioning::NEUTRAL)
                .expect("split forward")
                .audio
        };
        let base = render(None, None);

        // Doubling every token's scale should roughly double the frame count.
        let ds = vec![2.0f32; ids.len()];
        let (frames1, frames2) = (base.len() / engine.cfg.hop, render(Some(&ds), None).len() / engine.cfg.hop);
        println!("duration dictation: {frames1} -> {frames2} frames at 2x");
        assert!((frames2 as f32) > (frames1 as f32) * 1.8, "duration_scales: {frames1} -> {frames2}");

        // A flat −6 dB mel envelope moves output RMS by ≈ −6 dB (the vocoder
        // is linear in log-mel gain; 0.5 dB tolerance for fp16 graphs).
        let env = vec![-6.0f32; engine.cfg.max_mel];
        let delta_db = rms_db(&render(None, Some(&env))) - rms_db(&base);
        println!("mel-gain hook: requested -6.0 dB, measured {delta_db:.2} dB");
        assert!((delta_db + 6.0).abs() < 0.5, "mel-gain: requested -6 dB, measured {delta_db:.2} dB");
    }

    /// A contract-v2 export (one-hot delivery channels, a declared G2P front
    /// end) must be refused at load, not run as if its channels were v1's.
    #[test]
    #[cfg(unix)]
    fn refuses_contract_v2_configs() {
        if load(MODEL_DIR).is_none() {
            return;
        }
        let src = Path::new(MODEL_DIR);
        for (i, marker) in [r#""contract_version": 2"#, r#""control": {}"#, r#""g2p": {}"#].iter().enumerate() {
            let dir = std::env::temp_dir().join(format!("prosodia-v2-refusal-{}-{i}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for entry in std::fs::read_dir(src).unwrap().flatten() {
                let name = entry.file_name();
                if name != "config.json" {
                    let _ = std::os::unix::fs::symlink(entry.path().canonicalize().unwrap(), dir.join(&name));
                }
            }
            let cfg = std::fs::read_to_string(src.join("config.json")).unwrap();
            let cfg = cfg.trim_end().trim_end_matches('}').to_string() + &format!(", {marker}}}");
            std::fs::write(dir.join("config.json"), cfg).unwrap();
            let err = SplitGraphEngine::new(&dir).err();
            std::fs::remove_dir_all(&dir).unwrap();
            let err = err.unwrap_or_else(|| panic!("a config with {marker} must be refused"));
            assert!(err.contains("contract-v2"), "{err}");
        }
    }

    /// The committed `actor-split-24k` conditioning block.
    fn derisk_role() -> crate::controls::RoleConditioning {
        crate::controls::parse_role_conditioning(crate::controls::committed_models_json(), "actor-split-24k".to_string())
            .expect("the committed block parses")
            .expect("actor-split-24k has a block")
    }

    /// Renders `controls` the way `LiteRtActorEngine` does — through
    /// `resolve_controls` with the derisk role — but with fixed noise `z`.
    fn render_resolved(engine: &SplitGraphEngine, ids: &[i32], z: &[f32], controls: &SynthesisControls) -> Result<Vec<f32>, String> {
        let resolved = crate::controls::resolve_controls(controls, Some(&derisk_role()), engine.model_facts())?;
        engine
            .forward(
                ids,
                1.0,
                None,
                resolved.mel_gain_db.as_deref(),
                0.667,
                Some(z),
                Conditioning { speaker: resolved.speaker, vat: resolved.vat.as_deref() },
            )
            .map(|out| out.audio)
    }

    #[test]
    fn derisk_default_speaker_is_row_22_and_differs_from_row_0() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let z = fixed_noise(&engine, 0.667);
        let row0 = render_resolved(&engine, &ids, &z, &SynthesisControls { speaker: Some(0), ..Default::default() }).unwrap();
        let row22 = render_resolved(&engine, &ids, &z, &SynthesisControls { speaker: Some(22), ..Default::default() }).unwrap();
        let default = render_resolved(&engine, &ids, &z, &SynthesisControls::default()).unwrap();
        assert_eq!(default, row22, "speaker None must resolve to the role's default, row 22");
        let c = cosine(&row0, &row22);
        println!("derisk speakers 0 vs 22: cosine {c:.4}");
        assert!(c < 0.95, "rows 0 and 22 rendered alike (cosine {c:.4})");
    }

    /// The valence refusal, checked against the model facts read from the
    /// real derisk graphs.
    #[test]
    fn derisk_untrained_valence_is_refused_before_the_graph() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let err = crate::controls::resolve_controls(
            &SynthesisControls { vat: Some(vec![0.2, 0.0, 0.0]), ..Default::default() },
            Some(&derisk_role()),
            engine.model_facts(),
        )
        .unwrap_err();
        assert!(err.contains("vat[0] (valence)"), "{err}");
    }

    /// Volume on the 24 kHz graphs: −6 dB lowers RMS by 6 dB within 0.5 dB
    /// (Sonora's worst 24 kHz linearity error was 0.27 dB, at −12 dB).
    #[test]
    fn derisk_volume_is_db_exact() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let z = fixed_noise(&engine, 0.667);
        let base = render_resolved(&engine, &ids, &z, &SynthesisControls::default()).unwrap();
        let quiet = render_resolved(&engine, &ids, &z, &SynthesisControls { gain_db: Some(-6.0), ..Default::default() }).unwrap();
        let delta = rms_db(&quiet) - rms_db(&base);
        println!("derisk volume: requested -6.0 dB, measured {delta:.2} dB");
        assert!((delta + 6.0).abs() < 0.5, "Volume: requested -6 dB, measured {delta:.2} dB");
    }

    /// The energy sweep at the role's default speaker, through the refusal
    /// rules.
    #[test]
    fn derisk_energy_sweep_through_resolve_is_monotonic() {
        let Some(engine) = load(DERISK_DIR) else { return };
        let ids = ids_for(DERISK_DIR, PHRASE);
        let z = fixed_noise(&engine, 0.667);
        let render = |e: f32| {
            render_resolved(&engine, &ids, &z, &SynthesisControls { vat: Some(vec![0.0, e, 0.0]), ..Default::default() })
                .expect("energy is trained")
        };
        let (lo, mid, hi) = (rms_db(&render(-1.0)), rms_db(&render(0.0)), rms_db(&render(1.0)));
        println!("derisk energy sweep at row 22: e=-1 {lo:.2} dB, e=0 {mid:.2} dB, e=+1 {hi:.2} dB");
        assert!(lo < mid && mid < hi, "energy not monotonic: {lo:.2} / {mid:.2} / {hi:.2} dB");
    }
}
