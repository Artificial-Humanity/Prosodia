# Studio State: Active Execution Tracking

## Current Project Phase

* **Phase Objective:** Consolidating legacy, fragmented Swift multi-repo repositories into a unified Cargo Workspace Monorepo infrastructure.
* **Active Focus:** Migrating core token manipulation loops, tokenizer arrays, and structural orchestrators over to high-performance safe Rust primitives.

---

## Monorepo Migration Roadmap

### 🟩 1. Workspace Foundation (Completed)

- [x] Merge independent `Director`, `Actor`, `Stage`, and `FolioParser` files into a singular codebase.
- [x] Configure the root workspace-level `Cargo.toml`.
- [x] Establish the clean `/crates` layout separation sitting under standard local path constraints.
- [x] Consolidate Apple platforms Swift Package under `platforms/apple` with separate modular targets.
- [x] Migrate tuner app under `apps/tuner` and tuner extension under `apps/tuner-extension`.

### 🟨 2. Core Neural Engine Refactor (Active Sprints)

- [ ] Implement zero-dependency BPE vocab indices inside `crates/core`.
- [ ] Scaffold out the LiteRT-LM execution harness inside `crates/director` mapping context frames for Gemma 4.
- [ ] Migrate the core localized acoustic matrix pipeline into `crates/actor` targeting LiteRT abstractions.
- [ ] Wire up synchronous stream orchestration routines inside `crates/stage`.

### 🟦 3. Adapter Bridge & Platform Hooks (Penciled)

- [ ] Write the declarative UniFFI macro layers inside `bindings/ffi`.
- [ ] Setup the native local `apple/` Swift Package mapping PCM pointers to `AVAudioEngine`.
- [ ] Configure the NDK Gradle scaffolding inside `android/` tracking native `Oboe` loops.

### 🟦 4. Product Applications Integration (Penciled)

- [ ] Pull the production SwiftUI `apple-reader` directory into `apps/apple-reader`.

---

## Active Workspace Blockers & System Warnings

* **Warning:** Ensure the underlying model definitions in `crates/actor` decouple StyleTTS2 references cleanly to pave the way for seamless insertion of our proprietary hometown core speech synthesis variants.
* **Task:** Verify target cross-compilation environment linkages before invoking massive `/goal` automated code synthesis execution tracks.