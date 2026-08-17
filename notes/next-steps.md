# Next Steps & Technical Debt — "What do we work on next?"

The single entrypoint for picking up work. Active workstream first, then completed items (for
record), then deferred technical debt. Each debt entry states *what*, *why deferred*, and *what
"done" looks like* so it can be picked up cold. Change history lives in the commit log;
the curated snapshot is [STATE.md](STATE.md).

---

## ⭐ TL;DR — the one live workstream

**Verify at the desktop, then start directability.** The actor model is **trained, exported,
fidelity-verified, and human-auditioned** (Sonora baseline-ljspeech-22k, Epoch 199 — see
[Sonora STATE](../../Sonora/github/notes/STATE.md)): the 2026-07-12 encoder-LayerNorm fix restored onnx2tf
export fidelity (deterministic ONNX↔TFLite cosine 1.0000, ASR WER 0.000), the artifact was
verified **through the shipping Rust engine** on Linux, and a parallel LiteRT split-graph lane
was materialized at parity. The immediate queue:

1. ✅ **Desktop pair DONE (2026-07-13):** Debt F build-checked and the **Tuner audition
   passed** — intelligible, pacing fine by ear (the 1.18 s question closes as the engine's
   `length_scale = 1/speed` config mapping, not a bug). Voice = LJSpeech narrator, expected for
   Phase 0. Remaining desktop annoyance: time-to-first-audio ~minutes — recorded as a
   split-graph-runtime validation item in §B.
2. ✅ **Exploit-before-train measurement DONE (2026-07-14):** pace + loudness are free at
   inference (surgical per-token `duration_scales`, dB-exact pre-vocoder mel gain — both WER-safe);
   pitch + phonation are not (no lever exists) and are what VAT training must deliver. Results:
   Sonora `notes/archive/exploit-before-train-measurement.md`, deleted 2026-08-02 in `8bbf343` — git history is the archive.
3. **Directability (milestone 3) — NEXT:** VAT-conditioning code + labeled corpus — the real next
   build, now scoped: weight the corpus toward valence/tension expression (phonation, pitch);
   pace/energy ride the inference hooks. The Rust multi-graph runtime (§B) is the parallel
   critical-path build and should include the measurement-backed per-frame mel-gain hook.

## 1. Active workstream — Actor model, export & on-device audio  **(primary)**

