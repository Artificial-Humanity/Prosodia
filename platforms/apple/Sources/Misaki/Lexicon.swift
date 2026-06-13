import Foundation

internal let diphthongs = Set("AIOQWYʤʧ")
internal let subtokenJunks = Set("',-._‘’/")
internal let puncts = Set(";:,.!?—…\"“”")
internal let nonQuotePuncts = Set(puncts.filter { !"\"“”".contains($0) })
internal let punctTagPhonemes: [String: String] = [
    "-LRB-": "(",
    "-RRB-": ")",
    "``": "“",
    "\"\"": "”",
    "''": "”",
]
internal let punctTags: Set<String> = [".", ",", "-LRB-", "-RRB-", "``", "\"\"", "''", ":", "$", "#", "NFP"]
internal let consonants = Set("bdfhjklmnpstvwzðŋɡɹɾʃʒʤʧθ")
internal let usTaus = Set("AIOWYiuæɑəɛɪɹʊʌ")
internal let currencies: [String: (String, String)] = [
    "$": ("dollar", "cent"),
    "£": ("pound", "pence"),
    "€": ("euro", "cent"),
]
internal let ordinals: Set<String> = ["st", "nd", "rd", "th"]
internal let addSymbols: [String: String] = [".": "dot", "/": "slash"]
internal let symbols: [String: String] = ["%": "percent", "&": "and", "+": "plus", "@": "at"]
internal let usVocab = Set("AIOWYbdfhijklmnpstuvwzæðŋɑɔəɛɜɡɪɹɾʃʊʌʒʤʧˈˌθᵊᵻʔ")
internal let gbVocab = Set("AIQWYabdfhijklmnpstuvwzðŋɑɒɔəɛɜɡɪɹʃʊʌʒʤʧˈˌːθᵊ")
internal let stresses = Set("ˌˈ")
internal let primaryStress: Character = "ˈ"
internal let secondaryStress: Character = "ˌ"
internal let vowels = Set("AIOQWYaiuæɑɒɔəɛɜɪʊʌᵻ")

internal struct TokenContext {
    var futureVowel: Bool? = nil
    var futureTo = false
}

internal enum LexiconValue {
    case single(String)
    case variants([String: String?])
}

internal func stressWeight(_ phonemes: String?) -> Int {
    guard let phonemes else { return 0 }
    return phonemes.reduce(into: 0) { partial, character in
        partial += diphthongs.contains(character) ? 2 : 1
    }
}

internal func applyStress(_ phonemes: String?, _ stress: Double?) -> String? {
    guard let phonemes else { return nil }

    func restress(_ phonemeString: String) -> String {
        let characters = Array(phonemeString)
        var indexed = characters.enumerated().map { (index: Double($0.offset), character: $0.element) }
        for index in characters.indices where stresses.contains(characters[index]) {
            if let vowelIndex = characters[index...].firstIndex(where: { vowels.contains($0) }) {
                indexed[index].index = Double(vowelIndex) - 0.5
            }
        }
        return String(indexed.sorted(by: { $0.index < $1.index }).map(\.character))
    }

    guard let stress else { return phonemes }

    if stress < -1 {
        return phonemes.filter { $0 != primaryStress && $0 != secondaryStress }
    }

    if stress == -1 || ((stress == 0 || stress == -0.5) && phonemes.contains(primaryStress)) {
        return phonemes
            .replacingOccurrences(of: String(secondaryStress), with: "")
            .replacingOccurrences(of: String(primaryStress), with: String(secondaryStress))
    }

    if stress == 0 || stress == 0.5 || stress == 1 {
        if !phonemes.contains(where: { stresses.contains($0) }) {
            guard phonemes.contains(where: { vowels.contains($0) }) else { return phonemes }
            return restress(String(secondaryStress) + phonemes)
        }
        return phonemes
    }

    if stress >= 1, !phonemes.contains(primaryStress), phonemes.contains(secondaryStress) {
        return phonemes.replacingOccurrences(of: String(secondaryStress), with: String(primaryStress))
    }

    if stress > 1, !phonemes.contains(where: { stresses.contains($0) }) {
        guard phonemes.contains(where: { vowels.contains($0) }) else { return phonemes }
        return restress(String(primaryStress) + phonemes)
    }

    return phonemes
}

