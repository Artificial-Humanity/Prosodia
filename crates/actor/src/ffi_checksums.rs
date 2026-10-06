//! The committed Swift and Kotlin bindings carry a checksum per exported
//! item, and both apps refuse to start ("UniFFI API checksum mismatch") when
//! the library's checksum differs. uniffi 0.27 hashes each item's metadata —
//! signature AND doc comment — so a `///` line on an exported method breaks
//! the apps as surely as a signature change. This test compares every
//! checksum in the committed bindings against the library's own.
//!
//! The list below must name every checksum the bindings check; the test
//! fails, naming the gap, when the two drift apart (an export added to or
//! removed from the bindings without the list following).

use regex::Regex;
use std::collections::BTreeMap;

macro_rules! checksums {
    ($($name:ident,)*) => {
        mod ffi {
            extern "C" {
                $(pub fn $name() -> u16;)*
            }
        }
        /// `(name without the uniffi_actor_checksum_ prefix, library value)`.
        fn library_checksums() -> BTreeMap<String, u16> {
            let mut m = BTreeMap::new();
            $(
                let full = stringify!($name);
                // SAFETY: a uniffi checksum function takes no arguments, has no
                // side effects and returns a u16 (uniffi_macros::util).
                m.insert(full.trim_start_matches("uniffi_actor_checksum_").to_string(), unsafe { ffi::$name() });
            )*
            m
        }
    };
}

checksums! {
    uniffi_actor_checksum_constructor_defaultmodelassetmanager_new,
    uniffi_actor_checksum_constructor_litertactorengine_new,
    uniffi_actor_checksum_constructor_litertactorengine_new_with_conditioning,
    uniffi_actor_checksum_constructor_prosodiaactorengine_new,
    uniffi_actor_checksum_constructor_prosodiaactorpipeline_new,
    uniffi_actor_checksum_constructor_prosodiaspeech_new,
    uniffi_actor_checksum_constructor_prosodiaspeech_new_with_options,
    uniffi_actor_checksum_constructor_voiceloader_new,
    uniffi_actor_checksum_func_blend_style_packs,
    uniffi_actor_checksum_func_chunk_phonemes,
    uniffi_actor_checksum_func_chunk_tokens,
    uniffi_actor_checksum_func_normalize_style_pack,
    uniffi_actor_checksum_func_parse_blend_string,
    uniffi_actor_checksum_func_parse_role_conditioning,
    uniffi_actor_checksum_func_parse_safetensors,
    uniffi_actor_checksum_func_slice_style_row,
    uniffi_actor_checksum_method_audiochunkcallback_on_audio_chunk,
    uniffi_actor_checksum_method_audiosink_schedule_audio,
    uniffi_actor_checksum_method_defaultmodelassetmanager_load_style_anchor,
    uniffi_actor_checksum_method_defaultmodelassetmanager_resolve_casting_profile,
    uniffi_actor_checksum_method_litertactorengine_forward,
    uniffi_actor_checksum_method_litertactorengine_get_token_limit,
    uniffi_actor_checksum_method_litertactorengine_is_matcha,
    uniffi_actor_checksum_method_litertactorengine_reclaim_memory,
    uniffi_actor_checksum_method_modelassetmanager_load_style_anchor,
    uniffi_actor_checksum_method_modelassetmanager_resolve_casting_profile,
    uniffi_actor_checksum_method_prosodiaactorengine_process_and_synthesize,
    uniffi_actor_checksum_method_prosodiaactorengine_reclaim_memory,
    uniffi_actor_checksum_method_prosodiaactorengine_set_speaker,
    uniffi_actor_checksum_method_prosodiaactorpipeline_chunk_phonemes,
    uniffi_actor_checksum_method_prosodiaactorpipeline_chunk_tokens,
    uniffi_actor_checksum_method_prosodiaactorpipeline_prewarm,
    uniffi_actor_checksum_method_prosodiaactorpipeline_process_span,
    uniffi_actor_checksum_method_prosodiaactorpipeline_reclaim_memory,
    uniffi_actor_checksum_method_prosodiaactorpipeline_set_custom_g2p,
    uniffi_actor_checksum_method_prosodiaactorpipeline_should_map_ipa,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize_markup,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize_stream,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize_stream_with_morph,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize_with_timestamps,
    uniffi_actor_checksum_method_prosodiaactorpipeline_synthesize_with_timestamps_blend,
    uniffi_actor_checksum_method_prosodiaactorpipeline_tokenize,
    uniffi_actor_checksum_method_prosodiaactorpipeline_tokenize_phonemes,
    uniffi_actor_checksum_method_prosodiag2pprocessor_process,
    uniffi_actor_checksum_method_prosodiaspeechengine_forward,
    uniffi_actor_checksum_method_prosodiaspeechengine_get_token_limit,
    uniffi_actor_checksum_method_prosodiaspeechengine_is_matcha,
    uniffi_actor_checksum_method_prosodiaspeechengine_reclaim_memory,
    uniffi_actor_checksum_method_prosodiaspeechengine_synthesize,
    uniffi_actor_checksum_method_prosodiaspeech_process,
    uniffi_actor_checksum_method_voiceassetprovider_load_voice_bytes,
    uniffi_actor_checksum_method_voiceloader_clear_cache,
    uniffi_actor_checksum_method_voiceloader_lerp,
    uniffi_actor_checksum_method_voiceloader_load_blend,
    uniffi_actor_checksum_method_voiceloader_load_voice,
    uniffi_actor_checksum_method_voiceloader_resolve_parametric_voice,
    uniffi_actor_checksum_method_voiceloader_style_matrix,
    uniffi_actor_checksum_method_voiceloader_style_vector,
}

fn committed(path: &str) -> BTreeMap<String, u16> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let re = Regex::new(r"uniffi_actor_checksum_(\w+)\(\)\s*!=\s*(\d+)").unwrap();
    re.captures_iter(&text).map(|c| (c[1].to_string(), c[2].parse().unwrap())).collect()
}

#[test]
fn ffi_checksums_match_committed_bindings() {
    let library = library_checksums();
    for path in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../platforms/android/src/main/kotlin/uniffi/actor/actor.kt"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../platforms/apple/Sources/Kit/Generated/actor.swift"),
    ] {
        let bindings = committed(path);
        assert!(bindings.len() >= 50, "{path}: only {} checksums parsed", bindings.len());
        let unlisted: Vec<_> = bindings.keys().filter(|k| !library.contains_key(*k)).collect();
        let unchecked: Vec<_> = library.keys().filter(|k| !bindings.contains_key(*k)).collect();
        assert!(unlisted.is_empty() && unchecked.is_empty(),
            "{path}: checksum list out of step with the bindings — in bindings only: {unlisted:?}; in this list only: {unchecked:?}");
        let mismatched: Vec<String> = bindings.iter()
            .filter(|(k, v)| library[*k] != **v)
            .map(|(k, v)| format!("{k}: bindings {v}, library {}", library[k]))
            .collect();
        assert!(mismatched.is_empty(), "{path}: the apps would refuse to start:\n{}", mismatched.join("\n"));
    }
}
