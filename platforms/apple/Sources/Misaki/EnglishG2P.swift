import Foundation
import NaturalLanguage

internal enum TokenUnit {
    case token(MToken)
    case group([MToken])
}

internal struct FeatureSpan {
    enum Kind {
        case stress(Double)
        case phonemeOverride(String)
        case numFlags(String)
    }

    let range: Range<String.Index>
    let kind: Kind
}

internal struct PreprocessResult {
    let text: String
    let featureSpans: [FeatureSpan]
}

internal func mergeTokens(_ tokens: [MToken], unk: String? = nil) -> MToken {
    let stressesFound = Set(tokens.compactMap { $0.underscore?.stress })
    let currenciesFound = Set(tokens.compactMap { $0.underscore?.currency })
    let ratingsFound = Set(tokens.map { $0.rating })

    let phonemes: String?
    if let unk {
        var merged = ""
        for token in tokens {
            if token.underscore?.prespace == true,
               !merged.isEmpty,
               !(merged.last?.isWhitespace ?? false),
               let tokenPhonemes = token.phonemes,
               !tokenPhonemes.isEmpty {
                merged.append(" ")
            }
            merged += token.phonemes ?? unk
        }
        phonemes = merged
    } else {
        phonemes = nil
    }

    let mergedText = tokens.dropLast().reduce(into: "") { partial, token in
        partial += token.text + token.whitespace
    } + (tokens.last?.text ?? "")

    let chosenTag = tokens.max { lhs, rhs in
        caseWeight(lhs.text) < caseWeight(rhs.text)
    }?.tag ?? (tokens.last?.tag ?? "")

    let mergedUnderscore = MToken.Underscore(
        isHead: tokens.first?.underscore?.isHead ?? true,
        alias: nil,
        stress: stressesFound.count == 1 ? stressesFound.first : nil,
        currency: currenciesFound.max(),
        numFlags: String(Set(tokens.flatMap { Array($0.underscore?.numFlags ?? "") }).sorted()),
        prespace: tokens.first?.underscore?.prespace ?? false,
        rating: ratingsFound.contains(nil) ? nil : ratingsFound.compactMap { $0 }.min()
    )

    return MToken(
        text: mergedText,
        tag: chosenTag,
        whitespace: tokens.last?.whitespace ?? "",
        phonemes: phonemes,
        startTS: tokens.first?.startTS,
        endTS: tokens.last?.endTS,
        underscore: mergedUnderscore
    )
}

private func caseWeight(_ text: String) -> Int {
    text.reduce(into: 0) { partial, character in
        if character.isUppercase {
            partial += 2
        } else {
            partial += 1
        }
    }
}

public final class G2P {
    public let version: String?
    public let british: Bool
    public let lexicon: Lexicon
    public let unk: String
    nonisolated(unsafe) public static var defaultFallback: (@Sendable (String, Bool) -> String?)? = nil
    public var fallback: (@Sendable (String) -> String?)? = nil

    private static let subtokenRegex = try! NSRegularExpression(
        pattern: #"^['‘’]+|\p{Lu}(?=\p{Lu}\p{Ll})|(?:^-)?(?:\d?[,.]?\d)+|[-_]+|['‘’]{2,}|\p{L}*?(?:['‘’]\p{L})*?\p{Ll}(?=\p{Lu})|\p{L}+(?:['‘’]\p{L})*|[^-_\p{L}'‘’\d]|['‘’]+$"#,
        options: []
    )

    public init(version: String? = nil, british: Bool = false, unk: String = "❓") throws {
        self.version = version
        self.british = british
        self.lexicon = try Lexicon(british: british)
        self.unk = unk
        if let defaultFallback = Self.defaultFallback {
            self.fallback = { [british] word in
                defaultFallback(word, british)
            }
        }
    }

