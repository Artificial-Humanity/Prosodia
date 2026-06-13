use crate::prosody::{EmotionVector, ProsodyDirective};
use crate::prosody_payload::{encode_directive, encode_spans, decode_spans, ProsodySpan};

#[uniffi::export]
pub fn neutral_payload_for_passage(passage: &str) -> String {
    encode_directive(&ProsodyDirective {
        emotion: EmotionVector::neutral(),
        acoustics: None,
    }, passage)
}

#[uniffi::export]
pub fn payload_from_raw(raw: &str, passage: &str) -> String {
    let decoded = match decode_spans(raw) {
        Some(d) => d,
        None => return neutral_payload_for_passage(passage),
    };
    
    let reproduced = decoded.spans.iter().map(|s| s.text.as_str()).collect::<Vec<&str>>().join(" ");
    
    if words_match(&reproduced, passage) {
        let aligned = align_spans(decoded.spans, passage);
        return encode_spans(aligned);
    }
    
    encode_directive(&ProsodyDirective {
        emotion: decoded.overall,
        acoustics: None,
    }, passage)
}

fn words_match(a: &str, b: &str) -> bool {
    let canon_a: String = a.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    let canon_b: String = b.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    canon_a == canon_b
}

fn align_spans(spans: Vec<ProsodySpan>, passage: &str) -> Vec<ProsodySpan> {
    let mut aligned = Vec::new();
    let mut passage_idx = 0;
    
    let passage_chars: Vec<char> = passage.chars().collect();
    
    for (i, span) in spans.iter().enumerate() {
        let span_alnum_count = span.text.chars().filter(|c| c.is_alphanumeric()).count();
        let mut current_alnum_count = 0;
        let mut scan_idx = passage_idx;
        
        while scan_idx < passage_chars.len() {
            let c = passage_chars[scan_idx];
            if c.is_alphanumeric() {
                if current_alnum_count == span_alnum_count {
                    break;
                }
                current_alnum_count += 1;
            }
            scan_idx += 1;
        }
        
        let slice_end = if i == spans.len() - 1 { passage_chars.len() } else { scan_idx };
        let slice: String = passage_chars[passage_idx..slice_end].iter().collect();
        let cleaned = slice.trim().to_string();
        
        aligned.push(ProsodySpan {
            text: cleaned,
            emotion: span.emotion.clone(),
            leading_pause: span.leading_pause,
            acoustics: span.acoustics.clone(),
        });
        
        passage_idx = slice_end;
    }
    
    aligned
}
