use std::sync::RwLock;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EmotionVector {
    pub valence: f64,
    pub arousal: f64,
    pub tension: f64,
}

impl EmotionVector {
    pub fn new(valence: f64, arousal: f64, tension: f64) -> Self {
        Self {
            valence: valence.clamp(-1.0, 1.0),
            arousal: arousal.clamp(-1.0, 1.0),
            tension: tension.clamp(0.0, 1.0),
        }
    }

    pub fn neutral() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    pub fn distance(&self, other: &EmotionVector) -> f64 {
        let dv = self.valence - other.valence;
        let da = self.arousal - other.arousal;
        let dt = self.tension - other.tension;
        (dv * dv + da * da + dt * dt).sqrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum EmotionPreset {
    Baseline,
    Soft,
    Somber,
    Excited,
    Tense,
    Tender,
    Technical,
    Angry,
    Cold,
    Tired,
    Distraught,
    Theatrical,
    Stern,
    Pleading,
}

impl EmotionPreset {
    pub fn vector(&self) -> EmotionVector {
        match self {
            Self::Baseline => EmotionVector::new(0.0, 0.0, 0.0),
            Self::Soft => EmotionVector::new(0.2, -0.6, 0.1),
            Self::Somber => EmotionVector::new(-0.7, -0.5, 0.1),
            Self::Excited => EmotionVector::new(0.8, 0.8, 0.0),
            Self::Tense => EmotionVector::new(-0.3, 0.1, 0.9),
            Self::Tender => EmotionVector::new(-0.3, -0.3, 0.1),
            Self::Technical => EmotionVector::new(0.1, -0.6, 0.0),
            Self::Angry => EmotionVector::new(-0.95, 0.95, 0.95),
            Self::Cold => EmotionVector::new(-0.4, -0.5, 0.7),
            Self::Tired => EmotionVector::new(-0.2, -0.8, 0.1),
            Self::Distraught => EmotionVector::new(-0.9, 0.6, 0.95),
            Self::Theatrical => EmotionVector::new(0.6, 0.7, 0.3),
            Self::Stern => EmotionVector::new(-0.5, 0.0, 0.85),
            Self::Pleading => EmotionVector::new(-0.3, 0.4, 0.8),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CastingProfile {
    pub age_profile: f64,
    pub masculinity: f64,
    pub strain_or_rasp: f64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProsodyAcoustics {
    pub speed_multiplier: Option<f64>,
    pub speed_bias: Option<f64>,
    pub gain_multiplier: Option<f64>,
    pub gain_bias: Option<f64>,
    pub casting_profile: Option<CastingProfile>,
    pub speaker_lock: Option<String>,
    pub pause_multiplier: Option<f64>,
    pub pronunciation_override: Option<String>,
    pub pitch: Option<f64>,
    pub token_duration_scales: Option<Vec<f64>>,
    pub token_f0_biases: Option<Vec<f64>>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProsodyDirective {
    pub emotion: EmotionVector,
    pub acoustics: Option<ProsodyAcoustics>,
}

// Implement speed/gain resolution logic on ProsodyDirective
#[uniffi::export]
pub fn directive_speed_multiplier(directive: &ProsodyDirective) -> f64 {
    if let Some(ref acoustics) = directive.acoustics {
        if let Some(explicit) = acoustics.speed_multiplier {
            return explicit;
        }
    }
    let base = crate::acoustic_matrix::speed_for_emotion(&directive.emotion);
    let bias = directive.acoustics.as_ref().and_then(|a| a.speed_bias).unwrap_or(0.0);
    base + bias
}

#[uniffi::export]
pub fn directive_gain_multiplier(directive: &ProsodyDirective) -> f64 {
    if let Some(ref acoustics) = directive.acoustics {
        if let Some(explicit) = acoustics.gain_multiplier {
            return explicit;
        }
    }
    let base = crate::acoustic_matrix::gain_for_emotion(&directive.emotion);
    let bias = directive.acoustics.as_ref().and_then(|a| a.gain_bias).unwrap_or(0.0);
    base + bias
}
