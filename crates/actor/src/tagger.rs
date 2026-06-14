use regex::Regex;
use once_cell::sync::Lazy;
use crate::normalization::{FeatureSpan, FeatureSpanKind};

#[derive(Clone, Debug)]
pub struct InternalUnderscore {
    pub is_head: bool,
    pub alias: Option<String>,
    pub stress: Option<f64>,
    pub currency: Option<String>,
    pub num_flags: String,
    pub prespace: bool,
    pub rating: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct InternalMToken {
    pub text: String,
    pub tag: String,
    pub whitespace: String,
    pub phonemes: Option<String>,
    pub underscore: InternalUnderscore,
}

impl InternalMToken {
    pub fn new(text: String, tag: String, whitespace: String) -> Self {
        Self {
            text,
            tag,
            whitespace,
            phonemes: None,
            underscore: InternalUnderscore {
                is_head: true,
                alias: None,
                stress: None,
                currency: None,
                num_flags: String::new(),
                prespace: false,
                rating: None,
            },
        }
    }

    pub fn clone_token(&self) -> Self {
        Self {
            text: self.text.clone(),
            tag: self.tag.clone(),
            whitespace: self.whitespace.clone(),
            phonemes: self.phonemes.clone(),
            underscore: InternalUnderscore {
                is_head: self.underscore.is_head,
                alias: self.underscore.alias.clone(),
                stress: self.underscore.stress,
                currency: self.underscore.currency.clone(),
                num_flags: self.underscore.num_flags.clone(),
                prespace: self.underscore.prespace,
                rating: self.underscore.rating,
            },
        }
    }
}

static TOKEN_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\p{L}\p{N}]+(?:['‘’][\p{L}\p{N}]+)*|[-_]+|[^\p{L}\p{N}\s]").unwrap()
});

fn is_title_case(s: &str) -> bool {
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        first.is_uppercase() && chars.all(|c| c.is_lowercase())
    } else {
        false
    }
}

fn is_all_uppercase(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_uppercase())
}

fn map_tag(word: &str) -> String {
    // 1. Currency
    if ["$", "£", "€"].contains(&word) {
        return "$".to_string();
    }
    if word == "#" {
        return "#".to_string();
    }
    // 2. Numbers
    if word.chars().any(|c| c.is_numeric()) {
        return "CD".to_string();
    }
    // 3. Punctuation
    if word == "," {
        return ",".to_string();
    }
    if [".", "!", "?"].contains(&word) {
        return ".".to_string();
    }
    if [":", ";", "-", "–", "—"].contains(&word) {
        return ":".to_string();
    }
    if word == "(" {
        return "-LRB-".to_string();
    }
    if word == ")" {
        return "-RRB-".to_string();
    }
    if ["\"", "“", "”", "‘", "’", "'", "`"].contains(&word) {
        // Fallback for quotes
        return "``".to_string();
    }
    if word.chars().all(|c| c.is_ascii_punctuation()) {
        return "NFP".to_string();
    }

    // 4. Closed class lookups (lowercase comparison)
    let lower = word.to_lowercase();
    match lower.as_str() {
        "the" | "a" | "an" | "this" | "that" | "these" | "those" => "DT".to_string(),
        "i" | "me" | "my" | "myself" | "you" | "your" | "yours" | "yourself" |
        "he" | "him" | "his" | "himself" | "she" | "her" | "hers" | "herself" |
        "it" | "its" | "itself" | "we" | "us" | "our" | "ours" | "ourselves" |
        "they" | "them" | "their" | "theirs" | "themselves" | "who" | "whom" |
        "whose" | "which" | "what" => "PRP".to_string(),
        "to" => "TO".to_string(),
        "in" | "on" | "at" | "by" | "for" | "with" | "about" | "against" | "between" |
        "into" | "through" | "during" | "before" | "after" | "above" | "below" |
        "from" | "of" => "IN".to_string(),
        "and" | "but" | "or" | "so" | "nor" | "yet" => "CC".to_string(),
        _ => {
            // Capitalization for proper nouns NNP
            if is_all_uppercase(word) && word.chars().count() <= 4 {
                "NNP".to_string()
            } else if is_title_case(word) {
                "NNP".to_string()
            } else if lower.ends_with("ed") {
                "VBD".to_string()
            } else if lower.ends_with("ing") {
                "VBG".to_string()
            } else if lower.ends_with('s') {
                "VBZ".to_string()
            } else {
                "NN".to_string() // default noun
            }
        }
    }
}

