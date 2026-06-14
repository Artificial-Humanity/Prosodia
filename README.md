# 📖 Prosodia: The On-Device Dramatic Audiobook Engine

Welcome to **Prosodia**! 🎭✨

Prosodia is an on-device, directable neural audiobook engine. Instead of a robotic text-to-speech voice reading books like they're reciting tax codes, Prosodia acts as a digital rehearsal studio. It analyzes the text, determines the emotional subtext, blends custom voice casting profiles, and narrates audiobooks with performance-grade human expression.

Everything runs locally on-device via Google LiteRT (formerly TensorFlow Lite) and Apple Metal/Accelerate. No cloud APIs, no network latency, and absolutely zero chance of our AI models escaping to buy items on your credit card.

---

## 🎭 The Monorepo Cast & Crew

Our monorepo organizes the workspace into simple layers:

### 1. Crates (The Safe, Local Neural Core)
*   [**`core`**](file:///Users/lmcfarlin/Projects/Prosodia/crates/core): The vocabulary index, BPE tokenizer, and shared traits. The bedrock of our dependency graph.
*   [**`folioparser`**](file:///Users/lmcfarlin/Projects/Prosodia/crates/folioparser): Parses EPUB XML structures, extracts plain text, and prevents us from getting lost in OPF manifests.
*   [**`director`**](file:///Users/lmcfarlin/Projects/Prosodia/crates/director): The Emotional Director. Driven by Gemma 4 (LiteRT-LM), it reads book passages and provides performance notes—such as Valence, Arousal, Tension (VAD), and casting assignments.
*   [**`actor`**](file:///Users/lmcfarlin/Projects/Prosodia/crates/actor): The Voice Talent. Driven by StyleTTS2 (LiteRT), it takes the performance notes and synthesizes raw floating-point PCM audio matrices.
*   [**`stage`**](file:///Users/lmcfarlin/Projects/Prosodia/crates/stage): The Stage Manager. Coordinates the Director and the Actor, schedules queues, and runs around holding a clipboard making sure everything plays gaplessly.

### 2. Platforms (Hardware Bridges)
*   [**`apple`**](file:///Users/lmcfarlin/Projects/Prosodia/platforms/apple): A Swift Package combining the FFI target bridges and custom `AVAudioEngine` PCM loops.

### 3. Downstream Apps
*   [**`tuner`**](file:///Users/lmcfarlin/Projects/Prosodia/apps/tuner): The Rehearsal Studio mixing board. Tweak VAD sliders, swap casting parameters, A/B test models, and listen to the dramatic results.
*   [**`tuner-extension`**](file:///Users/lmcfarlin/Projects/Prosodia/apps/tuner-extension): Chrome Manifest V3 extension companion.

---

## 🚀 Getting Started

To get the digital theater up and running, you'll need standard Rust and Xcode setups.

### Build the Rust Workspace
Compile all Rust crates and check that the core neural logic compiles successfully:
```bash
cargo build --release
```

### Run the Rehearsal workbench
1. Make sure you have models populated in the `/Models` directory.
2. Open `apps/tuner/ProsodiaTuner.xcodeproj` in Xcode.
3. Select the `Tuner` scheme, hit **Run**, and start playing with the VAD sliders! 🎛️

---

## 📄 License

Dual-licensed under the GNU General Public License v3.0 and a commercial license. See the [Docs](Docs) folder for details.

---

*“Speak the speech, I pray you, as I pronounced it to you, trippingly on the tongue.” — Hamlet* 💀🎬
