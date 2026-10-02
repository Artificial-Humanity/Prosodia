//! The device G2P's data: the OpenPhonemizer dictionary and the contraction
//! tables exported from Sonora's `op_g2p`. Missing or incomplete data is an
//! error — never a fallback (Sonora's defect D-C1 hid behind one).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub(crate) const EMBEDDED_TABLES: &str = include_str!("../../resources/g2p_contractions.json");
const COMBINING_TILDE: char = '\u{303}';

pub struct Possessive {
    pub sibilant: Vec<String>,
    pub voiceless: Vec<String>,
    pub after_sibilant: String,
    pub after_voiceless: String,
    pub default: String,
    pub stress_marks: String,
}

pub struct ContractionTables {
    pub contractions: HashMap<String, String>,
    pub clitics: Vec<(String, String)>,
    pub possessive: Possessive,
}

pub struct G2pAssets {
    pub dict: HashMap<String, String>,
    pub tables: ContractionTables,
}

fn strings(v: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    v[key].as_array().ok_or(format!("possessive.{key} missing"))?.iter()
        .map(|s| s.as_str().map(String::from).ok_or(format!("possessive.{key} has a non-string")))
        .collect()
}

fn string(v: &serde_json::Value, key: &str) -> Result<String, String> {
    v[key].as_str().map(String::from).ok_or(format!("possessive.{key} missing"))
}

/// Parses and checks the contraction tables.
pub fn tables_from_json(text: &str) -> Result<ContractionTables, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("g2p_contractions.json: {e}"))?;
    for key in ["contractions", "clitics", "possessive"] {
        if v[key].as_object().map_or(true, |o| o.is_empty()) {
            return Err(format!("g2p_contractions.json is missing {key}; re-export it from op_g2p"));
        }
    }
    if v.get("homographs").is_some_and(|h| !h.is_null()) {
        return Err("g2p_contractions.json comes from a homograph-resolving host; \
                    the device G2P has no resolver, so the front ends cannot agree".to_string());
    }
    let map = |key: &str| -> Vec<(String, String)> {
        v[key].as_object().unwrap().iter()
            .map(|(k, s)| (k.clone(), s.as_str().unwrap_or_default().to_string()))
            .collect()
    };
    let p = &v["possessive"];
    Ok(ContractionTables {
        contractions: map("contractions").into_iter().collect(),
        clitics: map("clitics"),
        possessive: Possessive {
            sibilant: strings(p, "sibilant")?,
            voiceless: strings(p, "voiceless")?,
            after_sibilant: string(p, "after_sibilant")?,
            after_voiceless: string(p, "after_voiceless")?,
            default: string(p, "default")?,
            stress_marks: string(p, "stress_marks")?,
        },
    })
}

impl G2pAssets {
    /// Loads the dictionary from `dir` and the tables from `dir` when it has
    /// them (a model export's copy wins), else the embedded copy.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let gz = dir.join("g2p_dict.txt.gz");
        let plain = dir.join("g2p_dict.txt");
        let text = if gz.is_file() {
            let mut s = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(&gz).map_err(|e| format!("{}: {e}", gz.display()))?)
                .read_to_string(&mut s)
                .map_err(|e| format!("{}: {e}", gz.display()))?;
            s
        } else if plain.is_file() {
            std::fs::read_to_string(&plain).map_err(|e| format!("{}: {e}", plain.display()))?
        } else {
            return Err(format!("no g2p_dict.txt(.gz) in {}", dir.display()));
        };
        let dict = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(w, ipa)| (w.to_string(), ipa.replace(COMBINING_TILDE, "")))
            .collect();
        let tables_path = dir.join("g2p_contractions.json");
        let tables = if tables_path.is_file() {
            tables_from_json(&std::fs::read_to_string(&tables_path).map_err(|e| format!("{}: {e}", tables_path.display()))?)?
        } else {
            tables_from_json(EMBEDDED_TABLES)?
        };
        Ok(Self { dict, tables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSETS: &str = "/data/models/litert-community/Matcha-TTS";

    #[test]
    fn embedded_tables_parse_and_are_complete() {
        let t = tables_from_json(EMBEDDED_TABLES).unwrap();
        assert_eq!(t.contractions["won't"], "wˈoʊnt");
        assert_eq!(t.clitics.len(), 6);
        assert_eq!(t.possessive.after_sibilant, "ᵻz");
    }

    #[test]
    fn tables_missing_a_block_or_carrying_homographs_are_refused() {
        let err = tables_from_json(r#"{"contractions": {"a'b": "x"}, "possessive": {}}"#).err().unwrap();
        assert!(err.contains("clitics"), "{err}");
        let mut v: serde_json::Value = serde_json::from_str(EMBEDDED_TABLES).unwrap();
        v["homographs"] = serde_json::json!({"live": ["lˈɪv", "lˈaɪv"]});
        let err = tables_from_json(&v.to_string()).err().unwrap();
        assert!(err.contains("homograph"), "{err}");
    }

    #[test]
    fn a_missing_dictionary_is_an_error_naming_the_file() {
        let err = G2pAssets::load(Path::new("/nonexistent")).err().unwrap();
        assert!(err.contains("g2p_dict.txt"), "{err}");
    }

    #[test]
    fn loads_the_pinned_dictionary() {
        if !Path::new(ASSETS).join("g2p_dict.txt.gz").exists() {
            assert!(std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() != Ok("1"), "{ASSETS} missing");
            println!("Skipping: {ASSETS} not found");
            return;
        }
        let a = G2pAssets::load(Path::new(ASSETS)).unwrap();
        assert!(a.dict.len() > 270_000, "{} entries", a.dict.len());
        assert!(!a.dict.keys().any(|k| k.contains('\'')), "dictionary must have no apostrophe keys");
        assert!(!a.dict.values().any(|v| v.contains('\u{303}')), "combining tilde must be stripped");
    }
}
