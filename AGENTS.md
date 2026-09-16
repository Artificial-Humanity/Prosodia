# AGENTS — Project Prosodia

Prosodia is an on-device director–actor audio application with a Rust core and
native platform clients. Before starting work, read [PERSONA.md](PERSONA.md),
[WORKFLOW.md](WORKFLOW.md), [notes/STATE.md](notes/STATE.md) and
[notes/todo.md](notes/todo.md).

## Core Stack Matrix

* **Core:** safe, performance-first Rust in a multi-crate Cargo workspace.
* **Director:** Google LiteRT-LM with on-device, instruct-tuned Gemma 4 variants.
* **Actor:** neural speech synthesis through LiteRT/TFLite, orchestrated by Rust.
  Read `notes/STATE.md` for model integration status.

## Repository Layout and Dependencies

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the source of truth for repo topology.

* `bindings/ffi` generates Swift, Kotlin and C# wrappers.
* `apps/tuner` consumes the Apple package through `.package(path: "../../platforms/apple")`.
* `apps/tuner-extension` provides Chrome Manifest V3 companion controls.
* `apps/apple-reader` (SwiftUI) and `apps/android-reader` (Jetpack Compose) connect
  to the `StageCoordinator` pipeline.
* Sonora actor artifacts consumed by the Tuner come from `/data/models` on `ai-lab-0`.
  Follow the `AI-Lab-AMD` repo's `AGENTS.md` for archive promotion and read-only policy.

## File Naming Conventions

* Keep canonical root markers uppercase: `README.md`, `LICENSE`, `CONTRIBUTING.md`,
  `ROADMAP.md`, `AGENTS.md`. Use `SCREAMING_SNAKE_CASE` for multi-word markers
  and keep the set small.
* Use uppercase for top-level anchor docs, preferably one word:
  `ARCHITECTURE.md`, `STATE.md`.
* Use `lowercase-kebab-case.md` for other docs and notes.
* Follow each language's source naming convention: Rust `snake_case.rs`, Swift
  `PascalCase.swift`, Kotlin `PascalCase.kt`.
* Never distinguish paths by case alone. Use exact case in references.

## System Operational Mandates

### 1. SOLID Boundary Enforcement

* Keep strict functional boundaries between directories. Core crates in `crates/`
  must remain memory-isolated, multithread-scheduled and platform-agnostic.
* Keep speakers, audio hardware threads and OS windows out of neural logic crates.
* Run pipelines locally and on-device through LiteRT. Do not introduce cloud API
  client configurations into local targets.
* Favor clean Rust composition, explicit memory-isolated traits, zero-copy FFI
  pointer passes and performance profiling.

### 2. The Input/Output Data Interface Contract

* Terminate the processing pipeline by returning a raw pointer to a linear `[f32]`
  buffer containing PCM audio.
* Match each audio payload to its declared mono target sample rate.
* Keep hardware playback in `platforms/`: platform modules receive audio buffers
  through FFI and feed the device pipelines.

### 3. Commit Hygiene

* Keep changes to definitions, tokens and data schemas aligned across the Rust core,
  UniFFI bridge, platform frameworks and application UI in one atomic commit.
  Cross-check downstream SwiftUI/Kotlin code against Rust contracts.
* Follow [WORKFLOW.md](WORKFLOW.md) for branching, review, synchronization and landing.

### 4. Change history

Use commit messages as the record: state what changed, why and what was measured.
Git history is the archive; do not maintain a separate changelog.

### 5. Code Review Execution Standards

Follow [WORKFLOW.md](WORKFLOW.md) for review scope and conduct. Keep findings in the
review, not in timestamped `notes/code-review-*.md` files. File unresolved findings
as issues.

## Coding Guidelines

Match the surrounding code's naming, idiom and comment density. Document method
purpose, parameters and return values where needed for useful code completion and
library use.
