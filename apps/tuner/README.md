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

For real speech in the harness on macOS, models are resolved relative to the project workspace directory structure. Default models are seeded from the workspace-root `Models/` folder (one level above the Prosodia repo, shared across subprojects):

```text
/Models/                              # gitignored — this listing is the record (restructured 2026-07-12: org/repo layout)
├── config.json                       # Actor vocab (locked 178 symbols) + native sample rate — stays at root (engine reads it next to the model)
├── styletts2_lite.tflite             # Active Actor model — Sonora v1-ljspeech float32 e2e (fidelity-fixed 2026-07-12) — stays at root
├── Google/
│   ├── gemma-4-E2B-it.litertlm       # Gemma 4 E2B LiteRT-LM (Default Director model)
│   └── gemma-4-E4B-it.litertlm      # Gemma 4 E4B LiteRT-LM
├── Sonora/                           # HF clone: lmcfarlin/Sonora — our checkpoints + TFLite exports (v1-ljspeech/ incl. litert-split/)
├── litert-community/
│   └── Matcha-TTS/                   # HF clone — split-graph fp16 TFLite + espeak-free G2P assets
├── shivammehta25/
│   └── Matcha-TTS/                   # ⚠️ NOT a clean clone: June-2026 export-spike workspace (vendored source + ONNX/broken-TFLite artifacts; see its ARCHIVE.md)
├── IIEleven11/
│   └── StyleTTS2FineTune/            # StyleTTS2 fine-tuning pipeline (academic/side-discussion)
└── semidark/
    ├── StyleTTS2/                    # StyleTTS2 fork (academic/side-discussion)
    └── kikiri-tts/                   # kikiri-tts (academic/side-discussion)
```

> [!NOTE]
> **Model paths now resolve through `prosodia_models.json`** (repo root — role-based config,
> Debt F, commit `577a598`): the apps look up `actor`, `voices`, and `director-*` roles instead of
> hard-coding filenames, so the `Google/` Gemma location is handled by config. ⚠️ Authored
> remotely without `xcodebuild` — verify both app targets build (`apps/tuner/build.sh`) before
> deleting any root-level compatibility copies. `config.json` and `styletts2_lite.tflite` remain
> at the Models root because the Rust engine reads the config adjacent to the model file.

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
Dual-licensed under the GNU General Public License v3.0 and a commercial license. See [CONTRIBUTING.md](../../Docs/CONTRIBUTING.md) for details.

