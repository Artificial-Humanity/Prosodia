# Roadmap

> Placeholder — the public, forward-looking direction for Project Prosodia.
>
> Near-term execution detail and the current must-do list live in the private working
> notes; this roadmap captures the longer-horizon milestones and releases as they firm up.

## In progress

- **Run Sonora's current actor models on device.** Bring the Rust actor runtime up to
  Sonora's current export contract — multi-speaker (a speaker vector per voice), native
  24 kHz, and the V/A/T + delivery conditioning inputs of direction contract v2 — and
  match the device text front end to the one the model was trained on. Today the apps
  run Sonora's first single-speaker 22.05 kHz model.

## Planned

- **Fine-tuned on-device Director (Gemma 4 E2B).** Distill the lab's conveyance-markup
  director into the E2B-class model the apps already run as the `director-light` role,
  completing the fully on-device pipeline: text → conveyance markup → expressive synthesis.
  Gated on the conveyance markup schema stabilizing and the markup→acoustics round-trip
  validation passing; acceptance is parity with the lab director on held-out audited
  material. *(Added 2026-07-20; not started as of 2026-09-26 — both gates are still open.
  Rationale and trigger detail in the private notes.)*
