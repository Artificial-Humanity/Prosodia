use crate::prosody::{EmotionVector, ProsodyAcoustics, ProsodyDirective, CastingProfile};
use regex::Regex;
use once_cell::sync::Lazy;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProsodySpan {
    pub text: String,
    pub emotion: EmotionVector,
    pub leading_pause: f64,
    pub acoustics: Option<ProsodyAcoustics>,
}

pub static PREFIX: &str = "[V: ";
pub static AROUSAL_TAG: &str = " A: ";
pub static TENSION_TAG: &str = " T: ";
pub static SPEED_TAG: &str = " S: ";
pub static SPEED_BIAS_TAG: &str = " SB: ";
pub static GAIN_TAG: &str = " G: ";
pub static GAIN_BIAS_TAG: &str = " GB: ";
pub static AGE_TAG: &str = " AG: ";
pub static MASC_TAG: &str = " MA: ";
pub static STRAIN_TAG: &str = " ST: ";
pub static SPEAKER_LOCK_TAG: &str = " LK: ";
pub static PAUSE_MULTIPLIER_TAG: &str = " PB: ";
pub static ALIAS_TAG: &str = " PN: ";
pub static PITCH_TAG: &str = " P: ";
pub static DURATION_SCALES_TAG: &str = " DS: ";
pub static F0_BIASES_TAG: &str = " FB: ";
pub static SUFFIX: &str = "] ";

static DIRECTIVE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[V:\s*([-.\d]+)\s+A:\s*([-.\d]+)\s+T:\s*([-.\d]+)(.*?)\]\s*").unwrap()
});

#[uniffi::export]
pub fn encode_directive(directive: &ProsodyDirective, text: &str) -> String {
    format!("{}{}", encode_block(directive), text)
}

#[uniffi::export]
pub fn encode_spans(spans: Vec<ProsodySpan>) -> String {
    let mut parts = Vec::new();
    for span in spans {
        let directive = ProsodyDirective {
            emotion: span.emotion.clone(),
            acoustics: span.acoustics.clone(),
        };
        parts.push(format!("{}{}", encode_block(&directive), span.text));
    }
    parts.join(" ")
}

fn encode_block(directive: &ProsodyDirective) -> String {
    let e = &directive.emotion;
    let mut block = format!("{}{:.2}{}{:.2}{}{:.2}", PREFIX, e.valence, AROUSAL_TAG, e.arousal, TENSION_TAG, e.tension);
    
    if let Some(ref acoustics) = directive.acoustics {
        if let Some(speed) = acoustics.speed_multiplier {
            block.push_str(&format!("{}{:.3}", SPEED_TAG, speed));
        }
        if let Some(speed_bias) = acoustics.speed_bias {
            block.push_str(&format!("{}{:.3}", SPEED_BIAS_TAG, speed_bias));
        }
        if let Some(gain) = acoustics.gain_multiplier {
            block.push_str(&format!("{}{:.3}", GAIN_TAG, gain));
        }
        if let Some(gain_bias) = acoustics.gain_bias {
            block.push_str(&format!("{}{:.3}", GAIN_BIAS_TAG, gain_bias));
        }
        if let Some(ref casting) = acoustics.casting_profile {
            block.push_str(&format!("{}{:.2}{}{:.2}{}{:.2}", AGE_TAG, casting.age_profile, MASC_TAG, casting.masculinity, STRAIN_TAG, casting.strain_or_rasp));
        }
        if let Some(ref lock) = acoustics.speaker_lock {
            block.push_str(&format!("{}{}", SPEAKER_LOCK_TAG, lock));
        }
        if let Some(pause) = acoustics.pause_multiplier {
            block.push_str(&format!("{}{:.3}", PAUSE_MULTIPLIER_TAG, pause));
        }
        if let Some(ref alias) = acoustics.pronunciation_override {
            block.push_str(&format!("{}{}", ALIAS_TAG, alias));
        }
        if let Some(pitch) = acoustics.pitch {
            block.push_str(&format!("{}{:.1}", PITCH_TAG, pitch));
        }
        if let Some(ref ds) = acoustics.token_duration_scales {
            let str_vals: Vec<String> = ds.iter().map(|v| format!("{:.3}", v)).collect();
            block.push_str(&format!("{}{}", DURATION_SCALES_TAG, str_vals.join(",")));
        }
        if let Some(ref fb) = acoustics.token_f0_biases {
            let str_vals: Vec<String> = fb.iter().map(|v| format!("{:.3}", v)).collect();
            block.push_str(&format!("{}{}", F0_BIASES_TAG, str_vals.join(",")));
        }
    }
    block.push_str(SUFFIX);
    block
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DecodedPayload {
    pub overall: EmotionVector,
    pub acoustics: Option<ProsodyAcoustics>,
    pub spans: Vec<ProsodySpan>,
}

#[uniffi::export]
pub fn decode_spans(payload: &str) -> Option<DecodedPayload> {
    let mut parsed = Vec::new();
    let mut block_start = payload.find(PREFIX)?;

    while block_start < payload.len() {
        let rest = &payload[block_start..];
        let end_idx = match rest.find(']') {
            Some(idx) => idx,
            None => break,
        };
        let block_str = &rest[..=end_idx];
        
        let caps = match DIRECTIVE_REGEX.captures(block_str) {
            Some(c) => c,
            None => break,
        };
        
        let v = caps[1].parse::<f64>().ok()?;
        let a = caps[2].parse::<f64>().ok()?;
        let t = caps[3].parse::<f64>().ok()?;
        
        let directive = ProsodyDirective {
            emotion: EmotionVector::new(v, a, t),
            acoustics: None, // Simplified for now
        };
        
        let text_start = block_start + end_idx + 1;
        let next_block = payload[text_start..].find(PREFIX).map(|idx| text_start + idx);
        let text_end = next_block.unwrap_or(payload.len());
        
        let text = payload[text_start..text_end].trim().to_string();
        parsed.push((directive, text));
        
        if let Some(next) = next_block {
            block_start = next;
        } else {
            break;
        }
    }

    if parsed.is_empty() {
        return None;
    }

    let total_weight: usize = parsed.iter().map(|s| 1.max(s.1.chars().count())).sum();
    let mut v = 0.0;
    let mut a = 0.0;
    let mut t = 0.0;
    
    for span in &parsed {
        let w = (1.max(span.1.chars().count()) as f64) / (total_weight as f64);
        v += span.0.emotion.valence * w;
        a += span.0.emotion.arousal * w;
        t += span.0.emotion.tension * w;
    }
    
    let overall = EmotionVector::new(v, a, t);
    let acoustics = if parsed.len() == 1 { parsed[0].0.acoustics.clone() } else { None };
    let spans = parsed.into_iter().map(|p| ProsodySpan {
        text: p.1,
        emotion: p.0.emotion,
        leading_pause: 0.0,
        acoustics: p.0.acoustics,
    }).collect();
    
    Some(DecodedPayload { overall, acoustics, spans })
}