internal func isDigitString(_ text: String) -> Bool {
    !text.isEmpty && text.allSatisfy(\.isNumber)
}

internal func splitLowercaseWords(_ text: String) -> [String] {
    var words: [String] = []
    var current = ""
    for character in text {
        if character.isLetter {
            current.append(character)
        } else if !current.isEmpty {
            words.append(current)
            current.removeAll(keepingCapacity: true)
        }
    }
    if !current.isEmpty {
        words.append(current)
    }
    return words
}

internal func isAsciiLetter(_ character: Character) -> Bool {
    guard let scalar = character.unicodeScalars.first, character.unicodeScalars.count == 1 else { return false }
    return (65...90).contains(Int(scalar.value)) || (97...122).contains(Int(scalar.value))
}

internal func isLexiconOrdinalCharacter(_ character: Character) -> Bool {
    character == "'" || isAsciiLetter(character)
}

internal func isAllUppercase(_ text: String) -> Bool {
    !text.isEmpty && text == text.uppercased()
}

internal func isTitleCaseWord(_ text: String) -> Bool {
    guard let first = text.first else { return false }
    let rest = String(text.dropFirst())
    return first.isUppercase && rest == rest.lowercased()
}

public final class Lexicon {
    private enum LexiconError: Error {
        case missingResource(String)
        case malformedResource(String)
        case invalidPhoneme(String)
    }

    public let british: Bool
    public let capStresses: (Double, Double) = (0.5, 2.0)

    private let golds: [String: LexiconValue]
    private let silvers: [String: String]

    public init(british: Bool = false) throws {
        self.british = british
        self.golds = try Self.loadLexicon(named: british ? "gb_gold" : "us_gold", mixed: true)
        self.silvers = try Self.loadLexicon(named: british ? "gb_silver" : "us_silver", mixed: false)
        try Self.validate(golds: golds, british: british)
    }

    private static func loadLexicon(named name: String, mixed: Bool) throws -> [String: LexiconValue] {
        guard let url = Bundle.module.url(forResource: name, withExtension: "json") else {
            throw LexiconError.missingResource(name)
        }
        let data = try Data(contentsOf: url)
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw LexiconError.malformedResource(name)
        }

        var result: [String: LexiconValue] = [:]
        result.reserveCapacity(object.count * 2)

        for (key, value) in object {
            if let text = value as? String {
                result[key] = .single(text)
                continue
            }

            guard mixed, let dict = value as? [String: Any] else {
                throw LexiconError.malformedResource(name)
            }

            var mapped: [String: String?] = [:]
            for (variantKey, variantValue) in dict {
                if variantValue is NSNull {
                    mapped[variantKey] = nil
                } else if let variantText = variantValue as? String {
                    mapped[variantKey] = variantText
                } else {
                    throw LexiconError.malformedResource(name)
                }
            }
            result[key] = .variants(mapped)
        }

