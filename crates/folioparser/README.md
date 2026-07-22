# folioparser

EPUB parsing for Prosodia's on-device reader. ZIP/OCF container → OPF spine → EPUB2 NCX / EPUB3 nav
TOC → per-chapter clean plain-text extraction. uniffi-exported for Apple/Android.

Public surface:
- `parse_epub(path) -> Vec<EpubChapter { spine_index, title, text }>` — whole-book, chapter-ordered.
- `EpubTextExtractor::extract_plain_text(xhtml, options)` — XHTML → plain text; `options` lists
  ignored tags/classes (defaults drop `head/style/script/aside/table/footer/nav` +
  `footnote/toc/ad`).
- Container / OPF / EPUB3-nav / EPUB2-NCX parsers + `ZipArchive` reader.

## Scope boundary — segmentation is not here

FolioParser stops at **chapter-level plain text**. It is EPUB-only. Two things it deliberately does
NOT do:

- **Chunking/segmentation** lives downstream in [`stage::segmenter`](../stage/src/segmenter.rs) —
  `SentenceSegmenter` (quote-aware sentence split) + `NarrationGrouping::{Sentence,
  Paragraph{target_characters}}` (bounded-length grouping). Passage-level **VAD (Valence/Arousal/
  Tension) + casting** annotation is the [`director`](../director) crate (Gemma 4).
- **Non-EPUB inputs** (e.g. Project Gutenberg plain-text with header/trademark boilerplate) are not
  handled here and are intentionally not an on-device concern.

## Note for future functionality (2026-07-18)

The Sonora offline **book-prose synthesis** training-data pipeline reuses this exact chain —
FolioParser (EPUB→text) → `stage::segmenter` (chunk) → Gemma-4 director (VAD + casting) — to turn
permissive ebooks (Standard Ebooks, Project Gutenberg) into directed synthesis inputs. Every prep
run therefore **dogfoods the on-device parse→segment→direct path**.

Open design question parked here for the future: whether chunking should ever consolidate into
FolioParser (parse + segment in one crate) or stay split across `folioparser` + `stage::segmenter`.
No change proposed now — recorded so the reuse relationship is discoverable from the parser itself.

See `book-prose-operations.md` (operations plan) and `book-prose-synthesis-spike.md`
(rationale) in the `Sonora-GH` training repo's `notes/`.
