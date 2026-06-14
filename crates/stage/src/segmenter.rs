use std::sync::Arc;
use crate::prosody_payload::ProsodySpan;

/// Rule-based sentence splitter.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;
    let mut i = 0;

    let abbreviations: std::collections::HashSet<String> = [
        "mr", "mrs", "ms", "dr", "prof", "sr", "jr",
        "st", "co", "corp", "inc", "ltd", "approx", "vs",
        "eg", "ie", "ca", "jan", "feb", "mar", "apr",
        "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        "gen", "col", "lt", "capt", "sgt", "rep", "sen",
        "vol", "ed", "pp", "al", "etc", "a.m", "p.m", "cf"
    ].iter().map(|s| s.to_string()).collect();

    while i < chars.len() {
        let c = chars[i];
        if c == '.' || c == '?' || c == '!' || c == '…' {
            let mut is_abbrev = false;
            
            if c == '.' && i > 0 && i + 1 < chars.len() {
                if chars[i-1].is_ascii_digit() && chars[i+1].is_ascii_digit() {
                    is_abbrev = true;
                }
            }

            if !is_abbrev && i > 0 {
                let mut word_start = i - 1;
                while word_start > 0 && chars[word_start].is_alphabetic() {
                    word_start -= 1;
                }
                if !chars[word_start].is_alphabetic() {
                    word_start += 1;
                }
                let word: String = chars[word_start..i].iter().collect();
                if !word.is_empty() {
                    let word_lower = word.to_lowercase();
                    if abbreviations.contains(&word_lower) {
                        is_abbrev = true;
                    } else if word.len() == 1 && word.chars().next().unwrap().is_uppercase() {
                        is_abbrev = true;
                    }
                }
            }

            if !is_abbrev {
                let mut temp_i = i;
                while temp_i + 1 < chars.len() && (chars[temp_i+1] == '.' || chars[temp_i+1] == '?' || chars[temp_i+1] == '!' || chars[temp_i+1] == '…') {
                    temp_i += 1;
                }
                let quote_chars = ['"', '\'', '”', '’', '»', ')', ']', '}'];
                while temp_i + 1 < chars.len() && quote_chars.contains(&chars[temp_i+1]) {
                    temp_i += 1;
                }
                if temp_i + 1 < chars.len() && chars[temp_i+1] == '.' {
                    temp_i += 1;
                }

                let mut next_idx = temp_i + 1;
                while next_idx < chars.len() && chars[next_idx].is_whitespace() {
                    next_idx += 1;
                }
                if next_idx < chars.len() && chars[next_idx].is_lowercase() {
                    is_abbrev = true;
                }

                if !is_abbrev {
                    i = temp_i;
                    let sentence: String = chars[start..=i].iter().collect();
                    let trimmed = sentence.trim().to_string();
                    if !trimmed.is_empty() {
                        sentences.push(trimmed);
                    }
                    start = i + 1;
                }
            }
        }
        i += 1;
    }
    
    if start < chars.len() {
        let sentence: String = chars[start..].iter().collect();
        let trimmed = sentence.trim().to_string();
        if !trimmed.is_empty() {
            sentences.push(trimmed);
        }
    }

    sentences
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum NarrationGrouping {
    Sentence,
    Paragraph { target_characters: u32 },
}

impl NarrationGrouping {
    pub fn group(&self, sentences: &[String]) -> Vec<String> {
        match self {
            Self::Sentence => sentences.to_vec(),
            Self::Paragraph { target_characters } => {
                let target = *target_characters as usize;
                let mut chunks = Vec::new();
                let mut current = String::new();
                for sentence in sentences {
                    if current.is_empty() {
                        current.push_str(sentence);
                    } else {
                        current.push_str(" ");
                        current.push_str(sentence);
                    }
                    if current.chars().count() >= target {
                        chunks.push(current);
                        current = String::new();
                    }
                }
                if !current.is_empty() {
                    chunks.push(current);
                }
                chunks
            }
        }
    }
}

#[derive(uniffi::Object)]
pub struct SentenceSegmenter;

#[uniffi::export]
impl SentenceSegmenter {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    pub fn sentences(&self, text: String) -> Vec<String> {
        split_sentences(&text)
    }
}

