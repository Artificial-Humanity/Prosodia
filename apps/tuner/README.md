# ProsodiaTuner 🎛️🎭

Welcome to the **Rehearsal Studio**! 

`ProsodiaTuner` is the auditioning sandbox, mixing board, and parameter tuner for **Project Prosodia**. This is where we call our **Director** (LLM) and **Actor** (TTS) onto the stage, adjust Valence-Arousal-Tension (VAD) sliders, A/B test models, and tweak our acoustic matrix to ensure the show is spectacular.

> [!NOTE]
> The former production app target was removed to leave a clean slate for later. The one remaining target and scheme, `ProsodiaTuner`, is the parameter-tuning harness and testing workbench.

---

## 🛠️ Rehearsal Workspace

The project contains the following components:

- `ProsodiaTuner` app: The tuning tool and auditioning environment.
- `ProsodiaTuner.xcodeproj`: The Xcode configuration project (no test target; the engine's tests are the Rust crates' `cargo test`).

The app links the consolidated `platforms/apple` Swift package (`../../platforms/apple`), which exposes the `Stage` (Stage Manager), `Actor`, and `Director` engine modules.

---

## 🔨 Building

The harness sits on top of prebuilt Rust FFI xcframeworks, so building is a two-system chain. The one-shot path:

```bash
./build.sh          # rebuilds the Rust xcframeworks, then the app (extra args pass through to xcodebuild)
```

which is equivalent to `../../build_frameworks.sh` followed by:

```bash
xcodebuild -project ProsodiaTuner.xcodeproj -scheme ProsodiaTuner \
  -destination "platform=macOS,arch=arm64" build
```

Notes:

- **Scheme is `ProsodiaTuner`; destinations must be arm64** — the FFI xcframeworks carry no x86_64 slice.
- A **"Check FFI Framework Freshness"** build phase fails any build (including Xcode GUI Run) whose xcframeworks are older than the Rust sources under `crates/`, naming the newer file — rerun `build.sh` or `../../build_frameworks.sh` when it fires. It only checks; it never builds Rust itself.
- Do **not** use legacy `xcodebuild -target` builds: the LiteRT-LM package checkout has a Bazel `BUILD` file at its root, which collides with the `build/` directory that target-style builds create on case-insensitive filesystems.

---

## 💻 Local Models for the Harness

For real speech in the harness on macOS, models are resolved via `prosodia_models.json` (`modelsBase: ../models`). The shared library lives at `/data/models` on ai-lab-0 (`/Volumes/data/models` from the Mac over NFS; renamed from `/data/reference/models` in the 2026-07-23 /data reorganization — the old workspace-root `Reference/models` symlink was removed with the umbrella repo):

```text
/data/models/                         # Prosodia's entries only — other folders here belong to other projects (org/repo layout)
├── config.json                       # Actor vocab (locked 178 symbols) + native sample rate — stays at root (engine reads it next to the model)
├── sonora.tflite                     # Active Actor model — Sonora baseline-ljspeech-22k float32 e2e (fidelity-fixed 2026-07-12; renamed from styletts2_lite.tflite 2026-07-13 — it is a Matcha-architecture model, not StyleTTS2; registry artifact renamed from v1-ljspeech 2026-07-22) — stays at root
├── Google/
│   ├── gemma-4-E2B-it.litertlm       # Gemma 4 E2B LiteRT-LM (Default Director model)
│   └── gemma-4-E4B-it.litertlm       # Gemma 4 E4B LiteRT-LM
├── litert-community/
│   └── Matcha-TTS/                   # HF clone — split-graph fp16 TFLite + espeak-free G2P assets
└── shivammehta25/
    └── Matcha-TTS/                   # Clean upstream clone (reference). The old spike workspace was rescued + pruned 2026-07-13 (history: github.com/Artificial-Humanity/StyleTTS2FineTune; ONNX: Prosodia-Storage bucket archive/)
```

The Sonora HF registry (huggingface.co/artificial-humanity/Sonora — our checkpoints + TFLite exports, `baseline-ljspeech-22k/` incl. `litert-split/`) is **not** under `/data/models`: it is a working artifact registry, not a reference model. It's checked out at `Sonora/huggingface/` (superseding the `Registry/Sonora/` gitignored-clone layout from the retired umbrella-workspace era).

