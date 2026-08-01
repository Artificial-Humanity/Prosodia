# Voice Interruption & Discussion — the "Solo Book Club" interaction layer

> A **first-class, long-intended** goal that was de-prioritized across the project's platform
> transitions (Kotlin Multiplatform → pure Swift → Rust + multiplatform, plus reorganizations) and must
> not be lost again. The original project name was **"Solo Book Club"**: the listener can **interrupt
> the narration by voice** to ask a question or *discuss* the material read so far. This note is the
> **interaction/voice-input layer**; the **knowledge substrate** it draws on is
> [director-narrative-memory.md](director-narrative-memory.md). Together they are the Solo Book Club
> feature. This capability (voice interruption + spoiler-bounded Q&A) was once a patent track; it is
> now **defensively published, not filed** — see
> [open-decision-licensing.md](open-decision-licensing.md) and
> [patent-disclosure-expressive-control.md](patent-disclosure-expressive-control.md).
>
> **When this layer is approached, see also** [high-ambition-6 — the "Audience" (conveyance-aware STT)](../../Sonora/github/notes/high-ambition-6-audience-conveyance-stt.md): the voice-input side should perceive prosodic conveyance (emphasis, V/A/T) through the same control contract the Director dictates through — captured 2026-07-13.

---

## 1. Objective — barge-in to ask or discuss, then resume

