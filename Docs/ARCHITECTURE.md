# Architecture

Repository layout and module responsibilities — the single source of truth for topology
of **this repo**.

Project Prosodia is one of the flat, independent Artificial-Humanity repos (the umbrella
meta-repo was retired 2026-07-22; siblings live side by side in the workspace folder:
`AI-Lab-AMD/`, `Sonora/github` + `Sonora/huggingface`, `Council-of-Experts/`,
`ArtificialHumanity.io/`). The shared model library lives at `/data/models` on ai-lab-0,
resolved via `prosodia_models.json` (`modelsBase: ../models`); internal engineering notes
live in this repo's `notes/`.

```text
prosodia/ (Project Prosodia — one of the flat Artificial-Humanity repos)
├── Cargo.toml                   # Root Manifest defining workspace members and shared profiles
├── Cargo.lock                   # Pinned dependency versions
├── Docs/                        # PUBLIC canon documentation
│   ├── ARCHITECTURE.md          # This file — repository layout & structure
│   ├── CONTRIBUTING.md          # Unified contribution and CLA guidelines
│   └── ROADMAP.md               # Public forward-looking roadmap
├── LICENSE                      # Apache License 2.0 (Apache-2.0)
├── README.md                    # Master architectural framework documentation
├── build_android.sh             # Android NDK build helper
├── build_frameworks.sh          # Apple XCFramework build helper
├── prosodia_config.json         # Runtime configuration
│
├── crates/                      # ==========================================
│   │                            # CRATERS LAYER: Safe, Local Neural Systems
│   │                            # ==========================================
│   ├── core/
│   │   ├── Cargo.toml           # Abstract trait interfaces, primitive types, vocab indices (Penciled)
│   │   └── src/                 # Zero-dependency BPE vocab tokenizers & schemas
│   │
│   ├── folioparser/
│   │   ├── Cargo.toml           # Parser for EPUB structures, OPF XML, and text extraction
│   │   └── src/                 # Rust XML event streaming parser
│   │
│   ├── director/
│   │   ├── Cargo.toml           # Targets LiteRT-LM framework configurations (Gemma 4 pipeline)
│   │   └── src/                 # Gemma 4 director context orchestration mapping
│   │
│   ├── actor/
│   │   ├── Cargo.toml           # Targets LiteRT matrix engines (Local Prosodia voice model)
│   │   └── src/                 # StyleTTS2 & custom weights inference math pipelines
│   │
│   └── stage/
│       ├── Cargo.toml           # Internal path linkage: ../director & ../actor
│       └── src/                 # Synchronous pipeline coordination (Tokens -> Floating PCM matrices orchestration)
│
├── bindings/                    # ==========================================
│   │                            # BRIDGING LAYER: Native Translation Gateways
│   │                            # ==========================================
│   └── ffi/
│       ├── Cargo.toml           # Imports UniFFI runtime code generation macros (Declarative engine adapters) (Penciled)
│       └── src/                 # Declarative code definition sheets & pure C pointer maps
│
├── platforms/                   # ==========================================
│   │                            # PLATFORMS LAYER: OS Hardware Adaptations
│   │                            # ==========================================
│   ├── apple/
│   │   ├── Package.swift        # Swift Package Manager (SPM) structural manifest coordinating all targets
│   │   ├── FFIHeaders/          # FFI Headers and modulemaps used for compiling the binary targets
│   │   └── Sources/
│   │       ├── Kit/             # Consolidated Swift API wrapping FFI generated code & FolioParser
│   │       ├── Audio/           # Hand-coded native AVAudioEngine PCM loop streams
│   │       ├── Director/        # Swift interface driving the LiteRT-LM Gemma 4 director
│   │       ├── Actor/           # Swift interface driving the LiteRT StyleTTS2 actor
│   │       └── Stage/           # Swift coordinate coordinator (StageManager, interruption controllers)
│   │
│   ├── android/
│   │   ├── build.gradle.kts     # Native Android Gradle target configurations (NDK Gradle Package)
│   │   └── src/main/kotlin/     # Kotlin engine adapters hooking into C++ Oboe / AAudio queues
│   │
│   ├── linux/
│   │   ├── Cargo.toml           # Desktop system background runner daemon script
│   │   └── src/                 # Low-level sound hooks mapping to ALSA / PulseAudio
│   │
│   └── windows/
│       ├── ProsodiaWin.csproj   # C# .NET library framework setup sheets (WASAPI exclusive-mode)
│       └── src/                 # WASAPI Exclusive-Mode low-latency sample streaming rings
│
└── apps/                        # ==========================================
    │                            # APPLICATIONS LAYER: Production Client Interfaces
    │                            # ==========================================
    ├── apple-reader/            # SwiftUI local-first book interface app (iOS/macOS target) (Completed)
    │
    ├── android-reader/          # Jetpack Compose local-first application framework target (Penciled)
    │
    ├── tuner/                   # SwiftUI tuner app and Rehearsal Studio workbench
    │                            # (References local package: ../../platforms/apple)
    │
    └── tuner-extension/         # Chrome Extension asset workspace tool (TS/JS/HTML/CSS) - MV3 tuning companion
        ├── manifest.json        # Manifest V3 setup sheet (Storage, highlights, active content permissions)
        ├── popup/               # Glassmorphic parameter spectrogram rendering views
        └── scripts/             # background.js worker tracking aligned dataset collection caches
```