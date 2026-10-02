use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};
use once_cell::sync::Lazy;
use stage::prosody_payload::ProsodySpan;
use crate::g2p::{ProsodiaG2PProcessor, TokenPhonemes, MToken};
use crate::asset_manager::StyleVector;
use crate::engine::ProsodiaSpeechEngine;
use crate::voice_loader::VoiceLoader;

static WARNED_PHONEMES: Lazy<Mutex<HashSet<char>>> = Lazy::new(|| Mutex::new(HashSet::new()));

fn warn_unknown_phoneme(c: char, is_alignment: bool) {
    if let Ok(mut warned) = WARNED_PHONEMES.lock() {
        if warned.insert(c) {
            if is_alignment {
                eprintln!("Warning: unknown phoneme character dropped from vocabulary in alignment: {:?}", c);
            } else {
                eprintln!("Warning: unknown phoneme character dropped from vocabulary: {:?}", c);
            }
        }
    }
}


#[derive(Clone, Debug, uniffi::Record)]
pub struct PipelineOutput {
    pub phonemes: Vec<TokenPhonemes>,
    pub style: StyleVector,
    pub speed_multiplier: f64,
    pub gain_multiplier: f64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct WordTimestamp {
    pub word: String,
    pub start_char: u32,
    pub end_char: u32,
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SynthesisResult {
    pub graphemes: String,
    pub phonemes: String,
    pub audio: Vec<f32>,
    pub sample_rate: u32,
    pub timestamps: Option<Vec<WordTimestamp>>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PipelineError {
    #[error("json parse error: {msg}")]
    JsonParse { msg: String },
    #[error("speech engine error: {msg}")]
    SpeechEngine { msg: String },
    #[error("voice loader error: {msg}")]
    VoiceLoader { msg: String },
    #[error("invalid speed: {speed}")]
    InvalidSpeed { speed: f32 },
}

#[derive(serde::Deserialize)]
struct StyleTTS2Config {
    /// Engine-contract form: explicit symbol → id map.
    #[serde(default)]
    vocab: Option<HashMap<String, i32>>,
    /// Split-graph config form: ordered symbol list (id = index).
    #[serde(default)]
    symbols: Option<Vec<String>>,
    #[serde(default)]
    is_matcha_ipa: Option<bool>,
}

impl StyleTTS2Config {
    fn into_vocab(self) -> Result<(HashMap<String, i32>, Option<bool>), String> {
        if let Some(vocab) = self.vocab {
            return Ok((vocab, self.is_matcha_ipa));
        }
        if let Some(symbols) = self.symbols {
            let vocab = symbols
                .into_iter()
                .enumerate()
                .map(|(i, s)| (s, i as i32))
                .collect();
            return Ok((vocab, self.is_matcha_ipa));
        }
        Err("config.json has neither a \"vocab\" map nor a \"symbols\" list".to_string())
    }
}

#[uniffi::export(callback_interface)]
pub trait AudioChunkCallback: Send + Sync {
    fn on_audio_chunk(&self, chunk: Vec<f32>);
}

pub fn map_char_to_matcha_ipa(c: char) -> Option<&'static str> {
    match c {
        'A' => Some("eɪ"),
        'I' => Some("aɪ"),
        'O' => Some("oʊ"),
        'Q' => Some("əʊ"),
        'W' => Some("aʊ"),
        'Y' => Some("ɔɪ"),
        'ʤ' => Some("dʒ"),
        'ʧ' => Some("tʃ"),
        'ɐ' => Some("ə"),
        'ᵻ' => Some("ɪ"),
        'ᵊ' => Some("ə"),
        _ => None,
    }
}

pub fn map_styletts2_to_matcha_ipa(phonemes: &str) -> String {
    let mut mapped = String::with_capacity(phonemes.len());
    for c in phonemes.chars() {
        if let Some(rep) = map_char_to_matcha_ipa(c) {
            mapped.push_str(rep);
        } else {
            mapped.push(c);
        }
    }
    mapped
}

#[derive(uniffi::Object)]
pub struct ProsodiaActorPipeline {
    g2p: Mutex<Box<dyn ProsodiaG2PProcessor>>,
    voice_loader: Arc<VoiceLoader>,
    vocab: HashMap<String, i32>,
    sample_rate: u32,
    lang_code: String,
    is_matcha_ipa: bool,
}

#[uniffi::export]
impl ProsodiaActorPipeline {
    #[uniffi::constructor]
    pub fn new(
        g2p: Box<dyn ProsodiaG2PProcessor>,
        voice_loader: Arc<VoiceLoader>,
        config_json: String,
        sample_rate: u32,
        lang_code: String,
    ) -> Result<Arc<Self>, PipelineError> {
        let config: StyleTTS2Config = serde_json::from_str(&config_json)
            .map_err(|e| PipelineError::JsonParse { msg: e.to_string() })?;
        let (vocab, is_matcha_ipa) = config
            .into_vocab()
            .map_err(|msg| PipelineError::JsonParse { msg })?;
        Ok(Arc::new(Self {
            g2p: Mutex::new(g2p),
            voice_loader,
            vocab,
            sample_rate,
            lang_code,
            is_matcha_ipa: is_matcha_ipa.unwrap_or(true),
        }))
    }

    pub fn set_custom_g2p(&self, processor: Box<dyn ProsodiaG2PProcessor>) {
        let mut g2p = self.g2p.lock().unwrap();
        *g2p = processor;
    }

    // Retained for the FFI surface (Swift/Kotlin bindings); the pipeline no longer
    // maps — G2P processors emit Matcha IPA. Removal belongs with Phase B's app work.
    // A plain comment, not `///`: uniffi 0.27 checksums doc comments, and the
    // committed bindings check this method's checksum at startup
    // (see ffi_checksums.rs).
    pub fn should_map_ipa(&self, is_matcha: bool) -> bool {
        is_matcha && self.is_matcha_ipa
    }

    pub fn tokenize_phonemes(&self, phonemes: String, is_matcha: bool) -> Vec<i32> {
        self.tokenize(&phonemes, is_matcha)
    }

    fn tokenize(&self, phonemes: &str, is_matcha: bool) -> Vec<i32> {
        let mut ids = Vec::new();
        ids.push(0); // 0-bound padding identical to standard StyleTTS2 G2P tokenizer format
        let mapped = phonemes.to_string();
        for c in mapped.chars() {
            if let Some(&id) = self.vocab.get(&c.to_string()) {
                ids.push(id);
                if is_matcha {
                    // Matcha trains with add_blank=True — blank id 0 interspersed
                    // between every symbol (matcha.utils.intersperse), so the model
                    // expects [0, p1, 0, p2, …, pn, 0]. Feeding compact ids to a
                    // blank-trained checkpoint yields fast, garbled speech.
                    ids.push(0);
                }
            } else {
                warn_unknown_phoneme(c, false);
            }
        }
        if !is_matcha {
            ids.push(0); // 0-bound padding identical to standard StyleTTS2 G2P tokenizer format
        }
        ids
    }

    pub fn process_span(&self, span: ProsodySpan) -> PipelineOutput {
        let mtokens = self.g2p.lock().unwrap().process(span.text);
        let mut phonemes = Vec::new();
        for m in mtokens {
            if let Some(p) = m.phonemes {
                phonemes.push(TokenPhonemes {
                    phonemes: p,
                    whitespace: m.whitespace,
                });
            }
        }

        let mut speed = 1.0;
        let mut gain = 1.0;
        let mut style = StyleVector { data: vec![0.0; 64], shape: vec![64] };

        if let Some(acoustics) = span.acoustics {
            if let Some(s) = acoustics.speed_multiplier {
                speed = s;
            }
            if let Some(g) = acoustics.gain_multiplier {
                gain = g;
            }
            if let Some(profile) = acoustics.casting_profile {
                if let Ok(res_style) = self.voice_loader.resolve_parametric_voice(&profile) {
                    style = res_style;
                } else {
                    style = StyleVector { data: vec![profile.age_profile as f32; 64], shape: vec![64] };
                }
            }
        }

        PipelineOutput {
            phonemes,
            style,
            speed_multiplier: speed,
            gain_multiplier: gain,
        }
    }

    pub fn synthesize(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        text: String,
        voice: String,
        speed: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<SynthesisResult, PipelineError> {
        let tokens = self.g2p.lock().unwrap().process(text.clone());
        let voice_blends = vec![voice; tokens.len()];
        self.synthesize_with_timestamps_blend(
            speech_engine,
            text,
            voice_blends,
            speed,
            0.0,
            duration_scales,
            f0_bias,
        )
    }

    pub fn synthesize_with_timestamps(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        text: String,
        voice: String,
        speed: f32,
        pitch: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<SynthesisResult, PipelineError> {
        let tokens = self.g2p.lock().unwrap().process(text.clone());
        let voice_blends = vec![voice; tokens.len()];
        self.synthesize_with_timestamps_blend(
            speech_engine,
            text,
            voice_blends,
            speed,
            pitch,
            duration_scales,
            f0_bias,
        )
    }

    pub fn synthesize_with_timestamps_blend(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        text: String,
        voice_blends: Vec<String>,
        speed: f32,
        pitch: f32,
        duration_scales: Option<Vec<f32>>,
        f0_bias: Option<Vec<f32>>,
    ) -> Result<SynthesisResult, PipelineError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(PipelineError::InvalidSpeed { speed });
        }

        let is_matcha = speech_engine.is_matcha();
        let token_limit = speech_engine.get_token_limit();

        let tokens = self.g2p.lock().unwrap().process(text.clone());
        let token_chunks = self.chunk_tokens_for(&tokens, token_limit, is_matcha);
        let frame_duration = 512.0 / self.sample_rate as f64;

        let mut total_audio = Vec::new();
        let mut word_timestamps = Vec::new();
        let mut audio_time_offset = 0.0;
        let mut char_offset = 0u32;

        let mut token_offset = 0;
        for chunk in token_chunks {
            let mut chunk_phonemes = String::new();
            for token in &chunk {
                if let Some(ref p) = token.phonemes {
                    chunk_phonemes.push_str(p);
                }
                chunk_phonemes.push_str(&token.whitespace);
            }
            let trimmed_phonemes = chunk_phonemes.trim();
            if trimmed_phonemes.is_empty() {
                continue;
            }

            let start_blend = token_offset.min(voice_blends.len().saturating_sub(1));
            let chunk_casting_profiles = voice_blends[start_blend..]
                .iter()
                .take(chunk.len())
                .cloned()
                .collect::<Vec<_>>();

            let token_phonemes_list = chunk
                .iter()
                .map(|t| TokenPhonemes {
                    phonemes: t.phonemes.clone().unwrap_or_default(),
                    whitespace: t.whitespace.clone(),
                })
                .collect::<Vec<_>>();

            let style = self
                .voice_loader
                .style_matrix(
                    token_phonemes_list,
                    if chunk_casting_profiles.is_empty() {
                        vec!["".to_string()]
                    } else {
                        chunk_casting_profiles
                    },
                    self.vocab.clone(),
                )
                .map_err(|e| PipelineError::VoiceLoader {
                    msg: e.to_string(),
                })?;

            let ids = self.tokenize(&trimmed_phonemes, is_matcha);

            let mut chunk_duration_scales = None;
            if let Some(ref d_scales) = duration_scales {
                let mut scales = vec![1.0f32; ids.len()];
                let mut p_idx = 1;
                for (t_idx, token) in chunk.iter().enumerate() {
                    let global_token_idx = token_offset + t_idx;
                    let scale = d_scales.get(global_token_idx).copied().unwrap_or(1.0);

                    let word_phonemes = token.phonemes.as_deref().unwrap_or("");
                    let mapped_word_phonemes = word_phonemes.to_string();
                    for char in mapped_word_phonemes.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                scales[p_idx] = scale;
                                p_idx += 1;
                            }
                        }
                    }
                    let mapped_whitespace = token.whitespace.to_string();
                    for char in mapped_whitespace.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                scales[p_idx] = scale;
                                p_idx += 1;
                            }
                        }
                    }
                }
                smooth_parameters(&mut scales, 5);
                chunk_duration_scales = Some(scales);
            }

            let resolved_f0_bias = if let Some(ref biases) = f0_bias {
                let mut chunk_biases = vec![0.0f32; ids.len()];
                let mut p_idx = 1;
                for (t_idx, token) in chunk.iter().enumerate() {
                    let global_token_idx = token_offset + t_idx;
                    let bias = biases.get(global_token_idx).copied().unwrap_or(0.0);

                    let word_phonemes = token.phonemes.as_deref().unwrap_or("");
                    let mapped_word_phonemes = word_phonemes.to_string();
                    for char in mapped_word_phonemes.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                chunk_biases[p_idx] = bias;
                                p_idx += 1;
                            }
                        }
                    }
                    let mapped_whitespace = token.whitespace.to_string();
                    for char in mapped_whitespace.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                chunk_biases[p_idx] = bias;
                                p_idx += 1;
                            }
                        }
                    }
                }
                smooth_parameters(&mut chunk_biases, 5);
                Some(chunk_biases)
            } else if pitch != 0.0 {
                let mut biases = vec![pitch; ids.len()];
                smooth_parameters(&mut biases, 5);
                Some(biases)
            } else {
                None
            };

            let output = speech_engine
                .forward(
                    self.tokenize(&trimmed_phonemes, is_matcha),
                    style,
                    speed,
                    None,
                    chunk_duration_scales,
                    resolved_f0_bias,
                )
                .map_err(|e| PipelineError::SpeechEngine {
                    msg: e.to_string(),
                })?;

            let pred_dur = output.pred_dur;
            let mut token_idx = 1;
            let mut current_time = pred_dur.get(0).copied().unwrap_or(0) as f64 * frame_duration;

            for token in &chunk {
                let word_text = &token.text;
                let word_phonemes = token.phonemes.as_deref().unwrap_or("");
                let whitespace = &token.whitespace;

                let word_start_char_offset = char_offset;
                char_offset += (word_text.chars().count() + whitespace.chars().count()) as u32;

                let word_start_time = audio_time_offset + current_time;

                let mapped_word_phonemes = word_phonemes.to_string();
                for char in mapped_word_phonemes.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let mapped_whitespace = whitespace.to_string();
                for char in mapped_whitespace.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let word_end_time = audio_time_offset + current_time;

                let clean_word = word_text.trim();
                if !clean_word.is_empty()
                    && !word_phonemes.is_empty()
                    && token.tag != "."
                    && token.tag != ","
                    && token.tag != ":"
                {
                    word_timestamps.push(WordTimestamp {
                        word: clean_word.to_string(),
                        start_char: word_start_char_offset,
                        end_char: char_offset,
                        start_time: word_start_time,
                        end_time: word_end_time,
                    });
                }
            }

            total_audio.extend_from_slice(&output.audio);
            audio_time_offset += output.audio.len() as f64 / self.sample_rate as f64;
            token_offset += chunk.len();
        }

        let full_phonemes = tokens
            .iter()
            .map(|token| {
                format!(
                    "{}{}",
                    token.phonemes.as_deref().unwrap_or(""),
                    token.whitespace
                )
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();

        limit_audio(&mut total_audio);

        Ok(SynthesisResult {
            graphemes: text,
            phonemes: full_phonemes,
            audio: total_audio,
            sample_rate: self.sample_rate,
            timestamps: Some(word_timestamps),
        })
    }

    pub fn synthesize_markup(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        markup_text: String,
        voice: String,
        speed: f32,
    ) -> Result<SynthesisResult, PipelineError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(PipelineError::InvalidSpeed { speed });
        }

        let is_matcha = speech_engine.is_matcha();
        let token_limit = speech_engine.get_token_limit();

        let parsed = stage::markup_parser::parse_markup(markup_text);
        let clean_text = parsed.clean_text;
        let character_prosody = parsed.character_prosody;

        let tokens = self.g2p.lock().unwrap().process(clean_text.clone());
        let token_chunks = self.chunk_tokens_for(&tokens, token_limit, is_matcha);
        let frame_duration = 512.0 / self.sample_rate as f64;

        let mut total_audio = Vec::new();
        let mut word_timestamps = Vec::new();
        let mut audio_time_offset = 0.0;
        let mut char_offset = 0u32;

        let mut last_style: Option<StyleVector> = None;

        for chunk in token_chunks {
            let mut raw_phonemes = String::new();
            let mut raw_states = Vec::new();

            let mut word_start_char_offset = char_offset;
            for token in &chunk {
                let token_state = if (word_start_char_offset as usize) < character_prosody.len() {
                    character_prosody[word_start_char_offset as usize].clone()
                } else {
                    stage::markup_parser::ProsodyState::default()
                };

                let phonemes = token.phonemes.as_deref().unwrap_or("");
                let phonemes_and_space = format!("{}{}", phonemes, token.whitespace);
                for c in phonemes_and_space.chars() {
                    raw_phonemes.push(c);
                    raw_states.push(token_state.clone());
                }

                word_start_char_offset += (token.text.chars().count() + token.whitespace.chars().count()) as u32;
            }

            let mut start_idx = 0;
            let raw_chars: Vec<char> = raw_phonemes.chars().collect();
            while start_idx < raw_chars.len() && raw_chars[start_idx].is_whitespace() {
                start_idx += 1;
            }
            let mut end_idx = raw_chars.len();
            while end_idx > start_idx && raw_chars[end_idx - 1].is_whitespace() {
                end_idx -= 1;
            }

            let trimmed_phonemes: String = raw_chars[start_idx..end_idx].iter().collect();
            if trimmed_phonemes.is_empty() {
                char_offset = word_start_char_offset;
                continue;
            }

            let trimmed_states = &raw_states[start_idx..end_idx];

            let mut filtered_states = Vec::new();
            let mapped_phonemes = trimmed_phonemes.clone();
            for (idx, c) in trimmed_phonemes.chars().enumerate() {
                if self.vocab.contains_key(&c.to_string()) {
                    filtered_states.push(trimmed_states[idx].clone());
                } else {
                    warn_unknown_phoneme(c, true);
                }
            }

            let mut final_states = Vec::new();
            final_states.push(stage::markup_parser::ProsodyState::default());
            final_states.extend(filtered_states);
            final_states.push(stage::markup_parser::ProsodyState::default());

            let mut duration_scales: Vec<f32> = final_states.iter().map(|s| 1.0 / s.rate).collect();
            let mut f0_bias: Vec<f32> = final_states.iter().map(|s| s.pitch).collect();

            smooth_parameters(&mut duration_scales, 5);
            smooth_parameters(&mut f0_bias, 5);

            let mut style = self
                .voice_loader
                .style_vector(voice.clone(), trimmed_phonemes.chars().count() as i64)
                .map_err(|e| PipelineError::VoiceLoader {
                    msg: e.to_string(),
                })?;

            if let Some(ref prev) = last_style {
                if style.data.len() == prev.data.len() {
                    for (s, p) in style.data.iter_mut().zip(prev.data.iter()) {
                        *s = 0.5 * *s + 0.5 * p;
                    }
                }
            }
            last_style = Some(style.clone());

            let output = speech_engine
                .forward(
                    self.tokenize(&mapped_phonemes, is_matcha),
                    style,
                    speed,
                    None,
                    Some(duration_scales),
                    Some(f0_bias),
                )
                .map_err(|e| PipelineError::SpeechEngine {
                    msg: e.to_string(),
                })?;

            let pred_dur = output.pred_dur;
            let mut token_idx = 1;
            let mut current_time = pred_dur.get(0).copied().unwrap_or(0) as f64 * frame_duration;

            for token in &chunk {
                let word_text = &token.text;
                let word_phonemes = token.phonemes.as_deref().unwrap_or("");
                let whitespace = &token.whitespace;

                let word_start_char_offset = char_offset;
                char_offset += (word_text.chars().count() + whitespace.chars().count()) as u32;

                let word_start_time = audio_time_offset + current_time;

                let mapped_word_phonemes = word_phonemes.to_string();
                for char in mapped_word_phonemes.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let mapped_whitespace = whitespace.to_string();
                for char in mapped_whitespace.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let word_end_time = audio_time_offset + current_time;


                let clean_word = word_text.trim();
                if !clean_word.is_empty()
                    && !word_phonemes.is_empty()
                    && token.tag != "."
                    && token.tag != ","
                    && token.tag != ":"
                {
                    word_timestamps.push(WordTimestamp {
                        word: clean_word.to_string(),
                        start_char: word_start_char_offset,
                        end_char: char_offset,
                        start_time: word_start_time,
                        end_time: word_end_time,
                    });
                }
            }

            total_audio.extend_from_slice(&output.audio);
            audio_time_offset += output.audio.len() as f64 / self.sample_rate as f64;
        }

        let full_phonemes = tokens
            .iter()
            .map(|token| {
                format!(
                    "{}{}",
                    token.phonemes.as_deref().unwrap_or(""),
                    token.whitespace
                )
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();

        limit_audio(&mut total_audio);

        Ok(SynthesisResult {
            graphemes: clean_text,
            phonemes: full_phonemes,
            audio: total_audio,
            sample_rate: self.sample_rate,
            timestamps: Some(word_timestamps),
        })
    }

    pub fn synthesize_stream(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        text: String,
        voice: String,
        speed: f32,
        callback: Box<dyn AudioChunkCallback>,
    ) -> Result<(), PipelineError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(PipelineError::InvalidSpeed { speed });
        }

        let is_matcha = speech_engine.is_matcha();
        let token_limit = speech_engine.get_token_limit();

        let tokens = self.g2p.lock().unwrap().process(text);
        let full_phonemes = tokens
            .iter()
            .map(|token| {
                format!(
                    "{}{}",
                    token.phonemes.as_deref().unwrap_or(""),
                    token.whitespace
                )
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();

        let chunks = self.chunk_phonemes_for(&full_phonemes, token_limit, is_matcha);

        let mut last_style: Option<StyleVector> = None;

        for chunk in chunks {
            if chunk.is_empty() {
                continue;
            }
            let mut style = self
                .voice_loader
                .style_vector(voice.clone(), chunk.chars().count() as i64)
                .map_err(|e| PipelineError::VoiceLoader {
                    msg: e.to_string(),
                })?;

            if let Some(ref prev) = last_style {
                if style.data.len() == prev.data.len() {
                    for (s, p) in style.data.iter_mut().zip(prev.data.iter()) {
                        *s = 0.5 * *s + 0.5 * p;
                    }
                }
            }
            last_style = Some(style.clone());

            let output = speech_engine
                .forward(self.tokenize(&chunk, is_matcha), style, speed, None, None, None)
                .map_err(|e| PipelineError::SpeechEngine {
                    msg: e.to_string(),
                })?;

            callback.on_audio_chunk(output.audio);
        }

        Ok(())
    }

    pub fn synthesize_stream_with_morph(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        text: String,
        voice_blends: Vec<Vec<crate::voice_loader::VoiceBlend>>,
        speed: f32,
        callback: Box<dyn AudioChunkCallback>,
    ) -> Result<(), PipelineError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(PipelineError::InvalidSpeed { speed });
        }

        let is_matcha = speech_engine.is_matcha();
        let token_limit = speech_engine.get_token_limit();

        let tokens = self.g2p.lock().unwrap().process(text);
        let full_phonemes = tokens
            .iter()
            .map(|token| {
                format!(
                    "{}{}",
                    token.phonemes.as_deref().unwrap_or(""),
                    token.whitespace
                )
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();

        let chunks = self.chunk_phonemes_for(&full_phonemes, token_limit, is_matcha);

        let mut last_style: Option<StyleVector> = None;

        for (idx, chunk) in chunks.iter().enumerate() {
            if chunk.is_empty() {
                continue;
            }
            let blend_recipe = if idx < voice_blends.len() {
                voice_blends[idx].clone()
            } else {
                voice_blends.last().cloned().unwrap_or_default()
            };

            let pack = self
                .voice_loader
                .load_blend(blend_recipe)
                .map_err(|e| PipelineError::VoiceLoader {
                    msg: e.to_string(),
                })?;

            let mut style = crate::voice_loader::slice_style_row(pack, chunk.chars().count() as i64)
                .map_err(|e| PipelineError::VoiceLoader {
                    msg: e.to_string(),
                })?;

            if let Some(ref prev) = last_style {
                if style.data.len() == prev.data.len() {
                    for (s, p) in style.data.iter_mut().zip(prev.data.iter()) {
                        *s = 0.5 * *s + 0.5 * p;
                    }
                }
            }
            last_style = Some(style.clone());

            let output = speech_engine
                .forward(self.tokenize(&chunk, is_matcha), style, speed, None, None, None)
                .map_err(|e| PipelineError::SpeechEngine {
                    msg: e.to_string(),
                })?;

            callback.on_audio_chunk(output.audio);
        }

        Ok(())
    }

    pub fn prewarm(
        &self,
        speech_engine: Box<dyn ProsodiaSpeechEngine>,
        voice: String,
    ) -> Result<(), PipelineError> {
        let _ = self.synthesize(
            speech_engine,
            "a".to_string(),
            voice,
            1.0,
            None,
            None,
        )?;
        Ok(())
    }

    pub fn reclaim_memory(&self, speech_engine: Box<dyn ProsodiaSpeechEngine>) {
        speech_engine.reclaim_memory();
    }

    pub fn chunk_phonemes(&self, phonemes: &str, limit: u32) -> Vec<String> {
        split_phonemes(phonemes, limit as usize, |_| 1)
    }

    fn chunk_tokens(&self, tokens: &[MToken], limit: u32) -> Vec<Vec<MToken>> {
        group_tokens(tokens, limit as usize, |_| 1)
    }
}

