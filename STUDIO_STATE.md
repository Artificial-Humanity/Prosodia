# Studio State: Active Execution Tracking

## Current Project Phase

* **Phase Objective:** Consolidating legacy, fragmented Swift multi-repo repositories into a unified Cargo Workspace Monorepo infrastructure.
* **Active Focus:** Migrating core token manipulation loops, tokenizer arrays, and structural orchestrators over to high-performance safe Rust primitives.
* **Migration status (2026-06-14):** Legacy Swift-side G2P engines (`Misaki`, `ActorEspeak`) and tokenizer (`NativeBPETokenizer`) have been fully pruned. Lexicon JSON assets were relocated into the Rust workspace (`crates/actor/resources`) and path resolutions in Rust core updated. Cleaned up `Package.swift` and `project.pbxproj` targets to remove all references to legacy G2P targets, completely removing the GPL-licensed `espeak-ng` package dependency. Both `swift build` and downstream `ProsodiaTuner` Xcode scheme compile successfully with **BUILD SUCCEEDED**. Synthesis orchestration loops, parameter smoothing (EMA style), sentence segmentation, narration grouping, and boundary transition mitigations (gated pauses) have been fully ported to Rust. Phase 2 (LiteRT-LM Gemma Director integration and StyleTTS2 flatbuffer/TFLite inference) has been fully ported to Rust and verified. Phase 3 & 4 (Apple/Android Platforms scaffolding, NDK compilation, dynamic framework packaging, and SwiftUI/Compose applications integration) are fully completed. The monorepo migration is 100% finished. Additionally, the next-generation dynamic parametric voicing grid (continuous bilinear LERP of age/masculinity timbre anchors and style texture blending for vocal hoarseness/raspiness) has been fully implemented in the Rust voice loader and stage coordinator. Furthermore, the Voice Loader cache policy was upgraded from bounded FIFO to an Access-Ordered LRU Eviction Cache Policy. Additionally, a bounded asynchronous lookahead pre-rendering buffer (with configurable lookahead limits and standard backpressure pacing) was implemented in the Stage Coordinator, mitigating audio stuttering gaps during offline audiobook performance.

---

## Monorepo Migration Roadmap

### 🟩 1. Workspace Foundation (Completed)

- [x] Merge independent `Director`, `Actor`, `Stage`, and `FolioParser` files into a singular codebase.
- [x] Configure the root workspace-level `Cargo.toml`.
- [x] Establish the clean `/crates` layout separation sitting under standard local path constraints.
- [x] Consolidate Apple platforms Swift Package under `platforms/apple` with separate modular targets.
- [x] Migrate tuner app under `apps/tuner` and tuner extension under `apps/tuner-extension`.

### 🟩 2. Core Neural Engine Refactor (Completed)

- [x] Implement zero-dependency BPE vocab indices inside `crates/core`.
- [x] Scaffold out the LiteRT-LM execution harness inside `crates/director` mapping context frames for Gemma 4. *(Fully implemented in Rust using the C API FFI, lazy model initialization, rolling narrative context, and fully verified)*
- [x] Migrate the core localized acoustic matrix pipeline into `crates/actor` targeting LiteRT abstractions. *(phoneme/token chunking + `voice_loader` safetensors/blending + `ProsodiaSpeech` G2P engine + phoneme tokenization + synthesis loop orchestration + EMA style smoothing + parameter interpolation done)*
- [x] Wire up synchronous stream orchestration routines inside `crates/stage`. *(pull-based `StageCoordinator` done)*
- [x] Implement rule-based sentence segmentation, narration grouping, and boundary pauses inside `crates/stage` (replacing legacy `NLTokenizer` and moving all passage splitting/grouping to Rust).
- [x] Implement LiteRT StyleTTS2 execution loop directly in Rust using the TFLite C-API FFI, and map Swift engines to delegate to the Rust core.

### 🟦 3. Adapter Bridge & Platform Hooks (Completed)

- [x] Write the declarative UniFFI macro layers inside Rust crates (GemmaDirector, VocalActor, StageCoordinator, FolioParser).
- [x] Setup the native local `apple/` Swift Package mapping PCM pointers to `AVAudioEngine`.
- [x] Configure the NDK Gradle scaffolding inside `android/` tracking native loops.

### 🟦 4. Product Applications Integration (Completed)

- [x] Pull the production SwiftUI `apple-reader` directory into `apps/apple-reader`.
- [x] Scaffold the Jetpack Compose `android-reader` application under `apps/android-reader` and integrate it with the local Android platform module.

---

## Active Workspace Blockers & System Warnings

* **Footgun:** LiteRT-LM is a Git-LFS source package with a missing upstream LFS object — SPM resolve/build needs `GIT_LFS_SKIP_SMUDGE=1` (the macOS xcframework comes from GitHub Releases, not LFS).
* **Warning:** Ensure the underlying model definitions in `crates/actor` decouple StyleTTS2 references cleanly to pave the way for seamless insertion of our proprietary hometown core speech synthesis variants.
* **Task:** Verify target cross-compilation environment linkages before invoking massive `/goal` automated code synthesis execution tracks.