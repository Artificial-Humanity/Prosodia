use std::sync::Arc;

use crate::lexicon::{Lexicon, TokenContext, VOWELS, CONSONANTS, NON_QUOTE_PUNCTS, SUBTOKEN_JUNKS, PRIMARY_STRESS, apply_stress};
use crate::tagger::{InternalMToken, InternalUnderscore};
use crate::normalization::preprocess;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TokenPhonemes {
    pub phonemes: String,
    pub whitespace: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MToken {
    pub text: String,
    pub tag: String,
    pub whitespace: String,
    pub phonemes: Option<String>,
}

#[uniffi::export(callback_interface)]
pub trait ProsodiaG2PProcessor: Send + Sync {
    fn process(&self, text: String) -> Vec<MToken>;
}

// The native Prosodia G2P engine
#[derive(uniffi::Object)]
pub struct ProsodiaSpeech {
    lexicon: Lexicon,
    unk: String,
    version: Option<String>,
}

#[uniffi::export]
impl ProsodiaSpeech {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            lexicon: Lexicon::new(false),
            unk: "❓".to_string(),
            version: None,
        })
    }

    #[uniffi::constructor]
    pub fn new_with_options(british: bool, unk: String, version: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            lexicon: Lexicon::new(british),
            unk,
            version,
        })
    }
}

enum TokenUnit {
    Token(InternalMToken),
    Group(Vec<InternalMToken>),
}

fn fold_left(tokens: &[InternalMToken], unk: &str) -> Vec<InternalMToken> {
    let mut result: Vec<InternalMToken> = Vec::new();
    result.reserve(tokens.len());
    for token in tokens {
        if let Some(last) = result.last().cloned() {
            if !token.underscore.is_head {
                result.pop();
                result.push(merge_tokens(&[last, token.clone_token()], Some(unk)));
            } else {
                result.push(token.clone_token());
            }
        } else {
            result.push(token.clone_token());
        }
    }
    result
}

fn merge_tokens(tokens: &[InternalMToken], unk: Option<&str>) -> InternalMToken {
    let stresses_found: Vec<f64> = tokens.iter().filter_map(|t| t.underscore.stress).collect();
    let merged_stress = if !stresses_found.is_empty() && stresses_found.iter().all(|&s| s == stresses_found[0]) {
        Some(stresses_found[0])
    } else {
        None
    };

    let currencies_found: std::collections::HashSet<Option<String>> = tokens.iter().map(|t| t.underscore.currency.clone()).collect();
    let ratings_found: std::collections::HashSet<Option<i32>> = tokens.iter().map(|t| t.underscore.rating).collect();

    let phonemes = if let Some(u) = unk {
        let mut merged = String::new();
        for token in tokens {
            if token.underscore.prespace && !merged.is_empty() {
                if let Some(last_char) = merged.chars().last() {
                    if !last_char.is_whitespace() {
                        if let Some(ref tp) = token.phonemes {
                            if !tp.is_empty() {
                                merged.push(' ');
                            }
                        }
                    }
                }
            }
            merged.push_str(token.phonemes.as_deref().unwrap_or(u));
        }
        Some(merged)
    } else {
        None
    };

    let mut merged_text = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if i + 1 < tokens.len() {
            merged_text.push_str(&token.text);
            merged_text.push_str(&token.whitespace);
        } else {
            merged_text.push_str(&token.text);
        }
    }

    let chosen_tag = tokens.iter()
        .max_by_key(|t| case_weight(&t.text))
        .map(|t| t.tag.clone())
        .unwrap_or_else(|| tokens.last().map(|t| t.tag.clone()).unwrap_or_default());

    let mut num_flags_set = std::collections::BTreeSet::new();
    for t in tokens {
        for c in t.underscore.num_flags.chars() {
            num_flags_set.insert(c);
        }
    }
    let merged_num_flags: String = num_flags_set.into_iter().collect();

    let merged_underscore = InternalUnderscore {
        is_head: tokens.first().map(|t| t.underscore.is_head).unwrap_or(true),
        alias: None,
        stress: merged_stress,
        currency: currencies_found.into_iter().flatten().max(),
        num_flags: merged_num_flags,
        prespace: tokens.first().map(|t| t.underscore.prespace).unwrap_or(false),
        rating: if ratings_found.contains(&None) { None } else { ratings_found.into_iter().flatten().min() },
    };

    InternalMToken {
        text: merged_text,
        tag: chosen_tag,
        whitespace: tokens.last().map(|t| t.whitespace.clone()).unwrap_or_default(),
        phonemes,
        underscore: merged_underscore,
    }
}

