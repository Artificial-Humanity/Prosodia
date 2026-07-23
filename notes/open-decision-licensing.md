# Licensing & IP Posture — DECIDED (closed 2026-07-23)

_**Final decision (owner, 2026-07-23): defensive declaration on GitHub + fully open license
for all of it.** No patent pursuit; the public, dated, enabled disclosure is the deliberate
prior-art anchor, and the detailed invention-capture notes are intentionally public in this
repo. The analysis below is kept as the record of how the decision was reached._

## The component goals (owner's words, 2026-07-13)

* **Prosodia** — the novel system: an on-device *thinking model* (Director) continuously directing an
  on-device *TTS model* (Actor) to produce human-like speech from plain text, plus the conversational
  capacity of that text. **Might be sold** if a market appears. Dual license (GPL-3.0 + commercial
  exception, McFarlin Technologies, LLC) was adopted as a theft safeguard at the time.
  *(Superseded by the 2026-07-23 decision: Prosodia is now fully open — Apache-2.0, per the
  repo LICENSE.)*
* **Sonora** — the model: dynamic prosody + multi-voicing at a tiny size. **For everyone**; the return
  is recognition. Apache-2.0 (repo and HF weights; upstream Matcha MIT preserved).

## Why the current split fits

The pivotal fact is technical: `sonora.tflite` is a standard TFLite artifact — anyone can run the
Apache weights in a stock LiteRT interpreter without Prosodia. Therefore:

1. The "for everyone" goal is already fully served: the model, its future VAT-conditioned successors,
   and the model card's conditioning contract spread at Apache speed. Prosodia's GPL holds back
   nothing on the Sonora side.
2. Symmetrically, the GPL guards only what it should: Prosodia's own code (stage/director
   orchestration, Rust G2P, expressive-control plumbing, apps) — the maybe-sellable product.
3. License flow is clean: GPL Prosodia consuming Apache Sonora is fine; our own apps ship fine
   despite GPL because we hold all copyright (dual-licensing = we can grant ourselves anything).

**Accepted tension (knowingly):** an open Sonora with a documented conditioning contract means anyone
can build a competing director for it. You cannot have "everyone uses my directable model" and "only
I can direct it." The split chooses ecosystem + recognition for the model, and lets Prosodia compete
on execution: corpus, velocity, and being the people who trained the model.

**Considered and set aside (for now):** relicensing the embeddable core crates to Apache/MPL-2.0
(engine-open/product-guarded, Chromium-style). Right move only if the goal shifts from "Prosodia is
the product" to "Prosodia is the reference runtime others embed." Revisit if adoption-by-embedders
ever becomes the priority; note GPLv3 effectively blocks iOS/App Store embedding by third parties.

## Tightenings — status

1. ✅ **CLA** — already in place: public `Docs/CONTRIBUTING.md` §1 grants McFarlin Technologies, LLC
   an irrevocable copyright license incl. sublicensing "under any license terms chosen by the
   Company" plus a patent grant and original-work representation. Dual-licensing survives outside
   contributors. (Verified 2026-07-13; nothing to add.)
2. ⬜ **Trademarks** — "Prosodia", "Sonora", "Artificial Humanity". Apache/GPL let anyone
   redistribute; only the marks keep the origin identity (the actual recognition asset). Needs a
   real lawyer when acted on.
3. ⬜ **Sonora data cleanliness** — the "for everyone" promise requires every training input to be
   permissive. **Expresso is CC-BY-NC-4.0** and is penciled into the roadmap (casting grid /
   milestone 3) — replace it as a *training* source (fine as listening reference). Full survey with
   verified licenses now lives in
   [dataset-landscape.md](../Sonora/dataset-landscape.md) — headline keepers: LibriTTS-R (CC-BY),
   Parler's annotated LibriTTS-R (CC-BY), cdminix/libritts-r-aligned prosody measures (CC-BY),
   **Emilia-YODAS subset only** (CC-BY — the original 101k-h Emilia subset stays CC-BY-NC; the repo
   tag is misleading), GLOBE V2 (CC0).
4. ✅ **Patent decision** — Path B executed 2026-07-13; see below.

## IP / prior-art analysis (2026-07-13)

**Facts:** the mechanism has been publicly disclosed in enabled form since ~**2026-06-13/14** — the
public Prosodia repo contains the working control-contract code (`crates/stage/src/prosody_payload.rs`
et al.), the casting grid and acoustic matrix, and a README naming the director→actor design with
VAD and casting. The detailed invention-capture (patent-disclosure-expressive-control.md + Eureka
drafts + root PATENT.md) was **private** in the umbrella repo at the time of this analysis.
**Resolved 2026-07-23:** with the defensive-declaration decision (header), the capture note's
presence in this public repo's `notes/` is deliberate — it strengthens the prior-art anchor
rather than leaking anything worth protecting.

**Consequences:**

* **The prior-art (defensive) argument is NOT eliminated — it is anchored by the public repo.**
  Public, dated (git history), *enabled* (working code) disclosure is invalidating prior art against
  later filers for everything the public code shows. Keeping the Eureka material private neither
  helps nor hurts that; private documents are simply not prior art.
* **Two real gaps in the defensive posture:**
  1. **Coverage:** the not-yet-built embodiments — above all the milestone-3 *trained-to-obey*
     per-token conditioning actor, and the spoiler-aware casting gate — exist only in private notes.
     They currently have **no** prior-art protection; a third party could file on them.
  2. **Discoverability:** code-only prior art is weak in *practice* (examiners rarely find GitHub
     source; it surfaces mainly in litigation/IPR). A deliberate, indexable defensive publication
     (e.g., TDCommons, or a dated technical article) of the full mechanism makes the defense usable
     at examination time, not just in court.
* **Own-filing optionality:** the ~2026-06-13 public disclosure means (a) absolute-novelty
  jurisdictions (EU etc.) are likely already forfeited for the *disclosed* features; (b) the US
  12-month grace period runs to ~**June 2027** for those features; (c) the *undisclosed* milestone-3
  specifics retain full optionality — until they ship in the public repo or registry.

**The fork — DECIDED: Path B, executed 2026-07-13.**
[Defensive publication](../../Prosodia/Docs/defensive-publication-expressive-control.md) committed
to the public Prosodia repo (`946bcc2`), linked from the public README: enabling disclosure of the
full expressive-control mechanism including the prophetic milestone-3 trained-to-obey conditioning
and the spoiler-aware casting gate, mechanisms labeled [implemented] vs [contemplated], document
CC BY 4.0. Owner's rationale on record: Prosodia is the real crown jewel and Sonora enables it, but
sellability is doubted (depends on DRM-free ebooks); the primary goal is protection from outright
theft, with acquisition as the upside scenario — a posture served by freedom-to-operate + prior
art + brand rather than a patent portfolio. Own-filing optionality for THIS invention is
intentionally spent (US grace technically runs to ~June 2027 from the June code disclosure, but the
decided posture is defensive).

**Scope guard:** the OTHER patent track — the incremental narrative-knowledge-graph / spoiler-free
Q&A invention — was deliberately EXCLUDED from the publication and remains private with full
optionality. Do not publish its mechanism without a fresh, explicit decision.

**Discoverability follow-ups (optional, owner's call):** mirror the publication where examiners
search — TDCommons submission and/or a dated artificialhumanity.io article; a Zenodo DOI adds an
independent timestamp. (Account-requiring venues are owner actions.)

_Strategy notes, not legal advice; trademark and any filing deserve a professional hour._