    static func preprocess(_ text: String) -> PreprocessResult {
        let linkRegex = try! Regex(#"\[([^\]]+)\]\(([^)]*)\)"#, as: (Substring, Substring, Substring).self)
        let trimmed = text.drop(while: \.isWhitespace)
        var result = ""
        var featureSpans: [FeatureSpan] = []
        var currentIndex = trimmed.startIndex

        for match in trimmed.matches(of: linkRegex) {
            let wholeRange = match.range
            let visibleText = String(match.output.1)
            let featureText = String(match.output.2)

            if currentIndex < wholeRange.lowerBound {
                result += trimmed[currentIndex..<wholeRange.lowerBound]
            }

            let start = result.endIndex
            result += visibleText
            let end = result.endIndex

            if let feature = parseFeature(featureText) {
                featureSpans.append(FeatureSpan(range: start..<end, kind: feature))
            }

            currentIndex = wholeRange.upperBound
        }

        if currentIndex < trimmed.endIndex {
            result += trimmed[currentIndex..<trimmed.endIndex]
        }

        return PreprocessResult(text: result, featureSpans: featureSpans)
    }

    func tokenize(_ text: String, featureSpans: [FeatureSpan] = []) -> [MToken] {
        let tagger = NLTagger(tagSchemes: [.nameTypeOrLexicalClass, .lexicalClass, .nameType])
        tagger.string = text

        struct Slice {
            let range: Range<String.Index>
            let tag: String
        }

        var slices: [Slice] = []
        let options: NLTagger.Options = [.omitWhitespace]
        tagger.enumerateTags(in: text.startIndex..<text.endIndex, unit: .word, scheme: .nameTypeOrLexicalClass, options: options) { tag, range in
            let tokenText = String(text[range])
            slices.append(Slice(range: range, tag: Self.mapTag(tokenText: tokenText, tag: tag)))
            return true
        }

        var tokens: [MToken] = []
        tokens.reserveCapacity(slices.count)

        for (index, slice) in slices.enumerated() {
            let tokenText = String(text[slice.range])
            let whitespaceRange = slice.range.upperBound..<(index + 1 < slices.count ? slices[index + 1].range.lowerBound : text.endIndex)
            let whitespace = String(text[whitespaceRange])
            tokens.append(
                MToken(
                    text: tokenText,
                    tag: slice.tag,
                    whitespace: whitespace,
                    underscore: MToken.Underscore(isHead: true, numFlags: "", prespace: false)
                )
            )
        }

        // Refine tags for homographs (e.g. "read") based on sentence context
        for i in 0..<tokens.count {
            let token = tokens[i]
            let lowerText = token.text.lowercased()
            
            if lowerText == "read" {
                if token.tag.hasPrefix("VB") || token.tag == "VERB" || token.tag == "NN" {
                    var isPastParticiple = false
                    var isModal = false
                    
                    for j in max(0, i-3)..<i {
                        let prevText = tokens[j].text.lowercased()
                        if ["has", "have", "had", "was", "were", "been", "get", "got", "getting", "is", "am", "are"].contains(prevText) {
                            isPastParticiple = true
                        }
                        if ["will", "would", "should", "could", "can", "may", "might", "must", "shall", "to"].contains(prevText) {
                            isModal = true
                        }
                    }
                    
                    if isPastParticiple {
                        token.tag = "VBN"
                    } else if isModal {
                        token.tag = "VB"
                    } else {
                        var subjectTag: String? = nil
                        var subjectText: String? = nil
                        for j in (0..<i).reversed() {
                            let prevToken = tokens[j]
                            if [".", ",", ";", ":", "CC"].contains(prevToken.tag) || ["and", "but", "or", "because", "although", "when", "if", "while"].contains(prevToken.text.lowercased()) {
                                break
                            }
                            if prevToken.tag == "PRP" || prevToken.tag.hasPrefix("NN") {
                                subjectTag = prevToken.tag
                                subjectText = prevToken.text.lowercased()
                                break
                            }
                        }
                        
                        if let subText = subjectText {
                            if ["he", "she", "it", "this", "that", "someone", "somebody", "everyone", "everybody", "anyone", "anybody", "nobody", "noone"].contains(subText) {
                                token.tag = "VBD"
                            } else if ["i", "we", "they", "you", "these", "those"].contains(subText) {
                                var hasPastAdverb = false
                                for t in tokens {
                                    let word = t.text.lowercased()
                                    if ["yesterday", "ago", "previously", "earlier", "then", "once", "past"].contains(word) {
                                        hasPastAdverb = true
                                        break
                                    }
                                }
                                token.tag = hasPastAdverb ? "VBD" : "VB"
                            } else if let subTag = subjectTag, subTag == "NNP" || subTag == "NN" {
                                token.tag = "VBD"
                            }
                        } else {
                            var hasPastAdverb = false
                            for t in tokens {
                                let word = t.text.lowercased()
                                if ["yesterday", "ago", "previously", "earlier", "then", "once", "past"].contains(word) {
                                    hasPastAdverb = true
                                    break
                                }
                            }
                            token.tag = hasPastAdverb ? "VBD" : "VB"
                        }
                    }
                }
            }
        }

        guard !featureSpans.isEmpty else { return tokens }

        for feature in featureSpans {
            let indices = tokens.indices.filter { index in
                let tokenStart = slices[index].range.lowerBound
                let tokenEnd = slices[index].range.upperBound
                return tokenStart < feature.range.upperBound && tokenEnd > feature.range.lowerBound
            }
            guard !indices.isEmpty else { continue }

            switch feature.kind {
            case let .stress(stress):
                for index in indices {
                    tokens[index].underscore?.stress = stress
                }
            case let .phonemeOverride(override):
                for (offset, index) in indices.enumerated() {
                    tokens[index].underscore?.isHead = offset == 0
                    tokens[index].phonemes = offset == 0 ? override : ""
                    tokens[index].rating = 5
                }
            case let .numFlags(flags):
                for index in indices {
                    tokens[index].underscore?.numFlags = flags
                }
            }
        }

        return tokens
    }

    func foldLeft(_ tokens: [MToken]) -> [MToken] {
        var result: [MToken] = []
        result.reserveCapacity(tokens.count)
        for token in tokens {
            if let last = result.last, token.underscore?.isHead == false {
                _ = result.popLast()
                result.append(mergeTokens([last, token], unk: unk))
            } else {
                result.append(token)
            }
        }
        return result
    }

    static func retokenize(_ tokens: [MToken]) -> [TokenUnit] {
        var words: [TokenUnit] = []
        var currentCurrency: String?

        func appendStandalone(_ token: MToken) {
            words.append(.token(token))
        }

        func appendOrdinary(_ token: MToken) {
            if case let .group(existing)? = words.last, existing.last?.whitespace.isEmpty == true {
                token.underscore?.isHead = false
                _ = words.popLast()
                words.append(.group(existing + [token]))
            } else if token.whitespace.isEmpty {
                words.append(.group([token]))
            } else {
                words.append(.token(token))
            }
        }

        for (tokenIndex, token) in tokens.enumerated() {
            let pieces: [MToken]
            if token.underscore?.alias == nil, token.phonemes == nil {
                pieces = subtokenize(token.text).map { piece in
                    MToken(
                        text: piece,
                        tag: token.tag,
                        whitespace: "",
                        phonemes: nil,
                        startTS: token.startTS,
                        endTS: token.endTS,
                        underscore: MToken.Underscore(
                            isHead: true,
                            stress: token.underscore?.stress,
                            numFlags: token.underscore?.numFlags ?? "",
                            prespace: false
                        )
                    )
                }
            } else {
                pieces = [token]
            }

            guard !pieces.isEmpty else { continue }
            pieces[pieces.count - 1].whitespace = token.whitespace

            for (pieceIndex, piece) in pieces.enumerated() {
                if piece.underscore?.alias != nil || piece.phonemes != nil {
                    appendStandalone(piece)
                    continue
                }

                if piece.tag == "$", currencies[piece.text] != nil {
                    currentCurrency = piece.text
                    piece.phonemes = ""
                    piece.rating = 4
                    appendStandalone(piece)
                    continue
                }

                if piece.tag == ":", ["-", "–"].contains(piece.text) {
                    piece.phonemes = "—"
                    piece.rating = 3
                    appendStandalone(piece)
                    continue
                }

                if punctTags.contains(piece.tag), !piece.text.allSatisfy(isAsciiLetter) {
                    piece.phonemes = punctTagPhonemes[piece.tag] ?? String(piece.text.filter { puncts.contains($0) })
                    piece.rating = 4
                    appendStandalone(piece)
                    continue
                }

                if let currency = currentCurrency {
                    if piece.tag != "CD" {
                        currentCurrency = nil
                    } else if pieceIndex + 1 == pieces.count, (tokenIndex + 1 == tokens.count || tokens[tokenIndex + 1].tag != "CD") {
                        piece.underscore?.currency = currency
                    }
                }

                if pieceIndex > 0,
                   pieceIndex + 1 < pieces.count,
                   piece.text == "2",
                   let leftChar = pieces[pieceIndex - 1].text.last,
                   let rightChar = pieces[pieceIndex + 1].text.first,
                   leftChar.isLetter,
                   rightChar.isLetter {
                    piece.underscore?.alias = "to"
                }

                if piece.underscore?.alias != nil || piece.phonemes != nil {
                    appendStandalone(piece)
                } else {
                    appendOrdinary(piece)
                }
            }
        }

        return words.map {
            if case let .group(tokens) = $0, tokens.count == 1 {
                return .token(tokens[0])
            }
            return $0
        }
    }

    static func tokenContext(_ ctx: TokenContext, phonemes: String?, token: MToken) -> TokenContext {
        var vowel = ctx.futureVowel
        if let phonemes {
            for character in phonemes where vowels.contains(character) || consonants.contains(character) || nonQuotePuncts.contains(character) {
                vowel = nonQuotePuncts.contains(character) ? nil : vowels.contains(character)
                break
            }
        }
        let futureTo = token.text == "to" || token.text == "To" || (token.text == "TO" && (token.tag == "TO" || token.tag == "IN"))
        return TokenContext(futureVowel: vowel, futureTo: futureTo)
    }

    static func resolveTokens(_ tokens: [MToken]) {
        let text = tokens.dropLast().reduce(into: "") { partial, token in
            partial += token.text + token.whitespace
        } + (tokens.last?.text ?? "")

        let categories = Set(text.filter { !subtokenJunks.contains($0) }.map { character -> Int in
            if character.isLetter { return 0 }
            if character.isNumber { return 1 }
            return 2
        })

        let prespace = text.contains(" ") || text.contains("/") || categories.count > 1

        for (index, token) in tokens.enumerated() {
            if token.phonemes == nil {
                if index == tokens.count - 1, token.text.count == 1, let only = token.text.first, nonQuotePuncts.contains(only) {
                    token.phonemes = token.text
                    token.rating = 3
                } else if token.text.allSatisfy({ subtokenJunks.contains($0) }) {
                    token.phonemes = ""
                    token.rating = 3
                }
            } else if index > 0 {
                token.underscore?.prespace = prespace
            }
        }

        if prespace { return }

        let indices = tokens.enumerated().compactMap { index, token -> (Bool, Int, Int)? in
            guard let phonemes = token.phonemes, !phonemes.isEmpty else { return nil }
            return (phonemes.contains(primaryStress), stressWeight(phonemes), index)
        }

        if indices.count == 2, tokens[indices[0].2].text.count == 1 {
            let secondIndex = indices[1].2
            tokens[secondIndex].phonemes = applyStress(tokens[secondIndex].phonemes, -0.5)
            return
        }

        if indices.count < 2 || indices.filter(\.0).count <= (indices.count + 1) / 2 {
            return
        }

        for entry in indices.sorted(by: {
            if $0.0 != $1.0 { return $0.0 == false && $1.0 == true }
            if $0.1 != $1.1 { return $0.1 < $1.1 }
            return $0.2 < $1.2
        }).prefix(indices.count / 2) {
            tokens[entry.2].phonemes = applyStress(tokens[entry.2].phonemes, -0.5)
        }
    }

    public func callAsFunction(_ text: String, preprocess: Bool = true) -> (phonemes: String, tokens: [MToken]) {
        let preprocessed = preprocess ? Self.preprocess(text) : PreprocessResult(text: text, featureSpans: [])
        var tokens = tokenize(preprocessed.text, featureSpans: preprocessed.featureSpans)
        tokens = foldLeft(tokens)
        var units = Self.retokenize(tokens)
        var ctx = TokenContext()

        for index in units.indices.reversed() {
            switch units[index] {
            case let .token(token):
                if token.phonemes == nil {
                    let cloned = token.cloned()
                    var (phonemes, rating) = lexicon(cloned, ctx: ctx)
                    if phonemes == nil, let fallback = self.fallback {
                        if let fallbackPhonemes = fallback(cloned.text) {
                            phonemes = fallbackPhonemes
                            rating = 3
                        }
                    }
                    token.phonemes = phonemes
                    token.rating = rating
                }
                ctx = Self.tokenContext(ctx, phonemes: token.phonemes, token: token)

            case let .group(group):
                var left = 0
                var right = group.count

                while left < right {
                    let slice = Array(group[left..<right])
                    let combined: MToken? = slice.contains(where: { $0.underscore?.alias != nil || $0.phonemes != nil }) ? nil : mergeTokens(slice)
                    var resolved = combined.map { lexicon($0, ctx: ctx) } ?? (nil, nil)

                    if resolved.0 == nil, let combined, let fallback = self.fallback {
                        if let fallbackPhonemes = fallback(combined.text) {
                            resolved = (fallbackPhonemes, 3)
                        }
                    }

                    if let phonemes = resolved.0 {
                        group[left].phonemes = phonemes
                        group[left].rating = resolved.1
                        if left + 1 < right {
                            for token in group[(left + 1)..<right] {
                                token.phonemes = ""
                                token.rating = resolved.1
                            }
                        }
                        ctx = Self.tokenContext(ctx, phonemes: phonemes, token: combined!)
                        right = left
                        left = 0
                    } else if left + 1 < right {
                        left += 1
                    } else {
                        right -= 1
                        let token = group[right]
                        if token.phonemes == nil {
                            if token.text.allSatisfy({ subtokenJunks.contains($0) }) {
                                token.phonemes = ""
                                token.rating = 3
                            }
                        }
                        left = 0
                    }
                }

                Self.resolveTokens(group)
                units[index] = .group(group)
            }
        }

        let resolvedTokens: [MToken] = units.map {
            switch $0 {
            case let .token(token):
                return token
            case let .group(tokens):
                return mergeTokens(tokens, unk: unk)
            }
        }

        if version != "2.0" {
            for token in resolvedTokens where token.phonemes != nil {
                token.phonemes = token.phonemes?
                    .replacingOccurrences(of: "ɾ", with: "T")
                    .replacingOccurrences(of: "ʔ", with: "t")
            }
        }

        let phonemes = resolvedTokens.reduce(into: "") { partial, token in
            partial += (token.phonemes ?? unk) + token.whitespace
        }
        return (phonemes, resolvedTokens)
    }

    private static func parseFeature(_ feature: String) -> FeatureSpan.Kind? {
        if let value = Double(feature) {
            return .stress(value)
        }
        if feature.count > 1, feature.first == "/", feature.last == "/" {
            return .phonemeOverride("/" == feature ? "" : String(feature.dropFirst().dropLast()))
        } else if feature.count > 1, feature.first == "#", feature.last == "#" {
            return .numFlags(String(feature.dropFirst().dropLast()))
        }
        return nil
    }

    private static func subtokenize(_ word: String) -> [String] {
        let nsWord = word as NSString
        let matches = subtokenRegex.matches(in: word, range: NSRange(location: 0, length: nsWord.length))
        return matches.map { nsWord.substring(with: $0.range) }
    }

    private static func mapTag(tokenText: String, tag: NLTag?) -> String {
        if currencies[tokenText] != nil { return "$" }
        if tokenText == "#" { return "#" }
        if tokenText.contains(where: \.isNumber) { return "CD" }

        switch tag {
        case .personalName, .placeName, .organizationName:
            return "NNP"
        case .noun:
            if isAllUppercase(tokenText), tokenText.count <= 4 {
                return "NNP"
            }
            if isTitleCaseWord(tokenText) {
                return "NNP"
            }
            return "NN"
        case .verb:
            let lower = tokenText.lowercased()
            if lower == "used" || lower.hasSuffix("ed") || ["was", "were", "did", "had"].contains(lower) {
                return "VBD"
            }
            if lower.hasSuffix("ing") {
                return "VBG"
            }
            if lower.hasSuffix("s") {
                return "VBZ"
            }
            return "VB"
        case .adjective:
            return "JJ"
        case .adverb:
            return "RB"
        case .pronoun:
            return "PRP"
        case .determiner:
            return "DT"
        case .preposition:
            return tokenText.lowercased() == "to" ? "TO" : "IN"
        case .conjunction:
            return "CC"
        case .interjection:
            return "UH"
        case .classifier:
            return "CD"
        case .openParenthesis:
            return "-LRB-"
        case .closeParenthesis:
            return "-RRB-"
        case .openQuote:
            return "``"
        case .closeQuote:
            return "\"\""
        case .sentenceTerminator:
            return "."
        case .dash:
            return ":"
        case .wordJoiner, .otherPunctuation:
            if tokenText == "," { return "," }
            if tokenText == "$" { return "$" }
            if tokenText == "#" { return "#" }
            return ":"
        default:
            return tokenText.allSatisfy(\.isLetter) ? "NN" : "NFP"
        }
    }
}
