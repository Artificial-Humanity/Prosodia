//! Synthesis controls — speaker, VAT and gain — checked against what a model
//! and its role can take. Refused, never adjusted: every rule here returns an
//! error naming the value and the fact that refused it.
//!
//! The rules are pure functions, so they are tested without model files;
//! `LiteRtActorEngine` calls them before any graph runs.

use crate::engine::{SpeechEngineError, SynthesisControls, DEFAULT_VAT};

/// What the monolith path feeds and applies for one call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MonolithControls {
    /// Values for the graph's `vat`/`emotion`/`control` input, or `None` when
    /// the graph has no such input (Matcha e2e).
    pub vat: Option<[f32; 3]>,
    /// Linear PCM gain applied after the graph and followed by a ±1 clip;
    /// `None` leaves the PCM untouched.
    pub gain: Option<f32>,
}

/// Refuses a `gain_db` that is not a finite number. Shared by every role.
pub(crate) fn check_gain_db_finite(db: f32) -> Result<(), String> {
    if db.is_finite() {
        Ok(())
    } else {
        Err(format!("gain_db {db} refused: not a finite number of dB"))
    }
}

/// Applies the monolith rules to `controls`.
///
/// * `has_vat_input` — whether the graph has a `vat`/`emotion`/`control` input.
///
/// A monolith has no speaker table, so `Some(n)` with n ≠ 0 is refused. VAT is
/// fed as before: three values, else `DEFAULT_VAT`. A finite `gain_db` of any
/// sign becomes a linear gain; a non-finite one, or one whose linear gain
/// overflows, is refused.
pub(crate) fn resolve_monolith_controls(
    controls: &SynthesisControls,
    has_vat_input: bool,
) -> Result<MonolithControls, String> {
    if let Some(row) = controls.speaker.filter(|&row| row != 0) {
        return Err(format!(
            "speaker {row} refused: this model has no speaker table, so only speaker 0 (or none) is valid"
        ));
    }
    let gain = match controls.gain_db {
        None => None,
        Some(db) => {
            check_gain_db_finite(db)?;
            let g = 10f32.powf(db / 20.0);
            if !g.is_finite() {
                // 0 · inf is NaN, and the ±1 clip passes NaN.
                return Err(format!("gain_db {db} refused: its linear gain 10^(gain_db/20) is not finite"));
            }
            Some(g)
        }
    };
    let vat = has_vat_input.then(|| match controls.vat.as_deref() {
        Some(&[v, a, t]) => [v, a, t],
        _ => DEFAULT_VAT,
    });
    Ok(MonolithControls { vat, gain })
}

/// Multiplies `pcm` by `gain`, then clips it to ±1. `None` leaves `pcm`
/// untouched, with no clip.
pub(crate) fn apply_pcm_gain(pcm: &mut [f32], gain: Option<f32>) {
    if let Some(g) = gain {
        for s in pcm.iter_mut() {
            *s = (*s * g).clamp(-1.0, 1.0);
        }
    }
}

/// One actor role's conditioning facts from `prosodia_models.json`: which VAT
/// channels its model trained, its default speaker row, and its speaker
/// labels. The export does not record which channels trained, so the role
/// config is authoritative.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RoleConditioning {
    /// VAT channels the model trained: 0 valence, 1 arousal (Energy), 2 tension.
    pub trained_vat: Vec<u32>,
    /// Speaker row used when the app selects none.
    pub default_speaker: u32,
    /// Raw LibriTTS-R reader IDs ("229") by speaker row; empty when the block has no `speakers`.
    /// The Tuner formats the label, "LibriTTS-R 229".
    pub speaker_labels: Vec<String>,
    /// Where the facts come from.
    pub evidence: String,
}

/// The `conditioning` block as `prosodia_models.json` writes it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ConditioningBlock {
    trained_vat: Vec<u32>,
    #[serde(default)]
    default_speaker: u32,
    #[serde(default)]
    speakers: Option<SpeakerSource>,
    evidence: String,
}

/// The block's `speakers` entry: the labels, and the Sonora file they were
/// copied from (read only by the label test; nothing reads `/data` at runtime).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct SpeakerSource {
    pub source: String,
    pub source_sha256: String,
    pub libritts_ids: Vec<String>,
}

