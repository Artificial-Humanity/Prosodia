# High-Ambition 3 — 🚀 Child Voice Synthesis: Adaptation, Blending, & Fine-Tuning Options

> **Sequence:** 3 of 6. ([index — all six, and which repo each lives in](../../Sonora/github/notes/high-ambition-index.md)) Extends the casting/voicing range on top of the
> [1 — Matcha-TTS actor](../../Sonora/github/notes/high-ambition-1-matcha-actor.md) and the
> [2 — Dramatic Reader](../../Sonora/github/notes/high-ambition-2-dramatic-reader.md) multi-voice work. Then:
> [4 — Multilingual G2P](high-ambition-4-multilingual-g2p.md) ·
> [5 — StyleTTS2-Lite](../../Sonora/github/notes/archive/high-ambition-5-styletts2-lite.md). Options 1–2 work with the current engine
> today; Options 3–4 need the trained base model.

> [!NOTE]
> **Base-model framing:** Option 1 (DSP `f0_bias`/formant) and Option 4 (fine-tune) are
> base-agnostic. Options 2–3 are written in StyleTTS2 **style-vector** terms; under a
> [Matcha base](../../Sonora/github/notes/high-ambition-1-matcha-actor.md) they map to blending/extracting in the
> **speaker-embedding** space instead — same `voice_loader` machinery, different vector semantics.

This document outlines the proposed strategies, technical feasibility, and next steps for introducing high-quality child-like voices into the `Prosodia` TTS runtime.

---

## 🏛️ The Challenge of Child Voice Synthesis

Children's voices are notoriously difficult to synthesize using standard deep-learning models trained primarily on adult datasets. The key acoustic differences include:
*   **Higher Pitch (F0):** Children typically speak with a fundamental frequency (F0) between 250 Hz and 400 Hz (compared to ~120 Hz for adult males and ~200 Hz for adult females).
*   **Shorter Vocal Tracts (Formants):** Because a child's vocal tract is physically shorter, their resonance formants ($F_1, F_2, F_3$) are shifted upwards. Simply increasing pitch without adjusting formants creates a synthetic "chipmunk" or robotic effect.
*   **Distinct Speech Patterns:** Children exhibit different phoneme durations, breathiness, and emotional cadences compared to adult speakers.

---

## 🛠️ Proposed Options for Project Prosodia

We have identified four key implementation vectors, ranging from zero-code runtime adjustments to deep-learning custom weight training.

### Option 1: DSP Post-Processing & Pitch Bias (Low Effort / Low Quality)
*   **Concept:** Use the `f0_bias` parameter (surfaced through prosody acoustics) to artificially shift the F0 contour upwards by $+15$ to $+30$ Hz.
*   **Formant Shift:** Apply a phase vocoder or formant-scaling filter on the output float PCM buffer inside the audio sink block to simulate a shorter vocal tract.
*   **Pros:** Requires no new weights or training; works entirely in real-time.
*   **Cons:** Post-processing cannot introduce the unique speech patterns, breathiness, or pronunciation cues of a child, leading to a synthetic, unnatural tone if scaled aggressively.

### Option 2: Latent Style Vector Blending (Medium Effort / Medium Quality)
*   **Concept:** Blend existing, higher-pitched, or highly expressive female voices in the voice matrix to create a composite timbre that sounds subjectively younger.
*   **Formula:** Experiment in the Preset Audition Harness with combinations like:
    *   `65% anchor_female_adult` + `35% anchor_female_child`
*   **Pros:** Zero memory overhead (uses existing voice packs already loaded in the cache); fully supported by our current blending engine.
*   **Cons:** Timbre is still fundamentally bounded by the adult vocal tracts of the underlying anchors.

### Option 3: Zero-Shot Style Vector Extraction (Medium-High Effort / High Quality)
*   **Concept:** Leverage the zero-shot style extraction network of the StyleTTS2 architecture. 
*   **Execution:** 
    1. Obtain a clean, dry, 5-to-15-second audio recording of a child's voice (DRM-free or recorded locally).
    2. Feed this wav file into the StyleTTS2 style extractor network to generate a custom 64/128-dimensional latent style vector (`.safetensors` file).
    3. Load this file directly into the decoupled [VoiceLoader](../../Prosodia/crates/actor/src/voice_loader.rs) in the actor crate.
*   **Pros:** Creates a custom voice pack specific to a child without retraining the entire model.
*   **Cons:** Requires running the StyleTTS2 model setup to extract the style tensor from the reference wav.

### Option 4: Stage 2 Continuation Fine-Tuning (High Effort / Best Quality)
*   **Concept:** Spin up a cloud RTX 4090 GPU node to run a specialized fine-tuning round targeting child speech datasets (e.g. using the CMU Kids corpus or curated DRM-free children's audiobooks).
*   **Execution:**
    1. Freeze early phoneme mapping blocks and the text encoder to preserve baseline English pronunciation.
    2. Unfreeze and train the **Style Encoder** and **Prosody Predictor** layers of the StyleTTS2-Lite model on children's speech.
    3. Generate custom checkpoint weights (`styletts2-child-v1.safetensors`).
*   **Pros:** Produces the most realistic child speech, capturing unique child-like cadences, speech rhythms, and authentic acoustic qualities.
*   **Cons:** High compute/resource requirement; requires collecting and normalizing clean children's speech datasets.

---

## 📅 Roadmap Recommendation

1.  **Immediate:** Use **Option 2 (Style Blending)** in the Preset Audition Harness to test if combinations of high-pitched anchor voices provide a suitable placeholder.
2.  **Short-term:** Implement **Option 3 (Zero-Shot Style Extraction)** using a public child speech wav sample to generate a `.safetensors` style vector and add it as a new available voice anchor (`anchor_female_child`).
3.  **Long-term:** Evaluate **Option 4 (Fine-Tuning)** if the product requires dedicated child narrator personas with highly distinct, high-fidelity readings.
