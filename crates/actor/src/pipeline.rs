use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use stage::prosody_payload::ProsodySpan;
use crate::g2p::{ProsodiaG2PProcessor, TokenPhonemes, MToken};
use crate::asset_manager::StyleVector;
use crate::engine::ProsodiaSpeechEngine;
use crate::voice_loader::VoiceLoader;

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
    vocab: HashMap<String, i32>,
}

#[uniffi::export(callback_interface)]
pub trait AudioChunkCallback: Send + Sync {
    fn on_audio_chunk(&self, chunk: Vec<f32>);
}

pub fn map_styletts2_to_matcha_ipa(phonemes: &str) -> String {
    let mut mapped = phonemes.to_string();
    mapped = mapped.replace("A", "eɪ");
    mapped = mapped.replace("I", "aɪ");
    mapped = mapped.replace("O", "oʊ");
    mapped = mapped.replace("Q", "əʊ");
    mapped = mapped.replace("W", "aʊ");
    mapped = mapped.replace("Y", "ɔɪ");
    mapped = mapped.replace("ʤ", "dʒ");
    mapped = mapped.replace("ʧ", "tʃ");
    mapped = mapped.replace("ɐ", "ə");
    mapped = mapped.replace("ᵻ", "ɪ");
    mapped = mapped.replace("ᵊ", "ə");
    mapped
}

