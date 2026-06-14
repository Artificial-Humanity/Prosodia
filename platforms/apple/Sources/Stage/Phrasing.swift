import Foundation
import Kit

public final class SentencePhraser: ProsodyPhraser {
    private let segmenter: SentenceSegmenter
    public init(segmenter: SentenceSegmenter = SentenceSegmenter()) {
        self.segmenter = segmenter
    }
    public func spans(text: String, emotion: EmotionVector) -> [ProsodySpan] {
        let sentences = segmenter.sentences(in: text)
        var allSpans: [ProsodySpan] = []
        for sentence in sentences {
            let clauses = splitIntoClauses(sentence)
            for clause in clauses {
                allSpans.append(ProsodySpan(text: clause, emotion: emotion, leadingPause: 0.0, acoustics: nil))
            }
        }
        return allSpans
    }
    private func splitIntoClauses(_ sentence: String) -> [String] {
        var clauses: [String] = []
        var start = sentence.startIndex
        let separators: Set<Character> = [",", ";", ":", "—", "–"]
        var i = sentence.startIndex
        while i < sentence.endIndex {
            let char = sentence[i]
            if char == "," {
                let nextIdx = sentence.index(after: i)
                if i > sentence.startIndex && nextIdx < sentence.endIndex {
                    let prevIdx = sentence.index(before: i)
                    if sentence[prevIdx].isNumber && sentence[nextIdx].isNumber {
                        i = sentence.index(after: i)
                        continue
                    }
                }
            }
            if separators.contains(char) {
                let nextIdx = sentence.index(after: i)
                let clause = String(sentence[start..<nextIdx]).trimmingCharacters(in: .whitespacesAndNewlines)
                if !clause.isEmpty { clauses.append(clause) }
                start = nextIdx
            }
            i = sentence.index(after: i)
        }
        let remaining = String(sentence[start..<sentence.endIndex]).trimmingCharacters(in: .whitespacesAndNewlines)
        if !remaining.isEmpty { clauses.append(remaining) }
        return clauses
    }
}

public final class SingleSpanPhraser: ProsodyPhraser {
    public init() {}
    public func spans(text: String, emotion: EmotionVector) -> [ProsodySpan] {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }
        return [ProsodySpan(text: trimmed, emotion: emotion, leadingPause: 0.0, acoustics: nil)]
    }
}
