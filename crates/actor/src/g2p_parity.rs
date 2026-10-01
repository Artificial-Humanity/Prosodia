//! Measures Prosodia's text front end against Sonora's training front end.
//!
//! Sonora's export gate G7 requires a device front end to produce the training
//! front end's phoneme string exactly, over the probe corpus in
//! `tests/fixtures/g2p_parity/reference.json` (see its `generate.py`). This
//! module phonemizes the same probes the way the actor pipeline does — the
//! `ProsodiaSpeech` G2P, token phonemes and whitespace concatenated and
//! trimmed, `map_styletts2_to_matcha_ipa`, symbols outside the vocabulary
//! dropped — and compares the symbol sequences the model would receive.
//!
//! It reports the gap rather than requiring zero: the floors below record the
//! last measurement, so the test fails if parity gets worse and must be raised
//! when it gets better. Probes are counted in three groups, because a mismatch
//! means something different in each:
//!
//! * **lexicon** — the fixed probes the reference resolved from its dictionary;
//! * **neural** — the reference used its neural OOV graph, so a mismatch
//!   compares two OOV models rather than two lexicons;
//! * **contraction** — one sentence per contraction-table entry. Sonora's
//!   pinned models trained before that table existed, on apostrophe-stripped
//!   contractions, so a mismatch here is distance from the spec and not
//!   necessarily an audible error. Do not tune toward the old behaviour.

use crate::g2p::{ProsodiaG2PProcessor, ProsodiaSpeech};
use crate::pipeline::map_styletts2_to_matcha_ipa;
use std::collections::HashSet;

const REFERENCE: &str = "tests/fixtures/g2p_parity/reference.json";

/// Per group: probes whose symbol sequence matches the reference exactly, and
/// the mean per-probe symbol similarity (1 − edit distance ÷ longer length).
/// Raise them when parity improves; never lower one without a reason in the
/// commit.
const FLOORS: [(&str, usize, f64); 3] = [("lexicon", 0, 0.82), ("neural", 0, 0.50), ("contraction", 0, 0.91)];

/// Keeps only the symbols the tokenizer turns into ids, as `tokenize` does.
fn in_vocab(s: &str, vocab: &HashSet<char>) -> Vec<char> {
    s.chars().filter(|c| vocab.contains(c)).collect()
}

/// The pipeline's phoneme string for `text`, before tokenization.
fn prosodia_phonemes(g2p: &ProsodiaSpeech, text: &str) -> String {
    let mut s = String::new();
    for token in g2p.process(text.to_string()) {
        if let Some(p) = token.phonemes {
            s.push_str(&p);
        }
        s.push_str(&token.whitespace);
    }
    map_styletts2_to_matcha_ipa(s.trim())
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[test]
fn prosodia_g2p_parity_with_the_training_front_end() {
    let reference: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(REFERENCE).expect("reference.json")).unwrap();
    let vocab: HashSet<char> = reference["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s.as_str().unwrap().chars())
        .collect();
    assert_eq!(vocab.len(), 178, "the locked vocabulary is 178 symbols");
    let probes = reference["probes"].as_array().unwrap();
    assert!(probes.len() > 17, "expected PARITY_PROBES plus the contraction sentences");

    let g2p = ProsodiaSpeech::new();
    // group -> (probes, exact, similarity sum)
    let mut groups: std::collections::BTreeMap<&str, (usize, usize, f64)> = std::collections::BTreeMap::new();
    let mut mismatches = Vec::new();
    for probe in probes {
        let group = if probe["contraction"].as_bool().unwrap() {
            "contraction"
        } else if probe["neural"].as_bool().unwrap() {
            "neural"
        } else {
            "lexicon"
        };
        let text = probe["text"].as_str().unwrap();
        let want_raw = probe["ipa"].as_str().unwrap();
        let want = in_vocab(want_raw, &vocab);
        assert_eq!(
            want.len(),
            want_raw.chars().count(),
            "reference for {text:?} has symbols outside the vocabulary"
        );
        let got = in_vocab(&prosodia_phonemes(&g2p, text), &vocab);
        let similarity = 1.0 - edit_distance(&want, &got) as f64 / want.len().max(got.len()).max(1) as f64;
        let entry = groups.entry(group).or_default();
        entry.0 += 1;
        entry.2 += similarity;
        if want == got {
            entry.1 += 1;
        } else {
            mismatches.push((group, similarity, text, want.iter().collect::<String>(), got.iter().collect::<String>()));
        }
    }
    mismatches.sort_by(|a, b| a.0.cmp(b.0).then(a.1.partial_cmp(&b.1).unwrap()));
    for (group, similarity, text, want, got) in &mismatches {
        println!("[{group}] {similarity:.3}  {text}\n       want: {want}\n       got:  {got}");
    }
    println!("G2P parity vs Sonora {}:", reference["sonora_commit"].as_str().unwrap_or("?"));
    for (group, exact_floor, similarity_floor) in FLOORS {
        let (n, exact, sum) = groups.get(group).copied().unwrap_or_default();
        assert!(n > 0, "no {group} probes in the reference");
        let mean = sum / n as f64;
        println!("  {group:<11} {exact:>3} of {n:>3} exact, mean symbol similarity {mean:.3}");
        assert!(exact >= exact_floor, "{group}: exact matches fell to {exact} (floor {exact_floor})");
        assert!(mean >= similarity_floor, "{group}: mean similarity fell to {mean:.3} (floor {similarity_floor})");
    }
}

#[test]
fn edit_distance_counts_single_symbol_edits() {
    let c = |s: &str| s.chars().collect::<Vec<_>>();
    assert_eq!(edit_distance(&c("wˈɛl"), &c("wˈɪl")), 1);
    assert_eq!(edit_distance(&c(""), &c("abc")), 3);
    assert_eq!(edit_distance(&c("dˈoʊnt"), &c("dˈɔnt")), 2);
}
