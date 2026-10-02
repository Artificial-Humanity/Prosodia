//! Sonora's device text front end (`scripts/litert_export/device_g2p.py`),
//! ported: the executable spec Sonora's export gate G7 checks against the
//! training front end, phoneme string for phoneme string.
//!
//! One deliberate divergence: numbers. Sonora's spec drops digits (its
//! tokenizer deletes them; the training corpus dropped digit-bearing clips),
//! but a book reader must speak them, so `numbers` spells out currency,
//! percentages, ordinals, decades and plain numbers before tokenizing. G7
//! equality therefore holds for digit-free text, which is all the G7 probe
//! corpus contains; text with digits reads differently from the spec by design.

pub mod assets;
pub mod fold;
pub mod neural;
pub mod normalize;
pub mod numbers;

use crate::g2p::{MToken, ProsodiaG2PProcessor};
use assets::G2pAssets;
use neural::NeuralG2p;
use normalize::{is_word, normalize, tokens};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct DeviceG2p {
    assets: G2pAssets,
    /// `None` when `use_neural_oov` is false. The TFLite interpreter inside
    /// `NeuralG2p::word` mutates unsynchronized state across `set_input` ->
    /// `invoke` -> `read_output`, and `DeviceG2pProcessor` makes this struct
    /// shareable across threads via `Arc`, so every call is serialized
    /// through this lock.
    neural: Option<Mutex<NeuralG2p>>,
    /// Words nothing resolved; they pass through as letters. Read this —
    /// letters are inside the vocabulary, so nothing else reports them.
    pub oov_words: Mutex<BTreeSet<String>>,
    /// Apostrophe words that took the bare-letters guess (mostly names).
    pub apostrophe_fallback_words: Mutex<BTreeSet<String>>,
    /// Words the neural graph raised an error on. Distinct from `oov_words`
    /// — that set means "nothing matched"; this one means "the graph itself
    /// failed", which is read-this evidence of a broken asset or runtime
    /// rather than an ordinary miss.
    pub neural_failures: Mutex<BTreeSet<String>>,
    /// Texts where a digit survived number expansion and reached the
    /// tokenizer, which deletes it silently. Never populated in normal
    /// operation — read-this evidence of a gap in `numbers::expand_digits`.
    pub digit_drops: Mutex<BTreeSet<String>>,
}

impl DeviceG2p {
    /// Loads the assets from `dir`; `use_neural_oov` loads the OOV graph too.
    pub fn load(dir: &Path, use_neural_oov: bool) -> Result<Self, String> {
        Ok(Self {
            assets: G2pAssets::load(dir)?,
            neural: if use_neural_oov { Some(Mutex::new(NeuralG2p::load(dir)?)) } else { None },
            oov_words: Mutex::new(BTreeSet::new()),
            apostrophe_fallback_words: Mutex::new(BTreeSet::new()),
            neural_failures: Mutex::new(BTreeSet::new()),
            digit_drops: Mutex::new(BTreeSet::new()),
        })
    }

    /// Never panics: `process()` has no error channel, and a panic here
    /// would poison the pipeline's lock on this struct for every caller
    /// after it. A graph error is logged and recorded in
    /// `neural_failures`, then treated as a miss.
    fn neural_word(&self, word: &str) -> Option<String> {
        let neural = self.neural.as_ref()?;
        // Bound to a `let` so the guard drops here, not across the match —
        // a match scrutinee's temporaries can otherwise outlive their arms.
        let result = neural.lock().unwrap().word(word);
        match result {
            Ok(ipa) if !ipa.is_empty() => Some(ipa),
            Ok(_) => None,
            Err(e) => {
                eprintln!("device G2P: neural OOV failed for {word:?}: {e}");
                self.neural_failures.lock().unwrap().insert(word.to_string());
                None
            }
        }
    }

    /// Dictionary, then neural, for a word with no apostrophes.
    fn plain_word(&self, word: &str) -> Option<String> {
        self.assets.dict.get(word).cloned().or_else(|| self.neural_word(word))
    }

    fn possessive_suffix(&self, base: &str) -> &str {
        let p = &self.assets.tables.possessive;
        let tail = base.trim_end_matches(|c| p.stress_marks.contains(c));
        if p.sibilant.iter().any(|s| tail.ends_with(s.as_str())) {
            &p.after_sibilant
        } else if p.voiceless.iter().any(|s| tail.ends_with(s.as_str())) {
            &p.after_voiceless
        } else {
            &p.default
        }
    }

