//! The DeepPhonemizer OOV graph (`dp_g2p_matcha_fp16.tflite` + `g2p_meta.json`),
//! decoded as Sonora's `device_g2p.neural_word` does: characters repeated
//! `char_repeats` times between start and end ids, padded to `MAXT`, then
//! per-step argmax with repeats collapsed and specials dropped.

use crate::split_engine::GraphRunner;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct NeuralG2p {
    graph: GraphRunner,
    char_to_id: HashMap<char, f32>,
    id_to_phoneme: HashMap<usize, String>,
    repeats: usize,
    start: f32,
    end: f32,
    max_t: usize,
    special: HashSet<String>,
    n_phonemes: usize,
}

impl NeuralG2p {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let meta_path = dir.join("g2p_meta.json");
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&meta_path).map_err(|e| format!("{}: {e}", meta_path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", meta_path.display()))?;
        let graph = GraphRunner::new(&dir.join("dp_g2p_matcha_fp16.tflite"))?;
        let char_to_id = meta["char2idx"].as_object().ok_or("g2p_meta.json: char2idx missing")?
            .iter()
            .filter(|(k, _)| k.chars().count() == 1)
            .map(|(k, v)| (k.chars().next().unwrap(), v.as_f64().unwrap_or(0.0) as f32))
            .collect();
        let id_to_phoneme = meta["idx2ph"].as_object().ok_or("g2p_meta.json: idx2ph missing")?
            .iter()
            .map(|(k, v)| (k.parse::<usize>().unwrap_or(0), v.as_str().unwrap_or_default().to_string()))
            .collect();
        let num = |key: &str| meta[key].as_f64().ok_or(format!("g2p_meta.json: {key} missing"));
        Ok(Self {
            graph,
            char_to_id,
            id_to_phoneme,
            repeats: num("char_repeats")? as usize,
            start: num("start")? as f32,
            end: num("end")? as f32,
            max_t: num("MAXT")? as usize,
            special: meta["special"].as_array().ok_or("g2p_meta.json: special missing")?
                .iter().filter_map(|s| s.as_str().map(String::from)).collect(),
            n_phonemes: num("n_phonemes")? as usize,
        })
    }

    /// One out-of-dictionary word to IPA.
    pub fn word(&self, word: &str) -> Result<String, String> {
        let mut ids = vec![self.start];
        for ch in word.chars() {
            if let Some(&id) = self.char_to_id.get(&ch) {
                ids.extend(std::iter::repeat(id).take(self.repeats));
            }
        }
        ids.push(self.end);
        let length = ids.len().min(self.max_t);
        let mut padded = vec![0.0f32; self.max_t];
        padded[..length].copy_from_slice(&ids[..length]);
        self.graph.set_input(0, &padded)?;
        self.graph.invoke()?;
        let mut logits = Vec::new();
        self.graph.read_output(0, &mut logits)?;
        let mut pieces = String::new();
        let mut previous = usize::MAX;
        for t in 0..length {
            let row = &logits[t * self.n_phonemes..(t + 1) * self.n_phonemes];
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
            match self.id_to_phoneme.get(&best) {
                Some(p) if best != 0 && !self.special.contains(p) => pieces.extend(p.chars().filter(|c| *c != '-')),
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
}
