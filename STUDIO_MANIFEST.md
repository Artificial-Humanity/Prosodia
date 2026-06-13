# Studio Manifest: Project Prosodia

## Core Stack Matrix

* **Language Ecosystem:** Safe, performance-first Rust (Cargo Multi-Crate Workspace).
* **Text & Director Logic Framework:** Google LiteRT-LM framework core.
* **Director Neural Layer:** On-device Gemma 4 variants (instruct-tuned weights).
* **Audio & Acoustic Matrix Framework:** Google LiteRT runtime wrappers.
* **Actor Neural Voice Engine:** Proprietary localized Prosodia speech synthesis architecture.

---

## Global Repository Layout

For the comprehensive layout, directory structures, and file mappings of the monorepo, please refer directly to [PROJECT_TOPOLOGY.md](PROJECT_TOPOLOGY.md). Agents and developers should consult that file as the single source of truth for repository topology.

---

## Integration Dependencies

* **`bindings/ffi`** generates target `.swift`, `.kt`, and `.cs` wrapper structures safely.
* **`apps/tuner`** consumes `.package(path: "../../platforms/apple")` via local relative filesystem declaration.
* **`apps/tuner-extension`** provides Chrome Manifest V3 companion controls.