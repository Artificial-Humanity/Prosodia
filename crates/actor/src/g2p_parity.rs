//! Measures Prosodia's text front end against Sonora's training front end.
//!
//! Sonora's export gate G7 requires a device front end to produce the training
//! front end's phoneme string exactly, over the probe corpus in
//! `tests/fixtures/g2p_parity/reference.json` (see its `generate.py`). This
//! module sends each probe through the path the apps use —
//! `ProsodiaActorEngine::process_and_synthesize` over the `ProsodiaSpeech`
//! G2P — into a speech engine that only records the phoneme ids it is given,
//! and compares the symbol sequences the model would receive. The recorder
//! reports the split runtime's token limit (256); at the e2e engine's 50 most
//! probes would be split into separately synthesized chunks.
//!
//! Two front ends are measured on that path. `prosodia_g2p_parity_with_the_training_front_end`
//! measures the Misaki-based `ProsodiaSpeech` G2P against per-group floors
//! (below). `device_g2p_parity_on_the_app_path` measures the device G2P port
//! (`crate::device_g2p`), whose floor is all 86 probes exact: it is the
//! training front end's spec, so any mismatch is a regression.
//!
//! Four contraction probes (`i'd`, `i'll`, `i'm`, `i've`) use the table's
//! lowercase keys; Prosodia's lexicon is case-sensitive and resolves `I'm` in
//! real text, so their low scores are partly an artifact of the probe.
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

use crate::engine::{ActorEngineOutput, ProsodiaActorEngine, ProsodiaSpeechEngine, SpeechEngineError, SynthesisControls};
use crate::g2p::{MToken, ProsodiaG2PProcessor, ProsodiaSpeech};
use crate::pipeline::{PipelineOutput, ProsodiaActorPipeline};
use crate::asset_manager::StyleVector;
use crate::voice_loader::{VoiceAssetProvider, VoiceLoader};
use stage::prosody::EmotionVector;
use stage::prosody_payload::ProsodySpan;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const REFERENCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/g2p_parity/reference.json");
const SPLIT_TOKEN_LIMIT: i32 = 256;

/// Per group: probes whose symbol sequence matches the reference exactly, and
/// the mean per-probe symbol similarity (1 − edit distance ÷ longer length).
/// Raise them when parity improves; never lower one without a reason in the
/// commit.
const FLOORS: [(&str, usize, f64); 3] = [("lexicon", 0, 0.872), ("neural", 0, 0.476), ("contraction", 0, 0.908)];

/// Keeps only the symbols the tokenizer turns into ids, as `tokenize` does.
fn in_vocab(s: &str, vocab: &HashSet<char>) -> Vec<char> {
    s.chars().filter(|c| vocab.contains(c)).collect()
}

/// A speech engine that renders nothing and records the phoneme ids of every
/// forward, one entry per chunk.
struct Recorder(Arc<Mutex<Vec<Vec<i32>>>>);

impl ProsodiaSpeechEngine for Recorder {
    fn synthesize(&self, _input: PipelineOutput) -> ActorEngineOutput {
        ActorEngineOutput { audio: Vec::new(), pred_dur: Vec::new() }
    }

    fn forward(
        &self,
        phoneme_ids: Vec<i32>,
        _style: StyleVector,
        _speed: f32,
        _controls: SynthesisControls,
        _duration_scales: Option<Vec<f32>>,
        _f0_bias: Option<Vec<f32>>,
    ) -> Result<ActorEngineOutput, SpeechEngineError> {
        self.0.lock().unwrap().push(phoneme_ids);
        Ok(ActorEngineOutput { audio: Vec::new(), pred_dur: Vec::new() })
    }

    fn reclaim_memory(&self) {}

    fn is_matcha(&self) -> bool {
        true
    }

    fn get_token_limit(&self) -> i32 {
        SPLIT_TOKEN_LIMIT
    }
}

/// The app's G2P, boxed as the pipeline's processor.
struct AppG2p(Arc<ProsodiaSpeech>);

