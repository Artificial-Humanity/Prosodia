//! Verifies the model pins in `prosodia_models.json` against the files on disk.
//!
//! Sonora's registry convention is that consumers pin a revision. The config
//! records the registry checkout's git revision and a sha256 for every file a
//! pinned role loads, and these tests check both, so a model that changes
//! underneath a role path fails here instead of loading silently.
//!
//! Paths resolve **lexically**, as `ProsodiaModelsManager` does in the apps:
//! `modelsBase` is relative to the config file's directory, role paths are
//! relative to `modelsBase`, and `..` is removed without following symlinks.
//!
//! A role whose directory is absent on this machine is skipped with a message;
//! inside a directory that exists, a missing or stale pin fails. When the
//! registry checkout is present, verifying zero files fails too. Set
//! `PROSODIA_REQUIRE_PINNED_MODELS=1` to turn every skip into a failure.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Removes `.` and `..` components without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Hex sha256 of a file, streamed.
fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn require_pinned_models() -> bool {
    std::env::var("PROSODIA_REQUIRE_PINNED_MODELS").as_deref() == Ok("1")
}

/// Prints a skip, or panics when `PROSODIA_REQUIRE_PINNED_MODELS=1`.
fn skip(reason: &str) {
    if require_pinned_models() {
        panic!("PROSODIA_REQUIRE_PINNED_MODELS=1 but {reason}");
    }
    println!("Skipping: {reason}");
}

struct LoadedConfig {
    json: serde_json::Value,
    models_base: PathBuf,
}

fn load_config() -> LoadedConfig {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("prosodia_models.json");
    let text = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", config_path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("prosodia_models.json is not JSON");
    let base = json["modelsBase"].as_str().expect("modelsBase missing");
    let models_base = lexical_normalize(&repo_root.join(base));
    LoadedConfig { json, models_base }
}

/// Whether a role path names a split-model directory or a single model file.
/// Decided as the engines decide it — by what is on disk — and by the file
/// extension only when the path is absent.
fn is_directory_role(role_path: &Path) -> bool {
    if role_path.exists() {
        role_path.is_dir()
    } else {
        role_path.extension().is_none()
    }
}