fn case_weight(text: &str) -> usize {
    text.chars().fold(0, |acc, c| acc + if c.is_uppercase() { 2 } else { 1 })
}

fn subtokenize(word: &str) -> Vec<String> {
    if word.is_empty() {
        return Vec::new();
    }
    
    let chars: Vec<char> = word.chars().collect();
    let mut pieces = Vec::new();
    let mut current = String::new();
    
    let is_apostrophe = |c: char| c == '\'' || c == '‘' || c == '’';
    
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        
        if is_apostrophe(c) {
            let is_leading = i == 0;
            let is_trailing = i == chars.len() - 1;
            let next_is_ap = i + 1 < chars.len() && is_apostrophe(chars[i + 1]);
            
            if is_leading || is_trailing || next_is_ap || (i > 0 && is_apostrophe(chars[i - 1])) {
                if !current.is_empty() {
                    pieces.push(current);
                    current = String::new();
                }
                let mut ap_seq = String::new();
                while i < chars.len() && is_apostrophe(chars[i]) {
                    ap_seq.push(chars[i]);
                    i += 1;
                }
                pieces.push(ap_seq);
                continue;
            }
        }
        
        if c == '-' || c == '_' {
            if !current.is_empty() {
                pieces.push(current);
                current = String::new();
            }
            let mut seq = String::new();
            while i < chars.len() && (chars[i] == '-' || chars[i] == '_') {
                seq.push(chars[i]);
                i += 1;
            }
            pieces.push(seq);
            continue;
        }
        
        let is_digit = |ch: char| ch.is_numeric();
        if is_digit(c) || (c == '-' && i + 1 < chars.len() && is_digit(chars[i + 1])) {
            if !current.is_empty() {
                pieces.push(current);
                current = String::new();
            }
            let mut num = String::new();
            if c == '-' {
                num.push('-');
                i += 1;
            }
            while i < chars.len() {
                let ch = chars[i];
                if is_digit(ch) {
                    num.push(ch);
                } else if (ch == '.' || ch == ',') && i + 1 < chars.len() && is_digit(chars[i + 1]) {
                    num.push(ch);
                    num.push(chars[i + 1]);
                    i += 1;
                } else {
                    break;
                }
                i += 1;
            }
            pieces.push(num);
            continue;
        }
        
        if c.is_alphabetic() {
            if !current.is_empty() {
                let last_char = current.chars().last().unwrap();
                let is_camel = last_char.is_lowercase() && c.is_uppercase();
                let is_acronym_split = if last_char.is_uppercase() && c.is_uppercase() && i + 1 < chars.len() {
                    chars[i + 1].is_lowercase()
                } else {
                    false
                };
                
                if is_camel || is_acronym_split {
                    pieces.push(current);
                    current = String::new();
                }
            }
            
            current.push(c);
            i += 1;
            
            while i < chars.len() {
                let next_c = chars[i];
                if next_c.is_alphabetic() {
                    let last_char = current.chars().last().unwrap();
                    let is_camel = last_char.is_lowercase() && next_c.is_uppercase();
                    let is_acronym_split = if last_char.is_uppercase() && next_c.is_uppercase() && i + 1 < chars.len() {
                        chars[i + 1].is_lowercase()
                    } else {
                        false
                    };
                    
                    if is_camel || is_acronym_split {
                        pieces.push(current);
                        current = String::new();
                    }
                    current.push(next_c);
                    i += 1;
                } else if is_apostrophe(next_c) && i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
                    current.push(next_c);
                    current.push(chars[i + 1]);
                    i += 2;
                } else {
                    break;
                }
            }
            continue;
        }
        
        if !current.is_empty() {
            pieces.push(current);
            current = String::new();
        }
        pieces.push(c.to_string());
        i += 1;
    }
    
    if !current.is_empty() {
        pieces.push(current);
    }
    
    pieces
}

