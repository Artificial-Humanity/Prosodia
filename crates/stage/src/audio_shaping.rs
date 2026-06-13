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
