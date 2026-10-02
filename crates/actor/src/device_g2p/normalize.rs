//! Sonora's pre-tokenization chain (`op_g2p.normalize_for_tokens`) and its
//! tokenizer, in the same order: ASCII fold, lowercase, abbreviations,
//! brackets, dash runs to spaces, whitespace collapse.

use super::fold::ascii_fold;
use once_cell::sync::Lazy;
use regex::Regex;

static ABBREVIATIONS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    [
        ("mrs", "misess"), ("mr", "mister"), ("dr", "doctor"), ("st", "saint"),
        ("co", "company"), ("jr", "junior"), ("maj", "major"), ("gen", "general"),
        ("drs", "doctors"), ("rev", "reverend"), ("lt", "lieutenant"),
        ("hon", "honorable"), ("sgt", "sergeant"), ("capt", "captain"),
        ("esq", "esquire"), ("ltd", "limited"), ("col", "colonel"), ("ft", "fort"),
    ]
    .iter()
    .map(|(short, full)| (Regex::new(&format!(r"(?i)\b{short}\.")).unwrap(), *full))
    .collect()
});
static BRACKETS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\[\]\(\)\{\}]").unwrap());
static DASH_RUN: Lazy<Regex> = Lazy::new(|| Regex::new("[-\u{2010}-\u{2015}\u{2212}]+").unwrap());
static WHITESPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new("[a-z']+|[.,!?;:\u{2014}\u{2026}\"\u{00AB}\u{00BB}\u{201C}\u{201D}\u{00A1}\u{00BF}]").unwrap());
static WORD: Lazy<Regex> = Lazy::new(|| Regex::new("^[a-z']+$").unwrap());

/// The text the tokenizer sees, as `op_g2p.normalize_for_tokens` produces it.
pub fn normalize(text: &str) -> String {
    let mut t = ascii_fold(text).to_lowercase();
    for (re, full) in ABBREVIATIONS.iter() {
        t = re.replace_all(&t, *full).into_owned();
    }
    let t = BRACKETS.replace_all(&t, "");
    let t = DASH_RUN.replace_all(&t, " ");
    WHITESPACE.replace_all(&t, " ").into_owned()
}

/// Word tokens (letters and apostrophes) and vocabulary punctuation, in order.
/// Everything else — digits included — is dropped, as in the spec.
pub fn tokens(normalized: &str) -> Vec<&str> {
    TOKEN.find_iter(normalized).map(|m| m.as_str()).collect()
}

/// Whether a token is a word rather than punctuation.
pub fn is_word(token: &str) -> bool {
    WORD.is_match(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_op_g2p_normalize_for_tokens_on_every_probe() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_g2p/normalize.json")).unwrap(),
        )
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 86);
        for case in cases {
            let text = case["text"].as_str().unwrap();
            assert_eq!(normalize(text), case["normalized"].as_str().unwrap(), "{text:?}");
        }
    }

    #[test]
    fn tokens_keep_apostrophe_words_and_vocab_punctuation() {
        let n = normalize("Don't go -- 'Hello,' he said!");
        assert_eq!(tokens(&n), vec!["don't", "go", "'hello", ",", "'", "he", "said", "!"]);
        assert!(is_word("don't") && !is_word(","));
    }

    #[test]
    fn tokens_punctuation_set_each_returns_separately_and_is_not_word() {
        // Test every punctuation character in the vocabulary.
        // This string has: . , ! ? ; : — … " « » " " ¡ ¿
        let input = "a.b,c!d?e;f:g\u{2014}h\u{2026}i\"j\u{00AB}k\u{00BB}l\u{201C}m\u{201D}n\u{00A1}o\u{00BF}";
        let result = tokens(input);
        let expected = vec!["a", ".", "b", ",", "c", "!", "d", "?", "e", ";", "f", ":", "g", "\u{2014}", "h", "\u{2026}", "i", "\"", "j", "\u{00AB}", "k", "\u{00BB}", "l", "\u{201C}", "m", "\u{201D}", "n", "\u{00A1}", "o", "\u{00BF}"];
        assert_eq!(result, expected);

        // Verify each punctuation is not a word
        let punctuation = vec![".", ",", "!", "?", ";", ":", "\u{2014}", "\u{2026}", "\"", "\u{00AB}", "\u{00BB}", "\u{201C}", "\u{201D}", "\u{00A1}", "\u{00BF}"];
        for p in punctuation {
            assert!(!is_word(p), "punctuation {:?} should not be a word", p);
        }
    }
}