#[derive(uniffi::Object)]
pub struct ProsodiaActorPipeline {
    g2p: Mutex<Box<dyn ProsodiaG2PProcessor>>,
    voice_loader: Arc<VoiceLoader>,
    vocab: HashMap<String, i32>,
    sample_rate: u32,
    lang_code: String,
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
        Ok(Arc::new(Self {
            g2p: Mutex::new(g2p),
            voice_loader,
            vocab: config.vocab,
            sample_rate,
            lang_code,
        }))
    }

    pub fn set_custom_g2p(&self, processor: Box<dyn ProsodiaG2PProcessor>) {
        let mut g2p = self.g2p.lock().unwrap();
        *g2p = processor;
    }

    fn tokenize(&self, phonemes: &str, is_matcha: bool) -> Vec<i32> {
        let mut ids = Vec::new();
        ids.push(0); // 0-bound padding identical to standard StyleTTS2 G2P tokenizer format
        let mapped = if is_matcha {
            map_styletts2_to_matcha_ipa(phonemes)
        } else {
            phonemes.to_string()
        };
        for c in mapped.chars() {
            if let Some(&id) = self.vocab.get(&c.to_string()) {
                ids.push(id);
            }
        }
        ids.push(0); // 0-bound padding identical to standard StyleTTS2 G2P tokenizer format
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

        let tokens = self.g2p.lock().unwrap().process(text.clone());
        let token_chunks = self.chunk_tokens(&tokens, 510);
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

            let ids = self.tokenize(&trimmed_phonemes, speech_engine.is_matcha());

            let mut chunk_duration_scales = None;
            if let Some(ref d_scales) = duration_scales {
                let mut scales = vec![1.0f32; ids.len()];
                let mut p_idx = 1;
                for (t_idx, token) in chunk.iter().enumerate() {
                    let global_token_idx = token_offset + t_idx;
                    let scale = d_scales.get(global_token_idx).copied().unwrap_or(1.0);

                    let word_phonemes = token.phonemes.as_deref().unwrap_or("");
                    let mapped_word_phonemes = if speech_engine.is_matcha() {
                        map_styletts2_to_matcha_ipa(word_phonemes)
                    } else {
                        word_phonemes.to_string()
                    };
                    for char in mapped_word_phonemes.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                scales[p_idx] = scale;
                                p_idx += 1;
                            }
                        }
                    }
                    let mapped_whitespace = if speech_engine.is_matcha() {
                        map_styletts2_to_matcha_ipa(&token.whitespace)
                    } else {
                        token.whitespace.to_string()
                    };
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
                    let mapped_word_phonemes = if speech_engine.is_matcha() {
                        map_styletts2_to_matcha_ipa(word_phonemes)
                    } else {
                        word_phonemes.to_string()
                    };
                    for char in mapped_word_phonemes.chars() {
                        if self.vocab.contains_key(&char.to_string()) {
                            if p_idx < ids.len().saturating_sub(1) {
                                chunk_biases[p_idx] = bias;
                                p_idx += 1;
                            }
                        }
                    }
                    let mapped_whitespace = if speech_engine.is_matcha() {
                        map_styletts2_to_matcha_ipa(&token.whitespace)
                    } else {
                        token.whitespace.to_string()
                    };
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
                    self.tokenize(&trimmed_phonemes, speech_engine.is_matcha()),
                    style,
                    speed,
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

                let mapped_word_phonemes = if speech_engine.is_matcha() {
                    map_styletts2_to_matcha_ipa(word_phonemes)
                } else {
                    word_phonemes.to_string()
                };
                for char in mapped_word_phonemes.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let mapped_whitespace = if speech_engine.is_matcha() {
                    map_styletts2_to_matcha_ipa(whitespace)
                } else {
                    whitespace.to_string()
                };
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

        let parsed = stage::markup_parser::parse_markup(markup_text);
        let clean_text = parsed.clean_text;
        let character_prosody = parsed.character_prosody;

        let tokens = self.g2p.lock().unwrap().process(clean_text.clone());
        let token_chunks = self.chunk_tokens(&tokens, 510);
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
            let mut mapped_phonemes = String::new();

            if speech_engine.is_matcha() {
                for (idx, c) in trimmed_phonemes.chars().enumerate() {
                    let (rep, _repeat_count) = match c {
                        'A' => ("eɪ", 2),
                        'I' => ("aɪ", 2),
                        'O' => ("oʊ", 2),
                        'Q' => ("əʊ", 2),
                        'W' => ("aʊ", 2),
                        'Y' => ("ɔɪ", 2),
                        'ʤ' => ("dʒ", 2),
                        'ʧ' => ("tʃ", 2),
                        'ɐ' => ("ə", 1),
                        'ᵻ' => ("ɪ", 1),
                        'ᵊ' => ("ə", 1),
                        _ => {
                            mapped_phonemes.push(c);
                            if self.vocab.contains_key(&c.to_string()) {
                                filtered_states.push(trimmed_states[idx].clone());
                            }
                            continue;
                        }
                    };
                    mapped_phonemes.push_str(rep);
                    for sub_c in rep.chars() {
                        if self.vocab.contains_key(&sub_c.to_string()) {
                            filtered_states.push(trimmed_states[idx].clone());
                        }
                    }
                }
            } else {
                mapped_phonemes = trimmed_phonemes.clone();
                for (idx, c) in trimmed_phonemes.chars().enumerate() {
                    if self.vocab.contains_key(&c.to_string()) {
                        filtered_states.push(trimmed_states[idx].clone());
                    }
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
                    self.tokenize(&mapped_phonemes, speech_engine.is_matcha()),
                    style,
                    speed,
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

                let mapped_word_phonemes = if speech_engine.is_matcha() {
                    map_styletts2_to_matcha_ipa(word_phonemes)
                } else {
                    word_phonemes.to_string()
                };
                for char in mapped_word_phonemes.chars() {
                    if self.vocab.contains_key(&char.to_string()) {
                        if token_idx < pred_dur.len().saturating_sub(1) {
                            current_time += pred_dur[token_idx] as f64 * frame_duration;
                            token_idx += 1;
                        }
                    }
                }

                let mapped_whitespace = if speech_engine.is_matcha() {
                    map_styletts2_to_matcha_ipa(whitespace)
                } else {
                    whitespace.to_string()
                };
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

        let chunks = self.chunk_phonemes(&full_phonemes, 510);

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
                .forward(self.tokenize(&chunk, speech_engine.is_matcha()), style, speed, None, None)
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

        let chunks = self.chunk_phonemes(&full_phonemes, 510);

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
                .forward(self.tokenize(&chunk, speech_engine.is_matcha()), style, speed, None, None)
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
        let limit = limit as usize;
        let trimmed = phonemes.trim();
        let characters: Vec<char> = trimmed.chars().collect();
        if characters.len() <= limit {
            return if trimmed.is_empty() { vec![] } else { vec![trimmed.to_string()] };
        }

        let break_characters: std::collections::HashSet<char> =
            [" ", ".", ",", ";", ":", "!", "?", "—", "…"]
                .iter()
                .map(|s| s.chars().next().unwrap())
                .collect();

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < characters.len() {
            let end_limit = (start + limit).min(characters.len());
            if end_limit == characters.len() {
                let chunk_str: String = characters[start..end_limit].iter().collect();
                chunks.push(chunk_str.trim().to_string());
                break;
            }

            let mut split_index = end_limit;
            let mut cursor = end_limit - 1;
            while cursor > start + (limit / 2) {
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

    fn chunk_tokens(&self, tokens: &[MToken], limit: u32) -> Vec<Vec<MToken>> {
        let limit = limit as usize;
        let mut chunks = Vec::new();
        let mut current_chunk = Vec::new();
        let mut current_len = 0;

        for token in tokens {
            let token_len = token.phonemes.as_deref().unwrap_or("").chars().count() + token.whitespace.chars().count();
            if current_len + token_len > limit && !current_chunk.is_empty() {
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
}