// Not exported: chunking against an engine's token limit, which counts the ids
// `tokenize` emits for that engine, as `ProsodiaActorEngine::process_and_synthesize`
// does on the app path.
impl ProsodiaActorPipeline {
    /// Chunks a phoneme string so each chunk tokenizes to at most `limit` ids.
    fn chunk_phonemes_for(&self, phonemes: &str, limit: i32, is_matcha: bool) -> Vec<String> {
        split_phonemes(phonemes, self.id_budget(limit, is_matcha), |c| self.symbol_ids(c, is_matcha))
    }

    /// Groups tokens so each chunk tokenizes to at most `limit` ids.
    fn chunk_tokens_for(&self, tokens: &[MToken], limit: i32, is_matcha: bool) -> Vec<Vec<MToken>> {
        group_tokens(tokens, self.id_budget(limit, is_matcha), |c| self.symbol_ids(c, is_matcha))
    }

    /// The ids left for symbols once `tokenize` has framed a chunk (Matcha:
    /// a leading blank; StyleTTS2: a bound at each end). A limit of 0 or less
    /// means no limit.
    fn id_budget(&self, limit: i32, is_matcha: bool) -> usize {
        if limit <= 0 {
            return usize::MAX;
        }
        (limit as usize).saturating_sub(self.tokenize("", is_matcha).len())
    }

