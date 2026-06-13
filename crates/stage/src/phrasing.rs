use crate::prosody::EmotionVector;
use crate::prosody_payload::ProsodySpan;

#[uniffi::export(callback_interface)]
pub trait ProsodyPhraser: Send + Sync {
    fn spans(&self, text: String, emotion: EmotionVector) -> Vec<ProsodySpan>;
}

#[uniffi::export]
pub fn resolve_spans(phraser: Box<dyn ProsodyPhraser>, overall: &EmotionVector, decoded: Vec<ProsodySpan>) -> Vec<ProsodySpan> {
    if decoded.len() <= 1 {
        return decoded;
    }
    
    let original_acoustics = decoded.first().and_then(|s| s.acoustics.clone());
    let text = decoded.first().map(|s| s.text.as_str()).unwrap_or("");
    let split = phraser.spans(text.to_string(), overall.clone());
    
    split.into_iter().map(|mut s| {
        s.acoustics = original_acoustics.clone();
        s
    }).collect()
}