/// The directory a role's pinned filenames are relative to: the role path
/// itself for a directory role, its parent for a file role.
fn pin_dir(role_path: &Path) -> PathBuf {
    if is_directory_role(role_path) {
        role_path.to_path_buf()
    } else {
        role_path.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// Files the actor engines read from a role's directory. For a split model
/// directory that is every `matcha_{textenc,decoder,vocoder}*.tflite` graph
/// (`find_graph` takes the first match in directory order, so all candidates
/// count) plus `emb.bin` and `config.json`; for an e2e model it is the model
/// file plus the adjacent `config.json`.
fn loaded_files(role_path: &Path) -> Vec<String> {
    if !is_directory_role(role_path) {
        let model = role_path.file_name().unwrap().to_string_lossy().to_string();
        return vec![model, "config.json".to_string()];
    }
    let mut files = vec!["config.json".to_string(), "emb.bin".to_string()];
    if let Ok(entries) = std::fs::read_dir(role_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_graph = ["matcha_textenc", "matcha_decoder", "matcha_vocoder"]
                .iter()
                .any(|p| name.starts_with(p));
            if is_graph && name.ends_with(".tflite") {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}

/// Roles that carry `sha256` pins, with their lexically resolved paths.
fn pinned_roles(cfg: &LoadedConfig) -> Vec<(String, PathBuf, serde_json::Map<String, serde_json::Value>)> {
    let roles = cfg.json["roles"].as_object().expect("roles missing");
    roles
        .iter()
        .filter_map(|(name, role)| {
            let pins = role["sha256"].as_object()?.clone();
            let path = lexical_normalize(&cfg.models_base.join(role["path"].as_str().unwrap()));
            Some((name.clone(), path, pins))
        })
        .collect()
}

/// Whether the registry checkout named in the config is present on this
/// machine. When it is, the workspace layout is present, and verifying zero
/// pinned files means the layout and the config disagree.
fn registry_checkout_present(cfg: &LoadedConfig) -> bool {
    let checkout = cfg.json["registry"]["checkout"].as_str().expect("registry.checkout missing");
    lexical_normalize(&cfg.models_base.join(checkout)).join(".git").exists()
}

#[test]
fn pinned_roles_pin_exactly_the_files_the_engines_load() {
    let cfg = load_config();
    let roles = pinned_roles(&cfg);
    let names: Vec<&str> = roles.iter().map(|(n, _, _)| n.as_str()).collect();
    for required in ["actor", "actor-split"] {
        assert!(names.contains(&required), "role {required} has no sha256 pins");
    }
    for (name, role_path, pins) in &roles {
        assert!(!pins.is_empty(), "role {name} pins no files");
        if !pin_dir(role_path).is_dir() {
            skip(&format!("role {name}: {} not found", pin_dir(role_path).display()));
            continue;
        }
        let loaded = loaded_files(role_path);
        for file in &loaded {
            assert!(pins.contains_key(file), "role {name} loads {file} but does not pin it");
        }
        for file in pins.keys() {
            assert!(
                loaded.contains(file),
                "role {name} pins {file}, which the engines do not load (stale pin?)"
            );
        }
    }
}

#[test]
fn pinned_files_match_their_sha256() {
    let cfg = load_config();
    let mut checked = 0;
    let mut pinned_total = 0;
    for (name, role_path, pins) in pinned_roles(&cfg) {
        pinned_total += pins.len();
        let dir = pin_dir(&role_path);
        if !dir.is_dir() {
            skip(&format!("role {name}: {} not found", dir.display()));
            continue;
        }
        // The role is present, so every pinned file must be too: a missing
        // file is a mismatch, not a skip.
        let mut mismatches = BTreeMap::new();
        for (file, want) in &pins {
            let path = dir.join(file);
            let got = if path.is_file() {
                sha256_hex(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
            } else {
                "<missing>".to_string()
            };
            if got != want.as_str().unwrap() {
                mismatches.insert(file.clone(), got);
            }
            checked += 1;
        }
        assert!(
            mismatches.is_empty(),
            "role {name}: files differ from their pins (got sha256): {mismatches:?}"
        );
    }
    assert!(pinned_total > 0, "no role pins any file; nothing was verified");
    if registry_checkout_present(&cfg) {
        assert!(
            checked > 0,
            "the registry checkout is present but no pinned file was verified; \
             the config's paths do not match this workspace"
        );
    }
    println!("model pins: {checked} of {pinned_total} pinned files verified");
}

#[test]
fn registry_checkout_is_at_the_pinned_revision() {
    let cfg = load_config();
    let registry = &cfg.json["registry"];
    let want = registry["revision"].as_str().expect("registry.revision missing");
    let checkout = lexical_normalize(
        &cfg.models_base.join(registry["checkout"].as_str().expect("registry.checkout missing")),
    );
    if !checkout.join(".git").exists() {
        skip(&format!("registry checkout {} not found", checkout.display()));
        return;
    }
    // Clear git's repository overrides: when this runs inside a git hook they
    // point at Prosodia's own repository, and `-C` alone does not override them.
    let out = match Command::new("git")
        .arg("-C")
        .arg(&checkout)
        .args(["rev-parse", "HEAD"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            skip(&format!("cannot run git: {e}"));
            return;
        }
    };
    assert!(out.status.success(), "git rev-parse failed in {}", checkout.display());
    let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        got, want,
        "registry checkout {} is at {got}, pinned {want}; re-pin only after checking with Sonora's resident",
        checkout.display()
    );
}

#[test]
fn lexical_normalize_does_not_follow_parent_through_symlinks() {
    assert_eq!(
        lexical_normalize(Path::new("/ws/Prosodia/../models/../Sonora/huggingface")),
        PathBuf::from("/ws/Sonora/huggingface")
    );
}
