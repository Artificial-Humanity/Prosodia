import Foundation
import Misaki

public protocol ProsodiaG2PProcessor: Sendable {
    func phonemize(_ text: String, langCode: String) throws -> (phonemes: String, tokens: [MToken])
}

extension G2P: @unchecked Sendable, ProsodiaG2PProcessor {
    public func phonemize(_ text: String, langCode: String) throws -> (phonemes: String, tokens: [MToken]) {
        let res = self(text)
        return (res.phonemes, res.tokens)
    }
}

public typealias G2PRule = @Sendable (String, String) -> (String, String)?

/// A G2P processor that resolves graphemes to phonemes using loaded custom dictionaries.
/// Ideal for lightweight multilingual support without external dependencies.
public final class MultilingualDictionaryG2PProcessor: ProsodiaG2PProcessor, @unchecked Sendable {
    private let dictionary = Locked<[String: [String: String]]>([:]) // langCode: [word: phonemes]
    public let boundaryRules = Locked<[G2PRule]>([])

    public init(
        initialDictionaries: [String: [String: String]] = [:],
        initialRules: [G2PRule] = []
    ) {
        self.dictionary.withLock { $0 = initialDictionaries }
        self.boundaryRules.withLock { $0 = initialRules }
    }

    public func register(word: String, phonemes: String, forLanguage langCode: String) {
        dictionary.withLock { dict in
            let normalizedLang = langCode.lowercased()
            var langDict = dict[normalizedLang] ?? [:]
            langDict[word.lowercased()] = phonemes
            dict[normalizedLang] = langDict
        }
    }

    public func addBoundaryRule(_ rule: @escaping G2PRule) {
        boundaryRules.withLock { $0.append(rule) }
    }

    public func loadDictionary(from url: URL, forLanguage langCode: String) throws {
        let data = try Data(contentsOf: url)
        let decoded = try JSONDecoder().decode([String: String].self, from: data)
        dictionary.withLock { dict in
            dict[langCode.lowercased()] = decoded
        }
    }

    public func phonemize(_ text: String, langCode: String) throws -> (phonemes: String, tokens: [MToken]) {
        let normalizedLang = langCode.lowercased()
        let langDict = dictionary.withLock { $0[normalizedLang] ?? [:] }

        let components = text.components(separatedBy: .whitespacesAndNewlines)
        var phonemesArray: [String] = []
        var tokens: [MToken] = []

        for (idx, word) in components.enumerated() {
            guard !word.isEmpty else { continue }

            let cleanWord = word.trimmingCharacters(in: .punctuationCharacters).lowercased()
            let resolvedPhonemes = langDict[cleanWord] ?? cleanWord

            phonemesArray.append(resolvedPhonemes)

            let isLast = idx == components.count - 1
            let whitespace = isLast ? "" : " "

            let token = MToken(
                text: word,
                tag: cleanWord,
                whitespace: whitespace,
                phonemes: resolvedPhonemes,
                startTS: nil,
                endTS: nil,
                underscore: MToken.Underscore(isHead: true, prespace: idx > 0)
            )
            tokens.append(token)
        }

        // Apply boundary rules dynamically to resolve phoneme liaisons / boundary changes
        let rules = boundaryRules.withLock { $0 }
        if !rules.isEmpty && phonemesArray.count > 1 {
            var i = 0
            while i < phonemesArray.count - 1 {
                for rule in rules {
                    if let updated = rule(phonemesArray[i], phonemesArray[i + 1]) {
                        phonemesArray[i] = updated.0
                        phonemesArray[i + 1] = updated.1
                        
                        // Update matching tokens as well
                        if i < tokens.count {
                            tokens[i].phonemes = updated.0
                        }
                        if i + 1 < tokens.count {
                            tokens[i + 1].phonemes = updated.1
                        }
                        break
                    }
                }
                i += 1
            }
        }

        let phonemesString = phonemesArray.joined(separator: " ")
        return (phonemesString, tokens)
    }
}

/// A G2P processor that shells out to a local homebrew or system-installed `espeak-ng` binary on macOS.
/// Useful for debugging and offline multi-language synthesis testing.
public final class ShellEspeakG2PProcessor: ProsodiaG2PProcessor, @unchecked Sendable {
    public let espeakPath: String

    public init(espeakPath: String = "/opt/homebrew/bin/espeak-ng") {
        self.espeakPath = espeakPath
    }

    public func phonemize(_ text: String, langCode: String) throws -> (phonemes: String, tokens: [MToken]) {
        #if os(macOS)
        let process = Process()
        let pipe = Pipe()

        process.executableURL = URL(fileURLWithPath: espeakPath)
        process.arguments = ["-q", "-v", langCode, "--ipa", text]
        process.standardOutput = pipe

        try process.run()
        process.waitUntilExit()

        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        guard let output = String(data: data, encoding: .utf8) else {
            throw NSError(domain: "ShellEspeak", code: 1, userInfo: [NSLocalizedDescriptionKey: "Failed to read output from espeak-ng"])
        }

        let phonemesString = output.trimmingCharacters(in: .whitespacesAndNewlines)

        let components = text.components(separatedBy: .whitespacesAndNewlines)
        let cleanPhonemeWords = phonemesString.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }

        var tokens: [MToken] = []
        for (idx, word) in components.enumerated() {
            guard !word.isEmpty else { continue }
            let wordPhonemes = idx < cleanPhonemeWords.count ? cleanPhonemeWords[idx] : word

            let isLast = idx == components.count - 1
            let whitespace = isLast ? "" : " "

            let token = MToken(
                text: word,
                tag: word.lowercased(),
                whitespace: whitespace,
                phonemes: wordPhonemes,
                startTS: nil,
                endTS: nil,
                underscore: MToken.Underscore(isHead: true, prespace: idx > 0)
            )
            tokens.append(token)
        }

        return (phonemesString, tokens)
        #else
        throw NSError(domain: "ShellEspeak", code: 2, userInfo: [NSLocalizedDescriptionKey: "Shell-based espeak-ng is only supported on macOS."])
        #endif
    }
}
