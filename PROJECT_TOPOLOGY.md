# Project Topology

```text
prosodia/ (Unified Repository Root)
├── .github/                     # Organization health, workflows & profile profiles
├── Cargo.toml                   # Root Manifest defining workspace members and shared profiles
├── CONTRIBUTING.md               # Unified contribution and CLA guidelines
├── Documentation/               # Finalized, official notation about the project
├── LICENSE                      # GNU General Public License v3.0 (GPL-3.0)
├── LICENSE-COMMERCIAL.md        # McFarlin Technologies Commercial License (Draft)
├── Models/                      # Reference models and weight matrices (Gitignored)
├── Notes/                       # Provisional working notes and goals (Private Git Submodule)
├── README.md                    # Master architectural framework documentation
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
│   │       ├── Stage/           # Swift coordinate coordinator (StageManager, interruption controllers)
│   │       ├── Misaki/          # Localized TTS grapheme-to-phoneme engine helper
│   │       └── ActorEspeak/     # Espeak-ng Swift wrapper integration
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
    ├── apple-reader/            # SwiftUI local-first book interface app (iOS/macOS target) (Penciled)
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