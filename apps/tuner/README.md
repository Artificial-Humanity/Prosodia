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

## 💻 Local Models for the Harness

For real speech in the harness on macOS, models are resolved relative to the project workspace directory structure. Default models are seeded from the workspace-root `Models/` folder (one level above the Prosodia repo, shared across subprojects):

```text
/Models/
├── gemma-4-E2B-it.litertlm           # Gemma 4 E2B LiteRT-LM (Default Director model)
├── gemma-4-E4B-it.litertlm           # Gemma 4 E4B LiteRT-LM
└── StyleTTS2FineTune/                # StyleTTS2 Actor weights & fine-tuning pipeline
```

The speak functionality also checks for the fine-tuning checkpoint file in our harness at `StyleTTS2FineTune/StyleTTS2/Models/LibriTTS/epochs_2nd.pth`. Without the required model files present, the harness can still compute and preview VAD, speed, volume, and voice-blend metadata using the stub Actor.

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