    /// Contraction table, plural possessive, clitics, then "'s" — the table
    /// wins over decomposition.
    fn apostrophe_word(&self, word: &str) -> Option<String> {
        let t = &self.assets.tables;
        if let Some(ipa) = t.contractions.get(word) {
            return Some(ipa.clone());
        }
        if let Some(base) = word.strip_suffix('\'') {
            return self.plain_word(base);
        }
        for (clitic, suffix) in &t.clitics {
            if word.ends_with(clitic.as_str()) && word.len() > clitic.len() {
                return self.plain_word(&word[..word.len() - clitic.len()]).map(|b| b + suffix);
            }
        }
        if word.ends_with("'s") && word.len() > 2 {
            if let Some(base) = self.plain_word(&word[..word.len() - 2]) {
                let suffix = self.possessive_suffix(&base).to_string();
                return Some(base + &suffix);
            }
        }
        None
    }

    fn phonemize_word(&self, word: &str) -> Option<String> {
        if let Some(ipa) = self.assets.dict.get(word) {
            return Some(ipa.clone());
        }
        if word.contains('\'') {
            if let Some(ipa) = self.apostrophe_word(word).filter(|s| !s.is_empty()) {
                return Some(ipa);
            }
            let bare = word.replace('\'', "");
            if !bare.is_empty() {
                if let Some(ipa) = self.plain_word(&bare) {
                    self.apostrophe_fallback_words.lock().unwrap().insert(word.to_string());
                    return Some(ipa);
                }
            }
        } else if let Some(ipa) = self.neural_word(word) {
            return Some(ipa);
        }
        self.oov_words.lock().unwrap().insert(word.to_string());
        None
    }

    /// `(token, ipa)` pairs in order; punctuation maps to itself, words to
    /// IPA (or their letters when nothing resolves).
    pub fn phonemize_tokens(&self, text: &str) -> Vec<(String, String)> {
        let spelled = numbers::expand_digits(&fold::ascii_fold(&numbers::expand_symbols(text)));
        let normalized = normalize(&spelled);
        if normalize::carries_digits(&normalized) {
            // Never silent: the tokenizer would delete these.
            eprintln!("device G2P: digits survived number expansion and will be dropped: {text:?}");
            self.digit_drops.lock().unwrap().insert(text.to_string());
        }
        let mut out = Vec::new();
        for token in tokens(&normalized) {
            if !is_word(token) {
                out.push((token.to_string(), token.to_string()));
                continue;
            }
            let word = if self.assets.tables.contractions.contains_key(token) { token } else { token.trim_start_matches('\'') };
            if word.is_empty() || word == "'" {
                continue;
            }
            let ipa = self.phonemize_word(word).unwrap_or_else(|| word.to_string());
            out.push((word.to_string(), ipa));
        }
        out
    }

    /// The phoneme string, joined as Sonora joins it: a space before every
    /// word but the first, punctuation attached.
    pub fn phonemize(&self, text: &str) -> String {
        let mut s = String::new();
        let mut first = true;
        for (token, ipa) in self.phonemize_tokens(text) {
            if is_word(&token) {
                if !first {
                    s.push(' ');
                }
                first = false;
            }
            s.push_str(&ipa);
        }
        s
    }
}

/// The device G2P as the pipeline's processor. Each token carries its IPA;
/// the space Sonora puts before a word is the previous token's whitespace —
/// except before the FIRST word, which `phonemize` never precedes with a
/// space even when punctuation comes first (`"Hello,"` has no space after
/// the opening quote).
///
/// A span with no word ("…", "…!?") yields no tokens, so the pipeline
/// renders nothing for it rather than a model forward over bare
/// punctuation. `DeviceG2p::phonemize` still returns the spec's string
/// for such text ("...!?"); only the processor drops it.
pub struct DeviceG2pProcessor(pub Arc<DeviceG2p>);

