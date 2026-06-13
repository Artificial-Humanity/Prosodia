import Foundation
import NaturalLanguage

// MARK: - Sentence Segmenter

/// Splits book text into individual sentences using Apple's on-device NLP.
///
/// This is the seam between Stage 1 (Ingestion) and Stage 2 (Director). The
/// Director reads emotion from narrative context, so it needs more than a single
/// sentence at a time — see ``NarrationGrouping``. The segmenter also handles the
/// hard cases (abbreviations, ellipses, dialogue punctuation) better than a regex.
public struct SentenceSegmenter: Sendable {
    public let language: NLLanguage

    public init(language: NLLanguage = .english) {
        self.language = language
    }

    /// Returns the individual sentences in `text`, trimmed and non-empty.
    public func sentences(in text: String) -> [String] {
        guard !text.isEmpty else { return [] }
        let tokenizer = NLTokenizer(unit: .sentence)
        tokenizer.string = text
        tokenizer.setLanguage(language)

        var results: [String] = []
        tokenizer.enumerateTokens(in: text.startIndex..<text.endIndex) { range, _ in
            let sentence = String(text[range]).trimmingCharacters(in: .whitespacesAndNewlines)
            if !sentence.isEmpty { results.append(sentence) }
            return true
        }
        return results
    }
}

// MARK: - Narration Grouping

/// How chapter text is grouped into the chunks the Director annotates and the
/// Actor renders.
///
/// The Director infers emotion from *context*, so a single sentence is usually
/// too little — "Fine." reads neutral, tender, or devastating depending on the
/// surrounding story. ``paragraph(targetCharacters:)`` accumulates whole
/// sentences up to a soft character target so each chunk carries roughly a
/// paragraph of context. The target is intentionally a soft minimum, not a hard
/// cap: a single sentence longer than the target is still emitted whole, and a
/// chunk never splits a sentence. This measurement is expected to be tuned —
/// `400` characters is a first pass.
public enum NarrationGrouping: Sendable {
    /// One sentence per chunk. Minimal context; rarely the right choice for the
    /// real Director, but useful for tests and fine-grained control.
    case sentence
    /// Accumulate sentences until the chunk reaches `targetCharacters`, then emit.
    case paragraph(targetCharacters: Int = 400)

    /// Groups `sentences` into chunks per this policy.
    func group(_ sentences: [String]) -> [String] {
        switch self {
        case .sentence:
            return sentences
        case .paragraph(let target):
            var chunks: [String] = []
            var current = ""
            for sentence in sentences {
                if current.isEmpty {
                    current = sentence
                } else {
                    current += " " + sentence
                }
                if current.count >= target {
                    chunks.append(current)
                    current = ""
                }
            }
            if !current.isEmpty { chunks.append(current) }
            return chunks
        }
    }
}

// MARK: - BookDocument bridge

public extension BookDocument {
    /// Streams sentence-grouped chunks of the whole book in spine order — the
    /// input the ``DirectorInference`` consumes.
    ///
    /// - Parameters:
    ///   - segmenter: sentence tokenizer (default: English). Set the language
    ///     from book metadata when known for better boundary detection.
    ///   - grouping: how sentences are batched into chunks. Defaults to
    ///     paragraph-sized chunks so the Director has enough narrative context to
    ///     read emotion. Use ``NarrationGrouping/sentence`` for one sentence each.
    ///
    /// Chunks never cross a chapter boundary, so chapter structure is preserved.
    func narrationStream(
        segmenter: SentenceSegmenter = SentenceSegmenter(),
        grouping: NarrationGrouping = .paragraph()
    ) -> AsyncStream<String> {
        AsyncStream { continuation in
            Task {
                for index in 0..<chapterCount {
                    guard let chapter = try? await chapter(at: index) else { break }
                    let sentences = segmenter.sentences(in: chapter.text)
                    for chunk in grouping.group(sentences) {
                        continuation.yield(chunk)
                    }
                }
                continuation.finish()
            }
        }
    }

    /// Streams one sentence per chunk. Equivalent to
    /// ``narrationStream(segmenter:grouping:)`` with ``NarrationGrouping/sentence``.
    func sentenceStream(
        segmenter: SentenceSegmenter = SentenceSegmenter()
    ) -> AsyncStream<String> {
        narrationStream(segmenter: segmenter, grouping: .sentence)
    }
}
