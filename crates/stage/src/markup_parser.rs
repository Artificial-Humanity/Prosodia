use regex::Regex;
use once_cell::sync::Lazy;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ProsodyState {
    pub rate: f32,
    pub pitch: f32,
}

impl Default for ProsodyState {
    fn default() -> Self {
        Self {
            rate: 1.0,
            pitch: 0.0,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ParsedMarkup {
    pub clean_text: String,
    pub character_prosody: Vec<ProsodyState>,
}

static ATTR_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(\w+)\s*=\s*"([^"]+)""#).unwrap()
});

fn parse_attributes(attr_string: &str, state: &mut ProsodyState) {
    for cap in ATTR_REGEX.captures_iter(attr_string) {
        if cap.len() == 3 {
            let key = cap[1].to_lowercase();
            let val = &cap[2];
            
            if key == "rate" {
                if val.ends_with('%') {
                    if let Ok(percent_val) = val[..val.len() - 1].parse::<f32>() {
                        state.rate = percent_val / 100.0;
                    }
                } else if let Ok(rate_val) = val.parse::<f32>() {
                    state.rate = rate_val;
                }
            } else if key == "pitch" {
                let mut clean_val = val.to_lowercase();
                if clean_val.ends_with("hz") {
                    clean_val = clean_val[..clean_val.len() - 2].to_string();
                }
                if let Ok(pitch_val) = clean_val.parse::<f32>() {
                    state.pitch = pitch_val;
                }
            }
        }
    }
}

#[uniffi::export]
pub fn parse_markup(input: String) -> ParsedMarkup {
    let mut clean_text = String::new();
    let mut character_prosody = Vec::new();
    
    let mut stack = vec![ProsodyState::default()];
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if chars[i] == '<' {
            // Potential tag
            if let Some(tag_end_relative) = chars[i..].iter().position(|&c| c == '>') {
                let tag_end_idx = i + tag_end_relative;
                let tag_content: String = chars[i + 1..tag_end_idx].iter().collect();
                let tag_content = tag_content.trim();
                i = tag_end_idx + 1;
                
                if tag_content.starts_with('/') {
                    // Closing tag
                    if stack.len() > 1 {
                        stack.pop();
                    }
                } else {
                    // Opening tag, e.g. prosody rate="1.5" pitch="+20Hz"
                    let parts: Vec<&str> = tag_content.splitn(2, ' ').collect();
                    let tag_name = parts[0].to_lowercase();
                    
                    if tag_name == "prosody" && parts.len() > 1 {
                        let mut new_state = stack.last().cloned().unwrap_or_default();
                        parse_attributes(parts[1], &mut new_state);
                        stack.push(new_state);
                    } else {
                        // Unsupported tag, push duplicate state to keep stack balanced
                        stack.push(stack.last().cloned().unwrap_or_default());
                    }
                }
                continue;
            }
        }
        
        // Normal character
        let char = chars[i];
        clean_text.push(char);
        character_prosody.push(stack.last().cloned().unwrap_or_default());
        i += 1;
    }
    
    ParsedMarkup {
        clean_text,
        character_prosody,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markup_plain() {
        let parsed = parse_markup("Hello world".to_string());
        assert_eq!(parsed.clean_text, "Hello world");
        assert_eq!(parsed.character_prosody.len(), 11);
        for state in parsed.character_prosody {
            assert_eq!(state.rate, 1.0);
            assert_eq!(state.pitch, 0.0);
        }
    }

    #[test]
    fn test_parse_markup_with_tags() {
        let input = "Hello <prosody rate=\"1.5\" pitch=\"+20Hz\">fast and high</prosody> normal".to_string();
        let parsed = parse_markup(input);
        assert_eq!(parsed.clean_text, "Hello fast and high normal");
        
        // Check "Hello " -> 6 chars
        for state in &parsed.character_prosody[0..6] {
            assert_eq!(state.rate, 1.0);
            assert_eq!(state.pitch, 0.0);
        }
        
        // Check "fast and high" -> 13 chars
        for state in &parsed.character_prosody[6..19] {
            assert_eq!(state.rate, 1.5);
            assert_eq!(state.pitch, 20.0);
        }
        
        // Check " normal" -> 7 chars
        for state in &parsed.character_prosody[19..26] {
            assert_eq!(state.rate, 1.0);
            assert_eq!(state.pitch, 0.0);
        }
    }
}