    /// The ids `tokenize` emits for one symbol: none when it is outside the
    /// vocabulary, otherwise one, and for Matcha a blank after it.
    fn symbol_ids(&self, c: char, is_matcha: bool) -> usize {
        let mut buf = [0u8; 4];
        self.tokenize(c.encode_utf8(&mut buf), is_matcha).len() - self.tokenize("", is_matcha).len()
    }
}

/// Splits a phoneme string into chunks whose symbols cost at most `budget`.
///
/// When a chunk would end mid-word, the split is pulled back to the last break
/// character past the chunk's midpoint; without one it is cut at the budget. A
/// symbol that alone exceeds the budget gets a chunk of its own. Chunks are
/// whitespace-trimmed and empty ones dropped.
fn split_phonemes(phonemes: &str, budget: usize, cost: impl Fn(char) -> usize) -> Vec<String> {
    let trimmed = phonemes.trim();
    let characters: Vec<char> = trimmed.chars().collect();
    let costs: Vec<usize> = characters.iter().map(|&c| cost(c)).collect();
    if costs.iter().sum::<usize>() <= budget {
        return if trimmed.is_empty() { vec![] } else { vec![trimmed.to_string()] };
    }

    let break_characters = [' ', '.', ',', ';', ':', '!', '?', '—', '…'];

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < characters.len() {
        let mut end_limit = start;
        let mut used = 0;
        while end_limit < characters.len() && used + costs[end_limit] <= budget {
            used += costs[end_limit];
            end_limit += 1;
        }
        let end_limit = end_limit.max(start + 1);
        if end_limit == characters.len() {
            let chunk_str: String = characters[start..end_limit].iter().collect();
            chunks.push(chunk_str.trim().to_string());
            break;
        }

        let mut split_index = end_limit;
        let mut cursor = end_limit - 1;
        while cursor > start + (end_limit - start) / 2 {
            if break_characters.contains(&characters[cursor]) {
                split_index = cursor + 1;
                break;
            }
            cursor -= 1;
        }

        let chunk_str: String = characters[start..split_index].iter().collect();
        chunks.push(chunk_str.trim().to_string());
        start = split_index;
        while start < characters.len() && characters[start].is_whitespace() {
            start += 1;
        }
    }

    chunks.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Groups tokens into chunks whose phonemes and whitespace cost at most
/// `budget`. A token that alone exceeds the budget gets a chunk of its own.
fn group_tokens(tokens: &[MToken], budget: usize, cost: impl Fn(char) -> usize) -> Vec<Vec<MToken>> {
    let mut chunks = Vec::new();
    let mut current_chunk = Vec::new();
    let mut current_len = 0;

    for token in tokens {
        let token_len: usize = token.phonemes.as_deref().unwrap_or("").chars().chain(token.whitespace.chars()).map(&cost).sum();
        if current_len + token_len > budget && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = vec![token.clone()];
            current_len = token_len;
        } else {
            current_chunk.push(token.clone());
            current_len += token_len;
        }
    }
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }
    chunks
}

