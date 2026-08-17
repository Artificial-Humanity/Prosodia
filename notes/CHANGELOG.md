# Changelog

This document tracks technical changes, refactoring milestones, and build-system adjustments for Project Prosodia.

> **Maintenance:** This changelog is append-only within a release cycle — keep it current every
> session and prune entries only when tagging a new release of the overall project (see
> [AGENTS.md](../../AI-Lab-AMD/AGENTS.md)). It was first maintained at `Documentation/Notes/changelog.md`,
> then `Notes/CHANGELOG.md`; it now lives at `Notes/Prosodia/CHANGELOG.md`. `Notes/` was
> formerly a Git submodule, then a standalone private repo; it is now a plain folder inside the
> private `Artificial-Humanity` umbrella repo, keeping the internal engineering log out of the
> public Prosodia repo.

---
## [2026-08-10]

### Added
- **`.github/workflows/claude-fix.yml` — the review loop is closed** (`70f9a24`). The reviewer only
  ever commented; nothing acted on those comments. Adding the `claude-fix` label to a PR now runs
  a fix pass that reads the inline review comments (they are **not** in `gh pr view` — it fetches
  `/pulls/N/comments`), commits fixes, replies with what it changed and what it deliberately did
  not, and removes the label.
  - **Label-gated on purpose.** Firing on `pull_request_review` submitted oscillates: fix pushes →
    `synchronize` → reviewer posts → fix pushes → …, each lap billed at full model rates. The
    vendor ships no loop guard and does not document which token it pushes with, so GitHub's own
    recursion protection can't be relied on either. One label, one pass; re-label to repeat.
  - Load-bearing details: `contents: write` (this job pushes, the reviewer only comments);
    checkout `ref: head.ref` + `repository: head.repo.full_name` (the default checkout is a
    detached merge commit, which cannot be pushed from); `fetch-depth: 0`; and
    `cancel-in-progress: false`, since cancelling mid-pass can leave edits applied but uncommitted.

### Fixed
- **`.github/workflows/claude-review.yml` — the automated PR reviewer could not post** (`d7749c7`).
  It ran to completion and delivered nothing. Three independent gaps, each a silent no-op on
  its own, plus a wrong secret name:
  - **No `permissions:` block.** This repo's default workflow token is read-only
    (`default_workflow_permissions: "read"`), so the job analyzed the diff, billed the tokens,
    and had no right to comment. Added `contents: read` / `pull-requests: write` /
    `id-token: write`.
  - **No `--allowedTools`.** Permissions grant the TOKEN the right; `--allowedTools` grants the
    AGENT the tool. Without the inline-comment MCP tool and the `gh` allowlist there was no
    mechanism to post at all.
  - **The prompt never said to post to GitHub** — added `REPO` / `PR NUMBER` context and
    explicit `gh pr comment` / `create_inline_comment` instructions.
  - **The secret is the org-level `CLAUDE_OAUTH_TOKEN`**, not `CLAUDE_CODE_OAUTH_TOKEN`. The
    action's *input* name stays `claude_code_oauth_token`, so the two deliberately disagree;
    the file carries a comment on that line because it reads as a typo.
- Dropped `--effort xhigh` — not a documented `claude_args` flag, an unrecognized flag fails the
  run outright, and Claude Code already defaults to xhigh effort on capable models.
- Replaced the severity filter. "Ignore superficial style or formatting nitpicks" is followed
  *literally*: the model finds the bugs, then declines to report anything below the bar, so
  precision looks excellent while real findings vanish. Now a concrete bar (anything that could
  cause incorrect behavior, a test failure, a security weakness, or a misleading result) with
  explicit severity, and an instruction not to filter past it.
- Softened "security exploits" to "security vulnerabilities" (`claude-fable-5` runs classifiers
  over cybersecurity content and can return `stop_reason: "refusal"`), added a `concurrency`
  group so rapid pushes cancel superseded billed runs, and pinned checkout `@v6`.

## [2026-07-14]

### Added
- **De-risk training phase STAGED AND STARTED (Sonora `44a82e1` + `9dda843`, umbrella compose):**
  (1) **Corpus derived** — `scripts/derive_vat_corpus.py` over LibriTTS-R train-clean-100 @
  native 24 kHz: 30,351 clips / 45.7 h / 247 speakers, v0 labels (A = per-speaker LUFS z-score,
  clamped ±1 @ 2σ; V=T=0), op-G2P phonemized (0 vocab violations, 0 unresolved), mel stats
  −5.5048/2.3861, `libritts_r_vat` data config + `derisk_energy` experiment. (2) **Vocoder
  fine-tune LAUNCHED** — `vocoder_training` compose service (own profile): HiFi-GAN 24 kHz/
  80-band (fmax 12000), warm-started from UNIVERSAL_V1 generator+discriminator, on LibriTTS-R;
  meldataset modernized (librosa≥0.10 kwargs, stft return_complex). (3) **De-risk acoustic run
  QUEUED** — warm-start ckpt built by `scripts/make_warmstart.py` (matcha_vctk donor: 306 warm /
  33 fresh tensors, spk_emb 109→247 shape-guarded); `sonora_training` compose command points at
  `experiment=derisk_energy` — launch after the vocoder finishes (shared GPU), then judge with
  the eval harness against the §7 thresholds.
- **VAT FiLM conditioning code shipped (Sonora `ad2baea`)** — milestone 3's model-code
  prerequisite, implemented to the same-day design decision
  ([vat-conditioning-design.md](../../Sonora/github/docs/vat-channels.md)): zero-init FiLM per
  encoder block + per CFM U-Net level, shared trunk, raw `[B,3,T]` input, cond dropout 0.15,
  `load_vat` filelist field; off by default. Verified: bit-identical warm start from Phase 0
  (vat 0/None/hot — an `initialize_weights()` kaiming clobber of the zero-init was caught and
  fixed), healthy training forward (grads in all 24 heads), litert-torch export gate GPU-clean
  at corr 1.000000. Standing gates committed as `scripts/test_vat_identity.py` +
  `scripts/test_film_export_gate.py`. *Residual:* the conversion harness wrappers gain the
  `vat` graph input when the first VAT checkpoint is converted.
- **Rust multi-graph (split) runtime landed — Plan A implemented** (Prosodia `f08351c` +
  `1d24fae` + xcframework refreshes; new `crates/actor/src/split_engine.rs`): host-orchestrated
  textenc/decoder/vocoder pipeline per the litert-samples recipe. Parity vs a Python reference on
  the shipped fp16 graphs with identical noise: cosine 1.000000. **Two contract channels are live
  for the first time:** per-token `duration_scales` dictation (2× → 66→132 frames) and the
  measurement-backed per-frame mel-gain energy hook (−6.0 dB → −6.01 dB). Split models are
  directories; engine dispatches by path type (token limit 256, real per-token `pred_dur`,
  XNNPACK per graph); Swift provider + `actor-split` role added. Remaining: Tuner audition +
  latency acceptance check, payload energy routing + streaming with the milestone-3 FFI rework.
  38/38 actor tests; both app targets build.
- **Standing eval harness shipped (Sonora `c58028c`)** — the objective half of the
  listen→iterate loop and the §7 gate: `scripts/eval_harness.py`, manifest-driven, with the
  pre-registered thresholds (ρ ≥ 0.9, ECAPA leakage ≤ 0.2 vs a real inter-speaker gap,
  WER Δ ≤ +0.10). Smoke test on the exploit-before-train renders: energy passes all gates at
  inference; duration flags its ×0.6 WER boundary and tempo-extreme identity drift (0.244).
- **Espeak-free training G2P lane shipped (Sonora `d5dd4fc`)** — closes north star §8.3:
  `matcha/text/op_g2p.py` (OpenPhonemizer 275k dict + DeepPhonemizer TFLite OOV + U+0303 rule),
  `scripts/phonemize_filelist.py` (offline pre-phonemization, locked-vocab validation),
  `no_cleaners` passthrough, `ljspeech_op` data/experiment configs, lazy espeak init. LJSpeech
  train+val phonemized: 13,100 lines, 0 vocab violations, 0 unresolved (95.8% dict / 4.2%
  neural). `espeak-ng` dropped from the `sonora_training` compose command (stale Phase-0 resume
  ckpt_path removed with it; playpen keeps espeak — dev-tool inference on arbitrary text).