pub fn tokenize_and_tag(text: &str, feature_spans: &[FeatureSpan]) -> Vec<InternalMToken> {
    let mut slices = Vec::new();
    let mut tokens = Vec::new();

    // Iterate regex matches
    for mat in TOKEN_REGEX.find_iter(text) {
        let mat_str = mat.as_str();
        let range = mat.range();
        // Translate byte offsets to char offsets for range matching
        let char_start = text[..range.start].chars().count();
        let char_end = text[..range.end].chars().count();
        slices.push((char_start..char_end, mat_str.to_string(), range.end));
    }

    for i in 0..slices.len() {
        let (ref _range, ref token_text, byte_end) = slices[i];
        let next_byte_start = if i + 1 < slices.len() {
            TOKEN_REGEX.find_at(text, byte_end).map(|m| m.start()).unwrap_or(text.len())
        } else {
            text.len()
        };
        let whitespace = text[byte_end..next_byte_start].to_string();
        
        let mut tag = map_tag(token_text);
        // Custom quotes refinement (open vs close quotes)
        if ["\"", "“", "”", "‘", "’", "'", "`"].contains(&token_text.as_str()) {
            // If preceding string contains space (or starts), we can guess it's open
            let prev_text = &text[..byte_end - token_text.len()];
            let is_open = prev_text.is_empty() || prev_text.chars().last().unwrap().is_whitespace();
            tag = if is_open { "``".to_string() } else { "''".to_string() };
        }

        tokens.push(InternalMToken::new(token_text.clone(), tag, whitespace));
    }

    // Refine tags for homographs (e.g. "read") based on sentence context
    for i in 0..tokens.len() {
        let lower_text = tokens[i].text.to_lowercase();
        if lower_text == "read" {
            let current_tag = &tokens[i].tag;
            if current_tag.starts_with("VB") || current_tag == "VERB" || current_tag == "NN" {
                var_refine_read(&mut tokens, i);
            }
        }
    }

    // Apply feature spans
    if !feature_spans.is_empty() {
        for feature in feature_spans {
            let matching_indices: Vec<usize> = (0..tokens.len())
                .filter(|&index| {
                    let (ref slice_range, _, _) = slices[index];
                    slice_range.start < feature.range.end && slice_range.end > feature.range.start
                })
                .collect();

            if matching_indices.is_empty() {
                continue;
            }

            match &feature.kind {
                FeatureSpanKind::Stress(stress) => {
                    for index in matching_indices {
                        tokens[index].underscore.stress = Some(*stress);
                    }
                }
                FeatureSpanKind::PhonemeOverride(override_val) => {
                    for (offset, index) in matching_indices.iter().enumerate() {
                        tokens[*index].underscore.is_head = offset == 0;
                        tokens[*index].phonemes = Some(if offset == 0 { override_val.clone() } else { String::new() });
                        tokens[*index].underscore.rating = Some(5);
                    }
                }
                FeatureSpanKind::NumFlags(flags) => {
                    for index in matching_indices {
                        tokens[index].underscore.num_flags = flags.clone();
                    }
                }
            }
        }
    }

    tokens
}

fn var_refine_read(tokens: &mut [InternalMToken], i: usize) {
    let mut is_past_participle = false;
    let mut is_modal = false;

    let start_j = if i >= 3 { i - 3 } else { 0 };
    for j in start_j..i {
        let prev_text = tokens[j].text.to_lowercase();
        if ["has", "have", "had", "was", "were", "been", "get", "got", "getting", "is", "am", "are"].contains(&prev_text.as_str()) {
            is_past_participle = true;
        }
        if ["will", "would", "should", "could", "can", "may", "might", "must", "shall", "to"].contains(&prev_text.as_str()) {
            is_modal = true;
        }
    }

    if is_past_participle {
        tokens[i].tag = "VBN".to_string();
    } else if is_modal {
        tokens[i].tag = "VB".to_string();
    } else {
        let mut subject_tag = None;
        let mut subject_text = None;
        for j in (0..i).rev() {
            let prev_token = &tokens[j];
            if [".", ",", ";", ":", "CC"].contains(&prev_token.tag.as_str()) ||
               ["and", "but", "or", "because", "although", "when", "if", "while"].contains(&prev_token.text.to_lowercase().as_str()) {
                break;
            }
            if prev_token.tag == "PRP" || prev_token.tag.starts_with("NN") {
                subject_tag = Some(prev_token.tag.clone());
                subject_text = Some(prev_token.text.to_lowercase());
                break;
            }
        }

        if let Some(sub_text) = subject_text {
            if ["he", "she", "it", "this", "that", "someone", "somebody", "everyone", "everybody", "anyone", "anybody", "nobody", "noone"].contains(&sub_text.as_str()) {
                tokens[i].tag = "VBD".to_string();
            } else if ["i", "we", "they", "you", "these", "those"].contains(&sub_text.as_str()) {
                let mut has_past_adverb = false;
                for t in tokens.iter() {
                    let word = t.text.to_lowercase();
                    if ["yesterday", "ago", "previously", "earlier", "then", "once", "past"].contains(&word.as_str()) {
                        has_past_adverb = true;
                        break;
                    }
                }
                tokens[i].tag = if has_past_adverb { "VBD".to_string() } else { "VB".to_string() };
            } else if let Some(sub_tag) = subject_tag {
                if sub_tag == "NNP" || sub_tag == "NN" {
                    tokens[i].tag = "VBD".to_string();
                }
            }
        } else {
            let mut has_past_adverb = false;
            for t in tokens.iter() {
                let word = t.text.to_lowercase();
                if ["yesterday", "ago", "previously", "earlier", "then", "once", "past"].contains(&word.as_str()) {
                    has_past_adverb = true;
                    break;
                }
            }
            tokens[i].tag = if has_past_adverb { "VBD".to_string() } else { "VB".to_string() };
        }
    }
}
