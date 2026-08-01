# Director Narrative Memory — the story graph, spoiler-safe narrator chat, and pre-reading

> A **Director-side** subsystem (distinct from the actor/voice high-ambition series): a persistent,
> queryable graph of what the book has established — plot points, character twists, relationships, and
> general knowledge — that the Director maintains as the reader progresses. It serves two consumers:
> the Director's own performance reasoning, and a planned **"chat with the narrator"** feature. The
> load-bearing constraint is **spoiler-safety**: for narrative works, nothing past the reader's current
> position may surface. Generalizes the character-only seed in
> [voicing-synthesis-and-tuning.md §5](voicing-synthesis-and-tuning.md). The Director is the
> ownable core of the project ([architecture-north-star.md §2](architecture-north-star.md)), so this
> memory layer is squarely "ours."

---

## 1. Objective — two deliverables on one substrate

1. **The story graph (the substrate).** The Director maintains a structured, persistent graph of the
   book's established knowledge so it can **refer back to past events without re-reading the whole
   book**. This improves performance reasoning (consistent casting/emotion: "who is this, what's their
   arc, how do they relate to the speaker") and is the backing store for #2. Today the Director only
   does ephemeral *rolling paragraph-history compilation*
   ([architecture-and-development.md](architecture-and-development.md), `director` crate) — this is the
   durable, queryable upgrade.
2. **Chat with the narrator (the consumer feature).** A conversational mode where the reader can ask
   the narrator about the story so far ("remind me who Mr. Tumnus is," "what happened at the manor?").
   It answers **only** from graph content at or behind the reader's position. The voice-input /
   barge-in interaction layer for this (the "Solo Book Club" feature) is detailed in
   [voice-interruption-and-discussion.md](voice-interruption-and-discussion.md); this note is the
   knowledge substrate it queries.

---

## 2. Scope — when spoiler-gating applies

| Content type | Gating | Rationale |
|---|---|---|
| **Narrative** — novels (fiction *and* biographical/narrative non-fiction), memoirs, story-driven works | **ON** (reveal-frontier enforced) | A future twist must never leak ahead of the reader |
| **Reference** — technical manuals, textbooks, cookbooks, API docs | **OFF** (full graph always visible) | No spoilers; random access is a *feature* ("where is X explained?") |

The fiction/non-fiction switch is a per-book property (detected at ingestion, user-overridable). It
selects whether the reveal-frontier gate (§4) is active — it does **not** change how the graph is
*built* or *stored*, only what the consumers may *see*.

---

## 3. The story graph — data model

Nodes (typed): **Character**, **PlotPoint / Event**, **Twist / Revelation**, **Relationship**,
**Location / Setting**, **Fact / General-Knowledge** (world rules, definitions; for manuals: concepts,
procedures, cross-references). Edges carry semantics (e.g. `character —betrays→ character`,
`event —causes→ event`, `fact —defined-in→ section`).

**Every node and edge is stamped with two things** (this is what makes both spoiler-safety and
incremental update work):
- **`reveal_offset`** — the reading position at which this fact first becomes *known to a linear
  reader*. For a twist that is foreshadowed on page 20 but revealed on page 200, `reveal_offset` is
  **page 200** (the reveal, not the foreshadowing).
- **`source_span`** — the text span(s) that established it, for citation/trace and re-extraction.

Storage is local and incremental (append/refine as the reader advances or as pre-reading runs). It
extends the existing `characterOffset`-bookmarked **Character Directory**
([voicing-synthesis-and-tuning.md §5](voicing-synthesis-and-tuning.md)) from "names → casting weights"
to the full typed graph; the Character Directory becomes the Character-node slice of this graph.

---

## 4. The reveal frontier — the one mechanism that reconciles everything

> **The reader's current position defines a frontier. Consumers see only nodes/edges with
> `reveal_offset ≤ position`. Everything past the frontier is invisible.**

This single rule resolves the apparent tension between "keep the pre-read isolated for spoiler-safety"
(§5 of the voicing note) and the stretch goal of "pre-read the whole book to build the graph":

- The graph may be **fully built** (even pre-read end-to-end) **without** compromising spoilers,
  *provided* every node is correctly `reveal_offset`-stamped.
- Spoiler-safety is then a **query-time filter**, not a build-time restriction.
- For **reference** books the filter is simply disabled (§2), so the whole graph is always queryable.

The hard part is correct `reveal_offset` stamping under pre-reading: the extractor sees the whole book,
so it must attribute each fact to the *in-narrative reveal point*, not to wherever it first had
evidence. Mis-stamping a twist early is a spoiler leak — so `reveal_offset` accuracy is a **safety
property**, and the conservative default on uncertainty is to stamp *later* (hide longer), never
earlier.

---

## 5. Narrator chat — built on the gated graph

- Retrieval is **frontier-filtered first, then answered**: the chat assembles context only from
  graph nodes with `reveal_offset ≤ position`, and the Gemma prompt is constructed from that filtered
  set — the model is never shown future content, so it cannot leak it even under adversarial prompting
  ("just tell me how it ends"). Filtering at retrieval (not relying on the model to "not say") is the
  safety boundary.
- Answers can **cite `source_span`** ("…as of chapter 7") to stay grounded and reduce hallucination.
- Naturally bounded for the Director's *own* performance use too: it only ever performs up to the
  reader's position, so its working knowledge is already behind the frontier — the graph just makes
  recall O(query) instead of O(re-read).

---

## 6. Building the graph — progressive (default) vs pre-read (stretch)

- **Progressive (default, required for fiction).** Extend the graph incrementally as the reader/Director
  advances. Each newly read span is distilled (by Gemma) into nodes/edges stamped at the current
  position. Cheap, streaming, and spoiler-safe by construction (it literally cannot know the future).
- **Pre-read (stretch goal).** Run the extractor over the whole book ahead of time to materialize the
  full graph.
  - **Reference books:** the ideal mode — enables random access, "find where X is covered," and
    cross-reference resolution with no spoiler risk.
  - **Fiction:** allowed only with rigorous `reveal_offset` stamping (§4); the payoff is richer
    performance planning (e.g. the Director can pace a scene knowing its dramatic weight) while the
    frontier filter keeps the *reader-facing* chat honest. Higher risk — gate behind stamping
    confidence.

---

## 7. Open questions / risks (to resolve before building)

- **On-device extraction cost.** Distilling prose → graph with Gemma E2B/E4B on-device, incrementally,
  without stalling playback. Likely a background pass on the same lookahead budget the stage already
  has.
- **`reveal_offset` accuracy = a safety property.** A single early-stamped twist is a spoiler. Needs an
  evaluation harness (does the chat ever reveal a fact before its reveal point?) before pre-read-fiction
  is enabled.
- **Hallucination / fabricated plot.** The chat must answer from the graph, not invent; `source_span`
  citation + "I don't know yet" on empty retrieval.
- **Graph size & storage** across a 200-page book; incremental update and compaction.
- **Fiction/non-fiction detection** accuracy and the user override UX.
- **Identity dedup** — coreference ("the captain" = "Ahab") so the graph doesn't fragment a character
  across aliases (ties into casting's speaker-lock).

---

## 8. Relationship to existing notes

- [voicing-synthesis-and-tuning.md §5](voicing-synthesis-and-tuning.md) — the **seed**: Character
  Directory + Chat Amnesia + `characterOffset`. Becomes the Character/casting slice of this graph; its
  spoiler-safety rule generalizes to the reveal frontier (§4).
- [architecture-and-development.md](architecture-and-development.md) — the `director` crate's current
  *rolling paragraph-history* is what this durable graph replaces/extends.
- [high-ambition-2-dramatic-reader.md](../../Sonora/github/notes/high-ambition-2-dramatic-reader.md) — full-cast performance
  consumes the Character nodes (identity, relationships) the graph maintains.
- [architecture-north-star.md §2](architecture-north-star.md) — the Director + its knowledge is the
  ownable, durable core; this memory layer deepens that moat and is independent of the (swappable)
  actor.