impl ProsodiaG2PProcessor for DeviceG2pProcessor {
    fn process(&self, text: String) -> Vec<MToken> {
        let pairs = self.0.phonemize_tokens(&text);
        if !pairs.iter().any(|(token, _)| is_word(token)) {
            return Vec::new();
        }
        let mut out: Vec<MToken> = Vec::with_capacity(pairs.len());
        let mut first = true;
        for (token, ipa) in pairs {
            if is_word(&token) {
                if !first {
                    if let Some(prev) = out.last_mut() {
                        prev.whitespace = " ".to_string();
                    }
                }
                first = false;
            }
            out.push(MToken { text: token, tag: String::new(), whitespace: String::new(), phonemes: Some(ipa) });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const ASSETS: &str = "/data/models/litert-community/Matcha-TTS";

    fn load() -> Option<DeviceG2p> {
        if !Path::new(ASSETS).join("g2p_dict.txt.gz").exists() {
            assert!(std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() != Ok("1"), "{ASSETS} missing");
            println!("Skipping: {ASSETS} not found");
            return None;
        }
        Some(DeviceG2p::load(Path::new(ASSETS), true).unwrap())
    }

    #[test]
    fn no_digit_reaches_the_tokenizer() {
        let Some(g2p) = load() else { return };
        let ipa = g2p.phonemize("Chapter 12: £5 and ½ cup, x², １００ pages, №3, in 1984.");
        assert!(g2p.digit_drops.lock().unwrap().is_empty(), "{:?}", g2p.digit_drops.lock().unwrap());
        assert!(ipa.contains("twˈɛlv") && ipa.contains("pˈaʊndz") && ipa.contains("hˈæf"), "{ipa}");
    }

    #[test]
    fn reproduces_the_training_front_end_on_every_g7_probe() {
        let Some(g2p) = load() else { return };
        let reference: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/g2p_parity/reference.json")).unwrap(),
        )
        .unwrap();
        let probes = reference["probes"].as_array().unwrap();
        assert_eq!(probes.len(), 86);
        let bad: Vec<String> = probes.iter()
            .filter_map(|p| {
                let (text, want) = (p["text"].as_str().unwrap(), p["ipa"].as_str().unwrap());
                let got = g2p.phonemize(text);
                (got != want).then(|| format!("{text}\n  want {want}\n  got  {got}"))
            })
            .collect();
        assert!(bad.is_empty(), "{} of 86 differ:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn the_d_c1_exemplars_resolve_through_the_tables() {
        let Some(g2p) = load() else { return };
        assert_eq!(g2p.phonemize("we'll"), "wiːl");
        assert_eq!(g2p.phonemize("don't"), "dˈoʊnt");
        assert_eq!(g2p.phonemize("the horse's"), "ðə hˈɔːɹsᵻz");
    }

    #[test]
    fn degenerate_input_yields_nothing_and_no_panic() {
        let Some(g2p) = load() else { return };
        let g2p = Arc::new(g2p);
        let processor = DeviceG2pProcessor(g2p.clone());
        // `phonemize` is the spec: OpenPhonemizerG2P(homographs=False).phonemize
        // gives exactly these. The processor renders no word-less span at all.
        for (text, spec) in [("", ""), ("   ", ""), ("…!?", "...!?")] {
            assert_eq!(g2p.phonemize(text), spec, "{text:?}");
            let tokens = processor.process(text.to_string());
            assert!(tokens.is_empty(), "{text:?} produced tokens: {:?}", tokens.iter().map(|t| &t.text).collect::<Vec<_>>());
        }
    }

    #[test]
    fn processor_symbols_equal_phonemize() {
        let Some(g2p) = load() else { return };
        let g2p = Arc::new(g2p);
        let processor = DeviceG2pProcessor(g2p.clone());
        for text in ["\"Hello,\" he said.", "¿verdad? Yes.", "The quick brown fox jumps over the lazy dog."] {
            let want = g2p.phonemize(text);
            let got: String = processor
                .process(text.to_string())
                .into_iter()
                .map(|t| format!("{}{}", t.phonemes.unwrap_or_default(), t.whitespace))
                .collect();
            assert_eq!(got.trim(), want, "{text:?}");
        }
    }

    #[test]
    fn foreign_scripts_and_symbols_stay_inside_the_vocabulary() {
        let Some(g2p) = load() else { return };
        let symbols: std::collections::HashSet<char> = serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/g2p_parity/reference.json")).unwrap(),
        )
        .unwrap()["symbols"].as_array().unwrap().iter().flat_map(|s| s.as_str().unwrap().chars().collect::<Vec<_>>()).collect();
        let out = g2p.phonemize("Москва & 東京 🎉 at a@b.com");
        assert!(out.chars().all(|c| symbols.contains(&c)), "{out}");
    }
}
