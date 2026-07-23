# Director fine-tune — Gemma 4 E2B (roadmap capture, 2026-07-20)

Owner call 2026-07-20: fine-tuning Gemma 4 E2B as Prosodia's on-device director has real
value and goes on the roadmap. This note records the reasoning and the trigger conditions so
the item can be picked up cold. Public-facing milestone entry: `Prosodia/Docs/ROADMAP.md`.

## Why

- **It completes the on-device story.** The acoustic side (Sonora LiteRT split-graph lane)
  already targets mobile; the apps already run **stock** `gemma-4-E2B-it.litertlm` as the
  `director-light` role over the LiteRT-LM C-API. A director that needs lab-server Gemma is
  half a product — a fine-tuned E2B closes the text → SCM markup → VAT synthesis loop
  entirely on-device. This is a *deployment* win more than a quality win.
- **The training data accrues for free.** Every lab director-pass yields (text → SCM markup)
  pairs, and the Dataset Auditions workflow (ratings.csv SSOT) layers accept/reject labels on
  top — a growing, quality-filtered distillation corpus as a byproduct of normal operations.
  Teacher-on-tap means unlimited synthetic pairs on demand.
- **The setup is maximally favorable.** Narrow task, structured output (SCM v-schema),
  frozen teacher to distill from, Unsloth already standing on ai-lab-0 for a QLoRA-class
  fine-tune of a ~2B model.

## Why not yet — trigger conditions (all three)

1. **SCM schema stabilizes** — v0.1 just had the T-axis rescoped (LAX↔TIGHT), valence audited
   at 62%; v1.1 (Emilia) lands changes. Fine-tuning now bakes a draft schema into weights;
   every amendment then costs a retrain instead of a prompt edit.
2. **Round-trip audition passes** (the post-vat3 SCM acceptance test) — proves the
   markup→acoustics loop produces what the director intends *before* we compress the
   director. Don't distill an unvalidated instruction language.
3. **Enough audited pairs banked** — months of director-pass output with audit verdicts.

## Role separation (preserve this)

The **frozen-annotator rule stands**: big frozen Gemma 4 remains the corpus labeler — that's
what keeps dataset labels consistent across campaigns ([[scm-markup-schema]] scope note).
The fine-tuned E2B is a *runtime* director for Prosodia — a separate artifact with a separate
job. The roles diverging is a feature, not a conflict.

## Acceptance bar

Match or beat the frozen teacher's markup-audit pass rate (93% at audit-markup-v0) on
held-out audit cards, evaluated with the same audition workflow. Ship as a `director-light`
role artifact via `prosodia_models.json` — the Debt-F role-key design means deployment is a
config edit, no app changes.

Related: [[vat-audit-verdicts]], [[scm-markup-schema]], [[audio-review-directory]],
`notes/director-narrative-memory.md`, `notes/next-steps.md` (milestone 3+).
