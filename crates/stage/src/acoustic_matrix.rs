use std::sync::RwLock;
use crate::prosody::EmotionVector;
use once_cell::sync::Lazy;

pub struct AcousticMatrixState {
    pub expressiveness: f64,
    pub speed_arousal_gain: f64,
    pub speed_tension_gain: f64,
    pub speed_valence_gain: f64,
    pub speed_range_min: f64,
    pub speed_range_max: f64,
    pub gain_arousal_gain: f64,
    pub gain_valence_gain: f64,
    pub gain_range_min: f64,
    pub gain_range_max: f64,
    pub sample_rate: u32,
}

impl Default for AcousticMatrixState {
    fn default() -> Self {
        Self {
            expressiveness: 3.25,
            speed_arousal_gain: 0.08,
            speed_tension_gain: 0.10,
            speed_valence_gain: 0.05,
            speed_range_min: 0.65,
            speed_range_max: 1.12,
            gain_arousal_gain: 0.25,
            gain_valence_gain: 0.08,
            gain_range_min: 0.60,
            gain_range_max: 1.20,
            sample_rate: 24000,
        }
    }
}

pub static ACOUSTIC_MATRIX: Lazy<RwLock<AcousticMatrixState>> = Lazy::new(|| RwLock::new(AcousticMatrixState::default()));

pub fn amplified(e: &EmotionVector) -> EmotionVector {
    let expressiveness = { ACOUSTIC_MATRIX.read().unwrap().expressiveness };
    EmotionVector::new(
        e.valence * expressiveness,
        e.arousal * expressiveness,
        e.tension * expressiveness,
    )
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AcousticMatrixConfig {
    pub expressiveness: f64,
    pub speed_arousal_gain: f64,
    pub speed_tension_gain: f64,
    pub speed_valence_gain: f64,
    pub speed_range_min: f64,
    pub speed_range_max: f64,
    pub gain_arousal_gain: f64,
    pub gain_valence_gain: f64,
    pub gain_range_min: f64,
    pub gain_range_max: f64,
    pub sample_rate: u32,
}

#[uniffi::export]
pub fn get_acoustic_matrix() -> AcousticMatrixConfig {
    let state = ACOUSTIC_MATRIX.read().unwrap();
    AcousticMatrixConfig {
        expressiveness: state.expressiveness,
        speed_arousal_gain: state.speed_arousal_gain,
        speed_tension_gain: state.speed_tension_gain,
        speed_valence_gain: state.speed_valence_gain,
        speed_range_min: state.speed_range_min,
        speed_range_max: state.speed_range_max,
        gain_arousal_gain: state.gain_arousal_gain,
        gain_valence_gain: state.gain_valence_gain,
        gain_range_min: state.gain_range_min,
        gain_range_max: state.gain_range_max,
        sample_rate: state.sample_rate,
    }
}

#[uniffi::export]
pub fn set_acoustic_matrix(config: AcousticMatrixConfig) {
    let mut state = ACOUSTIC_MATRIX.write().unwrap();
    state.expressiveness = config.expressiveness;
    state.speed_arousal_gain = config.speed_arousal_gain;
    state.speed_tension_gain = config.speed_tension_gain;
    state.speed_valence_gain = config.speed_valence_gain;
    state.speed_range_min = config.speed_range_min;
    state.speed_range_max = config.speed_range_max;
    state.gain_arousal_gain = config.gain_arousal_gain;
    state.gain_valence_gain = config.gain_valence_gain;
    state.gain_range_min = config.gain_range_min;
    state.gain_range_max = config.gain_range_max;
    state.sample_rate = config.sample_rate;
}

#[uniffi::export]
pub fn speed_for_emotion(emotion: &EmotionVector) -> f64 {
    let e = amplified(emotion);
    let state = ACOUSTIC_MATRIX.read().unwrap();
    let raw = 1.0
        + state.speed_arousal_gain * e.arousal
        - state.speed_tension_gain * e.tension
        + state.speed_valence_gain * e.valence;
    raw.clamp(state.speed_range_min, state.speed_range_max)
}

#[uniffi::export]
pub fn gain_for_emotion(emotion: &EmotionVector) -> f64 {
    let e = amplified(emotion);
    let state = ACOUSTIC_MATRIX.read().unwrap();
    let raw = 1.0
        + state.gain_arousal_gain * e.arousal
        + state.gain_valence_gain * e.valence;
    raw.clamp(state.gain_range_min, state.gain_range_max)
}

#[uniffi::export]
pub fn pitch_for_emotion(emotion: &EmotionVector) -> f64 {
    let e = amplified(emotion);
    
    if e.arousal >= 0.0 {
        let raw_aggression = 0f64.max(-emotion.valence) * emotion.arousal;
        let is_angry_shout = raw_aggression >= 0.75;
        
        if is_angry_shout {
            let aggression = 0f64.max(-e.valence) * e.arousal * e.tension;
            -aggression * 8.0
        } else {
            let shift = e.tension * 12.0 + e.arousal * 3.0;
            shift.min(15.0)
        }
    } else {
        if e.valence < 0.0 {
            let shift = 0f64.max(-e.arousal) * e.tension * 6.0;
            -shift
        } else {
            let shift = 0f64.max(-e.arousal) * 4.0;
            shift.min(15.0)
        }
    }
}

#[uniffi::export]
pub fn get_sample_rate() -> u32 {
    ACOUSTIC_MATRIX.read().unwrap().sample_rate
}
