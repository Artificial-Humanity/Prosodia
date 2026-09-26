# Architecture

Repository layout and module responsibilities — the single source of truth for topology
of **this repo**.

Project Prosodia is one of the flat, independent Artificial-Humanity repos (the umbrella
meta-repo was retired 2026-07-22; siblings live side by side in the workspace folder, among
them `Sonora/github` + `Sonora/huggingface`, whose exported actor models Prosodia consumes).
The shared model library lives at `/data/models` on ai-lab-0, resolved via
`prosodia_models.json` (`modelsBase: ../models`). Internal engineering notes are kept in a
separate private repository; `notes/` in a local checkout is an untracked link to them.

```text
prosodia/ (Project Prosodia — one of the flat Artificial-Humanity repos)
├── Cargo.toml                   # Root Manifest defining workspace members and shared profiles
├── Cargo.lock                   # Pinned dependency versions
├── AGENTS.md                    # Rules of record for agents working in this repo
├── WORKFLOW.md                  # Branch, review and merge workflow
├── PERSONA.md                   # The developer role
├── docs/                        # PUBLIC canon documentation
│   ├── ARCHITECTURE.md          # This file — repository layout & structure
│   ├── CONTRIBUTING.md          # Unified contribution and CLA guidelines
│   ├── ROADMAP.md               # Public forward-looking roadmap
│   └── defensive-publication-expressive-control.md  # Dated prior-art disclosure (2026-07-13)
├── LICENSE                      # Apache License 2.0 (Apache-2.0)
├── README.md                    # Master architectural framework documentation
├── assets/                      # README artwork
├── build_android.sh             # Android NDK build + Kotlin UniFFI binding generation
├── build_frameworks.sh          # UniFFI Swift bindings + Apple XCFramework build (macos-arm64 slice)
├── prosodia_config.json         # Runtime configuration (acoustic constants)
├── prosodia_models.json         # Role-keyed model paths (actor, voices, director-*)
│
├── crates/                      # ==========================================
│   │                            # CRATERS LAYER: Safe, Local Neural Systems
│   │                            # ==========================================
│   ├── core/
│   │   ├── Cargo.toml           # prosodia-core: byte-level BPE tokenizer
│   │   └── src/                 # BPE vocab tokenizer (regex, once_cell, thiserror)
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
│   │   ├── Cargo.toml           # Targets LiteRT/TFLite (the Sonora actor model)
│   │   └── src/                 # Sonora (Matcha-TTS) e2e + split-graph engines, G2P, voice loading
│   │
│   └── stage/
│       ├── Cargo.toml           # Internal path linkage: ../director & ../actor
│       └── src/                 # Synchronous pipeline coordination (Tokens -> Floating PCM matrices orchestration)
│
│   # FFI: each FFI-facing crate exports its own UniFFI scaffolding — there is no
│   # separate bindings crate. The build scripts generate the Swift and Kotlin wrappers.
│
├── platforms/                   # ==========================================
│   │                            # PLATFORMS LAYER: OS Hardware Adaptations
│   │                            # ==========================================
│   ├── apple/
│   │   ├── Package.swift        # Swift Package Manager (SPM) structural manifest coordinating all targets
│   │   ├── FFIHeaders/          # FFI Headers and modulemaps used for compiling the binary targets
│   │   ├── *FFI.xcframework     # Built Rust frameworks (actor, director, folioparser, stage)
│   │   ├── Vendor/              # Vendored espeak-ng source (GPL; unreferenced by Package.swift)
│   │   └── Sources/
│   │       ├── Kit/             # Consolidated Swift API wrapping FFI generated code & FolioParser
│   │       ├── Audio/           # Hand-coded native AVAudioEngine PCM loop streams
│   │       ├── Director/        # Swift interface driving the LiteRT-LM Gemma 4 director
│   │       ├── Actor/           # Swift interface driving the LiteRT Sonora actor
│   │       ├── Stage/           # Swift stage coordinator (StageManager, interruption controllers)
│   │       └── CLI/             # Command-line harness
│   │
│   ├── android/
│   │   ├── build.gradle.kts     # Native Android Gradle target configurations (NDK Gradle Package)
│   │   └── src/main/kotlin/     # Kotlin engine adapters and an AudioTrack PCM sink
│   │
│   ├── linux/
│   │   ├── Cargo.toml           # Desktop background runner (a Cargo workspace member)
│   │   └── src/                 # Low-level sound hooks mapping to ALSA / PulseAudio
│   │
│   └── windows/
│       ├── ProsodiaWin.csproj   # C# .NET library framework setup sheets (WASAPI exclusive-mode)
│       └── src/                 # WASAPI Exclusive-Mode low-latency sample streaming rings
│
└── apps/                        # ==========================================
    │                            # APPLICATIONS LAYER: Production Client Interfaces
    │                            # ==========================================
    ├── apple-reader/            # SwiftUI local-first book interface app (builds for macOS today; iOS needs iOS xcframework slices)
    │
    ├── android-reader/          # Jetpack Compose local-first application framework target (Penciled)
    │
    ├── tuner/                   # SwiftUI tuner app and Rehearsal Studio workbench
    │                            # (References local package: ../../platforms/apple; build with build.sh)
    │
    └── tuner-extension/         # Chrome Extension (JS/HTML/CSS) - MV3 tuning companion
        ├── manifest.json        # Manifest V3 setup sheet (Storage, highlights, active content permissions)
        ├── popup.{html,css,js}  # Glassmorphic parameter and spectrogram views
        └── content.js           # Content script
```