impl ProsodiaG2PProcessor for AppG2p {
    fn process(&self, text: String) -> Vec<MToken> {
        self.0.process(text)
    }
}

struct NoVoices;

impl VoiceAssetProvider for NoVoices {
    fn load_voice_bytes(&self, _voice_name: String) -> Option<Vec<u8>> {
        None
    }
}

/// The symbols the model receives for `text` on the app path. Chunks are
/// joined with a space, which is what a chunk boundary replaces.
fn app_path_symbols(g2p: Box<dyn ProsodiaG2PProcessor>, symbols: &[String], text: &str) -> String {
    let config = serde_json::json!({ "symbols": symbols }).to_string();
    let pipeline = ProsodiaActorPipeline::new(
        g2p,
        VoiceLoader::new(Box::new(NoVoices)),
        config,
        24000,
        "en-us".to_string(),
    )
    .expect("pipeline");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let engine = ProsodiaActorEngine::new(pipeline, Box::new(Recorder(recorded.clone())));
    let span = ProsodySpan {
        text: text.to_string(),
        emotion: EmotionVector { valence: 0.0, arousal: 0.0, tension: 0.0 },
        leading_pause: 0.0,
        acoustics: None,
    };
    engine.process_and_synthesize(span).expect("process_and_synthesize");
    let chunks = recorded.lock().unwrap();
    chunks
        .iter()
        .map(|ids| {
            // Matcha ids interleave the blank (0): [0, p1, 0, p2, …, pn, 0].
            ids.iter().skip(1).step_by(2).map(|&id| symbols[id as usize].as_str()).collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    let symbols: Vec<String> = reference["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let vocab: HashSet<char> = symbols.iter().flat_map(|s| s.chars()).collect();
    assert_eq!(vocab.len(), 178, "the locked vocabulary is 178 symbols");
    let probes = reference["probes"].as_array().unwrap();
    assert!(probes.len() > 17, "expected PARITY_PROBES plus the contraction sentences");

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
        let got: Vec<char> = app_path_symbols(Box::new(AppG2p(ProsodiaSpeech::new())), &symbols, text).chars().collect();
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
        println!("  {group:<11} {exact:>3} of {n:>3} exact, mean symbol similarity {mean:.4}");
        assert!(exact >= exact_floor, "{group}: exact matches fell to {exact} (floor {exact_floor})");
        assert!(mean >= similarity_floor, "{group}: mean similarity fell to {mean:.3} (floor {similarity_floor})");
    }
}

#[test]
fn device_g2p_parity_on_the_app_path() {
    const ASSETS: &str = "/data/models/litert-community/Matcha-TTS";
    if !std::path::Path::new(ASSETS).join("g2p_dict.txt.gz").exists() {
        assert!(std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() != Ok("1"), "{ASSETS} missing");
        println!("Skipping: {ASSETS} not found");
        return;
    }
    let g2p = Arc::new(crate::device_g2p::DeviceG2p::load(std::path::Path::new(ASSETS), true).unwrap());
    let reference: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(REFERENCE).unwrap()).unwrap();
    let symbols: Vec<String> = reference["symbols"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    let mut exact = 0;
    for probe in reference["probes"].as_array().unwrap() {
        let got = app_path_symbols(Box::new(crate::device_g2p::DeviceG2pProcessor(g2p.clone())), &symbols, probe["text"].as_str().unwrap());
        if got == probe["ipa"].as_str().unwrap() {
            exact += 1;
        } else {
            println!("{}\n  want {}\n  got  {got}", probe["text"], probe["ipa"]);
        }
    }
    assert_eq!(exact, 86, "device G2P on the app path: {exact} of 86 exact");
}

#[test]
fn edit_distance_counts_single_symbol_edits() {
    let c = |s: &str| s.chars().collect::<Vec<_>>();
    assert_eq!(edit_distance(&c("wˈɛl"), &c("wˈɪl")), 1);
    assert_eq!(edit_distance(&c(""), &c("abc")), 3);
    assert_eq!(edit_distance(&c("dˈoʊnt"), &c("dˈɔnt")), 2);
}
