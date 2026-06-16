use std::sync::Arc;
use prosodia_core::{BpeTokenizer as CoreTokenizer, BpeError as CoreError};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TokenizerError {
    #[error("Tokenizer error: {msg}")]
    Error { msg: String },
}

impl From<CoreError> for TokenizerError {
    fn from(err: CoreError) -> Self {
        Self::Error {
            msg: err.to_string(),
        }
    }
}

#[derive(uniffi::Object)]
pub struct BpeTokenizer {
    inner: CoreTokenizer,
}

#[uniffi::export]
impl BpeTokenizer {
    #[uniffi::constructor]
    pub fn new(data: Vec<u8>, byte_level: bool, unknown_token_id: Option<i32>) -> Result<Arc<Self>, TokenizerError> {
        let inner = CoreTokenizer::new(&data, byte_level, unknown_token_id)?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn encode(&self, text: String) -> Vec<i32> {
        self.inner.encode(&text)
    }

    pub fn decode(&self, tokens: Vec<i32>) -> String {
        self.inner.decode(&tokens)
    }
}