- **License wall shipped (same commit)** — closes north star §8.2: `configs/data_licenses.yaml`
  (verified classes from dataset-landscape.md) + `matcha/data/license_wall.py` enforced in
  `TextMelDataModule.setup()`. Undeclared and NC data refused at training time;
  `SONORA_LICENSE_WALL=derisk` permits NC for §7 de-risk runs with a TAINTED banner; no "off"
  mode exists. Behavior test-verified (pass/block/banner/unknown-block).
- **Exploit-before-train measurement executed and written up**
  (Sonora `notes/archive/exploit-before-train-measurement.md`, deleted 2026-08-02 in `8bbf343` — git history is the archive):
  north-star §6 experiment run on `ai-lab-0` against the Epoch-199 litert split graphs via a new
  pure-inference harness (`/data/toolchain/litert-conversion/exploit_measure.py`; 21 WAVs +
  `results.json` alongside). Verdict: **pace and loudness are free at inference** (per-token
  `duration_scales` via host `logw`: surgical, ρ = 1.0, WER-safe to ×2.0; per-frame log-mel dB
  bias pre-vocoder: dB-exact, zero context bleed, WER 0 at −12 dB) — **pitch and phonation need
  training** (no F0 input; mel-bin roll ≈ −1.5 st before artifacts; no breathiness lever).
  Milestone 3 scoped accordingly: VAT conditioning owns pitch + voice quality; corpus weighting
  toward valence/tension expression; the multi-graph runtime gains a measurement-backed
  per-frame mel-gain (energy) hook requirement. STATE §objectives and next-steps §B/TL;DR
  updated in lockstep.

---
## [2026-07-13]

### Added
- **Dev-topology item captured (Debt H)** —
  [Ai-Lab-0/dev-topology-and-workstreams.md](../../AI-Lab-AMD/notes/dev-topology-and-workstreams.md): the
  owner wants workstream/machine separation of concerns in the near future (model work → ai-lab-0;
  Xcode + runtime audition → Mac; Rust → either) to enable simultaneous development instead of the
  current push/pull ping-pong. Note records this week's concrete costs (Debt-F rebase collision,
  3× tripwire xcframework rebuilds after Linux-authored crate changes, same-day docs rebases) and
  an option sketch (AGENTS routing table, branch discipline, CI as meeting point — Linux cargo +
  macOS xcframework/app jobs — then possibly retiring committed xcframeworks). Linked as Deferred
  Technical Debt H in next-steps.
- **Dataset landscape documented** ([dataset-landscape.md](../../Sonora/github/notes/dataset-landscape.md)):
  verified-license training options by roadmap role. Cleared: LibriTTS-R, Parler annotated
  LibriTTS-R, cdminix/libritts-r-aligned, **Emilia-YODAS subset only** (corrects the earlier
  "Emilia relicensed CC-BY" reading — the original 101k-h subset remains CC-BY-NC), GLOBE V2 (CC0);
  eval-side E-VOC + MANGO. Excluded with reasons: Expresso (NC), Emilia-original (NC),
  provenance-risky MIT-labeled scrapes. Strategy on record: milestone-3 corpus as a *derivation
  pipeline* over permissive LibriTTS-R derivatives + Emilia-YODAS mining, dual-use with the HA-6
  Audience. Cross-linked from STATE §VAT, open-decision tightening #3, and HA-6.
- **High-ambition 6 captured — the "Audience": conveyance-aware STT**
  ([high-ambition-6-audience-conveyance-stt.md](../../Sonora/github/notes/high-ambition-6-audience-conveyance-stt.md)):
  the reverse of the actor lane — a small, on-device, *typed* listener emitting the existing control
  contract as annotations (V/A/T + per-token emphasis) so conversation carries conveyance both ways
  (hear → contract → think → contract → speak). Key arguments on record: the milestone-3 VAT corpus
  is dual-use (audio→labels trains the listener), and the listening direction is NOT covered by the
  defensive publication (fresh IP decision required before it goes public). Vision note only —
  parked for the Solo Book Club conversation layer; cross-linked from
  voice-interruption-and-discussion.md.

### Changed
- **Defensive publication executed (Path B)** — the Director→Actor expressive-control invention is
  now published, enabling prior art in the public Prosodia repo
  (`Docs/defensive-publication-expressive-control.md`, commit `946bcc2`, linked from the README):
  control contract, V/A/T acoustic mapping, per-token dictation incl. the contemplated
  trained-to-obey conditioning, casting grid, binding layer, hush/full-cast embodiments, casting
  gate. The narrative-knowledge-graph / Q&A track was deliberately excluded and keeps full patent
  optionality. Rationale + follow-ups recorded in open-decision-licensing.md.
- **Licensing & IP posture decided in principle** — new
  [open-decision-licensing.md](open-decision-licensing.md): current split affirmed (Prosodia
  GPL+commercial as the maybe-sellable product; Sonora Apache "for everyone"), with the accepted
  open-model/competing-director tension on record. CLA verified already present in public
  CONTRIBUTING.md §1. Prior-art posture analyzed: public repo discloses the enabled mechanism since
  ~2026-06-13 (defensive anchor; US grace to ~June 2027; absolute-novelty jurisdictions likely
  forfeited for disclosed features); milestone-3 trained-to-obey conditioning remains undisclosed —
  Path A (provisional before it ships) vs Path B (deliberate defensive publication) must be chosen
  before milestone-3 work goes public.
