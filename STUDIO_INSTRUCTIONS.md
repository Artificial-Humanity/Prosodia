# Studio Instructions: Agent Behavioral Rules

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

### 4. Changelog Updates Requirement

* Always append a detailed chronological entry describing all technical modifications, refactoring milestones, and build-system changes made during the session to [Documentation/Notes/changelog.md](file:///Users/lmcfarlin/Projects/Artificial-Humanity/Prosodia/Documentation/Notes/changelog.md) before concluding your work.

---

## Agentic Personas & Coding Guidelines

* **Target Core Architecture:** Leverage idioms favoring clean Rust composition patterns, explicit memory-isolated traits, zero-copy pointer manipulation passes across FFI seams, and clear performance profiling.
* **Code Assistance Rules:** Never inject strict cloud API client configurations into local targets. All pipelines run locally and on-device via LiteRT runtimes.
* **Tooling Optimization:** Rely on localized context mapping loops to cross-evaluate changes between downstream SwiftUI/Kotlin files and underlying Rust layout contracts.