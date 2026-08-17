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
* **`main` is PR-only. Do not push to it directly** (owner, 2026-08-10). Branch, push the
  branch, open a PR, and let it merge. This applies to agent sessions exactly as it applies to
  the owner — an agent that "just needs one small fix on `main`" is the case the rule exists
  for. Two reasons it is a rule and not a preference:
  * **The Mac and `ai-lab-0` (and their agent sessions) commit concurrently.** Direct pushes to
    a shared `main` are how two sessions silently interleave half-finished work; a branch is a
    place for work to be incomplete without being everyone's problem.
  * **Nothing reviews a direct push.** `.github/workflows/claude-review.yml` triggered on
    `pull_request`, so work that skips the PR skips the review entirely — the automation
    cannot see a commit that was never proposed. ⚠ Since the lane was **stood down here**
    (#6, 2026-08-10) the automatic trigger is gone and nothing reviews a PR either unless
    someone dispatches it by hand; the reason to open one is unchanged.
* **Branch naming**: `<type>/<short-slug>` matching the commit type — `fix/`, `feat/`,
  `docs/`, `chore/`.
* **Work on the branch, commit and push liberally, open the PR only when the work is done**
  (owner, 2026-08-10). Pushing to your own branch is free and is the entire point of having
  one: commit early, commit often, push whenever, and let the branch hold work that is not
  yet finished. What is deliberate is the *timing of the PR*, not the timing of the commits.
  * **When completion is defined, completion opens the PR.** If a `/goal` has been set,
    achieving that goal IS the completion point — open the PR then, without being asked again.
  * **Otherwise the owner calls it.** With no goal set, work, push, and wait: the owner
    acknowledges the completion point and the PR follows from that.
  * **This is also what made it cheap.** `.github/workflows/claude-review.yml` fired when a
    PR was opened AND on every push to an open one, so a PR opened at the *start* of the work
    billed a full model-rate review of half-finished code on every intermediate push. Opening
    at completion buys exactly one review, of work that is actually ready to be read.
    ⚠ **Past tense since #6 (2026-08-10):** the lane is stood down in this repo and piloted in
    Sonora only. The triggers are commented out verbatim, leaving `workflow_dispatch`, so
    re-arming is a copy-paste and the billing argument above returns with it.
* **Pull before push, every time.** Run `git pull --rebase` as the first step of any
  commit-and-push sequence on your branch, and rebase on `main` before opening the PR. If the
  tree holds the owner's uncommitted local edits, fetch and check ahead/behind instead of
  forcing a rebase.
* **The exception is the owner's, not yours.** If the owner explicitly directs a direct push to
  `main`, that is their call and does not need re-litigating — state the rule once, then do as
  asked. An agent never grants itself the exception.
* ⚠ **A rule in this file is not an enforcement mechanism.** The authority is the branch
  protection on `main`; this section only explains it. If a direct push to `main` ever
  *succeeds*, the protection is missing or was bypassed — report that rather than treating it
  as permission.* **Review feedback is closed with the `claude-fix` label, not by hand-waving** — when the
  lane is armed. ⚠ It is **stood down in this repo** since #6; the mechanism below describes
  what a manual dispatch still does, and what re-arming restores. The review
  workflow only comments; `.github/workflows/claude-fix.yml` is what acts on those comments.
  Add the `claude-fix` label to the PR and the fix agent reads the inline comments, commits
  the fixes, replies, and removes the label. It is label-gated deliberately: firing it
  automatically on every submitted review oscillates (fix pushes → `synchronize` → new review
  → fix pushes), and the vendor ships no loop guard. One label, one pass; re-label to run it
  again. A review comment is an argument, not an order — the fix agent is expected to push
  back in a reply where a finding is wrong, rather than making a change it believes is wrong.


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