fn retokenize(tokens: &[InternalMToken]) -> Vec<TokenUnit> {
    let mut units: Vec<TokenUnit> = Vec::new();
    
    let append_standalone = |unit_tokens: &mut Vec<TokenUnit>, t: InternalMToken| {
        unit_tokens.push(TokenUnit::Token(t));
    };

    let append_ordinary = |unit_tokens: &mut Vec<TokenUnit>, mut t: InternalMToken| {
        if let Some(TokenUnit::Group(ref mut existing)) = unit_tokens.last_mut() {
            if let Some(last_tok) = existing.last() {
                if last_tok.whitespace.is_empty() {
                    t.underscore.is_head = false;
                    existing.push(t);
                    return;
                }
            }
        }
        if t.whitespace.is_empty() {
            unit_tokens.push(TokenUnit::Group(vec![t]));
        } else {
            unit_tokens.push(TokenUnit::Token(t));
        }
    };

    let currencies: std::collections::HashMap<&str, (&str, &str)> = [
        ("$", ("dollar", "cent")),
        ("£", ("pound", "pence")),
        ("€", ("euro", "cent")),
    ].iter().cloned().collect();

    let punct_tag_phonemes: std::collections::HashMap<&str, &str> = [
        ("-LRB-", "("),
        ("-RRB-", ")"),
        ("``", "“"),
        ("\"\"", "”"),
        ("''", "”"),
    ].iter().cloned().collect();

    let punct_tags: std::collections::HashSet<&str> = [
        ".", ",", "-LRB-", "-RRB-", "``", "\"\"", "''", ":", "$", "#", "NFP"
    ].iter().cloned().collect();

    let puncts = ";:,.!?—…\"“”";
    let is_ascii_letter = |c: char| c.is_ascii_alphabetic();

    let mut current_currency: Option<String> = None;

    for (token_index, token) in tokens.iter().enumerate() {
        let mut pieces: Vec<InternalMToken> = Vec::new();
        
        if token.underscore.alias.is_none() && token.phonemes.is_none() {
            let matched_pieces = subtokenize(&token.text);
            for piece in matched_pieces {
                pieces.push(InternalMToken {
                    text: piece,
                    tag: token.tag.clone(),
                    whitespace: String::new(),
                    phonemes: None,
                    underscore: InternalUnderscore {
                        is_head: true,
                        alias: None,
                        stress: token.underscore.stress,
                        currency: None,
                        num_flags: token.underscore.num_flags.clone(),
                        prespace: false,
                        rating: None,
                    },
                });
            }
        } else {
            pieces.push(token.clone_token());
        }

        if pieces.is_empty() {
            continue;
        }
        
        let last_idx = pieces.len() - 1;
        pieces[last_idx].whitespace = token.whitespace.clone();

        for piece_index in 0..pieces.len() {
            let mut piece = pieces[piece_index].clone_token();

            if piece.underscore.alias.is_some() || piece.phonemes.is_some() {
                append_standalone(&mut units, piece);
                continue;
            }

            if piece.tag == "$" && currencies.contains_key(piece.text.as_str()) {
                current_currency = Some(piece.text.clone());
                piece.phonemes = Some(String::new());
                piece.underscore.rating = Some(4);
                append_standalone(&mut units, piece);
                continue;
            }

            if piece.tag == ":" && (piece.text == "-" || piece.text == "–" || piece.text == "—") {
                piece.phonemes = Some("—".to_string());
                piece.underscore.rating = Some(3);
                append_standalone(&mut units, piece);
                continue;
            }

            if punct_tags.contains(piece.tag.as_str()) && !piece.text.chars().all(is_ascii_letter) {
                let ph = punct_tag_phonemes.get(piece.tag.as_str())
                    .map(|&s| s.to_string())
                    .unwrap_or_else(|| piece.text.chars().filter(|&c| puncts.contains(c)).collect());
                piece.phonemes = Some(ph);
                piece.underscore.rating = Some(4);
                append_standalone(&mut units, piece);
                continue;
            }

            if let Some(ref curr) = current_currency {
                if piece.tag != "CD" {
                    current_currency = None;
                } else if piece_index == last_idx && (token_index + 1 == tokens.len() || tokens[token_index + 1].tag != "CD") {
                    piece.underscore.currency = Some(curr.clone());
                }
            }

            if piece_index > 0 && piece_index + 1 < pieces.len() && piece.text == "2" {
                if let (Some(left_char), Some(right_char)) = (pieces[piece_index - 1].text.chars().last(), pieces[piece_index + 1].text.chars().next()) {
                    if left_char.is_ascii_alphabetic() && right_char.is_ascii_alphabetic() {
                        piece.underscore.alias = Some("to".to_string());
                    }
                }
            }

            if piece.underscore.alias.is_some() || piece.phonemes.is_some() {
                append_standalone(&mut units, piece);
            } else {
                append_ordinary(&mut units, piece);
            }
        }
    }

    units.into_iter().map(|u| {
        match u {
            TokenUnit::Group(tokens) => {
                if tokens.len() == 1 {
                    TokenUnit::Token(tokens[0].clone_token())
                } else {
                    TokenUnit::Group(tokens)
                }
            }
            TokenUnit::Token(token) => TokenUnit::Token(token),
        }
    }).collect()
}