/// Reads `role`'s `conditioning` block from the text of `prosodia_models.json`.
/// `Ok(None)` when the role has no block. An error when the role is not in
/// `roles`, or the block is malformed: an unknown key anywhere in it, a wrong
/// type, or a missing required field (`trainedVat`, `evidence`, and all three
/// keys of `speakers` when it is present).
#[uniffi::export]
pub fn parse_role_conditioning(
    models_json: String,
    role: String,
) -> Result<Option<RoleConditioning>, SpeechEngineError> {
    parse_conditioning_block(&models_json, &role)
        .map(|parsed| parsed.map(|(conditioning, _)| conditioning))
        .map_err(|msg| SpeechEngineError::Inference { msg })
}

/// `parse_role_conditioning`, keeping the `speakers` source for the label test.
pub(crate) fn parse_conditioning_block(
    models_json: &str,
    role: &str,
) -> Result<Option<(RoleConditioning, Option<SpeakerSource>)>, String> {
    let json: serde_json::Value = serde_json::from_str(models_json)
        .map_err(|e| format!("prosodia_models.json is not valid JSON: {e}"))?;
    let entry = json
        .get("roles")
        .and_then(|roles| roles.get(role))
        .ok_or_else(|| format!("role {role:?} is not in prosodia_models.json"))?;
    let Some(block) = entry.get("conditioning") else {
        return Ok(None);
    };
    let block: ConditioningBlock = serde_json::from_value(block.clone())
        .map_err(|e| format!("role {role:?}: malformed conditioning block: {e}"))?;
    let speaker_labels = block.speakers.as_ref().map(|s| s.libritts_ids.clone()).unwrap_or_default();
    Ok(Some((
        RoleConditioning {
            trained_vat: block.trained_vat,
            default_speaker: block.default_speaker,
            speaker_labels,
            evidence: block.evidence,
        },
        block.speakers,
    )))
}

/// What `resolve_controls` reads from the loaded split graphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModelFacts {
    /// Rows in the speaker table; 1 for a single-speaker model.
    pub speaker_count: usize,
    /// VAT channels the graphs take; 0 when they have no `vat` input.
    pub vat_dim: usize,
    /// Mel frames per graph call (`MAX_MEL`): the envelope's length.
    pub max_mel: usize,
}

/// What one split forward feeds: what `split_engine::Conditioning` borrows,
/// plus the mel-gain envelope.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedControls {
    /// Row of the speaker table.
    pub speaker: usize,
    /// Per-utterance VAT; `None` is the trained neutral, or no `vat` input.
    pub vat: Option<Vec<f32>>,
    /// Constant dB envelope, `max_mel` frames long; `None` is 0 dB.
    pub mel_gain_db: Option<Vec<f32>>,
}

/// The gain range Sonora's resident measured on the split graphs
/// (2026-10-04, `derisk-energy-24k`): RMS linear to 0.37 dB, WER flat.
pub(crate) const SPLIT_GAIN_DB_RANGE: std::ops::RangeInclusive<f32> = -12.0..=6.0;

const VAT_NAMES: [&str; 3] = ["valence", "arousal", "tension"];

