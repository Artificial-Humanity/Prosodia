use std::env;
use std::path::PathBuf;

fn main() {
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
        // Allow undefined symbols (e.g. LiteRtLm* / TfLite*) to be resolved at runtime
        println!("cargo:rustc-link-arg=-Wl,-z,undefs");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