fn token_context(ctx: &TokenContext, phonemes: Option<&str>, token: &InternalMToken) -> TokenContext {
    let mut vowel = ctx.future_vowel;
    if let Some(ph) = phonemes {
        for character in ph.chars() {
            if VOWELS.contains(character) || CONSONANTS.contains(character) || NON_QUOTE_PUNCTS.contains(character) {
                vowel = if NON_QUOTE_PUNCTS.contains(character) {
                    None
                } else {
                    Some(VOWELS.contains(character))
                };
                break;
            }
        }
    }
    let future_to = token.text == "to" || token.text == "To" || (token.text == "TO" && (token.tag == "TO" || token.tag == "IN"));
    TokenContext {
        future_vowel: vowel,
        future_to,
    }
}

fn resolve_tokens(tokens: &mut [InternalMToken]) {
    let mut text = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if i + 1 < tokens.len() {
            text.push_str(&token.text);
            text.push_str(&token.whitespace);
        } else {
            text.push_str(&token.text);
        }
    }

    let categories: std::collections::HashSet<usize> = text.chars()
        .filter(|&c| !SUBTOKEN_JUNKS.contains(c))
        .map(|c| {
            if c.is_alphabetic() { 0 }
            else if c.is_numeric() { 1 }
            else { 2 }
        })
        .collect();

    let prespace = text.contains(' ') || text.contains('/') || categories.len() > 1;

    let tokens_len = tokens.len();
    for (index, token) in tokens.iter_mut().enumerate() {
        if token.phonemes.is_none() {
            if index + 1 == tokens_len && token.text.chars().count() == 1 {
                if let Some(only) = token.text.chars().next() {
                    if NON_QUOTE_PUNCTS.contains(only) {
                        token.phonemes = Some(token.text.clone());
                        token.underscore.rating = Some(3);
                    }
                }
            }
            if token.phonemes.is_none() && token.text.chars().all(|c| SUBTOKEN_JUNKS.contains(c)) {
                token.phonemes = Some(String::new());
                token.underscore.rating = Some(3);
            }
        } else if index > 0 {
            token.underscore.prespace = prespace;
        }
    }

    if prespace {
        return;
    }

    let mut indices: Vec<(bool, i32, usize)> = tokens.iter().enumerate()
        .filter_map(|(index, token)| {
            let ph = token.phonemes.as_deref()?;
            if ph.is_empty() { return None; }
            let has_primary = ph.contains(PRIMARY_STRESS);
            let weight = Lexicon::stress_weight(Some(ph));
            Some((has_primary, weight, index))
        })
        .collect();

    if indices.len() == 2 && tokens[indices[0].2].text.chars().count() == 1 {
        let second_index = indices[1].2;
        tokens[second_index].phonemes = apply_stress(tokens[second_index].phonemes.clone(), Some(-0.5));
        return;
    }

    let primary_count = indices.iter().filter(|x| x.0).count();
    if indices.len() < 2 || primary_count <= (indices.len() + 1) / 2 {
        return;
    }

    indices.sort_by(|a, b| {
        if a.0 != b.0 {
            a.0.cmp(&b.0)
        } else if a.1 != b.1 {
            a.1.cmp(&b.1)
        } else {
            a.2.cmp(&b.2)
        }
    });

    let limit = indices.len() / 2;
    for entry in indices.iter().take(limit) {
        let idx = entry.2;
        tokens[idx].phonemes = apply_stress(tokens[idx].phonemes.clone(), Some(-0.5));
    }
}