While the Actor is narrating, the listener can **speak up at any time** to:
1. **Ask** a factual question about the story so far ("remind me who Mr. Tumnus is", "what happened at
   the manor?"), or
2. **Discuss** the material conversationally ("what did you make of that chapter?", "why would she lie
   to him?") — open-ended, multi-turn.

The system pauses narration, answers/discusses via the narrator's voice, and resumes from the pause
point. All responses are **spoiler-bounded** to what's been read (for narrative works) — see §5.

---

## 2. The honest gap — this is a net-new subsystem

The project today is **output-only**: the Actor renders speech (TTS); there is **no speech *input***
anywhere in the stack (no ASR, no microphone capture, no voice-activity detection — note "VAD" in
Prosodia means **Valence/Arousal/Tension**, not voice activity). So voice interruption is the single
largest net-new subsystem this feature requires. The Director (Gemma) and the Actor (Matcha/StyleTTS2)
exist; the **ear** does not.

---

## 3. Architecture sketch (three new concerns + reuse)

```
   listener speaks  ──▶  [Voice-input layer]  ──▶  transcript ──▶  Director (Gemma)
   (mic, while TTS playing)   on-device ASR              │            reasons over the
        │                     + barge-in detect          │            narrative-memory graph,
        │                     + echo cancellation         │            spoiler-bounded (§5)
        ▼                                                 ▼
   pause narration (Stage) ◀───────────────────  answer/discussion text
                                                          │
                                                          ▼
                                              Actor (TTS) speaks the answer
                                                          │
                                                          ▼
                                          resume narration from pause point (Stage)
```

- **Voice-input layer (new):** on-device **ASR** (e.g., a Whisper-class or other permissively-licensed
  on-device STT), **barge-in detection** (notice the listener is speaking *over* active narration and
  duck/pause), **acoustic echo cancellation** (the mic is open while the Actor's TTS is playing — must
  not transcribe our own narration), and an **activation model** (push-to-talk button vs. wake word vs.
  always-listening — privacy/battery tradeoff).
- **Turn management (new):** pause/resume hooks into the **Stage** coordinator (it already owns playback
  state, bookmarks, and the lookahead/backpressure buffer — reuse it), plus multi-turn conversation
  state for *discussion* mode.
- **Reasoning (reuse + extend):** the transcript routes to the **Director** (Gemma on LiteRT-LM), which
  answers from the **narrative-memory graph** under the reveal-frontier gate (§5). Discussion mode is a
  conversational extension of the same grounded reasoning.
- **Output (reuse):** answers are spoken by the **Actor** (the same TTS), so the narrator literally
  answers in-voice — reinforcing the "talking to the narrator" illusion.

---

## 4. Modality — voice-first (text as fallback)

The original Solo Book Club intent is **voice** interruption — it is the differentiator and the
accessibility win (hands-free, eyes-free). A **text** chat affordance is a reasonable secondary/fallback
(type a question), and the knowledge/reasoning layer is identical either way. The abandoned
patent draft's claims were voice-centric, which matches this intent; the capability is not yet
built, and this note is where it is planned.

---

## 5. Spoiler-safety & fiction/non-fiction (shared with narrative memory)

Answers and discussion are bounded by the **reveal frontier** defined in
[director-narrative-memory.md](director-narrative-memory.md): for narrative works (fiction and
narrative non-fiction) the narrator only "knows" what the listener has heard so far; for reference works
(technical manuals) the gate is off and discussion can range over the whole document. Filtering happens
at retrieval, before Gemma sees anything, so the narrator cannot leak the ending even when asked
directly. A **future-content deflection** ("you'll find out — keep reading") is the graceful response to
"how does it end?" queries.

---

## 6. Open questions / risks

- **On-device ASR:** model choice, size, latency, and accuracy on mobile; permissive license (same bar
  as the rest of the stack); language coverage (ties to [high-ambition-4-multilingual-g2p.md](high-ambition-4-multilingual-g2p.md)).
  → **Leading candidate as of 2026-08-01: the Baichuan-Audio tokenizer** (Whisper Large encoder +
  8-layer RVQ @ 12.5 Hz, **Apache-2.0 weights**, ZH/EN). It is the one surveyed option that keeps
  *semantic and acoustic* content in a single representation — i.e. it can hear the listener's
  **tone**, not just their words, which is what makes the ear symmetric with the contract per
  [high-ambition-6](../../Sonora/github/notes/high-ambition-6-audience-conveyance-stt.md).
  **Take the tokenizer, not the 7B LLM backbone** — the end-to-end design would dissolve the
  Director↔contract seam that §5's reveal-frontier gate depends on. Full analysis, licence flags and
  the alternatives considered: [matcha-siblings-study.md](../../Sonora/github/notes/matcha-siblings-study.md).
- **Barge-in over active TTS:** acoustic echo cancellation so the system doesn't hear itself; duck vs.
  hard-pause on detected speech.
- **Activation UX & privacy:** push-to-talk vs. wake-word vs. always-listening. On-device ASR (no cloud)
  is the privacy-preserving choice and aligns with the [north star](architecture-north-star.md) — the
  whole pipeline (ASR → Director → Actor) stays on device.
- **Discussion vs. Q&A:** multi-turn context management; keeping discussion grounded (no fabricated
  plot) and spoiler-bounded across turns.
- **Latency budget:** mic → ASR → Gemma → TTS round-trip must feel conversational without stalling the
  reading experience.
- **Battery/thermal** of an open mic + on-device ASR during long sessions.

---

## 7. Roadmap positioning (dependencies)

Voice interruption sits **downstream** of: (a) a shipped **Actor** (TTS) — see
[high-ambition-1-matcha-actor.md](../../Sonora/github/notes/high-ambition-1-matcha-actor.md); (b) the **Director** (exists); and
(c) the **narrative-memory graph** — see [director-narrative-memory.md](director-narrative-memory.md) —
for grounded, spoiler-safe answers. The **on-device ASR / barge-in** subsystem, however, can be
prototyped independently and in parallel, since it's the one piece with no current foothold. Net: a
later milestone, but the highest-novelty/identity piece of the product ("Solo Book Club") and the
subject of the defensive publication — so it belongs explicitly on the long-term roadmap, not in tribal
memory.

---

## 8. Relationship to other notes

- [director-narrative-memory.md](director-narrative-memory.md) — the knowledge substrate this layer
  queries; spoiler-safety, reveal frontier, fiction/non-fiction switch, pre-reading all live there.
- [voicing-synthesis-and-tuning.md §5](voicing-synthesis-and-tuning.md) — the character/casting +
  `characterOffset` seed; also the voice-casting **gate** (don't reveal a character's true voice ahead
  of the text), which is the *acoustic* analog of spoiler-safety.
- [architecture-north-star.md](architecture-north-star.md) — the Director + its knowledge/interaction
  is the ownable core; an on-device ear deepens that and keeps the whole loop private and offline.
- [patent-disclosure-expressive-control.md](patent-disclosure-expressive-control.md) — the retained
  invention capture (**no filing — defensively published**); its "voice interruption and Q&A module"
  and spoiler-bounded answering correspond to this note and §1–§5.
