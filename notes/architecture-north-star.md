# Architecture North Star — Project Prosodia

> The map, not the implementation: the *why* and the standing decisions the build serves. When a
> branch, experiment, or detour raises "wait, what are we doing again?" — this is the document that
> answers it. Live decision forks are marked **→ FORK**. Related notes:
> [architecture-and-development.md](architecture-and-development.md) ·
> [model-decisions.md](../../Sonora/github/docs/model-decisions.md) ·
> [high-ambition-1-matcha-actor.md](../../Sonora/github/notes/high-ambition-1-matcha-actor.md) ·
> [next-steps.md](next-steps.md)

---

## 1. The Vision (the thing everything serves)

A cross-platform book-reader (iOS / iPad / macOS first) that turns a PDF or epub into a **performed**
reading — not a flat TTS readout. The acceptance test is human, not a metric:

> Read Roald Dahl's *The Witches* to a room of third graders on Halloween and watch them get enraptured.

Two concrete north-star behaviors that define "done":
- **The hush** — the narrator lowers his voice (pitch down, breathy, slower, quieter) as the mice creep
  past the Grand High Witch, *while staying the same narrator*.
- **Full cast** — each character voiced as a seemingly different actor, each stable across the whole
  200-page book, each able to carry its own emotion.

---

## 2. What Is Actually Ours (and what is rented)

The single most important strategic fact about this project: **the durable, ownable asset is the
Director and the typed contract — not the acoustic weights.** The voice model is the commodity,
replaceable layer. Keep these straight, because almost every "should I build or adopt?" question
answers itself once you know which side of the line it sits on.

| Layer | Ours / durable | External / replaceable |
|---|---|---|
| **Director** — reads text, reasons about emotion, emits performance notes (VAD + casting). The sophisticated part, and it's *ours*. | ✅ the reasoning & schema | runs *on* Gemma + LiteRT-LM (rented runtime — see §8) |
| **The contract** — the typed Director→Actor payload + tag grammar. The crown jewel. | ✅ entirely ours | — |
| **Stage** — queues, gapless playback, timing, lookahead/backpressure. | ✅ ours | — |
| **Actor** — phonemes + conditioning → PCM. | the conditioning channels & export are ours | the flow-matching/acoustic core is commodity |

**The crown jewel, verified in code:** the Director→Actor contract is a real typed payload
(`ProsodyDirective { emotion: EmotionVector, acoustics: ProsodyAcoustics }` in
`crates/stage/src/prosody.rs`, serialized through the `[V: A: T: …]` tag grammar in
`prosody_payload.rs`), and the Actor is genuinely swappable behind the `ProsodiaSpeechEngine` trait
(`crates/actor/src/engine.rs`). The LiteRT engine even **detects model input tensors by name at
runtime** and wires `vat`/`f0_bias`/`duration_scales` only if the loaded checkpoint exposes them — so
a Matcha or StyleTTS2 checkpoint slots in with *zero* code change. Most projects fuse these layers and
pay forever; this one didn't.

> Workspace note: traits live where the work lives — `VocalActor`/`DirectorInference`/`NarrationSource`
> in `crates/stage`, `ProsodiaSpeechEngine` in `crates/actor`. `crates/core` is the BPE tokenizer.

**Implication for ownership.** "Make this mine, moldable for years, not bound to another project's
end-of-life" is *already substantially achieved* — by the contract, not by training weights. The
contract is what lets us ride whatever the best permissively-licensed checkpoint is in 2026, 2028,
2030 without touching the Director. Ownership lives in the seam, not the silicon. The temptation to
"own" the project by hand-training an acoustic model is largely owning the *most* replaceable layer.

---

## 3. The One Organizing Principle

Both hard goals — *hush-the-same-narrator* and *stable-distinct-emotional-cast* — collapse to a single
architectural principle:

> **Disentangle identity from prosody into separate channels the Director writes to independently.**

The four separable acoustic channels:
1. **Timbre / identity** — who is speaking
2. **Emotion / intensity** — menace, fear, warmth
3. **Dynamics** — volume / vocal effort (the hush)
4. **Pacing** — duration / pauses (the creep)

A real hush moves channels 3+4 (and pitch) while holding 1 fixed. Full-cast switches channel 1 per line
while each voice keeps its own channel 2. The contract (§2) is the mechanism by which the Director
addresses these channels separately; the open question (§6) is whether the *model* obeys.

