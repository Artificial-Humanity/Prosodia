use std::sync::Arc;

#[derive(Clone, Debug, uniffi::Record)]
pub struct TokenPhonemes {
    pub phonemes: String,
    pub whitespace: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MToken {
    pub text: String,
    pub phonemes: String,
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaG2PProcessor: Send + Sync {
    fn process(&self, text: String) -> Vec<MToken>;
}

// Simple fallback processor for testing
#[derive(uniffi::Object)]
pub struct BasicG2PProcessor {}

#[uniffi::export]
impl BasicG2PProcessor {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

#[uniffi::export]
impl ProsodiaG2PProcessor for BasicG2PProcessor {
    fn process(&self, text: String) -> Vec<MToken> {
        // Dummy implementation that just uses the text as phonemes
        text.split_whitespace()
            .map(|w| MToken {
                text: w.to_string(),
                phonemes: w.to_string(),
            })
            .collect()
    }
}
