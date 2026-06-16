use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(untagged)]
enum LexiconValue {
    Single(String),
    Variants(HashMap<String, Option<String>>),
}

fn grow_dictionary<T: Clone>(dictionary: HashMap<String, T>) -> HashMap<String, T> {
    let mut result = dictionary.clone();
    for (key, value) in dictionary.iter() {
        if key.chars().count() >= 2 {
            if key == &key.to_lowercase() {
                let mut chars = key.chars();
                if let Some(first) = chars.next() {
                    let capitalized = first.to_uppercase().to_string() + chars.as_str();
                    if &capitalized != key {
                        result.insert(capitalized, value.clone());
                    }
                }
            } else {
                let mut chars = key.chars();
                if let Some(first) = chars.next() {
                    let mut rest_lower = true;
                    for c in chars {
                        if !c.is_lowercase() {
                            rest_lower = false;
                            break;
                        }
                    }
                    if first.is_uppercase() && rest_lower {
                        result.insert(key.to_lowercase(), value.clone());
                    }
                }
            }
        }
    }
    result
}

fn serialize_silver(dict: HashMap<String, String>) -> Vec<u8> {
    let dict = grow_dictionary(dict);
    let mut entries: Vec<(String, String)> = dict.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut string_pool = Vec::new();
    let mut index_bytes = Vec::new();

    for (key, val) in &entries {
        let key_offset = u32::try_from(string_pool.len()).expect("G2P silver string pool offset exceeds u32::MAX");
        let key_len = u16::try_from(key.len()).expect("G2P silver key length exceeds u16::MAX");
        string_pool.extend_from_slice(key.as_bytes());

        let val_offset = u32::try_from(string_pool.len()).expect("G2P silver string pool offset exceeds u32::MAX");
        let val_len = u16::try_from(val.len()).expect("G2P silver value length exceeds u16::MAX");
        string_pool.extend_from_slice(val.as_bytes());

        index_bytes.extend_from_slice(&key_offset.to_le_bytes());
        index_bytes.extend_from_slice(&key_len.to_le_bytes());
        index_bytes.extend_from_slice(&val_offset.to_le_bytes());
        index_bytes.extend_from_slice(&val_len.to_le_bytes());
    }

    let num_entries = u32::try_from(entries.len()).expect("G2P silver entries count exceeds u32::MAX");
    let pool_size = u32::try_from(string_pool.len()).expect("G2P silver string pool size exceeds u32::MAX");

    let mut out = Vec::new();
    out.extend_from_slice(b"PSL1"); // Prosodia Silver Lexicon v1 magic
    out.extend_from_slice(&num_entries.to_le_bytes());
    out.extend_from_slice(&pool_size.to_le_bytes());
    out.extend_from_slice(&index_bytes);
    out.extend_from_slice(&string_pool);
    out
}