impl ProsodiaSpeech {
    fn process_internal(&self, text: String) -> Vec<InternalMToken> {
        let preprocessed = preprocess(&text);
        let mut tokens = crate::tagger::tokenize_and_tag(&preprocessed.text, &preprocessed.feature_spans);
        tokens = fold_left(&tokens, &self.unk);
        let mut units = retokenize(&tokens);
        let mut ctx = TokenContext::default();

        for index in (0..units.len()).rev() {
            match units[index] {
                TokenUnit::Token(ref mut token) => {
                    if token.phonemes.is_none() {
                        let cloned = token.clone_token();
                        let (phonemes, rating) = self.lexicon.lookup_token(&cloned, ctx.clone());
                        token.phonemes = phonemes;
                        token.underscore.rating = rating;
                    }
                    ctx = token_context(&ctx, token.phonemes.as_deref(), token);
                }
                TokenUnit::Group(ref mut group) => {
                    let mut left = 0;
                    let mut right = group.len();

                    while left < right {
                        let slice = &group[left..right];
                        let has_special = slice.iter().any(|t| t.underscore.alias.is_some() || t.phonemes.is_some());
                        let combined: Option<InternalMToken> = if has_special {
                            None
                        } else {
                            Some(merge_tokens(slice, None))
                        };

                        let resolved = if let Some(ref comb) = combined {
                            self.lexicon.lookup_token(comb, ctx.clone())
                        } else {
                            (None, None)
                        };

                        if let Some(phonemes) = resolved.0 {
                            group[left].phonemes = Some(phonemes.clone());
                            group[left].underscore.rating = resolved.1;
                            if left + 1 < right {
                                for token in &mut group[(left + 1)..right] {
                                    token.phonemes = Some(String::new());
                                    token.underscore.rating = resolved.1;
                                }
                            }
                            ctx = token_context(&ctx, Some(&phonemes), combined.as_ref().unwrap());
                            right = left;
                            left = 0;
                        } else if left + 1 < right {
                            left += 1;
                        } else {
                            right -= 1;
                            let token = &mut group[right];
                            if token.phonemes.is_none() {
                                if token.text.chars().all(|c| SUBTOKEN_JUNKS.contains(c)) {
                                    token.phonemes = Some(String::new());
                                    token.underscore.rating = Some(3);
                                }
                            }
                            left = 0;
                        }
                    }

                    resolve_tokens(group);
                }
            }
        }

        let mut resolved_tokens = Vec::new();
        for unit in units {
            match unit {
                TokenUnit::Token(token) => {
                    resolved_tokens.push(token);
                }
                TokenUnit::Group(tokens) => {
                    resolved_tokens.push(merge_tokens(&tokens, Some(&self.unk)));
                }
            }
        }

        if self.version.as_deref() != Some("2.0") {
            for token in &mut resolved_tokens {
                if let Some(ref mut ph) = token.phonemes {
                    *ph = ph.replace('?', "t").replace('ʔ', "t").replace('ɾ', "T");
                }
            }
        }

        resolved_tokens
    }
}

