# Defensive Publication — Director-Dictated Disentangled Prosody and Voice-Casting Control for On-Device Expressive Speech Narration

**Publisher:** McFarlin Technologies, LLC / Artificial Humanity
**First published:** 2026-07-13, in the public Prosodia repository (github.com/Artificial-Humanity/Prosodia)
**Prior public disclosure:** the working implementation of the mechanisms described here has been
public in this repository's source code since 2026-06-13 (see git history of
`crates/stage/src/prosody_payload.rs`, `crates/stage/src/acoustic_matrix.rs`,
`crates/actor/src/pipeline.rs`, `crates/actor/src/engine.rs`, `crates/actor/src/voice_loader.rs`).

> **Purpose.** This document is a **defensive publication**. It is published to place the described
> techniques — including the explicitly *contemplated* (not yet implemented) variants — into the
> public domain of ideas as citable, dated, enabling prior art, so that they remain free for
> everyone (including the authors) to practice. It is a technical disclosure, not a patent
> application, and no patent rights are asserted or sought by the authors for the subject matter
> disclosed herein.
>
> **License of this document:** CC BY 4.0. (The Prosodia source code remains under its own
> license; the Sonora model weights are Apache-2.0.)
>
> **Implementation status honesty:** mechanisms are labeled either **[implemented]** (public,
> working code in this repository) or **[contemplated]** (prophetic disclosure of a planned
> variant). No empirical performance figures are claimed.

---

## 1. Field

On-device text-to-speech (TTS) narration. Specifically: automatic generation, by a language-model
"Director," of a typed, per-span performance-control payload, and its application by a neural
"Actor" TTS model to dictate separable prosodic and vocal-identity channels — including per-token
pitch and duration control — and to synthesize multiple stable character voices without
per-character recordings, entirely on-device.

## 2. Problems addressed

1. **Entanglement.** In common expressive-TTS designs, identity (timbre) and prosody (emotion,
   pitch, rate, energy) are encoded in a single style vector or derived from one reference clip, so
   directing a stronger emotion also drifts the speaker's apparent identity. Identity cannot be held
   constant while the performance is directed.
2. **Suggestion, not dictation.** Control is exposed as free-text prompts or coarse tags that
   *steer* a model rather than dictating precise, reproducible targets; output drifts across a long
   work and cannot be directed at sub-sentence granularity.
3. **Recording dependence.** A "full cast" conventionally requires per-character reference audio or
   cloning; voices cannot be invented from parameters alone.
4. **Cloud dependence.** High-quality expressive control typically runs server-side, precluding
   private, offline narration.

## 3. System overview

A narration system comprising:

- **(A) Director** — an on-device instruction-tuned LLM (implemented: a Gemma-class model on an
  on-device LLM runtime, LiteRT-LM) that reads narrative text and emits, per text span, a **typed
  performance-control payload** with separable channels: (i) vocal identity/casting, (ii) emotion as
  a valence/arousal/tension (V/A/T) vector, (iii) dynamics/vocal effort, (iv) pitch, (v)
  pacing/duration — including **per-token sequences** for fine-grained control. **[implemented]**
  (the payload plumbing; LLM annotation quality is an ongoing tuning matter)
- **(B) Control contract** — a serialized, machine-readable tagged markup carrying the payload; a
  model-agnostic interface between Director and Actor. **[implemented]**
- **(C) Actor** — a non-autoregressive neural TTS model with an explicit duration predictor and a
  fundamental-frequency (F0) predictor, which renders speech by **applying** the payload as
  conditioning — including per-token duration-scale (multiplicative) and F0-bias (additive) tensors —
  so the dictated channels are obeyed rather than merely suggested. Embodiments include
  flow-matching/OT-CFM decoders (implemented lineage: Matcha-architecture "Sonora" models exported
  to LiteRT/TFLite) and StyleTTS2-style NAR variants **[contemplated]**.
- **(D) Continuous parametric casting grid** — synthesizes a speaker-identity/style vector for any
  character by interpolating among a small set of timbre anchors along continuous axes (age,
  masculinity) with a texture/strain blend, producing stable invented voices with **no
  per-character recordings**. **[implemented]**

Because identity and prosody travel in separate channels the Director writes independently, one
channel can be held fixed while another varies (e.g., the same narrator "hushes" without changing
who is speaking).

## 4. The control contract (serialization grammar)

Per span, a bracketed tagged block precedes the text. Channels (all optional except V/A/T):

```
[V:<valence> A:<arousal> T:<tension> S:<speed_mult> SB:<speed_bias> G:<gain_mult> GB:<gain_bias>
 AG:<age> MA:<masculinity> ST:<strain> LK:<speaker_lock_id> PB:<pause_mult> PN:<pronunciation>
 P:<pitch> DS:<dur_scale,dur_scale,...> FB:<f0_bias,f0_bias,...>] <span text>
```

Example: `[V: -0.50 A: 0.70 T: 0.85 P: -5.0] and grabbed her throat!`

Ranges: valence and arousal in [−1, 1]; tension in [0, 1]. `DS`/`FB` are per-token sequences
(comma-separated) enabling sub-sentence dynamics. `LK` (speaker-lock) pins a character's identity
across a whole work. Reference implementation: `crates/stage/src/prosody_payload.rs`.

## 5. Acoustic mapping (V/A/T → baseline targets)

The V/A/T vector is scaled by a tunable expressiveness scalar E (example E = 3.25): v′=vE, a′=aE,
t′=tE. Example baseline mapping (all coefficients tunable; reference:
`crates/stage/src/acoustic_matrix.rs`):

