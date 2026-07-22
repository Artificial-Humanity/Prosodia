# Project State

_Last updated: 2026-07-13._

The committed, curated snapshot of where the project stands and what to do next. Behavioral
rules and the stack/layout manifest live in [AGENTS.md](../../AGENTS.md).

---

## Current State

- **Actor speaks — export fidelity fixed and verified end-to-end (2026-07-12/13).** The 2026-07-11 TFLite garble was root-caused to an `onnx2tf` encoder-LayerNorm axis mislowering and fixed at the source (Sonora `a537e03`); re-exported artifacts pass a deterministic fidelity gate (ONNX↔TFLite cosine 1.0000) and multi-sentence ASR (WER 0.000). Verified **through the shipping Rust engine** on Linux (`test_reference_ids_render_tmp`; ASR on the engine render verbatim). A weights-only-fp16/f32-I/O mobile artifact (59.4 MB) and a **LiteRT split-graph lane** (per-graph corr 1.000000, GPU-clean, human-auditioned) are published to [artificial-humanity/Sonora](https://huggingface.co/artificial-humanity/Sonora). Standing fidelity gate: `Sonora/scripts/export_fidelity_referee.py`.
- **Model paths externalized — Debt F implemented (2026-07-13, `577a598`).** `prosodia_models.json` maps role keys (`actor`, `voices`, `director-*`) to paths; both apps resolve through `ProsodiaModelsManager`; `DirectorModel` persists role keys so `Models/` restructures no longer strand UserDefaults; both healing shims deleted. **Awaiting desktop `xcodebuild` verification.**
- **Stage crate warning-free (Debt E, 2026-07-13).** Dead-code cleanup in `prosody.rs`/`coordinator.rs`.
- **StyleTTS2-Lite lineage rescued (2026-07-13).** The deleted fork's sole-copy history (incl. the InstanceNorm ONNX-export fix) + an uncommitted `export_litert.py` now live at [Artificial-Humanity/StyleTTS2FineTune](https://github.com/Artificial-Humanity/StyleTTS2FineTune); the original `styletts2_lite.onnx` is archived in the Prosodia-Storage bucket. Relevant to the high-ambition-5 re-platform.
- **Baseline dataset preparation complete (2026-07-10).** Configured host NVMe storage partition `/data/` for high-throughput model training. Acquired and preprocessed three major datasets: `LJSpeech-1.1` (13,100 single-speaker clips), `LibriTTS-R train-clean-100` (9.0 GB multi-speaker dataset), and `Expresso` (11,615 expressive 24kHz mono clips with unified speaker metadata index).
- **Sonora baseline model training & export complete (2026-07-11).** Trained the Matcha-TTS acoustic model on the host Radeon 8060S GPU to Epoch 260. Optimal convergence at Epoch 199 was exported with the HiFi-GAN vocoder embedded to Float32 and Float16 TFLite formats. Published model weights and binaries to the Hugging Face repository at **[artificial-humanity/Sonora](https://huggingface.co/artificial-humanity/Sonora)** (under directory `v1-ljspeech`). Connected new HF Pro cloud storage bucket **[artificial-humanity/Prosodia-Storage](https://huggingface.co/buckets/artificial-humanity/Prosodia-Storage)** and Gradio space **[artificial-humanity/Prosodia](https://huggingface.co/spaces/artificial-humanity/Prosodia)**.
- **Monorepo migration complete.** The formerly separate Swift repositories are consolidated into a single Cargo workspace (`crates/` · `bindings/` · `platforms/` · `apps/`).
- **Builds & tests green (host).** `cargo build`/`cargo test --workspace` pass (44 tests); the Apple SwiftPM package builds clean (`swift build`).
- **Core ported to Rust.** Director runs Gemma 4 inference over the LiteRT-LM C-API; the Actor runs StyleTTS2 LiteRT/TFLite inference in Rust (the Swift engines are now thin delegating wrappers); phoneme tokenization moved into the Rust pipeline.
- **Voice & stage features landed.** Continuous parametric voicing grid (bilinear age/masculinity interpolation + gruff texture blend), access-ordered LRU voice cache, and a bounded lookahead/backpressure pre-render path in the stage coordinator.
- **Reader apps scaffolded.** `apps/apple-reader` (SwiftUI) and `apps/android-reader` (Jetpack Compose) wired to the `StageCoordinator` pipeline; Android NDK library module + vendored `jniLibs`.
- **Senior developer audit findings resolved (2026-06-14).** All issues from the overnight pass (default lookahead limit back-compatibility, flaky lookahead tests, synchronous LLM FFI wrapper execution, doc drifts, and Xcode scheme renaming) have been fully addressed and verified.
- **Swift→Rust core port complete.** The legacy Swift G2P (`Misaki`) and `ActorEspeak` targets are deleted; G2P, tokenization, sentence segmentation, prosody markup parsing, and actor orchestration now live in the Rust core (`crates/actor/{g2p,lexicon,pipeline}.rs`, `crates/stage/{segmenter,markup_parser}.rs`). espeak-ng (GPL) is fully out of the Apple build scope.
- **Config & sample-rate centralization (2026-06-15).** Sample rate is exposed across the FFI via `get_sample_rate()` and consumed by the Swift/Kotlin platform layers; acoustic-matrix and phrase-pause calibration defaults aligned in the Rust core. The dead `hexgrad/StyleTTS2-Lite` `VoiceDownloader` URL was repointed to `artificial-humanity/StyleTTS2-Lite`.
- **Lexicon & desktop scaffolding (2026-06-16).** G2P lexicons are now compiled to binary at build time (zero-copy `include_bytes!` maps, no runtime JSON parse). Linux (ALSA/PulseAudio) and Windows (WASAPI Exclusive) audio-sink scaffolding landed under `platforms/`; the project roadmap moved to `Docs/ROADMAP.md`. _Caveat:_ the desktop sinks have no build wiring/FFI bridge yet — see [next-steps.md](next-steps.md) (Tech Debt C).
- **Matcha-TTS vocabulary lock & sample rate integration complete (2026-06-18).** Locked vocabulary contract (exactly 178 symbols), added dynamic native sample rate configurations, resolved the latest code review findings regarding console log flooding (LN4 warning deduplication), and verified all tests pass.
- **Matcha-TTS & TFLite bindings code review resolved (2026-06-17).** Resolved all issues in the stock Matcha-TTS integration and TFLite C-API bindings (exact dtype querying, overflow limit checks, dynamic input indexing, unified IPA remapping, warning on unknown phonemes, and caching of is_matcha/limit properties to prevent redundant locks).
- **Matcha-TTS Discovery Spike & FFI Contract Lock complete (2026-06-17).** Verified the custom `onnx2tf` conversion pipeline on `model_e2e.onnx` successfully. Exposed `is_matcha` across the UniFFI bridge, aligned the Swift platform backend protocols/wrappers, and validated that the macOS `ProsodiaTuner` app harness builds cleanly.


## Next Steps

The single live workstream and all deferred debt are tracked in [next-steps.md](next-steps.md) — start there to answer "what do we work on next?" (short answer: **desktop verification pair — Debt-F build check + Tuner audition — then the exploit-before-train measurement and VAT directability**).

## Pointers

- Change history — [CHANGELOG.md](CHANGELOG.md)
- **What's next & technical debt** — [next-steps.md](next-steps.md)
- Latest code review — [code-review-20260709-233816.md](code-review-20260709-233816.md) (records the evaluated range; the next review resumes at its end SHA per AGENTS.md §5)
- Engineering & Architecture Notes — [architecture-and-development.md](architecture-and-development.md)
- Out-of-bounds references (external/parent paths catalog) — [out-of-bounds-references.md](out-of-bounds-references.md)
- Voicing, Synthesis & Tuning — [voicing-synthesis-and-tuning.md](voicing-synthesis-and-tuning.md)
- Director Narrative Memory (story graph, spoiler-safe narrator chat, pre-reading) — [director-narrative-memory.md](director-narrative-memory.md)
- Voice Interruption & Discussion ("Solo Book Club" — voice barge-in to ask/discuss) — [voice-interruption-and-discussion.md](voice-interruption-and-discussion.md)
- Patent disclosure — Director↔Actor expressive control (Eureka briefs, capture of record) — [patent-disclosure-expressive-control.md](patent-disclosure-expressive-control.md)
- Actor Model & Training (selection + hardware + first-run) — [actor-model-and-training.md](../Sonora/actor-model-and-training.md)
- High-ambition goals (in sequence) — [1 Matcha-TTS Actor](../Sonora/high-ambition-1-matcha-actor.md) · [2 Dramatic Reader](../Sonora/high-ambition-2-dramatic-reader.md) · [3 Child Voices](high-ambition-3-child-voices.md) · [4 Multilingual G2P](high-ambition-4-multilingual-g2p.md) · [5 StyleTTS2-Lite](../Sonora/high-ambition-5-styletts2-lite.md)
- Repository layout — [../../Prosodia/Docs/ARCHITECTURE.md](../../Prosodia/Docs/ARCHITECTURE.md)

## Environment footguns

1. LiteRT-LM is a Git-LFS source package with a **missing upstream LFS object**, so SPM resolve/build must skip LFS:

```bash
GIT_LFS_SKIP_SMUDGE=1 swift package resolve
GIT_LFS_SKIP_SMUDGE=1 swift build
```

2. **Never build the Tuner with legacy `xcodebuild -target` style** — LiteRT-LM is a Bazel repo with a `BUILD` file at its checkout root, and target-style builds try to `mkdir build/` in the same place; on case-insensitive macOS filesystems they collide and the build dies in `CreateBuildDirectory`. Scheme builds route products through DerivedData and are unaffected. The one shared scheme is **`ProsodiaTuner`** (the old `ProsodiaTuner Harness` name is gone), arm64-only destinations (the FFI xcframeworks carry no x86_64 slice). Canonical invocation: `apps/tuner/build.sh` (2026-07-11), which chains `build_frameworks.sh` → `xcodebuild`; the project also carries a **"Check FFI Framework Freshness" tripwire phase** that fails GUI builds loudly when the xcframeworks are older than `crates/` sources (this is what stale-Rust "unbuildable" reports were — rebuild via `build_frameworks.sh`). The tripwire requires `ENABLE_USER_SCRIPT_SANDBOXING = NO`, set in the project.
