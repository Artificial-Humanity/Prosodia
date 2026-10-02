//! The DeepPhonemizer OOV graph (`dp_g2p_matcha_fp16.tflite` + `g2p_meta.json`),
//! decoded as Sonora's `device_g2p.neural_word` does: characters repeated
//! `char_repeats` times between start and end ids, padded to `MAXT`, then
//! per-step argmax with repeats collapsed and specials dropped.

use crate::split_engine::GraphRunner;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Parsed, validated contents of `g2p_meta.json`.
#[derive(Debug)]
struct Meta {
    char_to_id: HashMap<char, f32>,
    id_to_phoneme: HashMap<usize, String>,
    repeats: usize,
    start: f32,
    end: f32,
    max_t: usize,
    special: HashSet<String>,
    n_phonemes: usize,
}

/// Parses and validates `g2p_meta.json`'s already-deserialized contents. A
/// missing or wrong-typed field is an error naming the field — never a
/// silent fallback (0 / "" / dropped), per the port's binding constraint
/// that a missing or incomplete asset refuses rather than degrades.
fn parse_meta(meta: &serde_json::Value) -> Result<Meta, String> {
    let mut char_to_id = HashMap::new();
    for (k, v) in meta["char2idx"].as_object().ok_or("g2p_meta.json: char2idx missing")? {
        if k.chars().count() != 1 {
            continue;
        }
        let id = v.as_f64().ok_or_else(|| format!("g2p_meta.json: char2idx[{k}] is not a number"))?;
        char_to_id.insert(k.chars().next().unwrap(), id as f32);
    }
    let mut id_to_phoneme = HashMap::new();
    for (k, v) in meta["idx2ph"].as_object().ok_or("g2p_meta.json: idx2ph missing")? {
        let id = k.parse::<usize>().map_err(|_| format!("g2p_meta.json: idx2ph key {k} is not a usize"))?;
        let phoneme = v.as_str().ok_or_else(|| format!("g2p_meta.json: idx2ph[{k}] is not a string"))?;
        id_to_phoneme.insert(id, phoneme.to_string());
    }
    let mut special = HashSet::new();
    for (i, s) in meta["special"].as_array().ok_or("g2p_meta.json: special missing")?.iter().enumerate() {
        let s = s.as_str().ok_or_else(|| format!("g2p_meta.json: special[{i}] is not a string"))?;
        special.insert(s.to_string());
    }
    let num = |key: &str| meta[key].as_f64().ok_or(format!("g2p_meta.json: {key} missing"));
    Ok(Meta {
        char_to_id,
        id_to_phoneme,
        repeats: num("char_repeats")? as usize,
        start: num("start")? as f32,
        end: num("end")? as f32,
        max_t: num("MAXT")? as usize,
        special,
        n_phonemes: num("n_phonemes")? as usize,
    })
}

pub struct NeuralG2p {
    graph: GraphRunner,
    meta: Meta,
}

impl NeuralG2p {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let meta_path = dir.join("g2p_meta.json");
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&meta_path).map_err(|e| format!("{}: {e}", meta_path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", meta_path.display()))?;
        let meta = parse_meta(&json)?;
        let graph = GraphRunner::new(&dir.join("dp_g2p_matcha_fp16.tflite"))?;
        Ok(Self { graph, meta })
    }

    /// One out-of-dictionary word to IPA.
    pub fn word(&self, word: &str) -> Result<String, String> {
        let mut ids = vec![self.meta.start];
        for ch in word.chars() {
            if let Some(&id) = self.meta.char_to_id.get(&ch) {
                ids.extend(std::iter::repeat(id).take(self.meta.repeats));
            }
        }
        ids.push(self.meta.end);
        let length = ids.len().min(self.meta.max_t);
        let mut padded = vec![0.0f32; self.meta.max_t];
        padded[..length].copy_from_slice(&ids[..length]);
        self.graph.set_input(0, &padded)?;
        self.graph.invoke()?;
        let mut logits = Vec::new();
        self.graph.read_output(0, &mut logits)?;
        let expected = length * self.meta.n_phonemes;
        if logits.len() < expected {
            return Err(format!(
                "neural graph output too short: got {} floats, need {expected} ({length} steps x {} phonemes)",
                logits.len(),
                self.meta.n_phonemes
            ));
        }
        let mut pieces = String::new();
        let mut previous = usize::MAX;
        for t in 0..length {
            let row = &logits[t * self.meta.n_phonemes..(t + 1) * self.meta.n_phonemes];
            // numpy's argmax returns the FIRST maximum; only replace on a
            // strictly greater value so ties resolve the same way here.
            let mut best = 0usize;
            let mut best_val = row[0];
            for (i, &v) in row.iter().enumerate().skip(1) {
                if v > best_val {
                    best = i;
                    best_val = v;
                }
            }
            if best == previous {
                continue;
            }
            previous = best;
            match self.meta.id_to_phoneme.get(&best) {
                Some(p) if best != 0 && !self.meta.special.contains(p) => pieces.extend(p.chars().filter(|c| *c != '-')),
                _ => {}
            }
        }
        Ok(pieces.replace('\u{303}', ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSETS: &str = "/data/models/litert-community/Matcha-TTS";

    fn load() -> Option<NeuralG2p> {
        if !Path::new(ASSETS).join("dp_g2p_matcha_fp16.tflite").exists() {
            assert!(std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() != Ok("1"), "{ASSETS} missing");
            println!("Skipping: {ASSETS} not found");
            return None;
        }
        Some(NeuralG2p::load(Path::new(ASSETS)).unwrap())
    }

    #[test]
    fn matches_the_oracle_on_every_word_it_sent_to_the_graph() {
        let Some(neural) = load() else { return };
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_g2p/neural.json")).unwrap(),
        )
        .unwrap();
        let words = fixture["words"].as_object().unwrap();
        assert!(words.len() >= 6);
        for (word, want) in words {
            assert_eq!(neural.word(word).unwrap(), want.as_str().unwrap(), "{word}");
        }
    }

    #[test]
    fn a_word_longer_than_the_window_is_truncated_not_a_panic() {
        let Some(neural) = load() else { return };
        let ipa = neural.word(&"blorp".repeat(8)).unwrap();
        assert!(!ipa.is_empty());
    }

    fn minimal_meta() -> serde_json::Value {
        serde_json::json!({
            "char2idx": {"a": 3},
            "idx2ph": {"0": "_"},
            "char_repeats": 3,
            "start": 1,
            "end": 2,
            "MAXT": 96,
            "special": ["_"],
            "n_phonemes": 64,
        })
    }

    #[test]
    fn a_wrong_typed_char2idx_value_is_refused_not_coerced_to_zero() {
        let mut meta = minimal_meta();
        meta["char2idx"] = serde_json::json!({"a": "x"});
        let err = parse_meta(&meta).unwrap_err();
        assert!(err.contains("char2idx"), "{err}");
    }

    #[test]
    fn a_wrong_typed_idx2ph_value_is_refused_not_dropped() {
        let mut meta = minimal_meta();
        meta["idx2ph"] = serde_json::json!({"1": 5});
        let err = parse_meta(&meta).unwrap_err();
        assert!(err.contains("idx2ph"), "{err}");
    }
}
