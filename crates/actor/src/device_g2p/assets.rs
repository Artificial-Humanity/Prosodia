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

/// Reflects Python's truthiness: non-empty container, truthy boolean, or non-zero number.
fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                false
            }
        }
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(o) => !o.is_empty(),
    }
}

/// Parses and checks the contraction tables.
pub fn tables_from_json(text: &str) -> Result<ContractionTables, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("g2p_contractions.json: {e}"))?;
    for key in ["contractions", "clitics", "possessive"] {
        if v[key].as_object().map_or(true, |o| o.is_empty()) {
            return Err(format!("g2p_contractions.json is missing {key}; re-export it from op_g2p"));
        }
    }
    if v.get("homographs").is_some_and(|h| truthy(h)) {
        return Err("g2p_contractions.json comes from a homograph-resolving host; \
                    the device G2P has no resolver, so the front ends cannot agree".to_string());
    }
    let map = |key: &str| -> Result<Vec<(String, String)>, String> {
        v[key].as_object().unwrap().iter()
            .map(|(k, s)| {
                s.as_str()
                    .map(|s| (k.clone(), s.to_string()))
                    .ok_or_else(|| format!("g2p_contractions.json: {key}.{k} is not a string"))
            })
            .collect()
    };
    let p = &v["possessive"];
    Ok(ContractionTables {
        contractions: map("contractions")?.into_iter().collect(),
        clitics: map("clitics")?,
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
        let (text, source) = if gz.is_file() {
            let mut s = String::new();
            flate2::read::GzDecoder::new(std::fs::File::open(&gz).map_err(|e| format!("{}: {e}", gz.display()))?)
                .read_to_string(&mut s)
                .map_err(|e| format!("{}: {e}", gz.display()))?;
            (s, gz)
        } else if plain.is_file() {
            (std::fs::read_to_string(&plain).map_err(|e| format!("{}: {e}", plain.display()))?, plain)
        } else {
            return Err(format!("no g2p_dict.txt(.gz) in {}", dir.display()));
        };
        let mut dict = HashMap::new();
        for (word, ipa) in text.lines().filter_map(|line| line.split_once('\t')) {
            let ipa = ipa.replace(COMBINING_TILDE, "");
            if ipa.is_empty() {
                return Err(format!("{}: {word:?} has no IPA; re-export the dictionary", source.display()));
            }
            dict.insert(word.to_string(), ipa);
        }
        if dict.is_empty() {
            return Err(format!("{}: no entries (no word<TAB>ipa lines); re-export the dictionary", source.display()));
        }
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

    /// A fresh directory under the system temp dir holding one plain
    /// `g2p_dict.txt` with `contents`.
    fn dict_dir(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("prosodia-g2p-assets-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("g2p_dict.txt"), contents).unwrap();
        dir
    }

    #[test]
    fn an_empty_dictionary_is_refused() {
        for (name, contents) in [("empty", ""), ("no-tabs", "hello\nworld\n")] {
            let dir = dict_dir(name, contents);
            let result = G2pAssets::load(&dir);
            std::fs::remove_dir_all(&dir).unwrap();
            let err = result.err().unwrap_or_else(|| panic!("{name}: an entry-less dictionary loaded"));
            assert!(err.contains("g2p_dict.txt") && err.contains("no entries"), "{name}: {err}");
        }
    }

    #[test]
    fn an_entry_with_no_ipa_is_refused_naming_the_word() {
        for (name, contents) in [("blank", "hello\thəlˈoʊ\nword\t\n"), ("tilde-only", "hello\thəlˈoʊ\nword\t\u{303}\n")] {
            let dir = dict_dir(name, contents);
            let result = G2pAssets::load(&dir);
            std::fs::remove_dir_all(&dir).unwrap();
            let err = result.err().unwrap_or_else(|| panic!("{name}: an empty IPA value loaded"));
            assert!(err.contains("\"word\""), "{name}: {err}");
        }
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

    #[test]
    fn non_string_table_values_are_refused() {
        let mut v: serde_json::Value = serde_json::from_str(EMBEDDED_TABLES).unwrap();
        v["contractions"]["won't"] = serde_json::json!(5);
        let err = tables_from_json(&v.to_string()).err().unwrap();
        assert!(err.contains("won't"), "{err}");
    }

    #[test]
    fn falsy_homographs_markers_load() {
        for falsy in &[
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!({}),
            serde_json::json!([]),
        ] {
            let mut v: serde_json::Value = serde_json::from_str(EMBEDDED_TABLES).unwrap();
            v["homographs"] = falsy.clone();
            assert!(tables_from_json(&v.to_string()).is_ok(), "failed for {falsy}");
        }
    }
}
