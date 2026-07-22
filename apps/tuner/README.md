# ProsodiaTuner 🎛️🎭

Welcome to the **Rehearsal Studio**! 

`ProsodiaTuner` is the auditioning sandbox, mixing board, and parameter tuner for **Project Prosodia**. This is where we call our **Director** (LLM) and **Actor** (TTS) onto the stage, adjust Valence-Arousal-Tension (VAD) sliders, A/B test models, and tweak our acoustic matrix to ensure the show is spectacular.

> [!NOTE]
> The production app target (`ProsodiaTuner`) has been removed from this repository to serve as a clean slate later. This repository is now strictly dedicated to the parameter tuning harness and testing workbench (`ProsodiaTuner`).

---

## 🛠️ Rehearsal Workspace

The project contains the following components:

- `ProsodiaTuner` app: The tuning tool and auditioning environment.
- `ProsodiaTuner.xcodeproj`: The Xcode configuration project.
- `ProsodiaTunerTests`: Unit tests for validating the harness.

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

For real speech in the harness on macOS, models are resolved relative to the project workspace directory structure. Default models are seeded from the shared `Reference/models/` folder at the workspace root (`../Reference/models` from the Prosodia repo — moved under `Reference/` 2026-07-13 to mark these as reference assets, not workspaces):

```text
/Reference/models/                    # gitignored — this listing is the record (restructured 2026-07-12: org/repo layout; moved under Reference/ 2026-07-13)
├── config.json                       # Actor vocab (locked 178 symbols) + native sample rate — stays at root (engine reads it next to the model)
├── sonora.tflite                     # Active Actor model — Sonora baseline-ljspeech-22k float32 e2e (fidelity-fixed 2026-07-12; renamed from styletts2_lite.tflite 2026-07-13 — it is a Matcha-architecture model, not StyleTTS2; registry artifact renamed from v1-ljspeech 2026-07-22) — stays at root
├── Google/
│   ├── gemma-4-E2B-it.litertlm       # Gemma 4 E2B LiteRT-LM (Default Director model)
│   ├── gemma-4-E4B-it.litertlm       # Gemma 4 E4B LiteRT-LM
│   └── gemma-4-26B-A4B-it-qat-q4_0-gguf/  # Gemma 4 26B-A4B MoE (128 experts/8 active, 256K ctx), QAT q4_0 GGUF, Apache-2.0 — OFFLINE server-side Director for Sonora book_ingest labeling (served via the ollama OpenAI API, :11434); NOT an on-device/Tuner model
├── litert-community/
│   └── Matcha-TTS/                   # HF clone — split-graph fp16 TFLite + espeak-free G2P assets
├── shivammehta25/
│   └── Matcha-TTS/                   # Clean upstream clone (reference). The old spike workspace was rescued + pruned 2026-07-13 (history: github.com/Artificial-Humanity/StyleTTS2FineTune; ONNX: Prosodia-Storage bucket archive/)
├── IIEleven11/
│   └── StyleTTS2FineTune/            # StyleTTS2 fine-tuning pipeline (academic/side-discussion)
└── semidark/
    ├── StyleTTS2/                    # StyleTTS2 fork (academic/side-discussion)
    └── kikiri-tts/                   # kikiri-tts (academic/side-discussion)
```

The Sonora HF registry (huggingface.co/artificial-humanity/Sonora — our checkpoints + TFLite exports, `baseline-ljspeech-22k/` incl. `litert-split/`) is **not** under `Reference/models/`: it is a working artifact registry, not a reference model. It's checked out as the `Sonora-HF` sibling repo directly (superseding the older `Registry/Sonora/` gitignored-clone layout from the umbrella-workspace era).

> [!TIP]
> **Plan A multi-graph runtime (2026-07-13):** the engine also accepts a split-model **directory**
> (textenc/decoder/vocoder graphs + `emb.bin` + `config.json`) — the `actor-split` role in
> `prosodia_models.json` points at the registry's `litert-split/` set. To audition it, swap the
> `actor` role's path to that directory: host-side Euler ODE, real per-token durations from `logw`
> (the `DS:` contract channel is live), no 50-token limit (256), fp16 graphs.

> [!NOTE]
> **Model paths now resolve through `prosodia_models.json`** (repo root — role-based config,
> Debt F, commit `577a598`): the apps look up `actor`, `voices`, and `director-*` roles instead of
> hard-coding filenames, so the `Google/` Gemma location is handled by config. ⚠️ Authored
> remotely without `xcodebuild` — verify both app targets build (`apps/tuner/build.sh`) before
> deleting any root-level compatibility copies. `config.json` and `sonora.tflite` remain
> at the `Reference/models/` root because the Rust engine reads the config adjacent to the model file.

The speak functionality also checks for the fine-tuning checkpoint file in our harness at `IIEleven11/StyleTTS2FineTune/StyleTTS2/Models/LibriTTS/epochs_2nd.pth`. Without the required model files present, the harness can still compute and preview VAD, speed, volume, and voice-blend metadata using the stub Actor.

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
Apache License 2.0. See [CONTRIBUTING.md](../../Docs/CONTRIBUTING.md) for details.