fn serialize_gold(dict: HashMap<String, LexiconValue>) -> Vec<u8> {
    let dict = grow_dictionary(dict);
    let mut entries: Vec<(String, LexiconValue)> = dict.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut string_pool = Vec::new();
    let mut values_pool = Vec::new();
    let mut index_bytes = Vec::new();

    let mut get_string_offset = |s: &str| -> (u32, u16) {
        let offset = u32::try_from(string_pool.len()).expect("G2P gold string pool offset exceeds u32::MAX");
        let len = u16::try_from(s.len()).expect("G2P gold string length exceeds u16::MAX");
        string_pool.extend_from_slice(s.as_bytes());
        (offset, len)
    };

    for (key, val) in &entries {
        let (key_offset, key_len) = get_string_offset(key);

        let val_type: u8;
        let val_data_offset: u32;
        let val_data_len: u16;

        match val {
            LexiconValue::Single(s) => {
                val_type = 0;
                let (offset, len) = get_string_offset(s);
                val_data_offset = offset;
                val_data_len = len;
            }
            LexiconValue::Variants(variants) => {
                val_type = 1;
                val_data_offset = u32::try_from(values_pool.len()).expect("G2P gold values pool offset exceeds u32::MAX");
                val_data_len = u16::try_from(variants.len()).expect("G2P gold variants count exceeds u16::MAX");

                for (tag, opt_val) in variants {
                    let (tag_offset, tag_len) = get_string_offset(tag);
                    let (has_val, val_offset, val_len) = match opt_val {
                        Some(v) => {
                            let (vo, vl) = get_string_offset(v);
                            (1u8, vo, vl)
                        }
                        None => (0u8, 0u32, 0u16),
                    };

                    values_pool.extend_from_slice(&tag_offset.to_le_bytes());
                    values_pool.extend_from_slice(&tag_len.to_le_bytes());
                    values_pool.push(has_val);
                    values_pool.extend_from_slice(&val_offset.to_le_bytes());
                    values_pool.extend_from_slice(&val_len.to_le_bytes());
                }
            }
        }

        index_bytes.extend_from_slice(&key_offset.to_le_bytes());
        index_bytes.extend_from_slice(&key_len.to_le_bytes());
        index_bytes.push(val_type);
        index_bytes.push(0u8); // padding to align next u32
        index_bytes.extend_from_slice(&val_data_offset.to_le_bytes());
        index_bytes.extend_from_slice(&val_data_len.to_le_bytes());
    }

    let num_entries = u32::try_from(entries.len()).expect("G2P gold entries count exceeds u32::MAX");
    let pool_size = u32::try_from(string_pool.len()).expect("G2P gold string pool size exceeds u32::MAX");
    let val_pool_size = u32::try_from(values_pool.len()).expect("G2P gold values pool size exceeds u32::MAX");

    let mut out = Vec::new();
    out.extend_from_slice(b"PGL1"); // Prosodia Gold Lexicon v1 magic
    out.extend_from_slice(&num_entries.to_le_bytes());
    out.extend_from_slice(&pool_size.to_le_bytes());
    out.extend_from_slice(&val_pool_size.to_le_bytes());
    out.extend_from_slice(&index_bytes);
    out.extend_from_slice(&string_pool);
    out.extend_from_slice(&values_pool);
    out
}

fn compile_lexicons() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let resources_dir = manifest_dir.join("resources");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Compile US Gold
    let us_gold_path = resources_dir.join("us_gold.json");
    let mut file = File::open(&us_gold_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let dict: HashMap<String, LexiconValue> = serde_json::from_str(&contents).unwrap();
    let bin = serialize_gold(dict);
    let mut out_file = File::create(out_dir.join("us_gold.bin")).unwrap();
    out_file.write_all(&bin).unwrap();

    // Compile GB Gold
    let gb_gold_path = resources_dir.join("gb_gold.json");
    let mut file = File::open(&gb_gold_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let dict: HashMap<String, LexiconValue> = serde_json::from_str(&contents).unwrap();
    let bin = serialize_gold(dict);
    let mut out_file = File::create(out_dir.join("gb_gold.bin")).unwrap();
    out_file.write_all(&bin).unwrap();

    // Compile US Silver
    let us_silver_path = resources_dir.join("us_silver.json");
    let mut file = File::open(&us_silver_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let dict: HashMap<String, String> = serde_json::from_str(&contents).unwrap();
    let bin = serialize_silver(dict);
    let mut out_file = File::create(out_dir.join("us_silver.bin")).unwrap();
    out_file.write_all(&bin).unwrap();

    // Compile GB Silver
    let gb_silver_path = resources_dir.join("gb_silver.json");
    let mut file = File::open(&gb_silver_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let dict: HashMap<String, String> = serde_json::from_str(&contents).unwrap();
    let bin = serialize_silver(dict);
    let mut out_file = File::create(out_dir.join("gb_silver.bin")).unwrap();
    out_file.write_all(&bin).unwrap();

    println!("cargo:rerun-if-changed=resources/us_gold.json");
    println!("cargo:rerun-if-changed=resources/gb_gold.json");
    println!("cargo:rerun-if-changed=resources/us_silver.json");
    println!("cargo:rerun-if-changed=resources/gb_silver.json");
}

fn main() {
    compile_lexicons();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "macos" || target_os == "ios" {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let root = manifest_dir.parent().unwrap().parent().unwrap();
        let dylib_dir = root
            .join("platforms/apple/.build/artifacts/litert-lm/CLiteRTLM_mac/CLiteRTLM_mac.xcframework/macos-arm64_x86_64");

        if dylib_dir.exists() {
            println!("cargo:rustc-link-search=native={}", dylib_dir.display());
            println!("cargo:rustc-link-lib=dylib=CLiteRTLM_mac");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dylib_dir.display());
        } else {
            println!("cargo:warning=CLiteRTLM_mac dylib directory not found at {}", dylib_dir.display());
        }
    } else if target_os == "android" {
        println!("cargo:rustc-link-arg=-Wl,-z,undefs");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