/// Apply boundary transition mitigations between consecutive spans.
pub fn apply_boundary_mitigations(mut spans: Vec<ProsodySpan>) -> Vec<ProsodySpan> {
    if spans.len() <= 1 {
        return spans;
    }
    
    for i in 1..spans.len() {
        let prev_speaker = spans[i-1].acoustics.as_ref().and_then(|a| a.speaker_lock.clone());
        let curr_speaker = spans[i].acoustics.as_ref().and_then(|a| a.speaker_lock.clone());
        
        if prev_speaker != curr_speaker {
            let text = &spans[i-1].text;
            let skip_chars = [' ', '\n', '\t', '"', '\'', ')', ']', '}', '»', '”', '’'];
            let last_char = text.chars().rev().find(|c| !skip_chars.contains(c));
            
            if let Some(c) = last_char {
                if ".!?…,;:—–".contains(c) {
                    spans[i].leading_pause = spans[i].leading_pause.max(0.25);
                }
            }
        }
    }
    
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prosody::{EmotionVector, ProsodyAcoustics};

    #[test]
    fn test_split_sentences_basic() {
        let text = "Hello world! This is a test. Mr. Smith went to Google.com (or was it yahoo?).";
        let res = split_sentences(text);
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], "Hello world!");
        assert_eq!(res[1], "This is a test.");
        assert_eq!(res[2], "Mr. Smith went to Google.com (or was it yahoo?).");
    }

    #[test]
    fn test_split_sentences_quotes() {
        let text = "He said, \"No way!\" and left. \"What did you say?\" she asked.";
        let res = split_sentences(text);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], "He said, \"No way!\" and left.");
        assert_eq!(res[1], "\"What did you say?\" she asked.");
    }

    #[test]
    fn test_narration_grouping_sentence() {
        let sents = vec!["One.".to_string(), "Two.".to_string(), "Three.".to_string()];
        let grouping = NarrationGrouping::Sentence;
        let res = grouping.group(&sents);
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], "One.");
        assert_eq!(res[1], "Two.");
        assert_eq!(res[2], "Three.");
    }

    #[test]
    fn test_narration_grouping_paragraph() {
        let sents = vec![
            "Short sentence.".to_string(),
            "Another short sentence.".to_string(),
            "This is a longer sentence to trigger grouping limit.".to_string(),
        ];
        let grouping = NarrationGrouping::Paragraph { target_characters: 30 };
        let res = grouping.group(&sents);
        // "Short sentence." (15) + " Another short sentence." (24) = 39 (exceeds 30) -> first group
        // "This is a longer sentence to trigger grouping limit." (52) -> second group
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], "Short sentence. Another short sentence.");
        assert_eq!(res[1], "This is a longer sentence to trigger grouping limit.");
    }

    #[test]
    fn test_apply_boundary_mitigations() {
        let spans = vec![
            ProsodySpan {
                text: "He said, ".to_string(),
                emotion: EmotionVector::neutral(),
                leading_pause: 0.0,
                acoustics: Some(ProsodyAcoustics {
                    speaker_lock: None,
                    speed_multiplier: None,
                    speed_bias: None,
                    gain_multiplier: None,
                    gain_bias: None,
                    casting_profile: None,
                    pause_multiplier: None,
                    pronunciation_override: None,
                    pitch: None,
                    token_duration_scales: None,
                    token_f0_biases: None,
                }),
            },
            ProsodySpan {
                text: "\"Hello!\"".to_string(),
                emotion: EmotionVector::neutral(),
                leading_pause: 0.0,
                acoustics: Some(ProsodyAcoustics {
                    speaker_lock: Some("Alice".to_string()),
                    speed_multiplier: None,
                    speed_bias: None,
                    gain_multiplier: None,
                    gain_bias: None,
                    casting_profile: None,
                    pause_multiplier: None,
                    pronunciation_override: None,
                    pitch: None,
                    token_duration_scales: None,
                    token_f0_biases: None,
                }),
            },
        ];
        
        let mitigated = apply_boundary_mitigations(spans);
        assert_eq!(mitigated[0].leading_pause, 0.0);
        // Transition from None (narrator) to Some("Alice") after comma punctuation -> leading_pause set to 0.25!
        assert_eq!(mitigated[1].leading_pause, 0.25);
    }
}
