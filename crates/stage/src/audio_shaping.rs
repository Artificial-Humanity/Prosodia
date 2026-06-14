use once_cell::sync::Lazy;
use std::sync::RwLock;

#[uniffi::export]
pub fn trimming_silence(samples: &[f32], threshold: f32) -> Vec<f32> {
    let first = samples.iter().position(|&x| x.abs() > threshold);
    let last = samples.iter().rposition(|&x| x.abs() > threshold);

    if let (Some(f), Some(l)) = (first, last) {
        samples[f..=l].to_vec()
    } else {
        vec![]
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PhrasePauseConfig {
    pub sentence: f64,
    pub clause: f64,
}

pub struct PhrasePauseState {
    pub sentence: f64,
    pub clause: f64,
}

pub static PHRASE_PAUSE: Lazy<RwLock<PhrasePauseState>> = Lazy::new(|| {
    RwLock::new(PhrasePauseState {
        sentence: 0.24,
        clause: 0.14,
    })
});

#[uniffi::export]
pub fn get_phrase_pause() -> PhrasePauseConfig {
    let state = PHRASE_PAUSE.read().unwrap();
    PhrasePauseConfig {
        sentence: state.sentence,
        clause: state.clause,
    }
}

#[uniffi::export]
pub fn set_phrase_pause(config: PhrasePauseConfig) {
    let mut state = PHRASE_PAUSE.write().unwrap();
    state.sentence = config.sentence;
    state.clause = config.clause;
}

#[uniffi::export]
pub fn pause_after(text: &str) -> f64 {
    let skip_chars = [' ', '\n', '\t', '"', '\'', ')', ']', '}', '»', '”', '’'];
    let last_char = text.chars().rev().find(|c| !skip_chars.contains(c));

    if let Some(c) = last_char {
        if ".!?…".contains(c) {
            return PHRASE_PAUSE.read().unwrap().sentence;
        }
        if ",;:—–".contains(c) {
            return PHRASE_PAUSE.read().unwrap().clause;
        }
    }
    0.0
}
