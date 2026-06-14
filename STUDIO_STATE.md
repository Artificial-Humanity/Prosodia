# Studio State: Active Execution Tracking

## Current Project Phase

* **Phase Objective:** Consolidating legacy, fragmented Swift multi-repo repositories into a unified Cargo Workspace Monorepo infrastructure.
* **Active Focus:** Migrating core token manipulation loops, tokenizer arrays, and structural orchestrators over to high-performance safe Rust primitives.
* **Migration status (2026-06-13):** Architecture refactor **Stages 1–4 core complete** (custom ZIP reader; phoneme/token chunking; safetensors + style blending; pull-based `StageCoordinator`). Full macOS `swift build` passes against regenerated UniFFI bindings. **Next step (the G2P engine) is gated on a decision** — see `Documentation/Notes/open_decisions.md`. Detailed plan + leftovers: `Documentation/Notes/{architecture_refactoring_plan,unported_logic}.md`.

---

## Monorepo Migration Roadmap

### 🟩 1. Workspace Foundation (Completed)

- [x] Merge independent `Director`, `Actor`, `Stage`, and `FolioParser` files into a singular codebase.
- [x] Configure the root workspace-level `Cargo.toml`.
- [x] Establish the clean `/crates` layout separation sitting under standard local path constraints.
- [x] Consolidate Apple platforms Swift Package under `platforms/apple` with separate modular targets.
- [x] Migrate tuner app under `apps/tuner` and tuner extension under `apps/tuner-extension`.

### 🟨 2. Core Neural Engine Refactor (Active Sprints)

- [ ] Implement zero-dependency BPE vocab indices inside `crates/core`. *(crate still empty)*
- [~] Scaffold out the LiteRT-LM execution harness inside `crates/director` mapping context frames for Gemma 4. *(`GemmaDirector` scaffold + prompts exist; `tag_passage` is still a mock — real inference is the Swift `LiteRtLmDirector`)*
- [~] Migrate the core localized acoustic matrix pipeline into `crates/actor` targeting LiteRT abstractions. *(phoneme/token chunking + `voice_loader` safetensors/blending done; **G2P engine** and the synthesis orchestration loops remain — see open decision)*
- [x] Wire up synchronous stream orchestration routines inside `crates/stage`. *(pull-based `StageCoordinator`, Stage 4)*

### 🟦 3. Adapter Bridge & Platform Hooks (Penciled)

- [ ] Write the declarative UniFFI macro layers inside `bindings/ffi`.
- [ ] Setup the native local `apple/` Swift Package mapping PCM pointers to `AVAudioEngine`.
- [ ] Configure the NDK Gradle scaffolding inside `android/` tracking native `Oboe` loops.

### 🟦 4. Product Applications Integration (Penciled)

- [ ] Pull the production SwiftUI `apple-reader` directory into `apps/apple-reader`.

---

## Active Workspace Blockers & System Warnings

* **⏳ Decision pending (gates next step):** how to bring the **G2P engine** into Rust — port Misaki in-core vs an isolated opt-in espeak-ng crate vs keep it in Swift. Driven by the **GPLv3** boundary (espeak-ng must not contaminate the permissive core). See `Documentation/Notes/open_decisions.md` §1.
* **Footgun:** LiteRT-LM is a Git-LFS source package with a missing upstream LFS object — SPM resolve/build needs `GIT_LFS_SKIP_SMUDGE=1` (the macOS xcframework comes from GitHub Releases, not LFS).
* **Warning:** Ensure the underlying model definitions in `crates/actor` decouple StyleTTS2 references cleanly to pave the way for seamless insertion of our proprietary hometown core speech synthesis variants.
* **Task:** Verify target cross-compilation environment linkages before invoking massive `/goal` automated code synthesis execution tracks.