        return growDictionary(result)
    }

    private static func loadLexicon(named name: String, mixed _: Bool) throws -> [String: String] {
        guard let url = Bundle.module.url(forResource: name, withExtension: "json") else {
            throw LexiconError.missingResource(name)
        }
        let data = try Data(contentsOf: url)
        let decoded = try JSONDecoder().decode([String: String].self, from: data)
        return growDictionary(decoded)
    }

    private static func growDictionary<T>(_ dictionary: [String: T]) -> [String: T] {
        var extras: [String: T] = [:]
        extras.reserveCapacity(dictionary.count / 4)
        for (key, value) in dictionary where key.count >= 2 {
            if key == key.lowercased() {
                let capitalized = key.prefix(1).uppercased() + key.dropFirst()
                if key != capitalized {
                    extras[capitalized] = value
                }
            } else if key == key.lowercased().capitalized {
                extras[key.lowercased()] = value
            }
        }
        return extras.merging(dictionary, uniquingKeysWith: { _, new in new })
    }

    private static func validate(golds: [String: LexiconValue], british: Bool) throws {
        let vocab = british ? gbVocab : usVocab
        for value in golds.values {
            switch value {
            case let .single(phonemes):
                guard phonemes.allSatisfy({ vocab.contains($0) }) else {
                    throw LexiconError.invalidPhoneme(phonemes)
                }
            case let .variants(variants):
                guard variants["DEFAULT"] != nil else {
                    throw LexiconError.malformedResource("DEFAULT missing")
                }
                for phonemes in variants.values.compactMap({ $0 }) where !phonemes.allSatisfy({ vocab.contains($0) }) {
                    throw LexiconError.invalidPhoneme(phonemes)
                }
            }
        }
    }

    public func getNNP(_ word: String) -> (String?, Int?) {
        let pieces = word.compactMap { character -> String? in
            guard character.isLetter else { return nil }
            switch golds[String(character).uppercased()] {
            case let .single(phonemes):
                return phonemes
            case let .variants(variants):
                return variants["DEFAULT"] ?? nil
            case nil:
                return nil
            }
        }
        guard !pieces.isEmpty else { return (nil, nil) }
        let phonemes = pieces.joined()
        guard !phonemes.isEmpty else { return (nil, nil) }
        let stressed = applyStress(phonemes, 0)
        guard let stressed else { return (nil, nil) }
        let parts = stressed.split(separator: secondaryStress, maxSplits: 1, omittingEmptySubsequences: false)
        return (parts.map(String.init).joined(separator: String(primaryStress)), 3)
    }

    func getSpecialCase(_ word: String, tag: String?, stress: Double?, ctx: TokenContext) -> (String?, Int?) {
        if tag == "ADD", let symbolWord = addSymbols[word] {
            return lookup(symbolWord, tag: nil, stress: -0.5, ctx: ctx)
        }
        if let symbolWord = symbols[word] {
            return lookup(symbolWord, tag: nil, stress: nil, ctx: ctx)
        }
        if word.trimmingCharacters(in: CharacterSet(charactersIn: ".")).contains("."),
           word.replacingOccurrences(of: ".", with: "").allSatisfy(\.isLetter),
           word.split(separator: ".").map(\.count).max() ?? 0 < 3 {
            return getNNP(word)
        }
        if word == "a" || word == "A" {
            return (tag == "DT" ? "ɐ" : "ˈA", 4)
        }
        if ["am", "Am", "AM"].contains(word) {
            if (tag ?? "").hasPrefix("NN") {
                return getNNP(word)
            }
            if ctx.futureVowel == nil || word != "am" || ((stress ?? 0) > 0 && stress != nil) {
                return (goldString(for: "am"), 4)
            }
            return ("ɐm", 4)
        }
        if ["an", "An", "AN"].contains(word) {
            if word == "AN", (tag ?? "").hasPrefix("NN") {
                return getNNP(word)
            }
            return ("ɐn", 4)
        }
        if word == "I", tag == "PRP" {
            return ("\(secondaryStress)I", 4)
        }
        if ["by", "By", "BY"].contains(word), Self.parentTag(for: tag) == "ADV" {
            return ("bˈI", 4)
        }
        if ["to", "To"].contains(word) || (word == "TO" && (tag == "TO" || tag == "IN")) {
            let phonemes: String
            switch ctx.futureVowel {
            case nil: phonemes = goldString(for: "to") ?? "tu"
            case false: phonemes = "tə"
            case true: phonemes = "tʊ"
            }
            return (phonemes, 4)
        }
        if ["in", "In"].contains(word) || (word == "IN" && tag != "NNP") {
            let stressMarker = (ctx.futureVowel == nil || tag != "IN") ? String(primaryStress) : ""
            return ("\(stressMarker)ɪn", 4)
        }
        if ["the", "The"].contains(word) || (word == "THE" && tag == "DT") {
            return (ctx.futureVowel == true ? "ði" : "ðə", 4)
        }
        if tag == "IN", word.range(of: #"(?i)^vs\.?$"#, options: .regularExpression) != nil {
            return lookup("versus", tag: nil, stress: nil, ctx: ctx)
        }
        if ["used", "Used", "USED"].contains(word) {
            if (tag == "VBD" || tag == "JJ" || tag == "VB"), ctx.futureTo {
                if case let .variants(variants)? = golds["used"] {
                    return (variants["VBD"] ?? nil, 4)
                }
            }
            if case let .variants(variants)? = golds["used"] {
                return (variants["DEFAULT"] ?? nil, 4)
            }
        }
        return (nil, nil)
    }

    public static func parentTag(for tag: String?) -> String? {
        guard let tag else { return nil }
        if tag.hasPrefix("VB") { return "VERB" }
        if tag.hasPrefix("NN") { return "NOUN" }
        if tag.hasPrefix("ADV") || tag.hasPrefix("RB") { return "ADV" }
        if tag.hasPrefix("ADJ") || tag.hasPrefix("JJ") { return "ADJ" }
        return tag
    }

    public func isKnown(_ word: String, tag: String?) -> Bool {
        if golds[word] != nil || symbols[word] != nil || silvers[word] != nil {
            return true
        }
        if !word.allSatisfy(isLexiconOrdinalCharacter) {
            return false
        }
        if word.count == 1 {
            return true
        }
        if isAllUppercase(word), golds[word.lowercased()] != nil {
            return true
        }
        let suffix = String(word.dropFirst())
        return suffix == suffix.uppercased()
    }

    func lookup(_ word: String, tag: String?, stress: Double?, ctx: TokenContext?) -> (String?, Int?) {
        var lookupWord = word
        var properNameGuess: Bool? = nil

        if isAllUppercase(lookupWord), golds[lookupWord] == nil {
            lookupWord = lookupWord.lowercased()
            properNameGuess = tag == "NNP"
        }

        var phonemes: String?
        var rating = 4

        switch golds[lookupWord] {
        case let .single(value)?:
            phonemes = value
        case let .variants(variants)?:
            var selectedTag = tag
            if let ctx, ctx.futureVowel == nil, variants["None"] != nil {
                selectedTag = "None"
            } else if let currentTag = selectedTag, variants[currentTag] == nil {
                selectedTag = Self.parentTag(for: currentTag)
            }
            phonemes = variants[selectedTag ?? "DEFAULT"] ?? variants["DEFAULT"] ?? nil
        case nil:
            break
        }

        if phonemes == nil, properNameGuess != true {
            phonemes = silvers[lookupWord]
            if phonemes != nil {
                rating = 3
            }
        }

        if phonemes == nil || (properNameGuess == true && !(phonemes?.contains(primaryStress) ?? false)) {
            let (properNamePhonemes, properNameRating) = getNNP(lookupWord)
            if properNamePhonemes != nil {
                return (properNamePhonemes, properNameRating)
            }
        }

        return (applyStress(phonemes, stress), phonemes == nil ? nil : rating)
    }

    public func pluralSuffix(_ stem: String?) -> String? {
        guard let stem, let last = stem.last else { return nil }
        if "ptkfθ".contains(last) {
            return stem + "s"
        }
        if "szʃʒʧʤ".contains(last) {
            return stem + (british ? "ɪ" : "ᵻ") + "z"
        }
        return stem + "z"
    }

    func stemS(_ word: String, tag: String?, stress: Double?, ctx: TokenContext?) -> (String?, Int?) {
        guard word.count >= 3, word.hasSuffix("s") else { return (nil, nil) }

        let stem: String?
        if !word.hasSuffix("ss"), isKnown(String(word.dropLast()), tag: tag) {
            stem = String(word.dropLast())
        } else if (word.hasSuffix("'s") || (word.count > 4 && word.hasSuffix("es") && !word.hasSuffix("ies"))),
                  isKnown(String(word.dropLast(2)), tag: tag) {
            stem = String(word.dropLast(2))
        } else if word.count > 4, word.hasSuffix("ies"), isKnown(String(word.dropLast(3)) + "y", tag: tag) {
            stem = String(word.dropLast(3)) + "y"
        } else {
            stem = nil
        }

        guard let stem else { return (nil, nil) }
        let (stemPhonemes, rating) = lookup(stem, tag: tag, stress: stress, ctx: ctx)
        return (pluralSuffix(stemPhonemes), rating)
    }

    public func edSuffix(_ stem: String?) -> String? {
        guard let stem, let last = stem.last else { return nil }
        if "pkfθʃsʧ".contains(last) {
            return stem + "t"
        }
        if last == "d" {
            return stem + (british ? "ɪ" : "ᵻ") + "d"
        }
        if last != "t" {
            return stem + "d"
        }
        if british || stem.count < 2 {
            return stem + "ɪd"
        }
        let characters = Array(stem)
        if characters.count >= 2, usTaus.contains(characters[characters.count - 2]) {
            return String(characters.dropLast()) + "ɾᵻd"
        }
        return stem + "ᵻd"
    }

    func stemED(_ word: String, tag: String?, stress: Double?, ctx: TokenContext?) -> (String?, Int?) {
        guard word.count >= 4, word.hasSuffix("d") else { return (nil, nil) }

        let stem: String?
        if !word.hasSuffix("dd"), isKnown(String(word.dropLast()), tag: tag) {
            stem = String(word.dropLast())
        } else if word.count > 4, word.hasSuffix("ed"), !word.hasSuffix("eed"), isKnown(String(word.dropLast(2)), tag: tag) {
            stem = String(word.dropLast(2))
        } else {
            stem = nil
        }

        guard let stem else { return (nil, nil) }
        let (stemPhonemes, rating) = lookup(stem, tag: tag, stress: stress, ctx: ctx)
        return (edSuffix(stemPhonemes), rating)
    }

    public func ingSuffix(_ stem: String?) -> String? {
        guard let stem, let last = stem.last else { return nil }
        if british {
            if last == "ə" || last == "ː" {
                return nil
            }
        } else if stem.count > 1 {
            let characters = Array(stem)
            if last == "t", usTaus.contains(characters[characters.count - 2]) {
                return String(characters.dropLast()) + "ɾɪŋ"
            }
        }
        return stem + "ɪŋ"
    }

    func stemING(_ word: String, tag: String?, stress: Double?, ctx: TokenContext?) -> (String?, Int?) {
        guard word.count >= 5, word.hasSuffix("ing") else { return (nil, nil) }

        let base = String(word.dropLast(3))
        let stem: String?

        if word.count > 5, isKnown(base, tag: tag) {
            stem = base
        } else if isKnown(base + "e", tag: tag) {
            stem = base + "e"
        } else if word.count > 5,
                  word.range(of: #"([bcdgklmnprstvxz])\1ing$|cking$"#, options: .regularExpression) != nil,
                  isKnown(String(word.dropLast(4)), tag: tag) {
            stem = String(word.dropLast(4))
        } else {
            stem = nil
        }

        guard let stem else { return (nil, nil) }
        let (stemPhonemes, rating) = lookup(stem, tag: tag, stress: stress, ctx: ctx)
        return (ingSuffix(stemPhonemes), rating)
    }

    func getWord(_ word: String, tag: String?, stress: Double?, ctx: TokenContext) -> (String?, Int?) {
        let (specialCasePhonemes, specialCaseRating) = getSpecialCase(word, tag: tag, stress: stress, ctx: ctx)
        if specialCasePhonemes != nil {
            return (specialCasePhonemes, specialCaseRating)
        }

        var lookupWord = word
        let lowercased = word.lowercased()
        let deapostrophized = word.replacingOccurrences(of: "'", with: "")

        if word.count > 1,
           !deapostrophized.isEmpty,
           deapostrophized.allSatisfy(\.isLetter),
           word != word.lowercased(),
           tag != "NNP" || word.count > 7,
           golds[word] == nil,
           silvers[word] == nil,
           isAllUppercase(word) || String(word.dropFirst()) == String(word.dropFirst()).lowercased(),
           golds[lowercased] != nil || silvers[lowercased] != nil || [stemS(lowercased, tag: tag, stress: stress, ctx: ctx).0, stemED(lowercased, tag: tag, stress: stress, ctx: ctx).0, stemING(lowercased, tag: tag, stress: stress, ctx: ctx).0].contains(where: { $0 != nil }) {
            lookupWord = lowercased
        }

        if isKnown(lookupWord, tag: tag) {
            return lookup(lookupWord, tag: tag, stress: stress, ctx: ctx)
        }

        if lookupWord.hasSuffix("s'"), isKnown(String(lookupWord.dropLast(2)) + "'s", tag: tag) {
            return lookup(String(lookupWord.dropLast(2)) + "'s", tag: tag, stress: stress, ctx: ctx)
        }

        if lookupWord.hasSuffix("'"), isKnown(String(lookupWord.dropLast()), tag: tag) {
            return lookup(String(lookupWord.dropLast()), tag: tag, stress: stress, ctx: ctx)
        }

        let sResult = stemS(lookupWord, tag: tag, stress: stress, ctx: ctx)
        if sResult.0 != nil { return sResult }

        let edResult = stemED(lookupWord, tag: tag, stress: stress, ctx: ctx)
        if edResult.0 != nil { return edResult }

        let ingStress = stress ?? 0.5
        let ingResult = stemING(lookupWord, tag: tag, stress: ingStress, ctx: ctx)
        if ingResult.0 != nil { return ingResult }

        return (nil, nil)
    }

    public static func isCurrency(_ word: String) -> Bool {
        if !word.contains(".") {
            return true
        }
        if word.filter({ $0 == "." }).count > 1 {
            return false
        }
        let cents = word.split(separator: ".", omittingEmptySubsequences: false).dropFirst().first.map(String.init) ?? ""
        return cents.count < 3 || Set(cents) == Set("0")
    }

    public func getNumber(_ word: String, currency: String?, isHead: Bool, numFlags: String) -> (String?, Int?) {
        let suffixMatch = word.range(of: #"[a-z']+$"#, options: [.regularExpression, .caseInsensitive])
        let suffix = suffixMatch.map { String(word[$0]) }
        let suffixLower = suffix?.lowercased()
        let numericWord = suffix == nil ? word : String(word.dropLast(suffix!.count))

        var workingWord = numericWord
        var result: [(String, Int)] = []

        if workingWord.hasPrefix("-") {
            if let minus = lookup("minus", tag: nil, stress: nil, ctx: nil).0,
               let minusRating = lookup("minus", tag: nil, stress: nil, ctx: nil).1 {
                result.append((minus, minusRating))
            }
            workingWord.removeFirst()
        }

        func appendWord(_ word: String, first: Bool, escape: Bool = false) {
            let source = escape ? word : (NumberToWords.cardinal(word) ?? word)
            let splits = splitLowercaseWords(source)
            for (index, split) in splits.enumerated() {
                if split != "and" || numFlags.contains("&") {
                    if first, index == 0, splits.count > 1, split == "one", numFlags.contains("a") {
                        result.append(("ə", 4))
                    } else {
                        let stress = split == "point" ? -2.0 : nil
                        if let phonemes = lookup(split, tag: nil, stress: stress, ctx: nil).0,
                           let rating = lookup(split, tag: nil, stress: stress, ctx: nil).1 {
                            result.append((phonemes, rating))
                        }
                    }
                } else if split == "and", numFlags.contains("n"), !result.isEmpty {
                    let last = result.removeLast()
                    result.append((last.0 + "ən", last.1))
                }
            }
        }

        if isDigitString(workingWord), let suffixLower, ordinals.contains(suffixLower) {
            appendWord(NumberToWords.ordinal(workingWord) ?? workingWord, first: true, escape: true)
        } else if result.isEmpty, workingWord.count == 4, currency == nil, isDigitString(workingWord) {
            appendWord(NumberToWords.year(workingWord) ?? workingWord, first: true, escape: true)
        } else if !isHead, !workingWord.contains(".") {
            let compact = workingWord.replacingOccurrences(of: ",", with: "")
            if compact.first == "0" || compact.count > 3 {
                compact.forEach { appendWord(String($0), first: false) }
            } else if compact.count == 3, !compact.hasSuffix("00") {
                appendWord(String(compact.prefix(1)), first: true)
                let tensAndOnes = String(compact.suffix(2))
                if tensAndOnes.first == "0" {
                    if let oPhonemes = lookup("O", tag: nil, stress: -2, ctx: nil).0,
                       let oRating = lookup("O", tag: nil, stress: -2, ctx: nil).1 {
                        result.append((oPhonemes, oRating))
                    }
                    appendWord(String(tensAndOnes.suffix(1)), first: false)
                } else {
                    appendWord(tensAndOnes, first: false)
                }
            } else {
                appendWord(compact, first: true)
            }
        } else if workingWord.filter({ $0 == "." }).count > 1 || !isHead {
            var first = true
            for chunk in workingWord.replacingOccurrences(of: ",", with: "").split(separator: ".", omittingEmptySubsequences: false).map(String.init) {
                if chunk.isEmpty {
                    continue
                }
                if chunk.first == "0" || (chunk.count != 2 && chunk.dropFirst().contains(where: { $0 != "0" })) {
                    chunk.forEach { appendWord(String($0), first: false) }
                } else {
                    appendWord(chunk, first: first)
                }
                first = false
            }
        } else if let currency, let currencyUnits = currencies[currency], Self.isCurrency(workingWord) {
            var pairs = workingWord.replacingOccurrences(of: ",", with: "")
                .split(separator: ".", omittingEmptySubsequences: false)
                .map(String.init)
            while pairs.count < 2 {
                pairs.append("")
            }
            var quantities = zip(pairs, [currencyUnits.0, currencyUnits.1]).map { chunk, unit -> (Int64, String) in
                (Int64(chunk) ?? 0, unit)
            }
            if quantities.count > 1 {
                if quantities[1].0 == 0 {
                    quantities = [quantities[0]]
                } else if quantities[0].0 == 0 {
                    quantities = [quantities[1]]
                }
            }
            for (index, pair) in quantities.enumerated() {
                if index > 0,
                   let andPhonemes = lookup("and", tag: nil, stress: nil, ctx: nil).0,
                   let andRating = lookup("and", tag: nil, stress: nil, ctx: nil).1 {
                    result.append((andPhonemes, andRating))
                }
                appendWord(String(pair.0), first: index == 0)
                if abs(pair.0) != 1, pair.1 != "pence" {
                    let pluralized = stemS(pair.1 + "s", tag: nil, stress: nil, ctx: nil)
                    if let phonemes = pluralized.0, let rating = pluralized.1 {
                        result.append((phonemes, rating))
                    }
                } else if let phonemes = lookup(pair.1, tag: nil, stress: nil, ctx: nil).0,
                          let rating = lookup(pair.1, tag: nil, stress: nil, ctx: nil).1 {
                    result.append((phonemes, rating))
                }
            }
        } else {
            let spoken: String
            if isDigitString(workingWord) {
                spoken = NumberToWords.cardinal(workingWord) ?? workingWord
            } else if !workingWord.contains(".") {
                let compact = workingWord.replacingOccurrences(of: ",", with: "")
                spoken = (suffixLower.flatMap { ordinals.contains($0) ? NumberToWords.ordinal(compact) : NumberToWords.cardinal(compact) }) ?? NumberToWords.cardinal(compact) ?? compact
            } else {
                let compact = workingWord.replacingOccurrences(of: ",", with: "")
                if compact.first == "." {
                    let decimalDigits = compact.dropFirst().map { NumberToWords.cardinal(String($0)) ?? String($0) }.joined(separator: " ")
                    spoken = "point \(decimalDigits)"
                } else {
                    let parts = compact.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
                    let left = NumberToWords.cardinal(parts[0]) ?? parts[0]
                    let right = parts.count > 1 ? parts[1].map { NumberToWords.cardinal(String($0)) ?? String($0) }.joined(separator: " ") : ""
                    spoken = right.isEmpty ? left : "\(left) point \(right)"
                }
            }
            appendWord(spoken, first: true, escape: true)
        }

        guard !result.isEmpty else { return (nil, nil) }
        let phonemes = result.map(\.0).joined(separator: " ")
        let rating = result.map(\.1).min()

        if suffixLower == "s" || suffixLower == "'s" {
            return (pluralSuffix(phonemes), rating)
        }
        if suffixLower == "ed" || suffixLower == "'d" {
            return (edSuffix(phonemes), rating)
        }
        if suffixLower == "ing" {
            return (ingSuffix(phonemes), rating)
        }
        return (phonemes, rating)
    }

    public func appendCurrency(_ phonemes: String, currency: String?) -> String {
        guard let currency else { return phonemes }
        guard let unit = currencies[currency]?.0 else { return phonemes }
        guard let pluralized = stemS(unit + "s", tag: nil, stress: nil, ctx: nil).0 else { return phonemes }
        return "\(phonemes) \(pluralized)"
    }

    public static func numericIfNeeded(_ character: Character) -> String {
        guard character.isNumber, let value = character.wholeNumberValue else {
            return String(character)
        }
        return String(value)
    }

    public static func isNumber(_ word: String, isHead: Bool) -> Bool {
        guard word.contains(where: \.isNumber) else { return false }

        let suffixes = ["ing", "'d", "ed", "'s", "st", "nd", "rd", "th", "s"]
        var candidate = word
        let candidateLower = candidate.lowercased()
        if let suffix = suffixes.first(where: { candidateLower.hasSuffix($0) }) {
            candidate.removeLast(suffix.count)
        }

        return candidate.enumerated().allSatisfy { index, character in
            character.isNumber || character == "," || character == "." || (isHead && index == 0 && character == "-")
        }
    }

    func callAsFunction(_ token: MToken, ctx: TokenContext) -> (String?, Int?) {
        var word = (token.underscore?.alias ?? token.text)
            .replacingOccurrences(of: "‘", with: "'")
            .replacingOccurrences(of: "’", with: "'")
        word = word.precomposedStringWithCompatibilityMapping
        word = word.map(Self.numericIfNeeded).joined()

        let stress: Double?
        if word == word.lowercased() {
            stress = nil
        } else {
            stress = isAllUppercase(word) ? capStresses.1 : capStresses.0
        }

        let (wordPhonemes, wordRating) = getWord(word, tag: token.tag, stress: stress, ctx: ctx)
        if let wordPhonemes {
            return (applyStress(appendCurrency(wordPhonemes, currency: token.underscore?.currency), token.underscore?.stress), wordRating)
        }

        if Self.isNumber(word, isHead: token.underscore?.isHead ?? true) {
            let (numberPhonemes, numberRating) = getNumber(
                word,
                currency: token.underscore?.currency,
                isHead: token.underscore?.isHead ?? true,
                numFlags: token.underscore?.numFlags ?? ""
            )
            return (applyStress(numberPhonemes, token.underscore?.stress), numberRating)
        }

        guard word.allSatisfy(isLexiconOrdinalCharacter) else {
            return (nil, nil)
        }
        return (nil, nil)
    }

    private func goldString(for key: String) -> String? {
        switch golds[key] {
        case let .single(value)?:
            return value
        case let .variants(variants)?:
            return variants["DEFAULT"] ?? nil
        case nil:
            return nil
        }
    }
}