> [!TIP]
> **Plan A multi-graph runtime (2026-07-13):** the engine also accepts a split-model **directory**
> (textenc/decoder/vocoder graphs + `emb.bin` + `config.json`): host-side Euler ODE, real per-token
> durations from `logw` (the `DS:` contract channel is live), no 50-token limit (256), fp16 graphs.
> The **Actor role** picker at the top of the harness chooses what Speak renders with: `actor` (the
> e2e monolith), `actor-split` (`baseline-ljspeech-22k`, single-speaker, 22.05 kHz) or
> `actor-split-24k` (`derisk-energy-24k`, 247 speakers, 24 kHz). On `actor-split-24k` a **Speaker**
> picker lists the LibriTTS-R readers (default "LibriTTS-R 229", row 22), and only **Energy** — the
> one VAT channel that model trained — is live: Valence and Tension are held at 0, the value sent. A
> split role without a `conditioning` block (`actor-split`) holds all three at 0. **Volume** is
> loudness (a mel-domain gain on split roles, a PCM gain on the monolith) and is bounded to
> 0.252–1.995 (−12…+6 dB) on split roles. **Energy** changes the voice itself only on a split role
> that trains it (`actor-split-24k`); on the `actor` monolith, which has no `vat` input, Energy does
> not reach the model and moves only the `AcousticMatrix`-derived speed and volume. Feedback logs
> and Copy Config record the actor role and the speaker.

> [!IMPORTANT]
> **Model pins.** `prosodia_models.json` records the registry checkout's git revision
> (`registry.revision`) and a sha256 for every file the `actor`, `actor-split` and `actor-split-24k`
> roles load. The Rust tests in `crates/actor/src/model_pins.rs` check both, and a changed model
> file fails them.
> To move a role to another artifact, check with Sonora's resident first, then update the path,
> the revision and the hashes together. A role whose directory is absent is skipped; run with
> `PROSODIA_REQUIRE_PINNED_MODELS=1` to make that a failure too. The `actor` files at the
> `/data/models` root are copies from outside the registry: `sonora.tflite` is
> `baseline-ljspeech-22k/checkpoint_epoch=199_e2e_float32.tflite` and `config.json` is
> `baseline-ljspeech-22k/config.json`, so the registry revision does not cover them; only their
> hashes do.
> The `conditioning` block of `actor-split-24k` (trained VAT channels, default speaker, speaker
> labels) is not covered by these pins. The test
> `controls::tests::speaker_labels_match_their_sonora_source` checks the labels against Sonora's
> `speakers.json` (path and sha256 in the block) when that file exists;
> `PROSODIA_REQUIRE_SONORA_SOURCES=1` turns its skip into a failure.

> [!NOTE]
> **Model paths resolve through `prosodia_models.json`** (repo root — role-based config, commit
> `2425594`, desktop build-checked 2026-07-13): the apps look up `actor`, `voices`, and `director-*`
> roles instead of hard-coding filenames, so the `Google/` Gemma location is handled by config.
> `ProsodiaModels.swift` still carries a built-in fallback with model paths for when the config file
> is not found. `config.json` and `sonora.tflite` remain at the `/data/models` root because the Rust
> engine reads the config adjacent to the model file.

Without the required model files present, the harness can still compute and preview VAD, speed, volume, and voice-blend metadata using the stub Actor.

---

## 🎛️ Harness Workflow

Run the `ProsodiaTuner` scheme, then choose an emotion source:

- **Fixed Preset**: Uses an editable saved state. The built-ins start from `baseline`, `somber`, `tender`, and the rest, but their VAD values, speed, volume, and voice percentages can be changed and saved as new states.
- **Custom VAD**: Exposes valence, arousal, and tension sliders.
- **Gemma (LLM)**: Uses a registered Gemma model through the real Director path.

Each sample passage has its own Speak control, so you can audition one line repeatedly without playing the full list. The list itself is the preview surface: it shows the current VAD, speed, volume, and voice-blend metadata. 

On harness build, a build phase copies the committed `ProsodiaTuner/SamplePassages.txt.example` to `SamplePassages.txt` if the editable file does not exist yet. Edit the `.txt` file for local listening work.

---

## 📄 License
Apache License 2.0. See [CONTRIBUTING.md](../../docs/CONTRIBUTING.md) for details.

