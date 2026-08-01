# Voicing, Synthesis & Tuning — Project Prosodia

Consolidated reference for **how voices are represented and synthesized** (latent style space,
parametric casting grid, VAD blending, narration modes) and **how the engine is calibrated** from
Tuner feedback (default drift, Director prompting, saturation limits).

---
---

# PART 1 — VOICING & SYNTHESIS

## 1. The Latent Style Space in StyleTTS2

StyleTTS2 decouples vocal identity (timbre) from behavioral dynamics (prosody, intonation, rate, volume) using two specialized networks:
1.  **Reference Encoder**: Extracts a single low-dimensional style vector (e.g. $1 \times 64$ or $1 \times 128$) from a 3–10 second reference audio clip. This vector represents the speaker's vocal tract characteristics.
2.  **Style Predictor**: Takes the phoneme sequence and the style vector to dynamically project frame-level pitch, energy, alignment, and phoneme durations.

> [!NOTE]
> Shifting from Kokoro-82M's length-indexed 2D style matrices (`[511, 256]`) to StyleTTS2's single-embedding vectors (64 or 128 floats) reduces the voice profile storage footprint from 1 MB down to less than 512 bytes on disk. This enables shipping large character casts lightweight.

## 2. Dynamic Parametric Voicing (Zero-Shot Casting)

Instead of forcing the LLM Director to map character dialogues to hardcoded voice file lists, the system evaluates character details and outputs continuous scales ($0.0$ to $1.0$) representing theatrical attributes. The Actor dynamically interpolates baseline timbre and style anchors in the latent style space at runtime.

### The Director's Casting Protocol
The LLM Director generates a structured casting payload for paragraph spans containing quotes:
*   `age_profile` ($0.0 = \text{Child}$, $1.0 = \text{Elderly}$)
*   `masculinity` ($0.0 = \text{Feminine}$, $1.0 = \text{Masculine}$)
*   `tempo_multiplier` ($0.5$ to $2.0$, speeds up rate natively)
*   `vocal_energy` ($0.0$ to $1.0$, drives pitch variance and intensity)
*   `strain_or_rasp` ($0.0 = \text{Smooth}$, $1.0 = \text{Gruff/Hoarse}$)

### Timbre & Style Anchors Grid
The [VoiceLoader](../../Prosodia/crates/actor/src/voice_loader.rs) holds baseline 64-dimensional style vectors:
*   **Timbre Anchors (Vocal Tract Identity $V$):**
    *   $V_{F, C}$: Female Child | $V_{M, C}$: Male Child
    *   $V_{F, A}$: Female Adult | $V_{M, A}$: Male Adult
    *   $V_{F, E}$: Female Elderly | $V_{M, E}$: Male Elderly
*   **Style Texture Anchors (Behavioral Dynamics $S$):**
    *   $S_{\text{clean}}$: Smooth, clean vocal tract closure.
    *   $S_{\text{gruff}}$: Rough, gravelly texture (raspiness).
    *   $S_{\text{breathy}}$: Soft, whispery friction.

### Bilinear Timbre Interpolation Formula
To compute the mixed speaker identity vector $V_{\text{mixed}}$ for an arbitrary `age_profile` ($a \in [0, 1]$) and `masculinity` ($m \in [0, 1]$), we perform multi-dimensional bilinear interpolation over our identity anchor grid.