/// The split-path rules: resolves the speaker (explicit row, else the role's
/// `defaultSpeaker`, else 0), checks VAT against the role's trained channels,
/// and turns `gain_db` into a constant envelope. Refused, never adjusted.
///
/// * `role` — the role's conditioning; `None` treats no VAT channel as
///   trained (fail closed).
/// * `facts` — speaker count, `vat_dim` and `max_mel` of the loaded graphs.
pub(crate) fn resolve_controls(
    controls: &SynthesisControls,
    role: Option<&RoleConditioning>,
    facts: ModelFacts,
) -> Result<ResolvedControls, String> {
    let speaker = controls.speaker.or(role.map(|r| r.default_speaker)).unwrap_or(0) as usize;
    if speaker >= facts.speaker_count {
        let origin = if controls.speaker.is_some() { "requested" } else { "the role's defaultSpeaker" };
        return Err(format!(
            "speaker {speaker} ({origin}) refused: this model has {} speaker(s), rows 0-{}",
            facts.speaker_count,
            facts.speaker_count.saturating_sub(1)
        ));
    }

    let vat = match (&controls.vat, facts.vat_dim) {
        // No `vat` input: nothing to feed.
        (_, 0) | (None, _) => None,
        (Some(v), dim) => {
            if v.len() != dim {
                return Err(format!("vat has {} values, refused: this model takes {dim}", v.len()));
            }
            for (c, &x) in v.iter().enumerate() {
                let name = VAT_NAMES.get(c).copied().unwrap_or("channel");
                let trained = role.is_some_and(|r| r.trained_vat.contains(&(c as u32)));
                if trained {
                    if !x.is_finite() || x.abs() > 1.0 {
                        return Err(format!("vat[{c}] ({name}) = {x} refused: outside [-1, 1]"));
                    }
                } else if x != 0.0 {
                    // -0.0 == 0.0, so a payload's "-0.00" passes.
                    return Err(match role {
                        Some(r) => format!(
                            "vat[{c}] ({name}) = {x} refused: this role's conditioning trains VAT channels {:?} only",
                            r.trained_vat
                        ),
                        None => format!(
                            "vat[{c}] ({name}) = {x} refused: this engine has no conditioning block, so no VAT channel is trained"
                        ),
                    });
                }
            }
            Some(v.clone())
        }
    };

    let mel_gain_db = match controls.gain_db {
        None => None,
        Some(db) => {
            check_gain_db_finite(db)?;
            if !SPLIT_GAIN_DB_RANGE.contains(&db) {
                let g = 10f64.powf(f64::from(db) / 20.0);
                return Err(format!(
                    "gain_db {db} (G ≈ {g:.3}) refused: outside [-12, +6] dB, the range measured on the split graphs"
                ));
            }
            Some(vec![db; facts.max_mel])
        }
    };

    Ok(ResolvedControls { speaker, vat, mel_gain_db })
}

/// The payload's linear gain `G:` in dB. A G that is not finite, or is ≤ 0,
/// is refused.
pub(crate) fn gain_db_from_multiplier(g: f64) -> Result<f32, String> {
    if !g.is_finite() || g <= 0.0 {
        return Err(format!("gain multiplier G {g} refused: it must be finite and above 0"));
    }
    Ok((20.0 * g.log10()) as f32)
}

/// Checks a role's block against the model it was passed with. Any failure
/// refuses the load: a `defaultSpeaker` outside the speaker table, a
/// `trainedVat` channel the model does not take (any channel, when it has no
/// `vat` input), or speaker labels that do not number the speaker rows.
pub(crate) fn validate_conditioning(role: &RoleConditioning, facts: ModelFacts) -> Result<(), String> {
    if role.default_speaker as usize >= facts.speaker_count {
        return Err(format!(
            "conditioning defaultSpeaker {} refused: this model has {} speaker(s)",
            role.default_speaker, facts.speaker_count
        ));
    }
    if facts.vat_dim == 0 && !role.trained_vat.is_empty() {
        return Err(format!(
            "conditioning trainedVat {:?} refused: this model has no vat input",
            role.trained_vat
        ));
    }
    if let Some(&c) = role.trained_vat.iter().find(|&&c| c as usize >= facts.vat_dim) {
        return Err(format!(
            "conditioning trainedVat channel {c} refused: this model takes {} VAT channels",
            facts.vat_dim
        ));
    }
    if !role.speaker_labels.is_empty() && role.speaker_labels.len() != facts.speaker_count {
        return Err(format!(
            "conditioning has {} speaker labels, refused: this model has {} speaker(s)",
            role.speaker_labels.len(),
            facts.speaker_count
        ));
    }
    Ok(())
}