- **Registry layout settled: end-state C — clone moved `Sonora/model/` → workspace-root
  `Registry/Sonora/`** (sibling of `Reference/`, killing the repo-in-repo nesting; supersedes the
  same-day status-quo call). Updated: umbrella `.gitignore` (+`/Registry/Sonora`, symlink-proof, and
  the Reference comment's stale `/data/Models` target), Sonora `.gitignore` (dropped `/model/`),
  `bootstrap.sh` clone/copy paths + `--at-versions` entry, `snapshot_versions.sh` row name,
  AGENTS (Where-Things-Live row + §8), tuner README, Notes (STATE, registry-housekeeping,
  cleanup-chores promote flow, next-steps). Also recorded (earlier same day): Tuner
  time-to-first-audio latency as a split-graph-runtime validation item (next-steps §B).
  ai-lab-0 mirror of the `Registry/` move is pending (owner).
- **Registry housekeeping executed (items 1–4 of `Notes/Sonora/registry-housekeeping.md`);
  layout stays status-quo (end-state A).** Registry commit `a889bf0`: model card refreshed
  (Provenance section + per-promotion convention `train-repo:`/`run:`, stale consumption steps
  fixed for `Reference/models`/`sonora.tflite`, bogus cargo invocation replaced, `wer` metric) and
  the engine-contract `config.json` (locked 178-symbol vocab, 22050) now ships in the registry at
  `v1-ljspeech/`. Umbrella: `bootstrap.sh` fetches `config.json` from the registry (warning →
  fetch) and `--at-versions` restores the registry pin; `snapshot_versions.sh`/VERSIONS.md gained
  a `Sonora/model` row; AGENTS §8 states the provenance convention. Verified with a full
  bootstrap run + `--at-versions` round trip.
- **Actor model file renamed `styletts2_lite.tflite` → `sonora.tflite`** (Prosodia + umbrella):
  the file is a Matcha-architecture Sonora model; the StyleTTS2 name was a fossil from the
  pre-Matcha plan and would collide with the genuine StyleTTS2-Lite re-platform later. Identity
  stays in the role config (`prosodia_models.json` `actor` entry + display string). Updated:
  config, both Swift fallback tables, `engine.rs` tests, CLI, `bootstrap.sh` copy target, tuner
  README, AGENTS §8, out-of-bounds catalog. Engine loads the renamed file
  (`test_sonora_e2e_forward` passes, not skipped); both app targets rebuilt.
- **Workspace relayout: shared `Models/` moved to `Reference/models/`; Sonora HF clone moved to
  `Sonora/model/`.** The `Reference/` parent marks these as reference assets, not workspaces; the
  artificial-humanity/Sonora registry clone (our own artifacts, not a reference model) moved out of
  the shared dir into the Sonora project directory. Path updates in lockstep: Prosodia
  `prosodia_models.json` `modelsBase` (`../Models` → `../Reference/models`), the in-code fallbacks
  in both `ProsodiaModels.swift` copies, `crates/actor/src/engine.rs` test paths, Prosodia
  README/ARCHITECTURE/tuner-README (canonical listing), Prosodia + umbrella `.gitignore`s, umbrella
  `AGENTS.md` §8 (now "Reference Directory Mandate"), Sonora STATE, next-steps, and the
  out-of-bounds catalog. `ai-lab-0`'s `/data/Models` layout is unchanged (rename is Mac-side only);
  dated log entries retain the old paths as historical record.

### Fixed
- **Preset edits after the first Speak were silently discarded — speed dial inaudible** (Prosodia
  `846a22c`): the harness apps cached the director keyed on (model, emotionMode, narrationMode) —
  not the directive — and the preset-mode `StubDirectorInference` bakes the directive in at
  construction, so the first Speak froze the dials (speed, VAD, volume, casting). Preset mode now
  always builds a fresh stub (free); the cache remains for the Gemma path. Ruled out first: the
  payload `S:`-tag round-trip, phraser propagation, Rust pipeline/engine, and both model artifacts
  (ONNX oracle + shipped `sonora.tflite` scale output exactly with `length_scale`, probed
  0.5×/1×/2×). Ear-verified fixed.

### Performance
- **Tuner Play latency ~5× better** (Prosodia `104a9c8` + `1928ad2`): the actor engine attaches
  the XNNPACK delegate on Apple targets (exported by the `CLiteRTLM_mac` dylib; `num_threads` 4
  via a zeroed oversized options buffer — field 0 has always been `num_threads`, zero is the
  benign default for later fields; Linux TFLite builds have XNNPACK off, so it's cfg-gated with a
  plain-interpreter fallback). One fixed-shape e2e forward: ~14.8 s → ~2.9 s on the M1 Max
  (`test_sonora_e2e_forward` 15.2 → 3.3 s wall; 36/36 actor tests pass). The harness apps also
  warm the actor at launch with a throwaway background render, paying the 178 MB model load +
  XNNPACK weight packing before the first Speak (the Rust engine loads lazily, so construction
  alone wouldn't). The structural fix (per-chunk streaming, compute proportional to text length)
  remains with the split-graph runtime (next-steps §B).

### Fixed
- **Stage-crate dead-code cleanup (Debt E)** (commit `a584eff`): removed the unused `RwLock`
  import in `prosody.rs`; underscore-prefixed the never-read `worker_thread` JoinHandle field in
  `coordinator.rs` with a comment documenting its keep-alive intent. `cargo check -p stage` is
  warning-free.

### Added
- **Role-based model configuration — `prosodia_models.json` (Debt F resolved)** (commit `577a598`):
  - New declarative config at the Prosodia repo root maps role keys (`actor`, `voices`,
    `director-light`, `director-heavy`) to paths under a `modelsBase`; a relative `modelsBase`
    anchors to the config file's own directory, so no code knows where the workspace lives.
  - New `ProsodiaModels.swift` in both apps (duplicated per the `ProsodiaConfig` pattern):
    read-only `ProsodiaModelsManager` with env-var override (`PROSODIA_MODELS_PATH`), known
    workspace-location discovery, bundled-resource fallback, and an in-code fallback table
    mirroring the committed config.

### Changed
- **`TunerDemo.swift` / `ReaderDemo.swift`**: removed the `#filePath` project-root walks and the
  `styletts2_lite.tflite` / `gemma-4-*` filename literals; `modelsBase`, `resolvedModelPath`, and
  `resolvedVoiceDirectory` now resolve through roles (public surface unchanged).
- **`DirectorModel`** gains an optional `role` key (`id = role ?? path`): role-seeded entries
  re-resolve their path from config every launch, so UserDefaults survive `Models/` restructures
  (trigger: Gemma files moved into `Models/Google/`). Legacy path-persisted entries migrate onto
  role keys by filename match; both prior healing shims (Tuner filename re-resolution, Reader
  `apps/Models` rewrite) deleted.
- **`ReaderContentView.swift`**: the hard-coded default-Director path now resolves via the
  `director-light` role.
- ⚠️ Authored remotely without `xcodebuild`; **verify both app targets at the desktop**
  (`apps/tuner/build.sh`) before relying on the change.

---
## [2026-07-12]

### Changed
- **Export-path priority reversed: `litert-torch` split-graph promoted to Plan A; ONNX→`onnx2tf` monolith demoted to Plan B (fallback).**
  - Trigger: the litert-torch conversion of our own Epoch-199 checkpoint materialized at parity (per-graph corr 1.000000, e2e ≥0.9996 vs torch, human-ear validated), while the split's advantages are structural — host-visible `logw` (enables the wired `duration_scales`/`f0_bias` hooks + the exploit-before-train measurement), no 50-token static limit, per-graph mobile delegate placement, tunable ODE steps, 66 MB fp16.
  - The onnx2tf monolith stays maintained as the fallback and remains the desktop-Tuner audition artifact (`Models/styletts2_lite.tflite`) until the Rust multi-graph runtime (`crates/actor`: three interpreters + host ODE/length-regulator) lands — that runtime is now critical-path work, not an optimization.
  - The `torch → ONNX` stage keeps its role as the numerical oracle for any onnx2tf export; the LiteRT/TFLite **runtime** is unchanged by the reversal (no ONNX Runtime).
  - Docs updated in lockstep: `next-steps.md` (📌 callout, §B tasks, litert-community assessment), `architecture-north-star.md` §4, Sonora `STATE.md` (Current State + roadmap §§1–2), `actor-model-and-training.md`, `high-ambition-1-matcha-actor.md` §3, `high-ambition-5-styletts2-lite.md` header.

---
## [2026-06-19]

### Changed
- **Relocated `Models/` to the workspace root as a shared reference location** (commit `ee580a9`):
  - Moved `Models/` up out of the Prosodia repo to the `Artificial-Humanity` workspace root so multiple subprojects can share it. Large weights (`gemma-4-E2B/E4B-it.litertlm`, `matcha_stock.tflite`, `config.json`) and `StyleTTS2FineTune/` remain gitignored.
  - Repointed the `test_matcha_stock_forward` model path in `engine.rs` from `../../Models/` to `../../../Models/`.
  - Updated `modelsBase` in both `apps/tuner` (`TunerDemo.swift`) and `apps/apple-reader` (`ReaderDemo.swift`) to walk one level above the Prosodia repo root to the workspace root.
  - Removed the now-moot `!Models/Matcha-TTS/` exception from the Prosodia `.gitignore`; updated `Docs/ARCHITECTURE.md` (dropped `Models/` from the tree) and `apps/tuner/README.md`.
  - The locally-modified vendored Matcha-TTS source moved with `Models/` and is now version-controlled in the private `Artificial-Humanity` umbrella repo instead of Prosodia.
- **Improved local Linux and Android builds** (commit `0a952d2`):
  - Updated `build_android.sh` script to align build paths and variables.
  - Updated `LiteRtVocalActor.kt` and `actor.kt` JNI/Kotlin wrappers to include proper exception handling and runtime configuration.
  - Excluded Gradle wrapper build cache binaries and configured executable flag tracking.

---
## [2026-06-18]

### Changed
- **Relocated Matcha-TTS directory into `Models/Matcha-TTS`** (commit `5764060`):
  - Moved the Matcha-TTS sub-directory inside `Models/` for folder structure containment prior to root relayout.
- **Pruned Android build artifacts and fixed Gradle wrapper** (commit `84d6f03`):
  - Excluded dynamic JNI libraries (`jniLibs/arm64-v8a` and `x86_64`) from git tracking.
  - Registered `gradle-wrapper.jar` in Git LFS attributes to prevent repo bloat.

### Fixed
- **Deduplicated unknown phoneme character warnings** (commit `8e2ab8b`):
  - Introduced static `WARNED_PHONEMES` cache using `once_cell::sync::Lazy` and `std::collections::HashSet`.
  - Added helper `warn_unknown_phoneme` to log dropped characters only once to prevent console log flooding.
  - Replaced 4 instances of unconditional `eprintln!` warnings in `pipeline.rs`.
  - Added unit test `test_warn_unknown_phoneme_deduplication`.

### Added
- **Locked training vocabulary for custom Matcha-TTS models** (commit `7143617`):
  - Deduplicated the apostrophe symbol `'` and added the modifier letter schwa `ᵊ` in `symbols.py` (Matcha submodule) and `config.json` to yield exactly 178 unique symbols.
  - Implemented `is_matcha_ipa` support in `config.json` and `pipeline.rs` to allow custom direct Matcha models to bypass IPA conversions (`map_styletts2_to_matcha_ipa`) at runtime.
  - Added unit test `test_custom_matcha_direct_tokenization` in `pipeline.rs`.
- **Integrated dynamic native sample rate configurations** (commit `7143617`):
  - Added `"sample_rate": 24000` to `config.json`.
  - Implemented `get_model_sample_rate` inside `engine.rs` to extract native sample rate from adjacent config.
  - Dynamic linear resampling bypass in `forward_impl` if the model natively outputs `24000` Hz.
  - Added unit tests `test_get_model_sample_rate_fallback` and `test_get_model_sample_rate_from_config` in `engine.rs`.

## [2026-06-17]

### Changed
- **Tracked binary build artifacts with Git LFS** (commit `c418940`):
  - Added Git LFS tracking pattern constraints in `.gitattributes` for binary build assets (including `.so`, `.dylib`, `.a`, `.jar`, `.framework`).
  - Transitioned prebuilt and generated library artifacts to LFS pointers, reducing active repository size.

### Added
- **Addressed Technical Debt Item A: Thread emotion/acoustic parameters into Actor Engine** (commit `60acd01`):
  - Extended the `ProsodiaSpeechEngine` UniFFI trait and its implementation to accept `vat: Option<Vec<f32>>` in the `forward` method.
  - Dynamically resize and copy `duration_scales` and `f0_bias` into StyleTTS2 input tensors in `forward_impl` if they are detected in the loaded TFLite model.
  - Scaled output `pred_dur` values by `duration_scales` in `forward_impl`.
  - Exposed `tokenize_phonemes` on `ProsodiaActorPipeline` in `pipeline.rs`.
  - Refactored `process_and_synthesize` in `engine.rs` to extract parameters and call `forward` directly, avoiding the deprecated panicking `synthesize` path.
  - Aligned Swift FFI wrapper (`SwiftSpeechEngine`) and Android Kotlin wrapper (`KotlinSpeechEngine`) signatures.
- **Addressed Technical Debt Item B: Centralize configuration parameters and clean magic numbers** (commit `86e4755`):
  - Defined `DEFAULT_TOKEN_DURATION` and `DEFAULT_VAT` constants in `engine.rs` to clean up inline placeholders from hot inference paths.
  - Added `sample_rate` to `AcousticMatrixState` and `AcousticMatrixConfig` (defaulting to `24000`) inside `acoustic_matrix.rs`.
  - Refactored `get_sample_rate()` in `acoustic_matrix.rs` to query the state dynamically.
  - Updated C# `WasapiExclusivePlayer.cs` on Windows to query `get_sample_rate` dynamically via P/Invoke instead of hardcoding `24000`.
- **Addressed Technical Debt Item C: Desktop Audio Sinks Build integration & Wiring** (commit `9606ce7`):
  - Moved Linux sound hook C files into `platforms/linux/src/` and C# player file into `platforms/windows/src/` to adhere to Monorepo topology.
  - Created `platforms/linux/Cargo.toml` and a `build.rs` to conditionally compile either `audio_sink_alsa.c` or `audio_sink_pulse.c` depending on cargo features `alsa` / `pulse` when building on Linux.
  - Implemented `LinuxAudioSink` and a binary target daemon in `platforms/linux/src/main.rs`.
  - Added C# `platforms/windows/ProsodiaWin.csproj` target configurations.
  - Wired `platforms/linux` into workspace members in the top-level `Cargo.toml`.
- **Completed Option 1: Phase 0 (Untuned Matcha-TTS Discovery Spike & Contract Lock)** (commit `888c401`):
  - Validated custom TFLite export via `onnx2tf` on `model_e2e.onnx` successfully.
  - Copied the newly compiled model to `Models/matcha_stock.tflite` and verified execution via `cargo test -p actor`.
  - Exported `is_matcha` from the Rust `LiteRtActorEngine` struct to the UniFFI bridge to allow client-side detection.
  - Aligned Swift FFI backend protocol (`ProsodiaActorBackend`) by adding `isMatcha()` and `getTokenLimit()` with default extension implementations.
  - Implemented the delegated `isMatcha()` and `getTokenLimit()` methods in Swift wrappers `SwiftSpeechEngine` and `LiteRtActorEngine` to resolve compile errors and ensure correct FFI round-trip execution.
  - Verified building the macOS `ProsodiaTuner` app harness successfully using `xcodebuild`.
  - Ignored `model_e2e_test_tflite/` directory in `Models/Matcha-TTS/.gitignore`.

### Fixed
- **Resolved code review findings MN1, MN2, MN3, and LN2** (commit `d36e2ee`):
  - Propagated error result from `process_and_synthesize` in `engine.rs` instead of swallowing into silent empty audio.
  - Wrapped `processAndSynthesize` calls in try-catch/do-catch blocks inside Apple `Providers.swift` and Android `LiteRtVocalActor.kt` to catch and log errors.
  - Aligned WASAPI C# `RustCallStatus` and `RustBuffer` struct layouts in `WasapiExclusivePlayer.cs` to prevent stack corruption on error paths.
  - Ungated `LinuxAudioSink` and its `AudioSink` impl block in `platforms/linux/src/main.rs`, conditionalizing function bodies on `#[cfg(target_os = "linux")]` to enable compilation and type-checking on macOS dev machines.
  - Added build-time conflict check in `platforms/linux/build.rs` to panic if both `alsa` and `pulse` features are enabled simultaneously.
- **Addressed stock Matcha-TTS & TFLite bindings code review findings** (commit `60abd51`):
  - Expose C-API `TfLiteTensorType` and standard type constants (`kTfLiteInt32`, `kTfLiteInt64`) in `tflite.rs` to allow dynamic tensor data type querying.
  - Replaced the byte-size dtype guessing heuristic in `engine.rs` with robust data type checking using `TfLiteTensorType`.
  - Prevented silent truncation by returning a clear `SpeechEngineError::Inference` error if input token counts exceed the model's static limit in the Matcha path.
  - Refactored `get_token_limit` in `engine.rs` to dynamically scan input tensors by name (`x`, `phone`, `input_ids`, or `text`) rather than hardcoding index `0`.
  - Consolidated divergent IPA phoneme mapping logic in `pipeline.rs` into a unified `map_char_to_matcha_ipa` character mapper consumed by both the pipeline and alignment layers.
  - Stopped silently dropping unrecognized phonemes; added `eprintln!` warnings in `tokenize()` and the alignment block.
  - Cached `is_matcha` and `token_limit` checks outside loops across all pipeline synthesis entry points to prevent redundant mutex locks on the engine.
  - Wired dynamic token limits (`speech_engine.get_token_limit()`) into the `chunk_tokens()` and `chunk_phonemes()` calls in `pipeline.rs` instead of hardcoding `510`.
  - Added CFM temperature `0.667` and frame hop size `512` as named constants (`MATCHA_CFM_TEMPERATURE` and `STYLETTS2_HOP_SIZE` respectively) in `engine.rs`.
  - Configured git to ignore local generated and temporary assets under the `Models/Matcha-TTS` directory by extending `Models/Matcha-TTS/.gitignore`.

## [2026-06-16]

### Added
- **Integrated stock Matcha-TTS into Prosodia Rust actor speech engine** (commit `a411e80`):
  - Detected stock Matcha-TTS model inside `LiteRtActorEngine` by matching input tensor names (`x`, `x_lengths`, `scales`).
  - Added support for static input padding in `forward_impl` to match the compiled static shapes of the TFLite model, avoiding TF MLIR dynamic reshape propagation failures.
  - Implemented linear resampling from 22.05 kHz (Matcha output) to 24 kHz (Prosodia standard) inside the engine.
  - Estimated output durations by distributing the audio frame count evenly across the input phonemes, aligning word timestamps correctly.
  - Added a phonetic G2P mapping helper `map_styletts2_to_matcha_ipa` in `pipeline.rs` to convert StyleTTS2-compatible phonetic symbols to standard espeak-IPA digraphs.
  - Integrated `is_matcha` and `get_token_limit` methods on the `ProsodiaSpeechEngine` trait and `LiteRtActorEngine` to dynamically configure pipeline token chunking.
  - Added a unit test `test_matcha_stock_forward` to execute and verify forward pass correctness of the `matcha_stock.tflite` model.

### Changed
- **Consolidated & sequenced the `Notes/` folder** (14 → 11 files):

  - Numbered the high-ambition notes by necessary sequence, each with a sequence banner linking the chain: `high-ambition-1-matcha-actor.md` (the chosen first actor model — **new**) → `-2-dramatic-reader.md` → `-3-child-voices.md` → `-4-multilingual-g2p.md` → `-5-styletts2-lite.md`. Following the decision to ship **Matcha-TTS first**, the StyleTTS2-Lite note moved from slot 1 to slot 5 (the optional higher-ceiling re-platform), and the in-between notes were reframed to be base-aware (StyleTTS2 style-vector design vs Matcha speaker-embedding + VAT/FiLM conditioning).
  - Merged `actor-model-selection.md` + `actor-training-guide.md` → **`actor-model-and-training.md`**.
  - Merged `voicing-and-synthesis.md` + `tuner-feedback-and-calibration.md` → **`voicing-synthesis-and-tuning.md`**.
  - Merged `immediate-next-steps.md` + `technical-debt-and-followups.md` + the open code-review items → **`next-steps.md`**, the single "what do we work on next?" entrypoint (short answer: train the actor model).
  - Renamed the standalone code review to the new AGENTS.md §5 convention `code-review-20260616-125000.md` and added its evaluated commit range (start `f486b58` … end `70cf0dc`) so the next review knows to resume at `71813f1`; its findings were addressed in `71813f1` and are also tracked in `next-steps.md`. Refreshed `STATE.md` pointers and all cross-links; corrected the stale `ai-edge-torch` "Gold Standard" framing in the StyleTTS2-Lite note to point at the `torch → ONNX` decision.

### Fixed
- **Hardened Windows WASAPI Exclusive-Mode Player**:
  - Added strict COM HRESULT verification on all `IAudioClient`, `IAudioRenderClient`, `IMMDeviceEnumerator`, and `IMMDevice` API methods.
  - Implemented `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` (0x88890019) retry handling inside `InitializeWasapi()` to re-activate and initialize with a hardware-aligned buffer size.
  - Resolved mono-to-stereo playback mismatch on fallback devices by implementing on-the-fly 24 kHz mono to 48 kHz stereo linear interpolation upsampling and channel duplication.
  - Ordered initial silence buffer pre-filling in `Play()` before starting the client to prevent initial clock event glitches or underruns.
  - Reset active buffer frames, offsets, and cleared the concurrent audio queue inside `Stop()` to ensure consecutive playbacks start cleanly.
  - Cleaned up unused MMDeviceEnumerator COM classes and ensured closest match COM pointers are freed properly during initialization checks.
- **Removed Stale Git Submodule Declaration**:
  - Removed the orphaned `Notes` submodule configuration from `Prosodia/.gitmodules` now that it resides as a standalone repository adjacent to `Prosodia` in the monorepo root.
  - Added Android build cache `.gradle/` directories to `Prosodia/.gitignore`.
- **Hardened G2P Binary Lexicon Loading**:
  - Prepended `PSL1` (Silver) and `PGL1` (Gold) magic byte signatures to compiled G2P lexicons in `build.rs` to detect corrupt assets early.
  - Added static bounds and variant count `try_from().expect(...)` checks during build-time serialization to prevent silent `u16`/`u32` truncations.
  - Implemented load-time `validate()` checks in `crates/actor/src/lexicon.rs` to assert correct magic bytes, length matches, and bounds constraints during `Lexicon::new()` instantiation.

### Changed
- **Matcha-TTS Relocation & Setup**:
  - Relocated the `Matcha-TTS` repository from the `External/` directory directly into the `Prosodia` repository (`Prosodia/Models/Matcha-TTS`) for layout consistency and self-containment.
  - Re-linked and compiled its Cython extensions inside the active Python 3.12 environment in editable mode.
  - Incorporated the detailed Phase 0 (Untuned Matcha Discovery Spike) steps and contract lock checklist into the roadmap in `Notes/next-steps.md`.

### Added
- **Build-Time Compiled Binary Lexicon Maps**:
  - Replaced runtime JSON loading and parsing of the 12.6 MB G2P gold and silver lexicons with build-time compilation.
  - Implemented binary serialization logic in `crates/actor/build.rs` to parse, grow (capitalize), sort, and pack dictionaries into compact binary files in `OUT_DIR`.
  - Added `BinGoldMap` and `BinSilverMap` wrappers in `crates/actor/src/lexicon.rs` to query the static binary slices directly using zero-allocation, zero-copy binary search.
  - Added perfect alignment padding and tag mapping optimizations to save peak memory footprint on mobile devices.
  - Configured `serde` and `serde_json` as build-dependencies in `crates/actor/Cargo.toml` to completely isolate runtime builds from JSON-parsing overhead.
- **Linux Platform Audio Sink Scaffolding**:
  - Implemented PulseAudio Simple API sink in `platforms/linux/audio_sink_pulse.c` for low-latency floating-point PCM audio playback.
  - Implemented ALSA PCM audio sink in `platforms/linux/audio_sink_alsa.c` for direct hardware audio scheduling with automatic underrun recovery.
  - Added clean C-API public header declarations in `platforms/linux/audio_sink.h` to allow Linux daemon binaries to link and render PCM audio streams.
- **Windows Platform Audio Sink Scaffolding**:
  - Implemented low-latency WASAPI Exclusive-Mode playback in C# under `platforms/windows/WasapiExclusivePlayer.cs`.
  - Configured high-priority audio render thread loops, event-driven hardware callback scheduling (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`), and direct COM interop interfaces (`IMMDeviceEnumerator`, `IMMDevice`, `IAudioClient`, `IAudioRenderClient`) to bypass the Windows Audio Engine mixer for minimum latency.
- **ONNX Export and TFLite Conversion Validation**:
  - Resolved legacy ONNX JIT tracing errors regarding gradient-tracking variables by detaching all submodule parameters (`requires_grad = False`) and wrapping the export in `with torch.no_grad():`.
  - Patched `nn.InstanceNorm1d` and `nn.InstanceNorm2d` with custom traceable implementations in `export_onnx.py` using standard math operators (Mean, Var, Sqrt, Mul, Add). This bypasses the ONNX exporter's limitation regarding unknown channel sizes and generates a clean, highly compatible operator graph for mobile converters.
  - Successfully exported StyleTTS2-Lite to `styletts2_lite.onnx` (572 MB).
  - Proven the `onnx2tf` conversion pipeline end-to-end, converting over 4,100 operators successfully on CPU before hitting sandboxed task timeouts.

## [2026-06-15]

### Added
- **Centralized Sample Rate via UniFFI FFI Boundary**:
  - Exposed Rust-side audio sample rate dynamically using new `#[uniffi::export] pub fn get_sample_rate() -> u32` in `crates/stage/src/acoustic_matrix.rs`.
  - Refactored Swift platform layers (`StageCoordinator.swift`, `Providers.swift`, `Services.swift`, `main.swift`) to retrieve the default sample rate dynamically from `Kit.getSampleRate()`.
  - Refactored Kotlin platform layers (`StageCoordinator.kt`, `LiteRtVocalActor.kt`, `StageAudioSink.kt`) to utilize `uniffi.stage.getSampleRate()`.

### Changed
- **Aligned Configuration & Acoustic Matrix Calibration Defaults**:
  - Aligned `AcousticMatrixState` default values in Rust core (`crates/stage/src/acoustic_matrix.rs`) to match calibrated settings (`expressiveness: 3.25`, `speed_tension_gain: 0.10`, `speed_valence_gain: 0.05`, `gain_arousal_gain: 0.25`, `gain_valence_gain: 0.08`).
  - Aligned `PhrasePauseState` default pause thresholds (`sentence: 0.28`, `clause: 0.25`) in `audio_shaping.rs` to prevent mismatched values.
- **Fixed VoiceDownloader URL Endpoint**:
  - Updated default `remoteBaseURL` from the dead `hexgrad/StyleTTS2-Lite` endpoint to `https://huggingface.co/artificial-humanity/StyleTTS2-Lite/resolve/main/voices/` in Swift `VoiceDownloader.swift` to resolve HTTP 404 download failures.

## [2026-06-14]

### Fixed
- **Tuner Playback "Droning Tone" (Honest Missing-Model State)**:
  - Diagnosed the report that pressing a passage's Play button produced a short drone instead of speech: the StyleTTS2 actor model (`Models/styletts2_lite.tflite` + companion `config.json` + voice `.safetensors`) is absent, so `LiteRtVocalActorProvider.canHandle` returns false and `ProductionRunner.getActor()` fell back to a **non-silent** `StubVocalActor`, whose `render(payload:)` emits a hardcoded 1.0 s / 440 Hz sine tone. That placeholder tone was the "droning sound"; the Rust forward/TFLite/`StageAudioSink` path downstream was never reached.
  - Made `ProductionRunner.getActor()` return `(any VocalActor)?` — it now yields `nil` rather than the audible stub, and caches **only** a genuinely resolved actor (so dropping the model into `Models/` is picked up without an app relaunch).
  - Replaced the hardcoded `canSpeak { true }` with a real check via the new `VocalActorRegistry.canMakeActor(for:)`, which consults providers' lightweight `canHandle` file checks without constructing an actor. When the model is missing, every Speak control is now disabled and the section footer surfaces the existing "Add the StyleTTS2 actor model under /Models to enable Speak" guidance instead of playing a misleading tone. Both `speak` and `speakPassage` additionally guard on a resolved actor.
  - Verified: `swift build` (ApplePlatform package) and `xcodebuild -scheme ProsodiaTuner` both **BUILD SUCCEEDED**.
  - Generated the actor `Models/config.json` (`{"vocab": …}`) from the canonical StyleTTS2 symbol table (`StyleTTS2/text_utils.py`): 177 symbols, indices 0–176 (model `n_token: 178` is the standard one-spare-row convention, not an off-by-one). This is one of the three assets `LiteRtVocalActorProvider.canHandle` needs; it is gitignored (under `Models/`) and stays local. The remaining blockers — the trained/`ai-edge-torch`-exported `styletts2_lite.tflite` and `anchor_*` voice `.safetensors` — require a GPU pod and are captured with a runbook + the exact Rust↔TFLite I/O contract in the new follow-ups note.
- **App Launcher Model Path & try! Config Crash**:
  - Added a fourth `deletingLastPathComponent()` call to `projectRoot` resolution in `TunerDemo.swift` and `ReaderDemo.swift` to prevent them from resolving to the `apps` subdirectory instead of the actual monorepo root.
  - Added file-existence checks for both the `.tflite` model and companion `config.json` file to `LiteRtVocalActorProvider.canHandle(modelURL:)` in `Providers.swift`. This avoids raising unexpected `try!` cocoa domain file exceptions during instantiation, allowing the app to gracefully fallback to `StubVocalActor` when the weights are not found on disk.
  - Added self-healing path correction in `DirectorModelStore.init()` to automatically update stale `UserDefaults` model paths containing `apps/Models` to point to the correct root `Models` folder.
- **Android Gradle Build & Kotlin FFI Compilation**:
  - Renamed the `message` field to `msg` across all Rust `uniffi::Error` enums (`PipelineError`, `SpeechEngineError`, `VoiceLoaderError`, `FolioParserError`, `TokenizerError`) and their constructors. This resolves a Kotlin FFI compiler conflict on Android where constructor-generated properties clashed with the overridden JVM `Exception.message` property.
  - Implemented `DiskVoiceAssetProvider` in Kotlin and bridged `ProsodiaSpeech` to `ProsodiaG2pProcessor` using an anonymous object in `LiteRtVocalActor.kt`.
  - Prefix-qualified `EmotionVector`, `CastingProfile`, `ProsodyAcoustics`, and `ProsodySpan` using `uniffi.stage` in `LiteRtVocalActor.kt`.
  - Changed the secondary constructor of `InMemoryBookDocument` in `Services.kt` to take `Iterable<String>` instead of `List<String>`, resolving the JVM signature type-erasure clash with `List<BookChapter>`.
  - Corrected signed integer literal `24000` to unsigned `24000u` in `StageCoordinator.kt` and `LiteRtVocalActor.kt`.
  - Fixed syntax error in `MainActivity.kt` where `Color(0 PallidSlateDarkBg)` was used by replacing it with `Color(0xFF0C0E12)`.
- **macOS/iOS Dynamic Linking (`dyld`)**:
  - Disabled library validation (`com.apple.security.cs.disable-library-validation` entitlement) in `Harness-Debug.entitlements` for both `ProsodiaTuner` and `AppleReader` app targets. This allows macOS to load dynamic libraries (like `libfolioparser.dylib` built by Cargo) that do not share the Team ID of the developer certificate used to sign the main application during local debugging.

### Added
- **Lookahead & Backpressure Buffer (Option 2)**:
  - Implemented a bounded asynchronous lookahead pre-rendering driver in `crates/stage/src/coordinator.rs` to run Director and Actor synthesis in the background.
  - Configured a thread-safe `pre_rendered_queue` protected by a `Mutex` and `Condvar` to buffer upcoming audio chunks and mitigate playback stuttering.
  - Implemented producer backpressure to block the pre-rendering loop when the lookahead limit is reached, waking up automatically when a chunk is consumed.
  - Preserved a lookahead limit of `0` to run the original synchronous inline rendering pipeline, ensuring 100% backwards compatibility.
  - Wrote a unit test (`test_lookahead_rendering`) in `crates/stage/src/coordinator.rs` to verify queue limits, producer blocking, and consumption-driven wakeups.
  - Re-compiled all Android/iOS targets, regenerated FFI bindings, and confirmed that both reader applications build successfully (**BUILD SUCCEEDED**).
- **Access-Ordered LRU Eviction Cache Policy (Option 1)**:
  - Upgraded the `VoiceCache` struct in `crates/actor/src/voice_loader.rs` to track and maintain access order (Least Recently Used) of loaded voice packs.
  - Implemented MRU positioning upon cache read (`get`) and write (`insert`) operations.
  - Ensured that new insertions evict the oldest voice when the cache limit of 16 is exceeded.
  - Added the unit test `test_lru_cache_eviction` in `crates/actor/src/voice_loader.rs` to verify correct LRU ordering, MRU updates on read hits, and proper eviction behavior.
- **Dynamic Parametric Voicing Grid (Next-Gen Voicing)**:
  - Implemented continuous bilinear interpolation for voice identity embeddings based on `age_profile` and `masculinity` in the `VoiceLoader`.
  - Added support for style texture blending (e.g. `strain_or_rasp` mapping to the `anchor_style_gruff` vector).
  - Wrote a unit test (`test_resolve_parametric_voice`) in `crates/actor/src/voice_loader.rs` verifying the bilinear LERP and texture blending math.
  - Updated `ProsodiaActorPipeline::process_span` to resolve casting profiles using `VoiceLoader` instead of returning a dummy style.
  - Implemented robust tag parsing in `crates/stage/src/prosody_payload.rs` to extract all continuous acoustic and casting profile parameters (`AG`, `MA`, `ST`, `S`, `G`, etc.) from raw spans.
  - Added a span decoding unit test (`test_decode_spans_with_acoustics`) in `crates/stage/src/prosody_payload.rs`.
  - Regenerated all Kotlin and Swift UniFFI bindings and verified that all host and cross-compiled platforms compile cleanly (**BUILD SUCCEEDED**).
- **LiteRT-LM Gemma Director Port (Phase 2)**:
  - Replaced the mock `tag_passage` implementation in `crates/director/src/lib.rs` with real inference execution of the Gemma 4 model using the dynamic C-API of `libCLiteRTLM_mac.dylib`.
  - Configured prompt templates, rolling narrative context, and session options directly in the Rust core.
  - Overwrote Swift's `LiteRtLmDirector.swift` to delegate to Rust's FFI-backed `GemmaDirector` and pruned `DirectorPrompt.swift`.
- **LiteRT StyleTTS2 Inference Engine Port (Phase 2)**:
  - Implemented raw TensorFlow Lite C-API bindings in `crates/actor/src/tflite.rs` (mapping model loading, interpreter creation, tensor resizing, buffer copies, and interpreter execution).
  - Ported the entire StyleTTS2 execution loop from the Swift layer into `crates/actor/src/engine.rs` to perform on-device acoustic matrix inference.
  - Updated `platforms/apple/Sources/Actor/Engine/LiteRtActorEngine.swift` to act as a thin wrapper delegating to the unified Rust implementation.
- **Relocated Lexicon Resources in Rust Workspace**:
  - Copied all core JSON lexicon resource dictionaries (`us_gold.json`, `us_silver.json`, `gb_gold.json`, `gb_silver.json`) from `platforms/apple/Sources/Misaki/Resources/` to a dedicated `crates/actor/resources/` subdirectory in the Rust workspace.
  - Updated path mappings inside `crates/actor/src/lexicon.rs` to load from the crate-local resources directory, ensuring compile-time safety and independence of platform assets.
- **Android Platform Scaffolding & NDK Cross-Compilation (Phase 3)**:
  - Created `build_android.sh` to automate target compilation for Android `aarch64-linux-android` (arm64-v8a) and `x86_64-linux-android` (x86_64).
  - Configured gradle build setup (`build.gradle.kts`, `settings.gradle.kts`, `local.properties`, `gradle.properties`) under `platforms/android` to establish a standalone library module packaging JNI binaries and Kotlin UniFFI bindings.
  - Implemented Kotlin `StageAudioSink` wrapping Android's `AudioTrack` playing float PCM arrays with natural blocking backpressure.
  - Implemented Kotlin `StageCoordinator` mapping `NarrationSourceAdapter` and running a coroutine loop to pull and play audio chunks from the Rust core.
  - Implemented Kotlin wrappers `LiteRtLmDirector` and `LiteRtVocalActor` in parallel with Apple platforms.
- **Conditional Target-OS linking in Rust Crates**:
  - Updated `crates/actor/build.rs` and `crates/director/build.rs` to only link `CLiteRTLM_mac` when target OS is macOS/iOS, and to pass NDK linker flags allowing undefined symbols (`-Wl,-z,undefs`) when target OS is Android, resolving cross-compilation linking failures.
- **Apple Reader Application Integration (Phase 3)**:
  - Scaffolding the `AppleReader` target under `apps/apple-reader` by leveraging the Xcode project configurations of the Tuner scheme.
  - Implemented a premium, glassmorphic SwiftUI reader view ([ReaderContentView.swift](../../Prosodia/apps/apple-reader/AppleReader/ReaderContentView.swift)) featuring sidebar chapter navigation, typography and theme selection, and inline active sentence highlight synced to real-time playback.
  - Connected the reading view to the `StageCoordinator` and `BookDocument` FFI interfaces to run the end-to-end dramatic narration pipeline (Director + Actor) offline.
  - Verified compilation via `xcodebuild` where the application target builds successfully with **BUILD SUCCEEDED**.
- **Android Jetpack Compose Reader App (Phase 4)**:
  - Scaffolded the Kotlin Jetpack Compose application under `apps/android-reader` as a standalone Gradle project.
  - Configured `settings.gradle.kts` to dynamically reference the local `:platforms:android` library module.
  - Implemented a premium, glassmorphic UI using Compose featuring background gradients, card layouts, sidebar chapter navigation drawer, theme selection, font size/speed adjusters, and active sentence highlighting.
  - Provided robust offline stub fallbacks (`StubDirectorInference` and `StubVocalActor`) that automatically activate if local model file paths are not found on-device, matching the iOS fallback design.
  - Integrated `uniffi.stage.SentenceSegmenter` and `uniffi.folioparser.parseEpub` to split text sentences and parse EPUB files.

### Changed
- **Transitioned Apple apps to TFLite pathing & StubVocalActor fallback**:
  - Replaced legacy MLX checkpoint (`epochs_2nd.pth`) path definitions in `TunerDemo.swift` and `ReaderDemo.swift` with standard LiteRT TFLite model paths (`styletts2_lite.tflite` under `Models/`).
  - Redefined `canSpeak` in `TunerDemo.swift` and `ReaderDemo.swift` to always evaluate to `true` (since the compile-time `StubVocalActor` fallback is always available for narration pipeline testing).
  - Updated `ReaderContentView.swift` to resolve the actor using the static `resolvedModelPath` and `resolvedVoiceDirectory` paths, using `VocalActorRegistry.shared.makeActor` to fallback to `StubVocalActor` gracefully when the real weights are absent.
  - Replaced the silent 0.1-second zero-filled array output in `StubVocalActor.render` with a soft 1.0-second 440 Hz sine wave tone, providing clear audible playback feedback during debugging in stub mode.
- **Pruned Legacy Swift G2P Targets and Tokenizer**:
  - Deleted obsolete G2P engines (`Sources/Misaki` and `Sources/ActorEspeak`) and the unused BPE Tokenizer (`Sources/Stage/NativeBPETokenizer.swift`) on the Apple platform layer.
  - Removed target dependencies and binary linking setups for `Misaki` and `ActorEspeak` in `platforms/apple/Package.swift` and `apps/tuner/ProsodiaTuner.xcodeproj/project.pbxproj`.
  - Removed the external `espeak-ng` Swift Package Manager dependency, completely eliminating GPLv3-licensed source code from the Apple platform build scope.
  - Rewired `ProsodiaActor` target wrapper (`ProsodiaActor.swift`) to export `Kit` instead of `Misaki`.
  - Updated the downstream Tuner application main entry point (`ProsodiaTunerApp.swift`) to drop legacy G2P fallback overrides, relying entirely on the native Rust-side `ProsodiaSpeech` FFI G2P engine.
  - Updated `PROJECT_TOPOLOGY.md` to reflect the removal of the pruned G2P folders.

- **BPE Tokenizer in Rust (`prosodia-core`)**:
  - Created a new shared [crates/core](../../Prosodia/crates/core) crate for core token manipulation structures.
  - Implemented a pure-Rust, zero-dependency BPE (Byte-Pair Encoding) tokenizer in [crates/core/src/lib.rs](../../Prosodia/crates/core/src/lib.rs) that parses the binary `.pvocab` format, builds standard GPT-2 byte-to-unicode maps, and performs greedy BPE merging.
  - Wrapped and exposed the tokenizer as a UniFFI Object in [crates/stage/src/tokenizer.rs](../../Prosodia/crates/stage/src/tokenizer.rs) (exported via `stageFFI` bindings) to replace the legacy Swift BPE tokenizer.
- **Deep macOS Framework Packaging**:
  - Modified `build_frameworks.sh` to package dynamic xcframework binaries using standard macOS deep bundles (incorporating `Versions/A/` directory layouts and symlinks) instead of flat shallow bundles. This resolves `xcodebuild` validation utility failures on macOS.

### Changed
- **Rust-Side Phoneme Tokenization**:
  - Shifted phoneme-to-index mapping from the Swift engine to the Rust core pipeline.
  - Updated the `ProsodiaSpeechEngine::forward` trait signature in [crates/actor/src/engine.rs](../../Prosodia/crates/actor/src/engine.rs) to accept pre-tokenized `phoneme_ids: Vec<i32>` instead of `phonemes: String`.
  - Updated [crates/actor/src/pipeline.rs](../../Prosodia/crates/actor/src/pipeline.rs) to tokenize phoneme strings internally before calling the speech engine.
  - Simplified Swift's [LiteRtActorEngine.swift](../../Prosodia/platforms/apple/Sources/Actor/Engine/LiteRtActorEngine.swift) and [ProsodiaActorBackend.swift](../../Prosodia/platforms/apple/Sources/Actor/Engine/ProsodiaActorBackend.swift) by removing vocab parsing, storage, and string tokenization, allowing them to copy the pre-tokenized arrays directly into TFLite input tensors.
- **Stage Coordinator Lookahead Default & Swift Wiring**:
  - Changed default lookahead limit in Rust `StageCoordinator::new()` constructor from `4` to `0` to ensure 100% thread-free synchronous execution by default.
  - Wired the Swift `lookahead` argument in `StageCoordinator.run` to `StageCoordinator.newWithLookahead` instead of silently dropping it, and updated documentation.
- **Synchronous GemmaDirector FFI & Unused Dependencies**:
  - Made the `GemmaDirector` FFI methods (`tag_passage`, `generate_inference`, `get_or_init_engine`) synchronous in Rust, avoiding thread-blocking issues under UniFFI's foreign executor.
  - Simplified the Swift `LiteRtLmDirector` implementation to execute `tagPassage` synchronously on the calling thread, removing `Task` and `DispatchSemaphore` overhead.
  - Removed unused `tokio` dependency from `crates/director/Cargo.toml` and `crates/actor/Cargo.toml`.
- **Xcode Project Scheme Cleanups**:
  - Renamed the shared Xcode scheme in the `apple-reader` project from `ProsodiaTuner.xcscheme` to `AppleReader.xcscheme` and updated all internal target, product, and container references.

### Fixed
- **Swift Package Target Linkage & Conformance**:
  - Fixed missing conformance to `DirectorInference` on `StubDirectorInference` in [Services.swift](../../Prosodia/platforms/apple/Sources/Stage/Services.swift) by implementing `reclaimMemory()` and the synchronous FFI callback method `annotate(passage:)`.
  - Added `"Audio"` target dependency to `"Stage"` target in [Package.swift](../../Prosodia/platforms/apple/Package.swift) and imported `Audio` in [StageCoordinator.swift](../../Prosodia/platforms/apple/Sources/Stage/StageCoordinator.swift), resolving missing type errors for `StageAudioSink`.
  - Added `@preconcurrency import Kit` at the top of platform files to suppress FFI-level Sendable compiler warnings.
- **Tuner App Compilation**:
  - Disambiguated overlapping generated FFI classes and local Swift protocols (`VocalActor`, `DirectorInference`, `NarrationMode`, and `StageCoordinator`) in [TunerDemo.swift](../../Prosodia/apps/tuner/ProsodiaTuner/TunerDemo.swift) and [AuditionConfiguration.swift](../../Prosodia/apps/tuner/ProsodiaTuner/AuditionConfiguration.swift) using explicit `Stage.` namespace prefixes.
  - Restored the accidentally deleted `AuditionPreset.from` method signature in [AuditionConfiguration.swift](../../Prosodia/apps/tuner/ProsodiaTuner/AuditionConfiguration.swift).
  - Updated [TunerContentView.swift](../../Prosodia/apps/tuner/ProsodiaTuner/TunerContentView.swift) to parse and log the new continuous `CastingProfile` parameters (`ageProfile`, `masculinity`, `strainOrRasp`) instead of the legacy discrete voice entries, enabling the Tuner Xcode project to achieve **BUILD SUCCEEDED**!
- **Flaky Lookahead Rendering Test**:
  - Rewrote `test_lookahead_rendering` to lock coordinator state and wait on the internal `Condvar` for queue length updates instead of relying on timing-dependent `sleep(50ms)` calls.
- **Document & Comment Drift**:
  - Corrected `FIFO eviction` comment to `LRU eviction` in `crates/actor/src/voice_loader.rs`.
  - Updated lookahead and LRU cache status in `Notes/unported-logic.md` to reflect their implementation.
- **Swift 6 Concurrency Warnings**:
  - Marked the `rustDirector` property in `LiteRtLmDirector` as `nonisolated` to resolve actor isolation warnings in Swift 6 language mode.
- **Consolidated Internal Engineering Documentation**:
  - Reorganized and merged 12 miscellaneous notes files into 3 topic-based consolidated documents: `architecture-and-development.md`, `voicing-and-synthesis.md`, and `tuner-feedback-and-calibration.md`. This dramatically reduces documentation clutter in the `Notes/` folder.
  - Audited and updated the capitalized `STATE.md` document to reflect the resolved audit items and point to the new consolidated files.
  - Created a new target-checklist document `immediate-next-steps.md` to track immediate action items following the audit resolution.

### Removed
- **Removed Obsolete Notes & Findings**:
  - Deleted `code-review-findings.md` now that all audit findings have been resolved.
  - Deleted the 12 miscellaneous note source files following their successful consolidation.

---

## [2026-06-13]

### Added
- **Custom ZIP/EPUB Reader in Rust**:
  - Implemented a custom, zero-dependency ZIP archive reader in `crates/folioparser/src/zip_reader.rs` to parse the End-of-Central-Directory and scan entries on demand without loading the entire archive.
  - Integrated `miniz_oxide` for pure-Rust DEFLATE decompression, removing the heavy, feature-rich external `zip` crate from the workspace dependencies to simplify cross-platform compilation.
  - Updated `parse_epub` to use the new lightweight reader while maintaining identical FFI signatures and UniFFI compatibility.
- **Changelog**: Introduced this `changelog.md` file to track project adjustments.

### Changed
- **Renamed `FolioParser.swift` to `AppleFolioParser.swift`**:
  - Moved `platforms/apple/Sources/Kit/FolioParser/FolioParser.swift` to `platforms/apple/Sources/Kit/FolioParser/AppleFolioParser.swift`.
  - This resolves a critical macOS case-insensitivity collision with UniFFI-generated `folioparser.swift` (both previously resolved to `folioparser.swift.o` in the build directory, throwing 300 duplicate Swift symbol errors during compilation).
- **Overview Documentation Retailored**:
  - Overwrote `Notes/Overview.md` to properly document the completed 5-stage monorepo refactoring and Rust core migration.
  - Unified all workspace documentation paths to relative Markdown links.
  - Integrated Phase 2 (StyleTTS2-Lite Training) and Phase 3 (Dramatic Reader) roadmap tables, justifications, and ordering strategies.
- **Dynamic Framework Packaging**:
  - Swapped static libraries (`.a`) for dynamic frameworks (`.dylib`) in `build_frameworks.sh` to prevent massive intermediate duplicate symbol collisions in nested workspace dependency linkings.

### Fixed
- **Actor Unit Test Compilation**:
  - Fixed missing imports of `ActorEngineOutput` and `SpeechEngineError` inside the test module of `crates/actor/src/pipeline.rs`, allowing the full `cargo test` suite to run green.

### Removed
- **Deleted `Review/` Directory**:
  - Completely removed the obsolete `Notes/Review` folder and its remaining files (such as `high-ambition-goals.md`) now that their applicable contents have been fully migrated.
- **Legacy Swift Engine Orchestration**:
  - Cleaned up and deleted deprecated Swift files: `VoiceLoader.swift`, `ProsodiaActorPipeline.swift`, `ProsodyMarkupParser.swift`, and `ProsodiaG2PProcessor.swift`.

### Added (historical — Stages 1–4, reconstructed from git history)
These entries predate the introduction of this changelog and are reconstructed from the
commit log for completeness.
- **Monorepo Foundation** (Initial migration, `411b86c`): Consolidated the formerly separate Swift repositories (Director, Actor, Stage, FolioParser, Tuner) into a unified Cargo workspace monorepo. Established the root workspace `Cargo.toml` and the `crates/` · `bindings/` · `platforms/` · `apps/` layout.
- **Phoneme/Token Chunking → Rust** (Stage 2, `4bafac1`): Ported `chunkPhonemes` / `chunkTokens` from the Swift actor pipeline into `crates/actor/src/chunking.rs`.
- **Voice Loading & Style Blending → Rust** (Stage 3, `77eca8f`): Ported `.safetensors` voice-pack parsing, weighted blending, per-utterance style-row slicing, and the 3D style-matrix assembly into `crates/actor/src/voice_loader.rs`.
- **Pull-based StageCoordinator** (Stage 4, `4b45252`): Added the runtime-free pull-based coordinator in `crates/stage` (the `NarrationSource` / `DirectorInference` / `VocalActor` seams driven by `next_chunk`).

### Changed (historical — Stages 1–4)
- **UniFFI Bindings & xcframeworks Regenerated** (Stage 5 prep, `0231b6a`): Regenerated the Swift/Kotlin UniFFI bindings and rebuilt the FFI xcframeworks for the Stage 1–4 Rust surface.
- **Studio docs**: Updated `STUDIO_STATE.md` for Stages 1–4 and the pending G2P decision (`b68609b`); bumped the Notes submodule with the refactor/decision docs (`05662f7`).