First, partition the age dimension into two segments:
1.  **Child to Adult** ($a \in [0.0, 0.5]$): Scale to $a' = 2a$.
    $$V_{\text{age\_low}} = (1 - m) \cdot V_{F, C} + m \cdot V_{M, C}$$
    $$V_{\text{age\_high}} = (1 - m) \cdot V_{F, A} + m \cdot V_{M, A}$$
    $$V_{\text{mixed}} = (1 - a') \cdot V_{\text{age\_low}} + a' \cdot V_{\text{age\_high}}$$
2.  **Adult to Elderly** ($a \in [0.5, 1.0]$): Scale to $a' = 2(a - 0.5)$.
    $$V_{\text{age\_low}} = (1 - m) \cdot V_{F, A} + m \cdot V_{M, A}$$
    $$V_{\text{age\_high}} = (1 - m) \cdot V_{F, E} + m \cdot V_{M, E}$$
    $$V_{\text{mixed}} = (1 - a') \cdot V_{\text{age\_low}} + a' \cdot V_{\text{age\_high}}$$

### Style Texture Blending
The behavior style vector $S_{\text{mixed}}$ blends raspiness (`strain_or_rasp` $= r \in [0, 1]$) with clean defaults:
$$S_{\text{mixed}} = (1 - r) \cdot S_{\text{clean}} + r \cdot S_{\text{gruff}}$$

## 3. Voice Blending Math (VAD Space Mapping)

Subjective Valence/Arousal/Tension (VAD) outputs from the Director are resolved to continuous voice blends over anchor points mapped to coordinate spaces:

| Voice Anchor | Valence (V) | Arousal (A) | Tension (T) | Intended Role |
|---|:---:|:---:|:---:|---|
| **`anchor_narrator`** | `0.00` | `0.00` | `0.00` | Calm Neutral Narrator |
| **`anchor_sorrow`** | `-0.90` | `-0.50` | `0.05` | Grief / Mourning |
| **`anchor_tender`** | `-0.35` | `-0.55` | `0.08` | Tenderness / Softness |
| **`anchor_unease`** | `-0.20` | `-0.20` | `0.70` | Brooding / Suspicious |
| **`anchor_suspense`** | `-0.35` | `0.30` | `0.95` | Suspense / Anxiety |
| **`anchor_conversational`**| `0.40` | `0.20` | `0.15` | Warm Conversational |
| **`anchor_anticipation`** | `0.15` | `0.55` | `0.50` | Urgency / Excitement |
| **`anchor_joy`** | `0.75` | `0.60` | `0.20` | Optimistic Momentum |
| **`anchor_celebration`** | `0.95` | `0.95` | `0.00` | Shouting Peak Joy |

### Blending Algorithm
1.  **Target Scaling**: The VAD target vector is scaled by the **Expressiveness** multiplier to expand or flatten the dynamic range.
2.  **Gaussian Distance**: Distance $d$ between scaled target and anchor coordinate `at` maps to weight $w$:
    $$w = e^{-\frac{d^2}{2\sigma^2}}$$
    where $\sigma$ is the **Voice Blend Sigma** tuning coefficient.
3.  **Threshold Pruning**: Blend components falling below **Min Voice Blend %** (e.g. 5%) are pruned to maintain computing resource efficiency.
4.  **Snapping & Snubbing**: To keep identity stable, if a base voice is active, the narrator voice profile is anchored at a minimum of `60%` blend weight.

## 4. Audiobook Narration & Voice Acting Modes

The engine supports two primary narrative performance modes (the full-cast ambition is detailed in
[high-ambition-2-dramatic-reader.md](../../Sonora/github/notes/high-ambition-2-dramatic-reader.md)):

### Mode A: Solo Narrator Mode (Timbral Coloring)
A single narrator performs the entire narration but applies subtle changes to represent characters during dialogue quotes.
*   **Acoustic Coloring**: Instead of switching the voice entirely, blend $10\text{--}25\%$ ($\alpha$) of the character's style vector into the narrator's base vector:
    $$S_{\text{render}} = (1 - \alpha) \cdot S_{\text{narrator}} + \alpha \cdot S_{\text{character}}$$
*   **Pitch Biases**: Supplement the color with minor pitch offsets ($F_0$ shifts) via the `f0Bias` arrays (e.g. `P: 5.0` to raise pitch for children).

### Mode B: Full-Cast Mode (Direct Character Hand-off)
The narrator reads descriptive prose. When dialogue occurs, the voice transitions entirely to the character's style vector:
$$S_{\text{render}} = S_{\text{character}}$$

### Boundary Transition Smoothing
1.  **EMA style smoothing**: Transitions back to the narrator are smoothed across adjacent phrase boundaries using an Exponential Moving Average to prevent jarring timbre hops:
    $$S_{\text{smooth}, t} = \beta S_{\text{raw}, t} + (1 - \beta) S_{\text{smooth}, t-1}$$
2.  **Punctuation-Gated Pauses**: Inject brief silent buffers ($150\text{--}350\text{ ms}$) at narrator-character seams to simulate natural human speaking cadences.
3.  **Silence Join Trimming**: Edge-silence overlaps are stripped at segment boundaries, combined with micro-crossfades ($5\text{--}10\text{ms}$) to guarantee gap-free playback.

## 5. Character Pre-Determination & Spoiler-Safety

> This is the **character/casting slice** of the broader Director story graph — see
> [director-narrative-memory.md](director-narrative-memory.md), which generalizes the Character
> Directory + Chat Amnesia + `characterOffset` mechanism below into a full plot/twist/knowledge graph
> with a reading-position **reveal frontier** (and the spoiler-safe "chat with the narrator" feature).

To resolve dialogue roles before they are explicitly identified in text (e.g. opening quotes whose speaker is revealed paragraph-blocks later):

1.  **Ingestion Pre-Pass**: During EPUB ingestion, pre-read the text to generate a global **Character Directory** mapping names to casting profile weights.
2.  **Spoiler-Safety (Chat Amnesia)**: To prevent conversational assistants from spoiling future plot points, keep the pre-reading directory isolated. Progressive loading gates character context memories to only match content behind the user's active `characterOffset` bookmark.

---
---

# PART 2 — TUNER FEEDBACK & CALIBRATION

## 1. Feedback Ingestion & Calibration Flow

Subjective reviews are submitted through the `ProsodiaTuner` audition feedback sheet. They are appended to `ProsodiaTuner/TuningFeedback.md` and parsed into:
*   **Performance Rating**: Stars count (1–5). Ratings of 4/5 and 5/5 represent positive feedback milestones.
*   **VAD Coordinates**: Core Valence, Arousal, and Tension settings.
*   **Global Parameters**: Expressiveness, pause lengths, and voice matrix blending variables.
*   **Remarks**: Qualitative observations on pacing, pronunciation, stutters, and accents.

### Calibration Rules (Sweet-Spot Drift)
*   **Rule A: Default Value Calibration**: If positive reviews show a parameter is consistently elevated, shift the engine default setting closer to the median of the elevated values (e.g. shifting default `Expressiveness` from `1.5` to `3.25`).
*   **Rule B: Range Boundary Expansion**: If positive reviews cluster near boundaries, expand the slider limits (e.g. expanding max `Expressiveness` from `5.5` to `10.0`) to avoid UI clipping while staying within stable model limits.
*   **Rule C: Normalization Gains**: If elevated defaults increase expressiveness but destabilize neutral readings, reduce the underlying gains in the VAD formula, widening the user's stable linear slider range.

## 2. Active Audition Trends & Tuning Resolutions

Based on feedback logs compiled through June 10, 2026, the following calibrations have been applied to the default engine configurations. (These default values are now centralized in the Rust core — `crates/stage/src/acoustic_matrix.rs` and `audio_shaping.rs`, aligned 2026-06-15; see [CHANGELOG.md](CHANGELOG.md).)

### Trend 1: Expressiveness Limits
*   *Observation*: Lower expressiveness levels (defaults near `1.5`) suffered flat, unexpressive delivery. Elevated VAD ranges (3.0+) performed better.
*   *Resolution*: Tuned default Expressiveness from `3.0` to `3.25` and expanded the maximum slider limit to `10.0`.

### Trend 2: Muted Excitement (Arousal)
*   *Observation*: High-arousal exclamations felt flat, lacking corresponding speed/volume scaling.
*   *Resolution*: Raised the default `gainArousalGain` from `0.20` to `0.25` to allow positive arousal to scale volume higher.

### Trend 3: Storytelling Pacing & Pauses
*   *Observation*: Default phrase transitions felt rushed, causing narration to sound unnatural.
*   *Resolution*: Calibrated default `pauseClause` from `0.14s` to `0.25s`, and default `pauseSentence` from `0.24s` to `0.28s` to create natural conversational breath pauses.

### Trend 4: Valence & Tension Sensitivity
*   *Observation*: Valence speed and volume scaling was too weak at default settings.
*   *Resolution*: Raised default `speedValenceGain` from `0.015` to `0.05`, default `gainValenceGain` from `0.06` to `0.08`, and default `speedTensionGain` from `0.06` to `0.10`.

## 3. Director Prompting Advice & Few-Shot Examples

To continuously align Director outputs with these calibrations, system prompts (`soloNarratorSystemPrompt` and `fullCastSystemPrompt`) incorporate guidelines and few-shot examples for the LLM.

### 1. High Excitement & Celebratory Exclamations
*   *Guidance*: Maximize Arousal ($A \ge 0.90$) and Valence ($V \ge 0.90$). Add positive pitch bias (`P: 5.0` to `8.0`) on terminal expletives to simulate rising pitch.
*   *Example*:
    `[V: 0.90 A: 1.00 T: 0.70 P: 5.0] We won! [V: 0.90 A: 1.00 T: 0.80 P: 7.0] We actually won the championship!`

### 2. Suspenseful Climax / Threat
*   *Guidance*: Elevate Tension ($T \ge 0.70$) and Arousal ($A \ge 0.50$). Apply negative pitch bias (`P: -5.0` to `-10.0`) on threatening verbs/nouns to drop the tone.
*   *Example*:
    `[V: -0.30 A: 0.50 T: 0.60] he threw open his cell, [V: -0.40 A: 0.60 T: 0.70] leaped upon her [V: -0.50 A: 0.70 T: 0.85 P: -5.0] and grabbed her throat!`

### 3. Somber, Reflective Prose
*   *Guidance*: Use negative Valence ($V \le -0.40$), near-zero or slightly negative Arousal ($A \approx -0.10$), and moderate Tension ($T \approx 0.40$).
*   *Example*:
    `[V: -0.50 A: -0.10 T: 0.40] He had died alone, [V: -0.40 A: -0.20 T: 0.40] and the old house remembered him.`

### 4. Alphanumeric Vernacular ("Zero" -> "Oh")
*   *Guidance*: Detect codes/numbers containing "0" that human readers pronounce colloquially, and append the phonetic pronunciation tag (`PN: <phonetic representation>`).
*   *Example*:
    `[V: 0.30 A: 0.40 T: 0.20] He drove his Corvette [V: 0.30 A: 0.50 T: 0.20 PN: Z-oh-six] Z06 [V: 0.20 A: 0.20 T: 0.10] down the stretch of highway.`

## 4. Saturation & Clamping Boundaries

Tuning adjustments must respect mathematical saturation limits in the engine calculations:
*   **VAD Saturation**: `EmotionVector` coordinates are clamped to `[-1.0...1.0]` (and `[0.0...1.0]` for tension). If the expressiveness multiplier pushes a value beyond these bounds, the vector saturates.
*   **Speed/Volume Capping**: Speed and volume formulas have fixed caps. If the cap is lower than the UI's `Speed Max Limit`, increasing the limit slider has no effect on VAD-driven narration.
*   **Voice Blending Kernel Saturation**: If `blendSigma` is set excessively high, the Gaussian kernel becomes so wide that all anchor voices are blended uniformly, making emotional changes imperceptible.
