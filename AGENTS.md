# AGENTS — Project Prosodia

This is the single entry point for any agent or developer working in this repository.
It consolidates the project manifest (stack and layout) with the behavioral rules
agents must follow. Before starting work, read [Notes/STATE.md](Notes/STATE.md)
for the current state of the project and the most immediate must-do items.

---

## Where Things Live

| Kind | Location |
|---|---|
| Behavioral rules + manifest (this file) | `AGENTS.md` (repo root) |
| Public canon docs (contribution guide, repo topology) | `Docs/` |
| Internal / in-transit notes — engineering changelog, current state, open decisions, scratch | `Notes/` — **private submodule** (`git submodule update --init`) |
| Tool skills & slash-commands | `.claude/` (Claude Code) |

`Notes/` is a private GitHub submodule: internal docs stay out of the public repo while
remaining available to the team. Pointers into `Notes/…` below require submodule access.

---

## File Naming Conventions

Names must be predictable so links resolve on case-sensitive systems (Linux/CI) as well as
case-insensitive macOS/Windows.

* **Canonical root marker files → `UPPERCASE`** (`SCREAMING_SNAKE_CASE` if multi-word): `README.md`, `LICENSE`, `CONTRIBUTING.md`, `CHANGELOG.md`, `ROADMAP.md`, `AGENTS.md`. Keep this set small and curated.
* **Top-level anchor docs → `UPPERCASE`, single word preferred:** `ARCHITECTURE.md`, `STATE.md`.
* **All other docs & notes → `lowercase-kebab-case.md`:** e.g. `open-decisions.md`, `code-review-findings.md`. This is the rule for everything in `Notes/`.
* **Source code → the language's own convention:** Rust `snake_case.rs`, Swift `PascalCase.swift`, Kotlin `PascalCase.kt`.
* **Never** let case be the only difference between two paths, and always reference files with their exact case.

---

## Core Stack Matrix

* **Language Ecosystem:** Safe, performance-first Rust (Cargo Multi-Crate Workspace).
* **Text & Director Logic Framework:** Google LiteRT-LM framework core.
* **Director Neural Layer:** On-device Gemma 4 variants (instruct-tuned weights).
* **Audio & Acoustic Matrix Framework:** Google LiteRT runtime wrappers.
* **Actor Neural Voice Engine:** Proprietary localized Prosodia speech synthesis architecture.

---

## Global Repository Layout

For the comprehensive layout, directory structures, and file mappings of the monorepo,
refer to [Docs/ARCHITECTURE.md](Docs/ARCHITECTURE.md). Agents and developers should
consult that file as the single source of truth for repository topology.

### Integration Dependencies

* **`bindings/ffi`** generates target `.swift`, `.kt`, and `.cs` wrapper structures safely.
* **`apps/tuner`** consumes `.package(path: "../../platforms/apple")` via local relative filesystem declaration.
* **`apps/tuner-extension`** provides Chrome Manifest V3 companion controls.

---

## System Operational Mandates

### 1. SOLID Boundary Enforcement

* Maintain strict functional boundaries between directories. Core crates inside `/crates` are completely memory-isolated, multi-thread scheduled, and platform-agnostic.
* The neural logic crates have zero awareness of peripheral speakers, audio hardware threads, or target operating system windows.

### 2. The Input/Output Data Interface Contract

* The processing pipeline must terminate explicitly by returning a raw pointer referencing a standard linear float matrix (`[f32]`) representing pure PCM audio data.
* Every audio matrix payload must match its declared mono target sample rate configuration (e.g., `24000Hz` or `44100Hz`).
* Platform modules inside `/platforms` are strictly responsible for grabbing these raw memory arrays via the FFI boundary and feeding them into hardware device pipelines.

### 3. Cross-Stack Atomic Commits Requirement

* When executing code transformations or refactoring schemas, changes extending definitions, token structures, or data definitions must map symmetrically across the Rust core, the UniFFI bridge definitions, the platform frameworks, and the downstream application UI layers within a singular, atomic commit block.

### 4. Changelog Maintenance Requirement

* The project changelog lives at [Notes/CHANGELOG.md](Notes/CHANGELOG.md) (private submodule — the internal engineering log, kept out of the public repo). Always append a detailed chronological entry describing all technical modifications, refactoring milestones, and build-system changes made during the session **before concluding your work**.
* **The changelog is append-only across a release cycle.** Do not prune, rewrite, or remove historical entries. Entries are pruned/rolled over **only** when we tag and release a new version of the overall project — at which point the released entries are collected under that version's heading and the working section is reset for the next cycle.
* New entries go at the top under the current date, following the existing `Added` / `Changed` / `Fixed` / `Removed` structure.

---

## Agentic Personas & Coding Guidelines

* **Target Core Architecture:** Leverage idioms favoring clean Rust composition patterns, explicit memory-isolated traits, zero-copy pointer manipulation passes across FFI seams, and clear performance profiling.
* **Code Assistance Rules:** Never inject strict cloud API client configurations into local targets. All pipelines run locally and on-device via LiteRT runtimes.
* **Tooling Optimization:** Rely on localized context mapping loops to cross-evaluate changes between downstream SwiftUI/Kotlin files and underlying Rust layout contracts.