#[uniffi::export]
impl ProsodiaG2PProcessor for ProsodiaSpeech {
    fn process(&self, text: String) -> Vec<MToken> {
        let resolved = self.process_internal(text);
        resolved.into_iter()
            .map(|t| MToken {
                text: t.text,
                tag: t.tag,
                whitespace: t.whitespace,
                phonemes: t.phonemes,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalization::{preprocess, FeatureSpanKind, NumberToWords};
    use crate::tagger::tokenize_and_tag;

    #[test]
    fn test_markdown_preprocessing() {
        let prep = preprocess("This is [stressed](1.5) and [override](/wˈɜːd/) and [123](#n#).");
        assert_eq!(prep.text, "This is stressed and override and 123.");
        assert_eq!(prep.feature_spans.len(), 3);

        // check first feature span (stressed)
        assert_eq!(prep.feature_spans[0].kind, FeatureSpanKind::Stress(1.5));
        assert_eq!(&prep.text[prep.feature_spans[0].range.clone()], "stressed");

        // check second feature span (override)
        assert_eq!(prep.feature_spans[1].kind, FeatureSpanKind::PhonemeOverride("wˈɜːd".to_string()));
        assert_eq!(&prep.text[prep.feature_spans[1].range.clone()], "override");

        // check third feature span (123)
        assert_eq!(prep.feature_spans[2].kind, FeatureSpanKind::NumFlags("n".to_string()));
        assert_eq!(&prep.text[prep.feature_spans[2].range.clone()], "123");
    }

    #[test]
    fn test_number_to_words() {
        assert_eq!(NumberToWords::cardinal(0), "zero");
        assert_eq!(NumberToWords::cardinal(42), "forty-two");
        assert_eq!(NumberToWords::cardinal(123), "one hundred twenty-three");
        assert_eq!(NumberToWords::cardinal(2026), "two thousand twenty-six");

        assert_eq!(NumberToWords::year(1999), "nineteen ninety-nine");
        assert_eq!(NumberToWords::year(2005), "two thousand five");
        assert_eq!(NumberToWords::year(2026), "twenty twenty-six");

        assert_eq!(NumberToWords::ordinal(1), "first");
        assert_eq!(NumberToWords::ordinal(22), "twenty-second");
        assert_eq!(NumberToWords::ordinal(100), "one hundredth");
    }

    #[test]
    fn test_tagger_and_homograph_read() {
        // "read" present tense
        let tokens_present = tokenize_and_tag("I will read the book.", &[]);
        let read_present = tokens_present.iter().find(|t| t.text == "read").unwrap();
        assert_eq!(read_present.tag, "VB");

        // "read" past tense
        let tokens_past = tokenize_and_tag("I read the book yesterday.", &[]);
        let read_past = tokens_past.iter().find(|t| t.text == "read").unwrap();
        assert_eq!(read_past.tag, "VBD");

        // "read" past participle
        let tokens_participle = tokenize_and_tag("I have read the book.", &[]);
        let read_participle = tokens_participle.iter().find(|t| t.text == "read").unwrap();
        assert_eq!(read_participle.tag, "VBN");
    }

    #[test]
    fn test_end_to_end_us_g2p() {
        let g2p = ProsodiaSpeech::new(); // Defaults to US
        let tokens = g2p.process("I read a book.".to_string());
        
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].text, "I");
        assert_eq!(tokens[1].text, "read");
        assert_eq!(tokens[2].text, "a");
        assert_eq!(tokens[3].text, "book");
        assert_eq!(tokens[4].text, ".");

        // verify phonetic outputs are generated (not empty/unk)
        assert!(tokens[0].phonemes.is_some());
        assert!(tokens[1].phonemes.is_some());
        assert!(tokens[2].phonemes.is_some());
        assert!(tokens[3].phonemes.is_some());
        assert!(tokens[4].phonemes.is_some());
    }

    #[test]
    fn test_end_to_end_gb_g2p() {
        let g2p = ProsodiaSpeech::new_with_options(true, "❓".to_string(), None); // GB
        let tokens = g2p.process("I read a book.".to_string());
        
        assert_eq!(tokens.len(), 5);
        assert!(tokens[0].phonemes.is_some());
    }
}