**Context (updated 2026-07-13):** the actor exists and speaks — and is now **desktop-auditioned
through the shipping app** (intelligible, pacing fine). `/data/models/sonora.tflite` (renamed from
`styletts2_lite.tflite` 2026-07-13 — it is a Matcha-architecture model, not StyleTTS2) is
the fidelity-fixed Sonora baseline-ljspeech-22k float32 e2e export, verified through
`engine.rs::forward_impl` on Linux (ASR on the engine's own render: verbatim) and human-auditioned
via the litert-lane samples. Model paths now resolve through `prosodia_models.json` (Debt F,
`577a598` — desktop build-check pending). Voice `.safetensors` packs remain absent (voice blending
still pending). Historical context: the 2026-06-14 "droning" diagnosis (no model → stub tone) and
the export spike that locked the `torch → ONNX → onnx2tf` backbone are recorded below and in the
Sonora notes.

> [!IMPORTANT]
> **Two separate tracks — don't conflate them.** Getting the Tuner to make *any* sound only needs
> *a* compatible actor model + voices; it does **not** require training our own model. Training our
> own actor is the **high-ambition production goal** — the chosen first model is the
> [Matcha-TTS actor](../../Sonora/github/notes/high-ambition-1-matcha-actor.md). _(The StyleTTS2-Lite
> re-platform once planned as the later higher-ceiling option was **retired 2026-07-29** — [decision](../../Sonora/github/docs/model-decisions.md).)_ Those
> are the long-horizon goal, not the near-term playback blocker.

**Decisions made (2026-06-14):**
- Export backbone = **`torch → ONNX`** (proven robust; `ai-edge-torch`/`litert_torch` is not reliable
  for these architectures — matrix below). *(Superseded 2026-07-12: the `litert-torch` fixed-shape
  split-graph path is now **Plan A** and the ONNX→`onnx2tf` monolith is the fallback — see the 📌
  callout below. The 2026-06-14 finding still holds for* stock dynamic-shape *modules; Plan A works
  because the graphs are re-authored to fixed shapes.)*
- Recommended actor for the **first** training effort = **Matcha-TTS** (MIT, single-stage/no-GAN,
  official ONNX exporter, mobile-friendly); StyleTTS2-Lite is the higher-ceiling Phase 2.
- First training run on **RunPod (NVIDIA 4090)**, not the Strix Halo (use that for inference / later).

**Tasks (when training resumes):**

#### 🍵 Phase 0: Untuned Matcha Discovery Spike (De-risking & Contract Lock)
- [x] **Step 1: Stock Matcha Standalone on M1 Max** — synthesize sample passages using stock pretrained Matcha in the `Sonora` repository, and A/B against StyleTTS2 / Kokoro.
- [x] **Step 2: Export Spike** — take a stock checkpoint → official ONNX export (`python -m matcha.onnx.export`) → `onnx2tf` → TFLite, and run it from the Rust actor.
- [x] **Step 3: Minimal End-to-End through Tuner** — run with neutral synthesis to validate the Director → payload → FFI → audio-sink pipeline.
- [x] **Contract-Lock Checklist (discovery-spike / runtime bridge)** — proves the *stock* model runs
  through the actor. The *training-time* contracts are a **distinct** list in
  [high-ambition-1 §Contract-lock](../../Sonora/github/notes/high-ambition-1-matcha-actor.md); don't read these checks as those.
  Most are now locked (vocab + native sample rate via commit `7143617`, export/runtime via the spike);
  only the **training filelist + VAT-label derivation** remains open there:
  - [x] **G2P / Phoneme vocab mapping**: Lock `map_styletts2_to_matcha_ipa` in `pipeline.rs` (translates StyleTTS2 phonetic characters to standard espeak-IPA equivalents compatible with the `config.json` vocab mapping).
  - [x] **Sample rate alignment**: Dynamically resample 22.05 kHz Matcha outputs to 24 kHz in the Rust actor to keep `StageAudioSink`/coordinator interface unmodified.
  - [x] **Data & label format**: Lock the emotion representation as `[V, A, T]` floats matching the `ProsodySpan` payload contract.
  - [x] **Export/runtime decision**: Lock TensorFlow Lite (TFLite) using `onnx2tf` as the runtime format. *(Runtime format still TFLite; the producing toolchain reversed 2026-07-12 — `litert-torch` split is Plan A, `onnx2tf` the fallback.)*

#### 🚦 Pre-training gates — everything doable *before* training is simply the only thing left
The plumbing, export route, and FFI contract are done; nothing technical blocks training. These are
the *training-time* gates (distinct from the discovery-spike/runtime contracts above; mirrors
[high-ambition-1 §Contract-lock](../../Sonora/github/notes/high-ambition-1-matcha-actor.md)), grouped by what each unblocks.

**A. Gates the *plain* fine-tune (the immediate blocker — kept short on purpose):**
- [x] **Lock the training vocab.** Decide the Matcha symbol inventory; bring `config.json` + the Rust
  G2P into lockstep. (Gate 1 completed 2026-06-18)
- [x] **Decide the native sample rate.** Fine-tune at 24 kHz with a 24 kHz vocoder (Gate 2
  completed 2026-06-18; Phase 0 pragmatically deviated to 22.05 kHz/LJSpeech). **Re-affirmed
  2026-07-14 for milestone 3 (owner call): native 24 kHz, no resampling; vocoder = HiFi-GAN
  fine-tuned to 24 kHz/80-band (preserves warm start + mel contract + export lane; the
  fine-tune is on the critical path to the §7 verdict and starts early). Details:
  [sample-rate-24khz-decision.md](../../Sonora/github/docs/model-decisions.md).**
- [x] **Pick + prep the dataset.** A small clean permissive set (a LibriTTS speaker / LJSpeech / Expresso):
  clean/trim, resample to the chosen rate, phonemize transcripts into the locked vocab, build the filelist. (Completed 2026-07-10)
- [x] **Stand up training platform**:
  - [ ] **Plan A (Recommended):** Stand up RunPod (4090 + persistent volume).
  - [x] **Plan B (Alternative):** Configure local Strix Halo Docker container (`rocm/pytorch` mapping `/dev/kfd` and `/dev/dri`). (Completed 2026-07-10)

**B. Doable now in parallel — unblocks the *directability* fine-tune (milestone 3), no training needed:**
- [x] **Split-graph re-export of the Epoch 199 checkpoint** (textenc / decoder / vocoder) —
  **✅ EXPORT DONE via `litert-torch` (2026-07-12), and the split path is now Plan A.** Priority
  history, for the record: promoted 2026-07-11 (on the belief `onnx2tf` was fundamentally broken
  here) → de-promoted 2026-07-12 morning when the root-cause disproved that (a per-op
  `onnx2tf -cotof` report localized **all 95 diverging ops to the encoder's channel-axis
  LayerNorm**; fixed at the source — channels-last rewrite in
  `matcha/models/components/text_encoder.py`, Sonora `a537e03`; the monolithic `onnx2tf` TFLite
  then passed 559/559 ops, deterministic ONNX-vs-TFLite cosine **1.0000**) → **re-promoted to
  Plan A 2026-07-12 evening on merit** once the litert-torch conversion of our own checkpoint
  materialized at parity (per-graph corr 1.000000, e2e ≥0.9996 vs torch, human-ear validated). The
  structural wins stand regardless of onnx2tf's health: (a) host-visible `logw` for the
  `duration_scales`/`f0_bias` hooks — the exploit-before-train measurement can run now, (b) no
  50-token static limit + lower per-forward latency, (c) per-graph mobile delegate placement.
- [x] **Rust multi-graph runtime for the split graphs (`crates/actor`) — ✅ CORE LANDED
  (2026-07-14, Prosodia `f08351c` + `1d24fae`, `crates/actor/src/split_engine.rs`).** Three
  interpreters + host embedding lookup/pad/mask, durations from `logw` (**`duration_scales`
  dictation is live**: 2× scales → 66→132 frames, test-verified), length regulator, Euler ODE with
  host sinusoidal time embedding, denormalize → vocoder → clip/trim; **the measurement-backed
  per-frame mel-gain (energy) hook is in** (−6.0 dB requested → −6.01 dB measured, matching the
  box's −6.04). Parity vs a Python reference of the litert recipe on the shipped fp16 graphs with
  identical noise: **cosine 1.000000**, identical frame count. A split model is a *directory*
  (graphs + `emb.bin` + `config.json`); `LiteRtActorEngine` dispatches by path type — token limit
  256 (vs the monolith's 50), real per-token `pred_dur`, XNNPACK per graph. Swift provider handles
  directory models; `actor-split` role added to `prosodia_models.json` (swap the `actor` role's
  path to audition). 38/38 actor tests; both app targets build.
  **Remaining for full task close-out:** (a) desktop Tuner audition through the split path (incl.
  the §B latency acceptance check — time-to-first-audio should drop: the split dispatch test
  renders in ~2 s including engine construction); (b) payload routing of the energy channel
  (G:/per-frame) through `ProsodiaSpeechEngine::forward` — deferred to the milestone-3 VAT
  conditioning rework because that FFI trait has Swift *and Kotlin* implementers; (c) per-chunk
  streaming (same seam). Until auditioned, the Tuner's default `actor` role stays on the monolithic
  artifact (Plan B). **Fidelity gates stand for both plans:**
  `Sonora/scripts/export_fidelity_referee.py` for any onnx2tf export — real-input ONNX-vs-TFLite
  check; `--temperature 0` for an RNG-free number (the graph's `RandomNormalLike` makes stochastic
  end-to-end cosine meaningless), `--asr` for intelligibility; **do NOT trust `onnx2tf`'s own
  `-cotof` self-report** (it skipped the nondeterministic decoder ops and emitted a false
  `cosine=1 pass=True` after its validator crashed on an fd limit) — and the litert-conversion
  harness's per-graph correlation gate for Plan A. Still keep **I/O tensors f32** when cutting fp16
  variants (weights-only fp16): the earlier fp16 e2e export had f16 `scales`/`wav` tensors, which
  `engine.rs::forward_impl` reads/writes as f32 — silent garbage, no error (confirmed 2026-07-12:
  the fixed fp16 tflite won't even allocate on the CPU interpreter). Delegate notes in
  [§LiteRT-community assessment](#litert-community-matcha-repo-assessment-2026-07-11--no-pivot-mine-the-exportruntime-layer).
  **Also fixes — validate when this lands (measured 2026-07-13, desktop audition):** the Tuner's
  **time-to-first-audio latency**. Today a Play tap waits ~a minute+ on the M1 Max because
  (a) the engine renders **all** ≤50-token chunks of a span and concatenates before returning any
  audio (`engine.rs` chunk loop), and (b) the fixed-shape e2e graph pays the full 512-mel-frame
  decode + 262,144-sample vocode per chunk regardless of text length — one forward ≈ 10–15 s wall
  on the M1 Max via the `CLiteRTLM_mac` kernels (first play adds the 178 MB model load; actor
  cached after). The split runtime removes both structurally: compute proportional to actual
  token/frame counts, and per-module graphs enable streaming/chunk-level playback.
  **Acceptance check:** time-to-first-audio on a multi-sentence passage drops from ~minutes to
  low seconds. **Interim relief landed 2026-07-13 (Prosodia `104a9c8`):** the engine now attaches
  the XNNPACK delegate on Apple (~5× — one forward 14.8 s → 2.9 s on the M1 Max; the delegate API
  ships in the CLiteRTLM_mac dylib; Linux TFLite is built without XNNPACK so it's Apple-gated),
  and the harness apps warm the actor with a throwaway background render at launch (model load +
  XNNPACK weight packing off the first Speak). Updated arithmetic: ~2.9 s per ≤50-token chunk —
  **user-verified in the Tuner: ~5 s to start a passage, acceptable for testing** (2026-07-13 —
  the post-relief baseline for this task's acceptance check); long passages still pay all chunks
  up front, which stays this runtime task's job (per-chunk streaming + proportional compute).
- [x] **Exploit-before-train measurement — ✅ DONE (2026-07-14).** Run on `ai-lab-0` against the
  Epoch-199 split graphs (`/data/toolchain/litert-conversion/exploit_measure.py`; results note:
  Sonora `notes/archive/exploit-before-train-measurement.md`, deleted 2026-08-02 in `8bbf343` — git history is the archive).
  **Pace and loudness fall out free at inference:** per-token `duration_scales` via host `logw`
  is surgical and WER-safe to ×2.0 (ρ = 1.0; phrase-local stretch with ≤1.4% context drift), and
  per-frame log-mel dB bias before the vocoder is dB-exact with zero context bleed (WER 0 at
  −12 dB). **Pitch and phonation don't:** Matcha has no F0 input (`f0_bias` has nothing to grab;
  a crude mel-bin roll buys ~−1.5 st before words break) and no breathiness/tension lever exists.
  **Scoping verdict for milestone 3:** VAT conditioning must own pitch + voice quality; pace and
  energy channels can ship through inference-time hooks. Two follow-ups folded into other items:
  the multi-graph runtime should add the **per-frame mel-gain (energy) hook** between decoder and
  vocoder (measurement-backed), and the VAT corpus should weight valence/tension expression
  (phonation, pitch behavior) over tempo/loudness variety.
- [x] **Adopt the espeak-free training G2P — ✅ DONE (2026-07-14, Sonora `d5dd4fc`).**
  `matcha/text/op_g2p.py` (OpenPhonemizer dict primary + DeepPhonemizer TFLite OOV fallback +
  U+0303 strip rule) + `scripts/phonemize_filelist.py` (offline filelist phonemization with
  locked-vocab validation) + `no_cleaners` passthrough + `ljspeech_op` data/experiment configs.
  Run over LJSpeech train+val: **13,100 lines, 0 vocab violations, 0 unresolved words** (95.8%
  dict, 4.2% neural OOV — mostly proper nouns/possessives; spot-check vs espeak shows matching
  IPA incl. possessives, the one systematic delta being citation stress on function words vs
  espeak's contextual destressing). Espeak init made lazy (module imports espeak-free — verified
  `phonemizer` is never imported on the `no_cleaners` path); `espeak-ng` dropped from the
  `sonora_training` container command (compose; the stale Phase-0 resume `ckpt_path` removed with
  it). The vocalizer container keeps espeak for now — it phonemizes arbitrary user text at
  inference via `matcha.cli.process_text` and is a dev tool, not the commercial data path.
  Closes [north star §8.3](architecture-north-star.md); train-time and runtime G2P now share the
  litert-community source of truth. *(Earlier validation, 2026-07-12: 99.997% of the 274,927-entry
  dict phonemizes into the locked vocab; Sonora's `symbols.py` IS the locked vocab.)*
- [ ] **Assemble the VAT-labeled expressive corpus** — the real bottleneck
  ([north star §9](architecture-north-star.md)): permissive expressive multi-speaker audio + a
  VAT-labeling method (prosodic-feature derivation and/or local-LLM annotation on the Strix Halo) + the
  training filelist + label schema.
- [x] **Draw the license wall in code — ✅ DONE (2026-07-14, Sonora `d5dd4fc`).**
  `configs/data_licenses.yaml` declares every dataset's verified license class (sources from
  [dataset-landscape.md](../../Sonora/github/notes/dataset-landscape.md)); `matcha/data/license_wall.py`, hooked
  into `TextMelDataModule.setup()`, refuses **undeclared** and **NC** data at training time.
  `SONORA_LICENSE_WALL=derisk` permits NC for §7 de-risk runs with a loud TAINTED banner;
  deliberately no "off" mode. All four behaviors test-verified (permissive pass, NC block,
  derisk banner, unknown block). Closes [north star §8.2](architecture-north-star.md).
- [x] **Write the VAT-conditioning code — ✅ DONE (2026-07-14, Sonora `ad2baea`),** same day the
  design was decided (owner call; spec:
  [vat-conditioning-design.md](../../Sonora/github/docs/vat-channels.md)): full FiLM blocks
  (zero-init scale+shift) per encoder block + per CFM U-Net level fed by a shared `VATTrunk`
  from raw `[B,3,T]`; per-utterance labels broadcast; frame alignment rides the same attn
  matmul as `mu_y` (inherits the `out_size` cut); conditioning dropout p = 0.15; optional
  trailing `v,a,t` filelist field (`load_vat`). All off by default — existing checkpoints and
  configs unaffected. **Guarantees verified** (`scripts/test_vat_identity.py`): warm-start
  synthesise from the Phase-0 checkpoint is bit-identical for vat = 0/None/hot at init (caught
  en route: `Decoder.initialize_weights()` kaiming-inits every Conv1d and silently defeated the
  zero-init — heads re-zeroed after it), training forward healthy with grads in all 24 FiLM
  heads. **Export gate** (`scripts/test_film_export_gate.py`): trunk+FiLM chain converts
  litert-torch fixed-shape, GPU-clean, corr 1.000000. *Residual for the training/export cycle:*
  the conversion harness's re-authored textenc/decoder wrappers still need the `vat` graph
  input added when the first VAT checkpoint is converted (per-graph parity re-run is already a
  standing spec requirement after any fine-tune).
- [x] **Build the eval harness — ✅ DONE (2026-07-14, Sonora `c58028c`).**
  `scripts/eval_harness.py`: manifest-driven (JSONL rows: wav/text/group/requested/baseline),
  reusable across channels and Actor checkpoints. Three gate families with the §7 pre-registered
  thresholds: controllability (Spearman ρ ≥ 0.9 requested-vs-produced; LUFS loudness, duration,
  pyin F0), identity (ECAPA-TDNN drift; with `--speaker-refs` normalized by a real inter-speaker
  gap → leakage ratio ≤ 0.2), intelligibility (faster-whisper WER delta ≤ +0.10 — subsumes the
  2026-07-11 prototype as the committed standing gate; the `engine.rs` diagnostic render tests
  remain its render-side counterpart). Smoke-tested on the exploit-before-train renders:
  **energy channel passes all gates at inference** (ρ = 1.0, leakage 0.054, WER 0); duration
  correctly flags its ×0.6 WER boundary and 0.244 identity drift at tempo extremes (×0.6/×2.0) —
  first calibration hint that even "free" channels want the leakage gate. Eval-only deps stay out
  of requirements.txt (documented in the script docstring).

#### 🔄 Training & Fine-Tuning milestones (ordered)
Make the first success boring; add novelty only after it sings.
- [x] **1 — Plain fine-tune on RunPod (Plan A) or Local Strix Halo (Plan B)** (no VAT) — Completed on local AMD ROCm container (resolute). Trained to Epoch 260; optimal convergence selected at Epoch 199.
- [x] **2 — Export-verify the trained checkpoint** — ONNX → `onnx2tf` → TFLite (Float32/Float16) with HiFi-GAN vocoder embedded. Checkpoint assets successfully stored at **[artificial-humanity/Sonora](https://huggingface.co/artificial-humanity/Sonora)** (directory `baseline-ljspeech-22k`).
- [ ] **3 — Directability fine-tune** — add VAT conditioning, retrain with VAT labels, verify
  directability in the Tuner. Gated by **B**.
- [ ] **4 — Casting / blend layer** — re-derive anchor embeddings in speaker-embedding space; verify the casting grid.
- [ ] **5 — Multi-speaker / expressive data** for range, then iterate on quality.
- [x] Fix the dead `VoiceDownloader` URL (was `hexgrad/StyleTTS2-Lite` 404 → now
  `artificial-humanity/StyleTTS2-Lite`, 2026-06-15). _Note:_ that HF repo must actually host the
  `anchor_*` voice packs before downloads succeed.

### Export-toolchain finding (spike, 2026-06-14)
Overturns the earlier assumption that `ai-edge-torch` is the low-friction path. Empirically on a dev
Mac (`uv` venv, `torch` 2.12 + `litert-torch`):

| block | `litert_torch` (official) | `torch → ONNX` |
|---|---|---|
| bidirectional LSTM | ❌ no control-flow lowering; specializes seq length | ✅ native dynamic LSTM, matches torch 1e-7 |
| Conv1d, dynamic length | ❌ bakes a static `RESHAPE` (fails resize, even w/o XNNPACK) | ✅ |
| Linear over dynamic seq | ❌ symbolic `view` fails the jax-bridge | ✅ |
| attention (batched matmul) | ❌ mislowered to `FULLY_CONNECTED`; fails fixed **and** dynamic | ✅ |
| trivial / elementwise | ✅ | ✅ |

The mature, architecture-agnostic, dynamic-length-robust path is **`torch → ONNX`**. (`ai-edge-torch`
*does* ship working Gemma/SD exports — but via its bespoke `generative` layer library + recipes, not
arbitrary modules.) **Runtime fork — resolved (2026-06-17) in favor of TFLite via `onnx2tf`:** the
options were ONNX → TFLite via `onnx2tf` (keeps the LiteRT runtime + Rust actor + mandate; needs a
TensorFlow build-time dep) **vs** ship ONNX via ONNX Runtime (shortest path; rewrites `tflite.rs`,
breaks the mandate). The `onnx2tf` spike succeeded (validated on `model_e2e.onnx`; see
[STATE.md](STATE.md)), so TFLite is locked and ONNX Runtime was not adopted.

> **📌 Primary export path (Plan A) — `litert-torch` fixed-shape split graphs.** *(Priority
> REVERSED 2026-07-12: this was the documented reserve; it and the onnx2tf monolith have swapped
> places.)* **Our Epoch-199 checkpoint is converted and verified** on this path (per-graph corr
> 1.000000, e2e fp16 waveform corr ≥0.9996 vs torch, GPU-clean, ASR-verbatim, human-ear validated —
> details in [Sonora STATE](../../Sonora/github/notes/STATE.md); artifacts pushed to HF at
> [`baseline-ljspeech-22k/litert-split`](https://huggingface.co/artificial-humanity/Sonora/tree/main/baseline-ljspeech-22k/litert-split);
> conversion workspace + env recipe at `/data/toolchain/litert-conversion/`, built from the
> [litert-samples](https://github.com/google-ai-edge/litert-samples) conversion harness). It is
> Plan A on merit, not necessity — its advantages are structural and already banked: host-visible
> `logw` (the `duration_scales`/`f0_bias` hooks and the exploit-before-train measurement can run on
> these graphs *now*), no 50-token static limit, per-graph delegate placement (Pixel 8a recipe:
> decoder CPU, textenc+vocoder GPU, RTF ~0.8), tunable ODE steps, fp16 total 66 MB. **The gap to
> shipping it** — the Rust multi-graph runtime path (three interpreters + host
> ODE/length-regulator) in `crates/actor` — is therefore now on the critical path, not an
> optimization. Re-verify per-graph parity after any future fine-tune (VAT etc.) by re-running the
> harness against the new checkpoint.
>
> **Fallback (Plan B) — the monolithic `torch → ONNX → onnx2tf → TFLite` e2e graph.** Proven back
> on track by the 2026-07-12 encoder-LayerNorm fix (deterministic ONNX↔TFLite parity, cosine
> 1.0000), and it is what the desktop Tuner **auditions today** (staged as
> `/data/models/sonora.tflite`) until the multi-graph runtime lands. Keep it maintained as the
> fallback should the litert-torch toolchain regress on a future checkpoint, and keep the
> `torch → ONNX` stage as the permanent numerical **oracle** every onnx2tf export is graded against
> — it is what caught *and* verified the LayerNorm fix (`onnx2tf` TFLite garbled while the ONNX
> rendered cleanly). Its structural limits are why it lost Plan A: buried `logw`, the 50-token
> static window, and monolithic per-forward latency.
>
> Either plan ships **LiteRT/TFLite runtime** models — the runtime is unchanged by this reversal;
> we are **not** maintaining a parallel ONNX runtime.
>
> *(Retired: the earlier idea of shipping raw ONNX via ONNX Runtime as a second desktop runtime is
> dropped as a documented plan — ONNX is an export source and fidelity oracle, not a runtime fork.)*

### LiteRT-community Matcha repo assessment (2026-07-11) — no pivot; mine the export/runtime layer
[litert-community/Matcha-TTS](https://huggingface.co/litert-community/Matcha-TTS) (MIT, published
2026-07-02) prompted a "should we pivot our Matcha training to this?" review. **Verdict: no pivot —
there is nothing to pivot to.** The repo contains no training code; it is the official
`matcha_ljspeech` + `hifigan_T2_v1` checkpoints — the same upstream lineage our Phase 0 fine-tune
started from — converted to fp16 TFLite. Training stays on the `shivammehta25/Matcha-TTS` codebase.

It also does **not** overturn the 2026-06-14 export-toolchain spike above. Its graphs went through
`litert-torch` only after being *re-authored* to fixed shapes (256 phonemes / 512 mel frames ≈ 5.9 s
of audio) with runtime float masks — i.e. it confirms `litert-torch` cannot swallow stock
dynamic-shape Matcha, which is exactly what the spike found. *(Update 2026-07-12: that fixed-shape
recipe is precisely how our own Epoch-199 conversion succeeded, and the split-graph `litert-torch`
path has since been promoted to **Plan A**; the `torch → ONNX → onnx2tf` monolith is the documented
fallback — see the 📌 callout above.)* (Historical note for future "was LiteRT rejected?" confusion:
the spike rejected **stock-shape export through `litert-torch`/`ai-edge-torch`**, never the
LiteRT/TFLite **runtime** — the runtime was locked in on 2026-06-17 and is what we ship on either
plan.)

**What we adopt from it** (tasks tracked in [§B above](#-pre-training-gates--everything-doable-before-training-is-simply-the-only-thing-left)):

1. **Espeak-free G2P for the *training* pipeline** — closes [north star §8.3](architecture-north-star.md).
   The Rust runtime is already espeak-free (compiled lexicons, `crates/actor/src/g2p.rs`), but the
   Sonora training container still `apt-get install`s espeak-ng to phonemize transcripts — GPL in the
   commercial-build data path. The repo ships the clean replacement: a **275k-entry espeak-IPA
   dictionary** derived from OpenPhonemizer (Clear BSD, `g2p_dict.txt.gz`) as primary + a
   **DeepPhonemizer** (MIT) TFLite graph for out-of-dictionary words, emitting IPA that maps 1:1 onto
   the keithito 178-symbol set. Using it to phonemize training filelists removes GPL from training
   *and* lets train-time and runtime G2P share one source of truth (today's silent risk: the model is
   trained on espeak IPA while inference uses our own lexicons — divergent pronunciations degrade
   quality invisibly). The dictionary can also enrich/cross-check the compiled Rust lexicons.
   **Caveat — vocab delta:** our locked vocab (Gate 1, commit `7143617`) is a *modified* 178-symbol
   set (duplicate `'` removed, `ᵊ` added); the repo's `config.json` is stock keithito-178 (includes
   `ᵻ`). A small mapping/validation pass over the dictionary output against our locked vocab is
   required before use.

2. **Split-graph export pattern** — textenc / CFM decoder / vocoder as **three graphs**, with the
   Euler ODE loop and length regulation on the **host**. Our current Epoch 199 export is one
   monolithic e2e graph, which buries the duration predictor's `logw` where
   `crates/actor/src/pipeline.rs` can never scale it — so the already-wired per-token
   `duration_scales` / `f0_bias` hooks have nothing to grab. The split pattern surfaces durations to
   the host (the control hooks work on a *stock* checkpoint), lets ODE step count be tuned at runtime
   (quality/latency knob), and allows per-graph delegates. **This gates the exploit-before-train
   measurement (§B.1)** on our own exported TFLite. ~~Produce the split via the proven per-module
   ONNX → `onnx2tf` route — no need to trust `litert-torch`.~~ **Toolchain fork RESOLVED
   (2026-07-12) in favor of `litert-torch`:** fixed-shape re-authoring (litert-community's route)
   converted our Epoch-199 checkpoint first-try and passed the per-graph correlation gate
   (corr 1.000000 per graph) — the split export is done and the path is now **Plan A**. Per-module
   `onnx2tf` was never needed for the split; the (now-fixed) monolithic `onnx2tf` conversion
   remains the Plan B fallback.
   Their Pixel 8a delegate findings, for when Android placement matters: decoder must stay on **CPU**
   (Mali ML-Drift GPU mis-fuses its transformer blocks when fused — corr 0.006 fused vs 0.984
   standalone); textenc + vocoder on GPU; pipeline stays realtime, RTF ~0.8.

3. **Export QA discipline** — they report per-graph tflite-vs-torch correlation (1.000000 per graph,
   ≥0.99 end-to-end waveform). Cheap habit; bake a correlation check into our export scripts so every
   future export (VAT-conditioned included) ships with a numeric fidelity receipt.

**What it does *not* give us:** it's stock single-speaker LJSpeech — same ground our Phase 0 already
covers; zero progress toward VAT conditioning, multi-speaker casting, or milestones 3–5. Its fixed
512-mel-frame window (~5.9 s per synthesis call) matches our sentence-chunked Stage, but note it as a
constraint if we copy the fixed-shape masking trick.

**Sibling repo (assessed 2026-07-12, user-flagged):**
[mlboydaisuke/Matcha-TTS-LiteRT](https://huggingface.co/mlboydaisuke/Matcha-TTS-LiteRT) carries
**byte-identical artifacts** (same filenames and sizes) to litert-community/Matcha-TTS — likely the
original author's repo upstreamed to litert-community — so **no second clone is needed**. Its README
is the richer reference, though; mine it alongside the clone:
- **Exact split-graph I/O shapes:** textenc `emb[1,256,192] + mask[1,1,256] → mu[1,80,256] +
  logw[1,1,256]`; decoder `x,mu[1,80,512] + t_sin[1,160] + mask[1,1,512] → v[1,80,512]`; vocoder
  `mel[1,80,512] → wav[1,1,131072]`; plus `emb.bin` = the 178×192 f32 phoneme embedding table
  looked up **host-side** (embedding + intersperse + pad happen on the host, not in the graph).
- **Host orchestration recipe:** G2P → host embed/intersperse/pad → textenc → host durations +
  length-regulator → host Euler ODE loop over the decoder → host denormalize → vocoder.
- **The conversion-scripts pointer:**
  [google-ai-edge/litert-samples](https://github.com/google-ai-edge/litert-samples)
  (`compiled_model_api/text_to_speech`) hosts the full Android sample app **and the litert-torch
  conversion scripts** used to produce these graphs — this is the harness our own conversion was
  built from when the path was activated (2026-07-12; adapted copy at
  `/data/toolchain/litert-conversion/`), and its Android app is the reference for the delegate
  placement recipe.

### Rust ↔ TFLite I/O contract (must match `crates/actor/src/engine.rs::forward_impl`)
The exported graph's tensor **names** are matched by substring (case-insensitive):
- **phonemes input:** name contains `phone` / `input_ids` / `text` — shape `[1, token_count]`, `i32`.
- **style input:** name contains `style` / `ref` — `f32`.
- **speed input:** name contains `speed` / `tempo` (but not `vat`) — scalar `f32`.
- **emotion VAT input (optional):** name contains `vat` / `emotion` / `control` — `f32[3]`.
- **output 0:** the PCM waveform, `f32`, mono **24 kHz** (matches `StageAudioSink`/coordinator).

### Vocab / `config.json` (✅ done locally)
Generated `/data/models/config.json` = `{"sample_rate": 24000, "vocab": {symbol: index}}`. The **locked
Matcha vocab** (Gate 1, commit `7143617`, 2026-06-18) is exactly **178 unique symbols, dense indices
0–177**: the StyleTTS2-derived table (`[_pad] + punctuation + letters + IPA` from
`StyleTTS2/text_utils.py`) was deduped — the duplicate apostrophe `'` removed and the modifier-letter
schwa `ᵊ` added — so there is **no spare embedding row** anymore. (The earlier note about a
`177-symbols / n_token: 178` one-spare-row convention described the *old* StyleTTS2 derivation and no
longer holds; don't reintroduce it.) The trained model must use this exact mapping, and the Rust G2P
must agree with it. `config.json` is gitignored (lives under `/data/models/`); regenerate with the same
derivation if the symbol set ever changes — and change it in lockstep across G2P, training, and this file.

**Done looks like:** `/data/models/` contains the actor model (✅ staged 2026-07-11 as `styletts2_lite.tflite`, renamed `sonora.tflite` 2026-07-13 — the Sonora
baseline-ljspeech-22k float32 e2e export, contract pre-verified), `config.json` (✅, `sample_rate` corrected to
22050 for the 22.05 kHz Phase 0 voice), and the `anchor_*` voices; the Tuner produces intelligible
audio (✅ **export-fidelity fixed 2026-07-12: the `onnx2tf` conversion bug was root-caused to the
encoder LayerNorm and fixed at the source — re-exported TFLite now passes deterministic ONNX-parity
(cosine 1.0000); see Sonora STATE.md §1 and `scripts/export_fidelity_referee.py`.** Remaining: ~~push
the re-exported artifacts to HF~~ (✅ done by 2026-07-13), then a real listen through the desktop
Tuner at temperature > 0 — deferred to a desktop session. Runtime fixes that landed en route and stay: sink drain, static-limit
chunking, Matcha blank interspersion, per-passage play/stop UI). Source repos now live alongside in `/data/models/` — `shivammehta25/Matcha-TTS/` (spike workspace — see its ARCHIVE.md)
(vendored source + export artifacts, renamed from `Matcha-TTS/`) and `litert-community/Matcha-TTS/` (HF clone; G2P + split-graph reference); the Sonora registry clone moved to the workspace-root `Sonora/huggingface/` (2026-07-13) — canonical
listing in `Prosodia/apps/tuner/README.md`.

---

## 2. Completed & verified (record)

These were the active checklist items as of the 2026-06-14 audit; all are now done:

- [x] **Verify Apple app targets** (`xcodebuild`) — `ProsodiaTuner Harness` and `AppleReader` both
  **BUILD SUCCEEDED** against the regenerated UniFFI xcframeworks. _(The Tuner scheme has since been
  renamed to plain **`ProsodiaTuner`**; canonical build is `apps/tuner/build.sh` — see STATE.md
  §Environment footguns.)_
- [x] **Verify Android cross-compilation & app** — `./build_android.sh` + `./gradlew assembleDebug`;
  JNI libs (`libstage.so`, `libfolioparser.so`, …) load under the JVM.
- [x] **Finish the Rust G2P crate port** (also a license fix) — `crates/actor/src/g2p.rs` +
  `lexicon.rs` on a permissive lexicon; espeak-ng (GPL-3.0) removed from the shipping path. Lexicons
  are now compiled to zero-copy binary maps at build time.
- [x] **Linux & Windows platform scaffolding** — ALSA/PulseAudio C wrappers + WASAPI Exclusive C#.
  (Resolved and wired to build systems and FFI pipeline in Debt C).
- [x] **A. Dynamic parameter/emotion threading** (`60acd01`): Wired real VAT, `duration_scales`, and `f0_bias` into `forward_impl` and scaled output durations.
- [x] **B. Configuration centralization** (`86e4755`): Replaced literals with `DEFAULT_TOKEN_DURATION` and `DEFAULT_VAT`, dynamic `sample_rate` configuration, and C# P/Invoke.
- [x] **C. Desktop audio sink wiring** (`9606ce7`): Realigned layout, compiled Pulse/ALSA in custom `build.rs` under `platforms/linux`, built C# WASAPI project `ProsodiaWin.csproj`, and implemented `LinuxAudioSink` daemon.

---

## Deferred Technical Debt

- **H. Dev topology / workstream separation (captured 2026-07-13 — owner wants this "in the near
  future").** *What:* separate concerns so the Mac and `ai-lab-0` develop **simultaneously**
  instead of push/pull ping-pong on `main` — workstream→machine routing (model work → box; Xcode +
  runtime audition → Mac; Rust → either), branch discipline, and CI as the meeting point (Linux
  cargo test + macOS xcframework/app build — the piece that dissolves the tripwire round-trip).
  Full capture, this week's evidence, option sketch, and suggested first bites:
  [Ai-Lab-0/dev-topology-and-workstreams.md](../../AI-Lab-AMD/notes/dev-topology-and-workstreams.md).
  *Why deferred:* audition/directability is the critical path; nothing blocks today.
  *Done looks like:* AGENTS.md routing table + file-ownership convention; two CI jobs green on
  every push; rebase-collision round-trips stop appearing in the changelog. **Includes (same note): plan out
  building** — the build story accreted (ad-hoc per-machine scripts, committed binaries, human as
  artifact courier) and needs deliberate design: local builds / CI validation / artifact strategy /
  delivery. Illustrative end-state: unattended work + TestFlight delivery to the iPad.

- **D. Chain the Android build the same way the Tuner is chained (2026-07-11).**
  *What:* the Tuner now has a per-app build chain — `apps/tuner/build.sh` (Rust xcframeworks →
  `xcodebuild`) plus an in-project "Check FFI Framework Freshness" tripwire phase that fails builds
  linking stale Rust. The Android app still relies on manually running `./build_android.sh` before
  Gradle, with no staleness guard. *Why deferred:* Android isn't in the audition critical path.
  *Done looks like:* a `cargoNdkBuild` Gradle task wrapping `build_android.sh`, wired via
  `preBuild.dependsOn`, so `./gradlew assembleDebug` alone can never link stale JNI libs — Gradle
  handles cross-build-system dependencies far better than Xcode, so prefer real task wiring over a
  tripwire there.
- **E. Two dead-code warnings in `crates/stage` — ✅ DONE 2026-07-13:** (was:) unused `RwLock` import
  (`prosody.rs:1`) and never-read `worker_thread` field (`coordinator.rs:216`). Trivial cleanup;
  bundle with the next stage-crate change.
- **F. Move model pathing out of code into a config file — ✅ IMPLEMENTED (2026-07-13, commit
  `577a598`; awaiting desktop `xcodebuild` verification).** `prosodia_models.json` (repo root) maps
  role keys → paths; a relative `modelsBase` anchors to the config file's directory. Both apps
  resolve `actor`/`voices`/`director-*` through `ProsodiaModelsManager`; `DirectorModel` persists
  role keys (UserDefaults survive restructures); the two healing shims are deleted; the
  `Google/` Gemma move is handled by config, not code. **Remaining:** build + run both targets at
  the desktop (`apps/tuner/build.sh`), then clear the warning in `apps/tuner/README.md`.
  *What:* model locations and filenames are hard-coded in the apps — `TunerDemo.swift` derives
  `modelsBase` from `#filePath` walks, hard-codes `styletts2_lite.tflite`, the seeded Gemma
  filenames (`gemma-4-E2B/E4B-it.litertlm`), and `config.json`; `DirectorModelStore` then persists
  *absolute* paths in UserDefaults. This has bitten twice: the `apps/Models` shim (now deleted) and
  the 2026-07-11 "Gemma (missing)" bug after the umbrella-repo restructure, patched with self-healing
  re-resolution in `DirectorModelStore.init`. The healing is a mitigation, not the design.
  *Wanted design:* a declarative config (plist or JSON) mapping **role-based keys** to **relative**
  paths — the app must not know model identities at all (no "Gemma", no "E2B" in source). Keys name
  the *role* the app orchestrates; the config binds each role to an artifact:
  `"director-light" → "../Models/gemma-4-E2B-it.litertlm"`,
  `"director-heavy" → "../Models/gemma-4-E4B-it.litertlm"`,
  `"actor" → "../Models/styletts2_lite.tflite"` — resolved against a single configurable base at
  runtime; display names come from the config too; persist *role keys*, not paths, in UserDefaults.
  Swapping Gemma for any future Director model then touches only the config. Fold in other
  changeable constants the apps currently hard-code (default role selection, voice directory,
  expected filenames). Precedent already in the repo: `prosodia_config.json` +
  `ProsodiaConfig.swift` externalize the acoustic constants in both apps — extend that file or add
  a sibling `prosodia_models.json` loaded the same way.
  *Why deferred:* audition/testing is the critical path; the self-healing store unblocks it.
  *Done looks like:* no `#filePath` walks and no model filename literals in app source; both apps
  (Tuner, AppleReader) resolve every model through the config; a relocated `Models/` folder or
  renamed checkpoint needs only a config edit, no recompile; UserDefaults survives restructures
  because it stores keys.