```
speed = clamp(1.0 + 0.08·a′ − 0.10·t′ + 0.05·v′,  0.65, 1.12)
gain  = clamp(1.0 + 0.25·a′ + 0.08·v′,            0.60, 1.20)
pitch (semitone-like), piecewise:
  a′ ≥ 0:  raw = max(0, −v)·a  (unamplified)
           raw ≥ 0.75 → pitch = −8·(max(0,−v′)·a′·t′)      # "angry shout": effort up, pitch down
           else       → pitch = min(15, 12·t′ + 3·a′)
  a′ < 0:  v < 0 → pitch = −(6·max(0,−a′)·t′)
           v ≥ 0 → pitch = min(15, 4·max(0,−a′))
```

Per-span baselines are then refined by the per-token `DS`/`FB` sequences.

## 6. Per-token dictation (the "obey" mechanism)

- `f0_bias[token]` is an **additive** offset applied to the predicted F0 contour;
  `duration_scale[token]` is a **multiplicative** factor (e.g., `duration_scale = 1/rate`) applied
  to predicted token durations. Both sequences are smoothed (e.g., moving average, window ≈ 5
  tokens) to avoid discontinuities. **[implemented]** (contract + plumbing; see below for actor
  variants)
- **Inference-time override [implemented / in progress]:** for actors exposing host-visible duration
  logits (`logw`) — e.g., split-graph exports where the host performs the length-regulator step —
  the per-token scales are applied directly to the predicted durations at inference, and F0 bias to
  the predicted contour.
- **Trained-to-obey conditioning [contemplated]:** an actor *fine-tuned to consume* per-token
  F0-bias and duration-scale as external conditioning inputs (e.g., FiLM/AdaLN-style conditioning on
  a V/A/T vector plus per-token control tracks, with conditioning dropout during training so the
  model performs plausibly when controls are absent). In this contemplated embodiment the model's
  duration and F0 predictors are trained with the external tracks as inputs, so dictated targets are
  followed as learned behavior rather than post-hoc override. Training-data preparation may derive
  per-token pitch/duration/energy labels from forced alignments and pitch tracking over expressive
  corpora, mapping utterance-level V/A/T labels from annotated descriptions and/or acoustic
  statistics. This paragraph is published precisely so that this variant, too, is prior art.

What is deterministic is the **dictated prosodic target** (the per-token values are exact,
externally supplied inputs); the mel/waveform sampler itself may be stochastic (e.g., flow-matching
at temperature ≈ 0.667) unless seeded or run at zero temperature.

## 7. Continuous parametric casting grid

Six timbre anchors (female/male × child/adult/elderly), each a low-dimensional style/speaker
embedding (e.g., 64-dim) extracted via a reference/style encoder or stored precomputed, plus a
gruff/texture anchor. For age a and masculinity m in [0,1] (age split at 0.5; reference:
`crates/actor/src/voice_loader.rs`):

```
V_lowAge  = (1−m)·V_female_lowAge + m·V_male_lowAge     (likewise V_highAge)
V_identity = (1−a″)·V_lowAge + a″·V_highAge             (a″ = age rescaled within its segment)
strain r > 0.05:  V_voice = (1−r)·V_identity + r·V_gruff
```

Blends are weight-normalized. The resulting embedding is cached per character identifier (e.g., LRU,
example capacity 16) and pinned by the speaker-lock field — an identical invented voice for that
character across the entire work, with **no per-character recordings**.

## 8. Model-agnostic runtime binding layer

The Actor runtime enumerates the loaded model's named input tensors and binds payload fields by
case-insensitive substring match (reference: `crates/actor/src/engine.rs`):

```
phonemes/text      ← "x" | "phone" | "input_ids" | "text"
style/speaker      ← "style" | "ref"
speed/tempo        ← "speed" | "tempo"
emotion vector     ← "vat" | "emotion" | "control"
per-token duration ← "duration_scale" | "dur_scale"
per-token F0       ← "f0_bias" | "pitch_bias"
```

Unmatched fields are ignored, so the same serialized payload drives different actor models exposing
different input subsets — the actor is swappable behind a stable contract (e.g., a monolithic
fixed-shape graph detected by its `x + x_lengths + scales` signature vs. a split
textenc/decoder/vocoder graph set with host-side ODE and length regulation).

## 9. Worked embodiments

- **"The hush" [implemented mechanism]:** for one span the Director holds casting + speaker-lock
  constant while lowering gain, applying a negative pitch term and negative per-token F0 bias, and
  slowing pacing (speed < 1, per-token duration_scale > 1, raised pause multiplier). The same
  narrator audibly lowers his voice — quieter, deeper, slower — with no identity change; the
  per-token tracks let the hush deepen mid-sentence.
- **"Full cast" [implemented mechanism]:** each character receives a distinct casting profile
  {age, masculinity, strain}; the grid synthesizes a distinct stable voice; speaker-locks pin each
  voice across chapters; each character independently carries its own V/A/T channel. Narrator and
  character voices alternate at quote boundaries.
- **Spoiler-aware casting gate [contemplated]:** a gate compares Director-generated casting
  parameters against the user's current reading position and substitutes a neutral profile until the
  corresponding character trait has been disclosed in the text — preventing "voice spoilers."
- **On-device pipeline [implemented]:** Director on LiteRT-LM; Actor exported (e.g.,
  torch → ONNX → TFLite, or per-module litert conversions) and executed on LiteRT/TFLite; the entire
  text → performance → audio pipeline executes on-device, offline.

## 10. Statement

The authors intend this publication, together with the dated public source code cited above, to
constitute enabling prior art for all mechanisms described, including the contemplated variants in
§6 and §9. Anyone may freely implement them.
