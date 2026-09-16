# AGENTS — Project Prosodia

This is the entry point for any agent or developer working on Project Prosodia (on-device
speech & logic). This is an independent GitHub repo. Internal engineering notes —
current state, open decisions — live in [notes/](notes/). Before starting work, read
[notes/STATE.md](notes/STATE.md) for the current state of the project and the most immediate
must-do items.

---

## Core Stack Matrix

* **Language Ecosystem:** Safe, performance-first Rust (Cargo Multi-Crate Workspace).
* **Text & Director Logic Framework:** Google LiteRT-LM framework core.
* **Director Neural Layer:** On-device Gemma 4 variants (instruct-tuned weights).
* **Audio & Acoustic Matrix Framework:** Google LiteRT runtime wrappers.
* **Actor Neural Voice Engine:** On-device neural speech synthesis (StyleTTS2 today; Matcha-TTS in progress) running via LiteRT/TFLite, orchestrated by the Prosodia Rust core.

---

## Global Repository Layout

For the comprehensive layout, directory structures, and file mappings of this repo, refer to
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Agents and developers should consult that file
as the single source of truth for repo topology.

### Integration Dependencies

* **`bindings/ffi`** generates target `.swift`, `.kt`, and `.cs` wrapper structures safely.
* **`apps/tuner`** consumes `.package(path: "../../platforms/apple")` via local relative filesystem declaration.
* **`apps/tuner-extension`** provides Chrome Manifest V3 companion controls.
* **`apps/apple-reader`** (SwiftUI) and **`apps/android-reader`** (Jetpack Compose) are the reader apps wired to the `StageCoordinator` pipeline.
* Model artifacts (the Sonora actor model consumed via `apps/tuner`) are drawn from the shared
  `/data/models` archive maintained on the `ai-lab-0` machine — see the `AI-Lab-AMD` repo's
  `AGENTS.md` for that archive's promotion/read-only policy.

---

## File Naming Conventions

Names must be predictable so links resolve on case-sensitive systems (Linux/CI) as well as
case-insensitive macOS/Windows.

* **Canonical root marker files → `UPPERCASE`** (`SCREAMING_SNAKE_CASE` if multi-word): `README.md`, `LICENSE`, `CONTRIBUTING.md`, `ROADMAP.md`, `AGENTS.md`. Keep this set small and curated.
* **Top-level anchor docs → `UPPERCASE`, single word preferred:** `ARCHITECTURE.md`, `STATE.md`.
* **All other docs & notes → `lowercase-kebab-case.md`:** e.g. `open-decisions.md`, `code-review-findings.md`. This is the rule for everything in `notes/`.
* **Source code → the language's own convention:** Rust `snake_case.rs`, Swift `PascalCase.swift`, Kotlin `PascalCase.kt`.
* **Never** let case be the only difference between two paths, and always reference files with their exact case.

---

## System Operational Mandates

### 1. SOLID Boundary Enforcement

* Maintain strict functional boundaries between directories. Core crates inside `crates/` are completely memory-isolated, multi-thread scheduled, and platform-agnostic.
* The neural logic crates have zero awareness of peripheral speakers, audio hardware threads, or target operating system windows.

### 2. The Input/Output Data Interface Contract

* The processing pipeline must terminate explicitly by returning a raw pointer referencing a standard linear float matrix (`[f32]`) representing pure PCM audio data.
* Every audio matrix payload must match its declared mono target sample rate configuration (e.g., `24000Hz` or `44100Hz`).
* Platform modules inside `platforms/` are strictly responsible for grabbing these raw memory arrays via the FFI boundary and feeding them into hardware device pipelines.

### 3. Commit Hygiene

* When executing code transformations or refactoring schemas, changes extending definitions, token structures, or data definitions must map symmetrically across the Rust core, the UniFFI bridge definitions, the platform frameworks, and the downstream application UI layers within a singular, atomic commit block.
* **How work gets done — [WORKFLOW.md](WORKFLOW.md).** Branching, review, and landing on
  `main` are there; read it before your first commit. **This supersedes the PR-only rule and
  the `claude-review.yml`/`claude-fix.yml` lane that used to live in this section** (owner,
  2026-09-16) — that lane was already stood down here (#6, 2026-08-10), and the review cycle
  WORKFLOW.md now describes replaces it rather than sitting alongside it.


### 4. Change history

* **The commit message is the record. Git history is the archive.** There is no changelog
  file; `notes/CHANGELOG.md` was deleted 2026-08-17, following Sonora, which retired the same
  convention on 2026-08-11.
* ⚠ **Why it went, rather than "it was tedious".** A changelog is a second, hand-maintained
  copy of what git already knows, and a copy that nothing compares against the original drifts
  by construction — every entry was a claim about a commit that no check could falsify. The
  same reasoning retired the timestamped review documents in §5.
* So write the commit message as the entry: what changed, why, and what was measured. That is
  the artifact a reader will actually have.

### 5. Code Review Execution Standards

* **Scope: code work only** — source, build config, and dependency manifests
  (`crates/`, `bindings/`, `platforms/`, `apps/`, `Cargo.toml`/`Cargo.lock`, build scripts).
  Docs-only commits are out of scope and need no review.
* **A review is a report, not a fix pass.** The reviewing agent takes on fixes only when the
  owner explicitly asks it to, never as a rider on the review itself.
* **Findings live on the PR, not in a file.** The review runs through
  `.github/workflows/claude-review.yml` and is closed with the `claude-fix` label (§3).
  ⚠ **That lane is STOOD DOWN in this repo** (#6, 2026-08-10) — piloted in Sonora only, and
  runnable here by `workflow_dispatch` from the Actions tab. So a review happens when someone
  asks for one. Retiring the review *documents* does not depend on the lane: the argument
  against them is that they were three unchecked restatements of one fact, which holds
  whether the bots run or not.
* ⚠ **The timestamped `notes/code-review-*.md` documents are RETIRED** (2026-08-17), and the
  last one was deleted with this change. The format required each review to delete its
  predecessor and repoint a `notes/STATE.md` pointer at itself — three hand-maintained
  statements of one fact, none of them checked. A finding that its cycle could not settle
  belongs in an issue, where it stays open until somebody closes it.

---

## Agentic Personas & Coding Guidelines

* **Target Core Architecture:** Leverage idioms favoring clean Rust composition patterns, explicit memory-isolated traits, zero-copy pointer manipulation passes across FFI seams, and clear performance profiling.
* **Code Assistance Rules:** Never inject strict cloud API client configurations into local targets. All pipelines run locally and on-device via LiteRT runtimes.
* **Tooling Optimization:** Rely on localized context mapping loops to cross-evaluate changes between downstream SwiftUI/Kotlin files and underlying Rust layout contracts.
* **Documentation & Code-Completion Comments:** Ensure all code is written with code-completion comments where applicable. This includes method signature documentation describing the purpose, parameters, and return values to keep libraries easy to use for future developers.