/// The committed `prosodia_models.json`, for tests that read the real config.
#[cfg(test)]
pub(crate) fn committed_models_json() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../prosodia_models.json"))
        .expect("prosodia_models.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_controls(speaker: Option<u32>, vat: Option<Vec<f32>>, gain_db: Option<f32>) -> SynthesisControls {
        SynthesisControls { speaker, vat, gain_db }
    }

    #[test]
    fn monolith_feeds_three_vat_values_and_defaults_otherwise() {
        let fed = |vat| resolve_monolith_controls(&mono_controls(None, vat, None), true).unwrap().vat;
        assert_eq!(fed(Some(vec![0.1, -0.2, 0.3])), Some([0.1, -0.2, 0.3]));
        assert_eq!(fed(None), Some(DEFAULT_VAT));
        assert_eq!(fed(Some(vec![0.1, 0.2])), Some(DEFAULT_VAT));
    }

    #[test]
    fn monolith_without_a_vat_input_ignores_vat() {
        let out = resolve_monolith_controls(&mono_controls(None, Some(vec![0.9, 0.9, 0.9]), None), false).unwrap();
        assert_eq!(out, MonolithControls { vat: None, gain: None });
    }

    #[test]
    fn monolith_refuses_a_nonzero_speaker() {
        let err = resolve_monolith_controls(&mono_controls(Some(1), None, None), false).unwrap_err();
        assert!(err.contains("speaker 1") && err.contains("no speaker table"), "{err}");
        for ok in [None, Some(0)] {
            assert!(resolve_monolith_controls(&mono_controls(ok, None, None), true).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn monolith_gain_is_linear_in_db_with_boost_allowed() {
        let gain = |db| resolve_monolith_controls(&mono_controls(None, None, Some(db)), false).unwrap().gain.unwrap();
        assert!((gain(-6.0) - 0.501_187).abs() < 1e-5);
        assert!((gain(6.0) - 1.995_262).abs() < 1e-5);
        assert_eq!(gain(0.0), 1.0);
        assert_eq!(resolve_monolith_controls(&mono_controls(None, None, None), false).unwrap().gain, None);
    }

    #[test]
    fn monolith_refuses_non_finite_gain() {
        for db in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = resolve_monolith_controls(&mono_controls(None, None, Some(db)), false).unwrap_err();
            assert!(err.contains("gain_db"), "{err}");
        }
    }

    /// A finite `gain_db` whose linear gain overflows f32 (above ~770 dB)
    /// would scale samples to NaN, which the clip passes; it is refused.
    #[test]
    fn monolith_refuses_a_gain_that_overflows() {
        let err = resolve_monolith_controls(&mono_controls(None, None, Some(1000.0)), false).unwrap_err();
        assert!(err.contains("gain_db 1000") && err.contains("not finite"), "{err}");
    }

    #[test]
    fn pcm_gain_none_leaves_samples_untouched_and_unclipped() {
        let mut pcm = vec![0.5, -1.5, 2.0];
        apply_pcm_gain(&mut pcm, None);
        assert_eq!(pcm, vec![0.5, -1.5, 2.0]);
    }

    #[test]
    fn pcm_gain_scales_then_clips_to_one() {
        let mut pcm = vec![0.25, -0.75, 0.6];
        apply_pcm_gain(&mut pcm, Some(2.0));
        assert_eq!(pcm, vec![0.5, -1.0, 1.0]);
    }

    /// A `prosodia_models.json` text whose `actor-x` role carries `block`.
    fn with_block(block: &str) -> String {
        format!(r#"{{"roles": {{"actor": {{"path": "a.tflite"}}, "actor-x": {{"path": "x", "conditioning": {block}}}}}}}"#)
    }

    fn parse_x(block: &str) -> Result<Option<RoleConditioning>, SpeechEngineError> {
        parse_role_conditioning(with_block(block), "actor-x".to_string())
    }

    #[test]
    fn committed_actor_split_24k_block_parses() {
        let c = parse_role_conditioning(committed_models_json(), "actor-split-24k".to_string())
            .unwrap()
            .expect("actor-split-24k has a conditioning block");
        assert_eq!(c.trained_vat, vec![1]);
        assert_eq!(c.default_speaker, 22);
        assert_eq!(c.speaker_labels.len(), 247);
        assert_eq!((c.speaker_labels[0].as_str(), c.speaker_labels[22].as_str()), ("19", "229"));
        assert!(c.evidence.starts_with("Sonora resident 2026-10-04"), "{}", c.evidence);
    }

    #[test]
    fn roles_without_a_block_parse_to_none() {
        for role in ["actor", "actor-split", "g2p"] {
            assert_eq!(parse_role_conditioning(committed_models_json(), role.to_string()).unwrap(), None, "{role}");
        }
    }

    #[test]
    fn an_unknown_role_is_an_error() {
        let err = parse_role_conditioning(committed_models_json(), "actor-missing".to_string()).unwrap_err();
        let text = err.to_string();
        assert!(text.starts_with("inference error: ") && text.contains("actor-missing"), "{text}");
    }

    #[test]
    fn unknown_keys_are_refused() {
        let err = parse_x(r#"{"trainedVat": [1], "evidence": "e", "trainedVAT": [0]}"#).unwrap_err();
        assert!(err.to_string().contains("unknown field `trainedVAT`"), "{err}");
        let err = parse_x(
            r#"{"trainedVat": [1], "evidence": "e",
                "speakers": {"source": "s", "sourceSha256": "h", "librittsIds": [], "names": []}}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field `names`"), "{err}");
    }

    #[test]
    fn wrong_types_are_refused() {
        for block in [
            r#"{"trainedVat": "1", "evidence": "e"}"#,
            r#"{"trainedVat": [1], "defaultSpeaker": -1, "evidence": "e"}"#,
            r#"{"trainedVat": [1], "evidence": 5}"#,
            r#"{"trainedVat": [1], "evidence": "e", "speakers": {"source": "s", "sourceSha256": "h", "librittsIds": [19]}}"#,
            r#"null"#,
        ] {
            let err = parse_x(block).unwrap_err();
            assert!(err.to_string().contains("malformed conditioning block"), "{block}: {err}");
        }
    }

    #[test]
    fn missing_required_fields_are_refused() {
        for (block, field) in [
            (r#"{"evidence": "e"}"#, "trainedVat"),
            (r#"{"trainedVat": [1]}"#, "evidence"),
            (r#"{"trainedVat": [1], "evidence": "e", "speakers": {"source": "s", "librittsIds": []}}"#, "sourceSha256"),
        ] {
            let err = parse_x(block).unwrap_err();
            assert!(err.to_string().contains(&format!("missing field `{field}`")), "{block}: {err}");
        }
    }

    #[test]
    fn optional_fields_take_their_defaults() {
        let c = parse_x(r#"{"trainedVat": [], "evidence": "e"}"#).unwrap().unwrap();
        assert_eq!(
            c,
            RoleConditioning { trained_vat: Vec::new(), default_speaker: 0, speaker_labels: Vec::new(), evidence: "e".to_string() }
        );
    }

    /// The inline labels against the Sonora file they were copied from. Skips
    /// when the file is absent; `PROSODIA_REQUIRE_SONORA_SOURCES=1` makes that
    /// a failure. `PROSODIA_REQUIRE_PINNED_MODELS` does not govern it.
    #[test]
    fn speaker_labels_match_their_sonora_source() {
        use sha2::{Digest, Sha256};
        let (_, speakers) = parse_conditioning_block(&committed_models_json(), "actor-split-24k")
            .unwrap()
            .expect("actor-split-24k has a conditioning block");
        let speakers = speakers.expect("actor-split-24k names its speakers' source");
        let path = std::path::Path::new(&speakers.source);
        if !path.is_file() {
            if std::env::var("PROSODIA_REQUIRE_SONORA_SOURCES").as_deref() == Ok("1") {
                panic!("PROSODIA_REQUIRE_SONORA_SOURCES=1 but {} not found", path.display());
            }
            println!("Skipping: speaker source {} not found", path.display());
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(digest, speakers.source_sha256, "{} changed since the labels were copied", path.display());
        let source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let index = source["libritts_id_to_index"].as_object().expect("libritts_id_to_index");
        let mut by_row: Vec<(u64, &str)> =
            index.iter().map(|(id, row)| (row.as_u64().expect("row index"), id.as_str())).collect();
        by_row.sort();
        let rows: Vec<u64> = by_row.iter().map(|&(row, _)| row).collect();
        assert_eq!(rows, (0..speakers.libritts_ids.len() as u64).collect::<Vec<_>>(), "rows are not 0..n, once each");
        let inverted: Vec<&str> = by_row.iter().map(|&(_, id)| id).collect();
        assert_eq!(inverted, speakers.libritts_ids);
    }

    const DERISK: ModelFacts = ModelFacts { speaker_count: 247, vat_dim: 3, max_mel: 8 };
    const BASELINE: ModelFacts = ModelFacts { speaker_count: 1, vat_dim: 0, max_mel: 8 };

    fn derisk_role() -> RoleConditioning {
        RoleConditioning {
            trained_vat: vec![1],
            default_speaker: 22,
            speaker_labels: Vec::new(),
            evidence: "test".to_string(),
        }
    }

    fn vat_controls(vat: Vec<f32>) -> SynthesisControls {
        SynthesisControls { vat: Some(vat), ..Default::default() }
    }

    #[test]
    fn untrained_channels_must_be_exactly_zero() {
        let role = derisk_role();
        for (vat, channel) in [
            (vec![0.3, 0.0, 0.0], "vat[0] (valence)"),
            (vec![0.0, 0.0, -0.4], "vat[2] (tension)"),
            (vec![f32::NAN, 0.0, 0.0], "vat[0] (valence)"),
        ] {
            let err = resolve_controls(&vat_controls(vat.clone()), Some(&role), DERISK).unwrap_err();
            assert!(err.contains(channel) && err.contains("trains VAT channels [1] only"), "{vat:?}: {err}");
        }
        let ok = resolve_controls(&vat_controls(vec![0.0, 0.7, 0.0]), Some(&role), DERISK).unwrap();
        assert_eq!(ok.vat, Some(vec![0.0, 0.7, 0.0]));
    }

    #[test]
    fn no_block_fails_closed() {
        let err = resolve_controls(&vat_controls(vec![0.0, 0.5, 0.0]), None, DERISK).unwrap_err();
        assert!(err.contains("vat[1] (arousal)") && err.contains("no conditioning block"), "{err}");
        let zeros = resolve_controls(&vat_controls(vec![0.0, 0.0, 0.0]), None, DERISK).unwrap();
        assert_eq!((zeros.speaker, zeros.vat), (0, Some(vec![0.0; 3])));
    }

    #[test]
    fn trained_channels_are_width_and_range_checked() {
        let role = derisk_role();
        for vat in [
            vec![0.0, 1.5, 0.0],
            vec![0.0, -1.01, 0.0],
            vec![0.0, f32::INFINITY, 0.0],
            vec![0.0, f32::NAN, 0.0],
            vec![0.0, 0.5],
        ] {
            let err = resolve_controls(&vat_controls(vat.clone()), Some(&role), DERISK).unwrap_err();
            assert!(err.contains("vat"), "{vat:?}: {err}");
        }
        for a in [-1.0, 1.0] {
            assert!(resolve_controls(&vat_controls(vec![0.0, a, 0.0]), Some(&role), DERISK).is_ok(), "A = {a}");
        }
    }

    #[test]
    fn a_model_without_a_vat_input_drops_vat() {
        let r = resolve_controls(&vat_controls(vec![0.3, 0.6, 0.9]), None, BASELINE).unwrap();
        assert_eq!(r.vat, None);
    }

    #[test]
    fn speaker_resolves_explicit_then_role_default_then_zero() {
        let role = derisk_role();
        let speaker = |s: Option<u32>, role: Option<&RoleConditioning>| {
            resolve_controls(&SynthesisControls { speaker: s, ..Default::default() }, role, DERISK).unwrap().speaker
        };
        assert_eq!(speaker(None, Some(&role)), 22);
        assert_eq!(speaker(Some(100), Some(&role)), 100);
        assert_eq!(speaker(Some(0), Some(&role)), 0);
        assert_eq!(speaker(None, None), 0);
    }

    #[test]
    fn speakers_outside_the_table_are_refused() {
        let err = resolve_controls(&SynthesisControls { speaker: Some(247), ..Default::default() }, Some(&derisk_role()), DERISK)
            .unwrap_err();
        assert!(err.contains("speaker 247") && err.contains("247 speaker"), "{err}");
        let err = resolve_controls(&SynthesisControls { speaker: Some(1), ..Default::default() }, None, BASELINE).unwrap_err();
        assert!(err.contains("speaker 1"), "{err}");
        let zero = resolve_controls(&SynthesisControls { speaker: Some(0), ..Default::default() }, None, BASELINE).unwrap();
        assert_eq!(zero.speaker, 0);
        let big_default = RoleConditioning { default_speaker: 300, ..derisk_role() };
        let err = resolve_controls(&SynthesisControls::default(), Some(&big_default), DERISK).unwrap_err();
        assert!(err.contains("defaultSpeaker"), "{err}");
    }

    /// Both edges, with G as the payload writes it (3 decimals) and parsed
    /// back by `decode_spans`.
    #[test]
    fn gain_edges_at_payload_precision() {
        let g_of = |g: &str| {
            stage::prosody_payload::decode_spans(&format!("[V: 0.00 A: 0.00 T: 0.00 G: {g}] Hi."))
                .expect("payload decodes")
                .spans[0]
                .acoustics
                .as_ref()
                .and_then(|a| a.gain_multiplier)
                .expect("G parsed")
        };
        let resolve_g = |g: &str| {
            gain_db_from_multiplier(g_of(g)).and_then(|db| {
                resolve_controls(&SynthesisControls { gain_db: Some(db), ..Default::default() }, Some(&derisk_role()), DERISK)
            })
        };
        for g in ["0.252", "1.995", "0.600", "1.200", "1.000"] {
            assert!(resolve_g(g).is_ok(), "G {g}: {:?}", resolve_g(g).err());
        }
        for g in ["0.251", "1.996"] {
            let err = resolve_g(g).unwrap_err();
            assert!(err.contains("outside [-12, +6] dB") && err.contains(&format!("(G ≈ {g})")), "G {g}: {err}");
        }
    }

    #[test]
    fn non_finite_gain_is_refused() {
        for db in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = resolve_controls(&SynthesisControls { gain_db: Some(db), ..Default::default() }, Some(&derisk_role()), DERISK)
                .unwrap_err();
            assert!(err.contains("not a finite number"), "{err}");
        }
    }

    #[test]
    fn gain_becomes_a_constant_envelope() {
        let r = resolve_controls(&SynthesisControls { gain_db: Some(-3.5), ..Default::default() }, Some(&derisk_role()), DERISK)
            .unwrap();
        assert_eq!(r.mel_gain_db, Some(vec![-3.5; 8]));
        let none = resolve_controls(&SynthesisControls::default(), Some(&derisk_role()), DERISK).unwrap();
        assert_eq!(none.mel_gain_db, None);
    }

    /// The payload writes VAT with two decimals, so a Director V of -0.004
    /// arrives as "-0.00", parsed to -0.0. It counts as 0.
    #[test]
    fn negative_zero_from_a_payload_counts_as_zero() {
        let decoded = stage::prosody_payload::decode_spans("[V: -0.00 A: 0.40 T: -0.00] Hello.").expect("payload decodes");
        let e = &decoded.spans[0].emotion;
        let vat = vec![e.valence as f32, e.arousal as f32, e.tension as f32];
        assert!(vat[0].is_sign_negative(), "the payload must carry -0.0 for this test to mean anything");
        let r = resolve_controls(&vat_controls(vat), Some(&derisk_role()), DERISK).expect("-0.0 is zero");
        assert!((r.vat.unwrap()[1] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn gain_multiplier_must_be_finite_and_positive() {
        for g in [0.0, -0.0, -0.5, f64::NAN, f64::INFINITY] {
            let err = gain_db_from_multiplier(g).unwrap_err();
            assert!(err.contains("gain multiplier G"), "{g}: {err}");
        }
        assert_eq!(gain_db_from_multiplier(1.0), Ok(0.0));
        assert!((gain_db_from_multiplier(0.5).unwrap() + 6.0206).abs() < 1e-3);
    }

    #[test]
    fn validation_accepts_a_block_that_fits() {
        let labelled = RoleConditioning { speaker_labels: vec!["19".to_string(); 247], ..derisk_role() };
        assert_eq!(validate_conditioning(&labelled, DERISK), Ok(()));
        assert_eq!(validate_conditioning(&derisk_role(), DERISK), Ok(()));
        let empty = RoleConditioning { trained_vat: Vec::new(), default_speaker: 0, ..derisk_role() };
        assert_eq!(validate_conditioning(&empty, BASELINE), Ok(()));
    }

    #[test]
    fn validation_refuses_each_misfit() {
        let cases = [
            (RoleConditioning { default_speaker: 247, ..derisk_role() }, DERISK, "defaultSpeaker 247"),
            (RoleConditioning { trained_vat: vec![3], ..derisk_role() }, DERISK, "trainedVat channel 3"),
            (RoleConditioning { speaker_labels: vec!["19".to_string()], ..derisk_role() }, DERISK, "1 speaker labels"),
            (RoleConditioning { default_speaker: 0, ..derisk_role() }, BASELINE, "no vat input"),
        ];
        for (role, facts, fact) in cases {
            let err = validate_conditioning(&role, facts).unwrap_err();
            assert!(err.contains(fact), "{fact}: {err}");
        }
    }
}