fn limit_audio(samples: &mut [f32]) {
    let threshold: f32 = 0.85;
    let ceiling: f32 = 0.98;
    let diff = ceiling - threshold;
    for x in samples.iter_mut() {
        let abs_x = x.abs();
        if abs_x > threshold {
            let sign = if *x >= 0.0 { 1.0 } else { -1.0 };
            *x = sign * (threshold + diff * ((abs_x - threshold) / diff).tanh());
        }
    }
}

fn smooth_parameters(values: &mut [f32], window_size: usize) {
    if values.len() <= 1 || window_size <= 1 {
        return;
    }
    let original = values.to_vec();
    let half = window_size / 2;
    for i in 0..values.len() {
        let start = i.saturating_sub(half);
        let end = (i + half + 1).min(original.len());
        let sum: f32 = original[start..end].iter().sum();
        values[i] = sum / (end - start) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ActorEngineOutput, SpeechEngineError};

    struct MockG2P;
    impl ProsodiaG2PProcessor for MockG2P {
        fn process(&self, text: String) -> Vec<MToken> {
            text.split_whitespace()
                .map(|w| MToken {
                    text: w.to_string(),
                    tag: "".to_string(),
                    whitespace: " ".to_string(),
                    phonemes: Some(w.to_string()),
                })
                .collect()
        }
    }

    struct MockSpeechEngine;
    impl ProsodiaSpeechEngine for MockSpeechEngine {
        fn synthesize(&self, _input: PipelineOutput) -> ActorEngineOutput {
            ActorEngineOutput {
                audio: vec![0.0; 24],
                pred_dur: vec![8; 2],
            }
        }

        fn forward(
            &self,
            phoneme_ids: Vec<i32>,
            _style: StyleVector,
            _speed: f32,
            _vat: Option<Vec<f32>>,
            _duration_scales: Option<Vec<f32>>,
            _f0_bias: Option<Vec<f32>>,
        ) -> Result<ActorEngineOutput, SpeechEngineError> {
            let count = phoneme_ids.len();
            Ok(ActorEngineOutput {
                audio: vec![0.1; 100],
                pred_dur: vec![8; count],
            })
        }

        fn reclaim_memory(&self) {}

        fn is_matcha(&self) -> bool {
            false
        }
    }

    struct MockAssetProvider;
    impl crate::voice_loader::VoiceAssetProvider for MockAssetProvider {
        fn load_voice_bytes(&self, _voice_name: String) -> Option<Vec<u8>> {
            let mut payload = Vec::new();
            for f in &[1.0f32, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0] {
                payload.extend_from_slice(&f.to_le_bytes());
            }
            let header = r#"{"style":{"dtype":"F32","shape":[4,2],"data_offsets":[0,32]}}"#;
            let header_bytes = header.as_bytes();
            let mut out = Vec::new();
            out.extend_from_slice(&(header_bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(header_bytes);
            out.extend_from_slice(&payload);
            Some(out)
        }
    }

    #[test]
    fn test_pipeline_synthesize_end_to_end() {
        let g2p = Box::new(MockG2P);
        let loader = VoiceLoader::new(Box::new(MockAssetProvider));
        let config_json = r#"{"vocab":{"h":1,"e":2,"l":3,"o":4,"w":5,"r":6,"d":7}}"#;
        
        let pipeline = ProsodiaActorPipeline::new(
            g2p,
            loader,
            config_json.to_string(),
            24000,
            "en-us".to_string(),
        ).unwrap();

        let engine = Box::new(MockSpeechEngine);
        let res = pipeline.synthesize(
            engine,
            "hello world".to_string(),
            "v".to_string(),
            1.0,
            None,
            None,
        ).unwrap();

        assert_eq!(res.graphemes, "hello world");
        assert!(!res.audio.is_empty());
        assert!(res.timestamps.unwrap().len() > 0);
    }

    #[test]
    fn test_smooth_parameters_basic() {
        let mut values = vec![1.0, 1.0, 5.0, 1.0, 1.0];
        smooth_parameters(&mut values, 3);
        assert!((values[0] - 1.0).abs() < 1e-5);
        assert!((values[1] - 2.3333333).abs() < 1e-5);
        assert!((values[2] - 2.3333333).abs() < 1e-5);
        assert!((values[3] - 2.3333333).abs() < 1e-5);
        assert!((values[4] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_custom_matcha_direct_tokenization() {
        let g2p = Box::new(MockG2P);
        let loader = VoiceLoader::new(Box::new(MockAssetProvider));
        let config_json = r#"{"vocab":{"A":1,"B":2},"is_matcha_ipa":false}"#;
        
        let pipeline = ProsodiaActorPipeline::new(
            g2p,
            loader,
            config_json.to_string(),
            24000,
            "en-us".to_string(),
        ).unwrap();

        assert!(!pipeline.is_matcha_ipa);

        let ids = pipeline.tokenize_phonemes("A".to_string(), true);
        assert_eq!(ids, vec![0, 1, 0]);

        // Matcha checkpoints train with add_blank=True (matcha.utils.intersperse):
        // a blank id 0 between every symbol, not just at the bounds.
        let ids = pipeline.tokenize_phonemes("AB".to_string(), true);
        assert_eq!(ids, vec![0, 1, 0, 2, 0]);

        // StyleTTS2 tokenization keeps plain 0-bounds with no interspersion.
        let ids = pipeline.tokenize_phonemes("AB".to_string(), false);
        assert_eq!(ids, vec![0, 1, 2, 0]);
    }

    #[test]
    fn test_warn_unknown_phoneme_deduplication() {
        assert!(WARNED_PHONEMES.lock().unwrap().insert('🔥'));
        assert!(!WARNED_PHONEMES.lock().unwrap().insert('🔥'));
        warn_unknown_phoneme('⭐', false);
        assert!(!WARNED_PHONEMES.lock().unwrap().insert('⭐'));
    }

    #[test]
    fn processors_emitting_ipa_reach_the_model_unmapped() {
        struct Ipa;
        impl crate::g2p::ProsodiaG2PProcessor for Ipa {
            fn process(&self, _t: String) -> Vec<crate::g2p::MToken> {
                vec![crate::g2p::MToken { text: "x".into(), tag: String::new(), whitespace: String::new(), phonemes: Some("hˈɔːɹsᵻz ɐ".into()) }]
            }
        }
        struct NoVoices;
        impl crate::voice_loader::VoiceAssetProvider for NoVoices {
            fn load_voice_bytes(&self, _v: String) -> Option<Vec<u8>> { None }
        }
        let symbols: Vec<String> = ["_", " ", "h", "ˈ", "ɔ", "ː", "ɹ", "s", "ᵻ", "z", "ɐ", "ɪ", "ə"].iter().map(|s| s.to_string()).collect();
        let pipeline = ProsodiaActorPipeline::new(
            Box::new(Ipa), crate::voice_loader::VoiceLoader::new(Box::new(NoVoices)),
            serde_json::json!({ "symbols": symbols }).to_string(), 24000, "en-us".into(),
        ).unwrap();
        let out = pipeline.process_span(stage::prosody_payload::ProsodySpan {
            text: "x".into(), emotion: stage::prosody::EmotionVector { valence: 0.0, arousal: 0.0, tension: 0.0 },
            leading_pause: 0.0, acoustics: None,
        });
        let ids = pipeline.tokenize_phonemes(out.phonemes[0].phonemes.clone(), true);
        let spelled: String = ids.iter().skip(1).step_by(2).map(|&i| symbols[i as usize].as_str()).collect();
        assert_eq!(spelled, "hˈɔːɹsᵻz ɐ");
    }

    /// Records the phoneme ids of every forward and reports a fixed token
    /// limit, as the split runtime reports its graph's MAX_TEXT.
    struct LimitedEngine {
        ids: Arc<Mutex<Vec<Vec<i32>>>>,
        limit: i32,
        matcha: bool,
    }

    impl ProsodiaSpeechEngine for LimitedEngine {
        fn synthesize(&self, _input: PipelineOutput) -> ActorEngineOutput {
            ActorEngineOutput { audio: Vec::new(), pred_dur: Vec::new() }
        }

        fn forward(
            &self,
            phoneme_ids: Vec<i32>,
            _style: StyleVector,
            _speed: f32,
            _vat: Option<Vec<f32>>,
            _duration_scales: Option<Vec<f32>>,
            _f0_bias: Option<Vec<f32>>,
        ) -> Result<ActorEngineOutput, SpeechEngineError> {
            let count = phoneme_ids.len();
            self.ids.lock().unwrap().push(phoneme_ids);
            Ok(ActorEngineOutput { audio: vec![0.1; 100], pred_dur: vec![8; count] })
        }

        fn reclaim_memory(&self) {}

        fn is_matcha(&self) -> bool {
            self.matcha
        }

        fn get_token_limit(&self) -> i32 {
            self.limit
        }
    }

    struct IgnoreAudio;
    impl AudioChunkCallback for IgnoreAudio {
        fn on_audio_chunk(&self, _chunk: Vec<f32>) {}
    }

    const LONG_TEXT: &str = "the quick brown fox jumps over the lazy dog while the cat sleeps by the warm fire tonight";

    fn limited_pipeline() -> Arc<ProsodiaActorPipeline> {
        let mut vocab: HashMap<String, i32> = HashMap::new();
        for (i, c) in " abcdefghijklmnopqrstuvwxyz".chars().enumerate() {
            vocab.insert(c.to_string(), i as i32 + 1);
        }
        let config = serde_json::json!({ "vocab": vocab }).to_string();
        ProsodiaActorPipeline::new(
            Box::new(MockG2P),
            VoiceLoader::new(Box::new(MockAssetProvider)),
            config,
            24000,
            "en-us".to_string(),
        )
        .unwrap()
    }

    /// The phoneme ids each non-app synthesis path sends to the engine, one
    /// entry per forward, by path name.
    fn ids_per_path(pipeline: &ProsodiaActorPipeline, text: &str, limit: i32, matcha: bool) -> Vec<(&'static str, Vec<Vec<i32>>)> {
        let mut out = Vec::new();
        let mut run = |name: &'static str, call: &dyn Fn(Box<dyn ProsodiaSpeechEngine>)| {
            let ids = Arc::new(Mutex::new(Vec::new()));
            call(Box::new(LimitedEngine { ids: ids.clone(), limit, matcha }));
            out.push((name, ids.lock().unwrap().clone()));
        };
        run("synthesize", &|engine| {
            pipeline.synthesize(engine, text.to_string(), "v".to_string(), 1.0, None, None).unwrap();
        });
        run("synthesize_markup", &|engine| {
            pipeline.synthesize_markup(engine, text.to_string(), "v".to_string(), 1.0).unwrap();
        });
        run("synthesize_stream", &|engine| {
            pipeline
                .synthesize_stream(engine, text.to_string(), "v".to_string(), 1.0, Box::new(IgnoreAudio))
                .unwrap();
        });
        run("synthesize_stream_with_morph", &|engine| {
            let blend = vec![vec![crate::voice_loader::VoiceBlend { voice: "v".to_string(), fraction: 1.0 }]];
            pipeline
                .synthesize_stream_with_morph(engine, text.to_string(), blend, 1.0, Box::new(IgnoreAudio))
                .unwrap();
        });
        out
    }

    /// The ids of a sequence without blanks and spaces (`limited_pipeline`
    /// gives the space id 1), which chunking may move or drop.
    fn symbol_ids<'a>(ids: impl IntoIterator<Item = &'a i32>) -> Vec<i32> {
        ids.into_iter().copied().filter(|&id| id > 1).collect()
    }

    /// Every path splits `text` into more than one forward, none longer than
    /// `limit` ids, and loses no symbol. Chunk boundaries replace a space.
    fn assert_chunks_fit(pipeline: &ProsodiaActorPipeline, text: &str, limit: i32, matcha: bool) {
        for (path, forwards) in ids_per_path(pipeline, text, limit, matcha) {
            assert!(forwards.len() > 1, "{path}: expected the text to be split, got {} forward(s)", forwards.len());
            for ids in &forwards {
                assert!(ids.len() <= limit as usize, "{path}: a forward has {} ids, over the limit of {limit}", ids.len());
            }
            assert_eq!(symbol_ids(forwards.iter().flatten()), symbol_ids(&pipeline.tokenize(text, matcha)), "{path}: symbols lost or reordered");
        }
    }

    #[test]
    fn matcha_chunks_count_interleaved_ids_against_the_limit() {
        // 2n + 1 ids for n symbols: at most 10 symbols per forward.
        assert_chunks_fit(&limited_pipeline(), LONG_TEXT, 21, true);
    }

    #[test]
    fn styletts2_chunks_count_the_bounding_ids_against_the_limit() {
        // n + 2 ids for n symbols.
        assert_chunks_fit(&limited_pipeline(), LONG_TEXT, 12, false);
    }

    #[test]
    fn chunks_fill_the_limit_after_the_framing_ids() {
        let pipeline = limited_pipeline();
        // Matcha: 10 symbols are 21 ids, 11 are 23. StyleTTS2: 10 are 12.
        assert_eq!(pipeline.chunk_phonemes_for("abcdefghijk", 21, true), ["abcdefghij", "k"]);
        assert_eq!(pipeline.chunk_phonemes_for("abcdefghijk", 22, true), ["abcdefghij", "k"]);
        assert_eq!(pipeline.chunk_phonemes_for("abcdefghijk", 12, false), ["abcdefghij", "k"]);

        let token = |p: &str, ws: &str| MToken {
            text: p.to_string(),
            tag: "".to_string(),
            whitespace: ws.to_string(),
            phonemes: Some(p.to_string()),
        };
        let words = [token("abcde", " "), token("fghij", "")];
        // "abcde fghij" is 11 symbols: 23 Matcha ids, 13 StyleTTS2 ids.
        let chunks = |limit, matcha| pipeline.chunk_tokens_for(&words, limit, matcha).len();
        assert_eq!(chunks(23, true), 1);
        assert_eq!(chunks(22, true), 2);
        assert_eq!(chunks(13, false), 1);
        assert_eq!(chunks(12, false), 2);
    }

    #[test]
    fn a_zero_token_limit_means_no_limit() {
        // As on the app path (`ProsodiaActorEngine::process_and_synthesize`).
        for (path, forwards) in ids_per_path(&limited_pipeline(), LONG_TEXT, 0, true) {
            assert_eq!(forwards.len(), 1, "{path}: a limit of 0 must not split the text");
        }
    }
}

