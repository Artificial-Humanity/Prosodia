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
//! A "model" for this engine is a **directory** containing
//! `matcha_textenc*.tflite`, `matcha_decoder*.tflite`, `matcha_vocoder*.tflite`,
//! `emb.bin` (n_vocab × n_channels f32, host embedding table), and
//! `config.json` (shapes, mel stats, symbols).

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
    /// Baseline length scale baked into the recipe (0.95 for v1-ljspeech).
    pub length_scale: f32,
    #[serde(rename = "n_timesteps_default", default = "default_timesteps")]
    pub n_timesteps: usize,
}

fn default_timesteps() -> usize {
    10
}

/// Dimension of the host-side sinusoidal time embedding fed to the decoder
/// (matcha `SinusoidalPosEmb`; the learned time MLP stays in-graph).
const TIME_EMB_DIM: usize = 160;
/// Matcha's fixed time scale for the sinusoidal embedding.
const TIME_EMB_SCALE: f32 = 1000.0;

/// One TFLite graph with positional (index-based) f32 I/O.
///
/// The split graphs expose anonymous `serving_default_args_N` tensor names, so
/// the monolithic engine's name-substring binding does not apply — argument
/// order is part of the recipe's contract instead.
struct GraphRunner {
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
    fn new(path: &Path) -> Result<Self, String> {
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

    fn set_input(&self, index: i32, data: &[f32]) -> Result<(), String> {
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

    fn invoke(&self) -> Result<(), String> {
        unsafe {
            let status = tflite::TfLiteInterpreterInvoke(self.interpreter);
            if status != 0 {
                return Err(format!("invoke failed (status {status})"));
            }
            Ok(())
        }
    }

    fn read_output(&self, index: i32, out: &mut Vec<f32>) -> Result<(), String> {
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
        && find_graph(path, "matcha_textenc").is_ok()
        && path.join("config.json").exists()
        && path.join("emb.bin").exists()
}

impl SplitGraphEngine {
    pub fn new(dir: &Path) -> Result<Self, String> {
        let cfg_str = std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("cannot read split config.json: {e}"))?;
        let cfg: SplitConfig = serde_json::from_str(&cfg_str)
            .map_err(|e| format!("cannot parse split config.json: {e}"))?;

        let emb_bytes = std::fs::read(dir.join("emb.bin"))
            .map_err(|e| format!("cannot read emb.bin: {e}"))?;
        let expected = cfg.n_vocab * cfg.n_channels * std::mem::size_of::<f32>();
        if emb_bytes.len() != expected {
            return Err(format!(
                "emb.bin size {} != n_vocab×n_channels×4 = {expected}",
                emb_bytes.len()
            ));
        }
        let emb: Vec<f32> = emb_bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let textenc = GraphRunner::new(&find_graph(dir, "matcha_textenc")?)?;
        let decoder = GraphRunner::new(&find_graph(dir, "matcha_decoder")?)?;
        let vocoder = GraphRunner::new(&find_graph(dir, "matcha_vocoder")?)?;

        // Identify textenc outputs by shape: mu is [1, n_feats, T], logw [1, 1, T].
        let (mu_output, logw_output) = if textenc.output_dim1(0) == cfg.n_feats as i32 {
            (0, 1)
        } else {
            (1, 0)
        };

        Ok(Self { textenc, decoder, vocoder, emb, cfg, mu_output, logw_output })
    }

    /// Host sinusoidal time embedding (matcha `SinusoidalPosEmb`, weight-free).
    fn time_embedding(t: f32) -> Vec<f32> {
        let half = TIME_EMB_DIM / 2;
        let mut out = vec![0.0f32; TIME_EMB_DIM];
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
    ///   denormalized log-mel between the decoder and vocoder graphs — the
    ///   energy channel. Measured 2026-07-14 (exploit-before-train): the
    ///   vocoder is linear in log-mel gain to within 0.1 dB, so this hook is
    ///   dB-exact, frame-addressable, and WER-safe to at least −12 dB; ramp
    ///   envelope edges (e.g. raised-cosine over ~8 frames) to avoid clicks.
    ///   Indexed in frames; entries beyond the envelope default to 0 dB.
    /// * `noise` — optional pre-scaled initial state x₀ (n_feats × MAX_MEL),
    ///   for reproducible runs and reference-parity tests; when absent, x₀ is
    ///   sampled N(0, temperature²).
    pub fn forward(
        &self,
        phoneme_ids: &[i32],
        speed: f32,
        duration_scales: Option<&[f32]>,
        mel_gain_db: Option<&[f32]>,
        temperature: f32,
        noise: Option<&[f32]>,
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

        // 1. Embedding lookup + pad; text mask.
        let mut emb_x = vec![0.0f32; cfg.max_text * cfg.n_channels];
        for (t, &id) in phoneme_ids.iter().enumerate() {
            let id = id.clamp(0, cfg.n_vocab as i32 - 1) as usize;
            let src = &self.emb[id * cfg.n_channels..(id + 1) * cfg.n_channels];
            emb_x[t * cfg.n_channels..(t + 1) * cfg.n_channels].copy_from_slice(src);
        }
        let mut tmask = vec![0.0f32; cfg.max_text];
        tmask[..t_x].fill(1.0);

        // 2. Text encoder → mu [n_feats × MAX_TEXT], logw [MAX_TEXT].
        self.textenc.set_input(0, &emb_x)?;
        self.textenc.set_input(1, &tmask)?;
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

        // 4. Length-regulate mu: frame f belongs to the first token whose
        //    cumulative duration exceeds f (float compare, per the reference's
        //    sequence_mask-based generate_path).
        let mut ymask = vec![0.0f32; cfg.max_mel];
        ymask[..y_lengths].fill(1.0);
        let mut mu_y = vec![0.0f32; cfg.n_feats * cfg.max_mel];
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
            }
        }

        // 5. Initial state x₀ (masked), then the host Euler ODE over the CFM
        //    vector field. Decoder inputs are positional: (x, mu, t_emb, ymask).
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
            let t_emb = Self::time_embedding(t);
            self.decoder.set_input(0, &x)?;
            self.decoder.set_input(1, &mu_y)?;
            self.decoder.set_input(2, &t_emb)?;
            self.decoder.set_input(3, &ymask)?;
            self.decoder.invoke()?;
            self.decoder.read_output(0, &mut v)?;
            for (xi, vi) in x.iter_mut().zip(v.iter()) {
                *xi += dt * vi;
            }
            t += dt;
        }

        // 6. Denormalize mel (masked), apply the per-frame energy envelope
        //    (dB → natural-log mel units), vocode, clip, trim.
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

    const MODEL_DIR: &str = "../../../Registry/Sonora/v1-ljspeech/litert-split";
    const FIXTURE_DIR: &str = "../../target/split_ref";

    /// Parity against the Python reference implementation of the
    /// litert-samples host pipeline (same graphs, same fixed noise). Skips
    /// when the registry clone or the generated fixtures are absent.
    #[test]
    fn test_split_parity_vs_reference() {
        let dir = Path::new(MODEL_DIR);
        let meta_path = format!("{FIXTURE_DIR}/meta.json");
        if !is_split_model_dir(dir) || !Path::new(&meta_path).exists() {
            println!("Skipping: split model dir or fixtures missing");
            return;
        }
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(meta_path).unwrap()).unwrap();
        let ids: Vec<i32> = meta["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let y_ref = meta["y_lengths"].as_i64().unwrap() as usize;

        let read_f32 = |p: String| -> Vec<f32> {
            std::fs::read(p)
                .unwrap()
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        };
        let z = read_f32(format!("{FIXTURE_DIR}/z.bin"));
        let wav_ref = read_f32(format!("{FIXTURE_DIR}/wav_ref.bin"));

        let engine = SplitGraphEngine::new(dir).expect("split engine init");
        // The fixture z is pre-scaled (×temperature) and masked by the
        // reference; pass it through unmodified.
        let out = engine
            .forward(&ids, 1.0, None, None, 1.0, Some(&z))
            .expect("split forward");

        assert_eq!(
            out.audio.len(),
            y_ref * engine.cfg.hop,
            "frame count mismatch: rust {} frames vs python {y_ref}",
            out.audio.len() / engine.cfg.hop
        );
        let n = out.audio.len().min(wav_ref.len());
        let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (a, b) = (out.audio[i] as f64, wav_ref[i] as f64);
            dot += a * b;
            na += a * a;
            nb += b * b;
        }
        let cosine = dot / (na.sqrt() * nb.sqrt()).max(1e-12);
        println!(
            "split parity: frames={} samples={} cosine={:.6}",
            y_ref, n, cosine
        );
        assert!(cosine > 0.999, "cosine {cosine} below parity threshold");

        // Per-token duration dictation sanity: doubling every token's scale
        // should roughly double the realized frame count.
        let ds = vec![2.0f32; ids.len()];
        let out2 = engine
            .forward(&ids, 1.0, Some(&ds), None, 1.0, Some(&z))
            .expect("split forward with duration scales");
        let frames1 = out.audio.len() / engine.cfg.hop;
        let frames2 = out2.audio.len() / engine.cfg.hop;
        println!("duration dictation: {frames1} -> {frames2} frames at 2x");
        assert!(
            (frames2 as f32) > (frames1 as f32) * 1.8,
            "duration_scales had insufficient effect: {frames1} -> {frames2}"
        );

        // Energy hook: a flat −6 dB mel envelope must move output RMS by
        // ≈ −6 dB (the vocoder is linear in log-mel gain; measured 0.1 dB
        // tolerance, we allow 0.5 here for fp16 graphs).
        let env = vec![-6.0f32; engine.cfg.max_mel];
        let out3 = engine
            .forward(&ids, 1.0, None, Some(&env), 1.0, Some(&z))
            .expect("split forward with mel gain");
        let rms = |a: &[f32]| {
            (a.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / a.len() as f64).sqrt()
        };
        let delta_db = 20.0 * (rms(&out3.audio) / rms(&out.audio)).log10();
        println!("mel-gain hook: requested -6.0 dB, measured {delta_db:.2} dB");
        assert!(
            (delta_db + 6.0).abs() < 0.5,
            "mel-gain inaccurate: requested -6 dB, measured {delta_db:.2} dB"
        );
    }
}