---

## 4. The Voice-Model Decision → FORK (the central one)

The Actor is a commodity behind a trait, so this is a *which-clay* choice, not a *bet-the-project*
choice. Three candidate baselines, scored on different axes (no single "best"):

| Axis | Winner |
|---|---|
| Expressiveness per unit effort | Chatterbox > StyleTTS2 > Matcha |
| Control precision (Director *dictates*, not suggests) | Matcha > StyleTTS2 > Chatterbox |
| Architectural freedom to make it yours | Matcha > StyleTTS2 > Chatterbox |
| Invent a cast without recordings | StyleTTS2 ≈ Chatterbox > Matcha |
| Mobile / LiteRT determinism | Matcha > StyleTTS2 > Chatterbox |

**Chosen north star: Matcha as "clay."** Both goals reward separable, *dictated* control, and Matcha
lets you build that separation as a first-class design instead of fighting entanglement. It is NAR +
TFLite-shaped, which matches the runtime already built (per-token F0 bias, duration predictor, speaker
embedding). It also aligns with where the build already leans.

- **→ FORK A — Matcha (chosen):** maximum freedom + best mobile determinism. Cost: you *build* the
  expressive machinery and curate the data. Base Matcha is near-blank clay, not a head start.
- **→ FORK B — StyleTTS2 (strong #2):** higher starting floor, samplable style space for inventing
  voices. Cost: its style vector entangles timbre + prosody — you spend effort prying them apart.
  Threatens full-cast stability first.
- **→ FORK C — Chatterbox Turbo (the demo path):** richest control surface out of the box. Fastest road
  to a magical demo. Cost: AR-codec model that *steers rather than obeys*, timbre drift, hardest LiteRT
  conversion, baked-in PerTh watermark — **and a second, foreign runtime** (llama.cpp) bolted next to
  the NAR/TFLite path we already maintain.

**Sequencing — favor one runtime, not two.** The earlier plan was to "run both timelines" (build the
demo on Chatterbox while developing the owned model on Matcha). For a solo, personally-funded project
that doubles the export and control-mapping surface for a one-time demo, and adds a runtime we'd
otherwise never carry. **Default: get the Halloween demo on the NAR/TFLite path already built**, even
if the first version is less expressive, and grow expressiveness inside the model we keep. Reach for
Chatterbox only if a real demo deadline proves the owned path can't get "enraptured" in time — a
fallback, not a parallel spine.

**One Director schema regardless:** per-span `{speaker_id, emotion, intensity, rate, pitch_shift,
energy}` that *expands* to full control vectors for the NAR model and could *degrade* to
tags+exaggeration for Chatterbox if ever needed. Design the contract once; it already largely exists.

---

## 5. Build-On vs From-Scratch → FORK (resolved)

**Build ON Matcha.** Your personal stamp does NOT live in the acoustic core (phonemes+conditioning →
mel is near-commoditized; Matcha's flow-matching core is good and MIT). It lives in the **layer above**:
separable conditioning channels, the Director contract, the curated data, the mobile export. All of
that is yours regardless of whose flow-matching core sits underneath (see §2).

Mixed answer per component when training does happen: **warm-start** the acoustic core + vocoder
(transfer solved capability), **train fresh** only the new conditioning it lacks, then **fine-tune
jointly**.

**→ FORK (reserved for v2):** true from-scratch only wins if (a) you have a genuine thesis the
*architecture* is wrong for your goal, or (b) the goal secretly became the research, not the app.
Neither holds today. Earn the right to from-scratch by shipping the Matcha version first.

---

## 6. The Real Gap: an obedient checkpoint, not control plumbing

This is the correction that keeps the north star honest. It is tempting to describe the gap as "a
shallow control layer, a fixable seam." **In code, that seam is already built end-to-end:**

- `pitch_for_emotion()` maps VAD → a real F0 shift, with angry-shout special-casing
  (`crates/stage/src/acoustic_matrix.rs`).
- The pipeline computes **per-token `f0_bias` and `duration_scales`**, smooths them, and passes them as
  **model input tensors** — into the F0 curve and duration predictor, *inside* synthesis
  (`crates/actor/src/pipeline.rs`).
- `CastingProfile { age, masculinity, strain_or_rasp }` interpolates the **speaker embedding itself**
  toward a gruff reference (`crates/actor/src/voice_loader.rs`) — touching timbre/effort, the actual
  ingredients of a hush.
- The engine wires a `vat` (V/A/T) conditioning tensor straight into the model when present
  (`engine.rs`).

So the rich VAD signal is **not** bottlenecked into an outside volume fader anymore. The plumbing that
reaches *inside* the model exists.

**What does not exist is a trained checkpoint that obeys those inputs.** Today there is no real Actor
model loaded — the single live workstream is *training one* (see [next-steps.md](next-steps.md)). The
risk has therefore moved: it is no longer "wire up control," it is the harder, data-bound problem of
**model responsiveness** — does moving `f0_bias`/`vat` actually move the right channel while identity
holds? That is what §7 gates and §9 costs. Do not let "just a seam" lull the schedule; the seam is
done, the hard part is the model and the data.

**The cheapest rung — exploit before you train.** Before any disentanglement training, the code already
supports driving `f0_bias` / `duration_scales` / `strain` on a **stock** checkpoint at inference time —
no training at all. A surprising fraction of "the hush" may fall out for free the moment a real Matcha
checkpoint loads. **Measure that first.** Only escalate to training when inference-time control
demonstrably isn't enough. This rung respects the budget and the stay-shallow-on-ML constraint, and it
is the true next experiment.

---

## 7. The De-Risk Gate (run before committing months to training)

Reach this gate only if §6's free inference-time control proves insufficient. **Riskiest assumption:**
"I can dictate one channel while holding another fixed." Test *that* cheaply, pre-registering pass/fail
so a borderline result can't be talked into a "yes."

- **Hypothesis (stated to fail):** on multi-speaker Matcha + one added scalar channel, I can move that
  channel across its full range while identity holds — AND the channel actually does something.
- **Pick energy** (measured loudness, e.g. LUFS / log-RMS) — free to label, continuous, objective. NOT
  an emotion label. Pitch (F0) is the harder, more identity-entangled follow-up.
- **Build:** warm-start a VCTK multi-speaker checkpoint; add one `energy_emb = MLP(1→D)` injected the
  same way the speaker embedding is; teacher-force ground-truth energy during training; override at
  inference. One scalar, one embedding, one injection point.
- **Data:** Expresso — its parallel structure (same speakers × 7 read-speech styles) *is* the
  cross-product that forbids conflating identity with energy.
- **Metrics, pre-registered:**
  - *Controllability:* Spearman ρ(requested, produced loudness) ≥ ~0.9 AND real dynamic range.
  - *Disentanglement:* leakage ratio = (identity drift across sweep) ÷ (gap between two real speakers)
    ≤ ~0.2, via ECAPA-TDNN; calibrate the numerator against the speaker's *own* natural
    whisper-vs-happy drift. Plus a speaker-classifier ≥95% confidence tripwire.
  - *Guardrail:* Whisper WER no worse than ~+5–10% at energy extremes.
- **Outcomes — note none say "from scratch":**
  - **Pass** → clay holds; commit; stack next channels (reference encoder → pitch → frame-level energy
    contours for mid-sentence hushes).
  - **Leak** → escalation ladder, still on Matcha: gradient-reversal adversarial speaker classifier →
    information bottleneck → rebalance data for full energy coverage per speaker.
  - **Dead knob** → plumbing bug, not a verdict — fix the hook.
- **Build the eval harness first** (loudness sweep + ECAPA leakage + Whisper WER) — reusable across
  every channel and every Actor. ~few days wall-clock on owned hardware.

> This whole section is **opt-in, not the spine.** The disentanglement program (adversarial classifiers,
> bottlenecks, cross-product corpus curation) is real ML research and real months of *your* time. Enter
> it deliberately, only after §6's free controls are exhausted — not by default.

---

## 8. Load-Bearing Constraints / Watch Items

1. **The Director runtime is the deeper lock-in — watch it more than the Actor.** The contract insulates
   you beautifully from voice-model end-of-life. It does NOT insulate you from **LiteRT-LM** (Google;
   STATE.md already documents a missing upstream LFS object footgun) or the **Gemma** weights license —
   and the Director is the part you most want to keep for years. The `DirectorInference` trait helps,
   but the Gemma+LiteRT-LM coupling is the longevity risk that actually threatens the irreplaceable
   layer. Keep an exit path in mind (alternative on-device LLM runtimes) even while LiteRT-LM is fine.
2. **License posture is load-bearing.** **Apache-2.0** (decided 2026-07-23, closing
   [open-decision-licensing.md](open-decision-licensing.md); *corrected here 2026-08-01 — this
   said "dual-license GPL-3.0 + commercial", a posture abandoned with the patent track*). Every
   dataset and checkpoint must be clean for **unrestricted open redistribution**, which is a
   *stricter* bar than the old commercial one, not a looser one — NC and research-only terms fail
   it outright. **Expresso is CC BY-NC — de-risk experiment ONLY; it must never enter the
   production corpus.** Draw that wall in the data pipeline *in code, now*.
3. **G2P licensing.** espeak-ng phonemizer is GPL, which an Apache-2.0 project cannot compile in
   at all — the old "fine for the GPL side" carve-out is gone with the dual licence. Verify a
   clean G2P path for every build. (espeak-ng is already out of the Apple build scope; keep it
   that way.)
   **Clean path identified (2026-07-11):** OpenPhonemizer's 275k espeak-IPA dictionary (Clear BSD) +
   DeepPhonemizer (MIT) OOV fallback, proven in-production by
   [litert-community/Matcha-TTS](https://huggingface.co/litert-community/Matcha-TTS) and 1:1-mappable
   onto our symbol set. Remaining espeak use is the *training* pipeline (Sonora containers) — adoption
   task tracked in [next-steps.md §B](next-steps.md).
4. **Decouple the runtimes.** Gemma on LiteRT-LM (its blessed path). Voice model on whatever fits its
   architecture — TFLite/ONNX for NAR (Matcha/StyleTTS2). **Do not force LiteRT for the voice model**,
   and **do not casually add a second voice runtime** (llama.cpp for AR codec) — each is months.
   *External validation (2026-07-11):* Google's litert-community team shipped stock Matcha+HiFi-GAN as
   fp16 TFLite running realtime on a Pixel 8a (RTF ~0.8) — independent confirmation that the chosen
   NAR/TFLite lane holds on-device. *(Update 2026-07-12: we adopted their fixed-shape `litert-torch`
   recipe ourselves — our Epoch-199 checkpoint converted at parity, and the split-graph path is now
   **Plan A**, with the `torch → ONNX → onnx2tf` monolith as the documented fallback; assessment in
   [next-steps.md](next-steps.md). The runtime is LiteRT/TFLite either way.)*
5. **Full-cast identity drift** — keep measuring it as emotion is pushed harder; it's the failure mode
   StyleTTS2 surfaces first.
6. **The one genuinely infeasible combo** (today's Pareto frontier): {≤300M + modern-tokenized-AR +
   LiteRT-native + rich emotion + zero-shot voices} all at once. Drop "LiteRT-native for the voice
   model" → Chatterbox gets most of it. Drop "modern-tokenized" → Matcha/StyleTTS2 ship a stable,
   mobile-clean v1 the Director makes expressive anyway.

---

## 9. The Real Bottleneck

Not GPUs (fine-tuning ≤500M is tens-to-low-hundreds of GPU-hours, effectively free on owned hardware).
**It's data, evaluation, and your time:** expressive, emotion-labeled, multi-speaker,
*permissively-licensed* data with cross-product coverage; the eval harness; and the listen→iterate
loop. For a personally-funded solo project, *your time* is the scarcest input — which is the strongest
argument for §6's exploit-before-you-train discipline and for keeping §7 opt-in. The AI team accelerates
the code (pipelines, schema, Rust bindings); it cannot shortcut the empirical grind.

---

## One-Breath Summary

What's yours is the Director and the contract — not the weights; the voice model is replaceable clay
behind a trait, and that swappability *is* the protection against any project's end-of-life. The control
plumbing that reaches inside the model already exists end-to-end; the real gap is a trained checkpoint
that obeys it, so exploit a stock checkpoint's controls before training anything, and keep
disentanglement training and any second runtime opt-in rather than the spine. Ship the demo on the one
NAR/TFLite path already built. Watch the Director's LiteRT-LM/Gemma coupling more closely than the
Actor's — that's the lock-in that touches the part you can't replace.
