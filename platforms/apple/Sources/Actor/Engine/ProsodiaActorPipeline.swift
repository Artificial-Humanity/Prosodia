import Foundation
import Misaki

public actor ProsodiaActorPipeline {
    public struct WordTimestamp: Sendable, Codable, Equatable {
        public let word: String
        public let range: Range<Int>
        public let startTime: Double
        public let endTime: Double

        public init(word: String, range: Range<Int>, startTime: Double, endTime: Double) {
            self.word = word
            self.range = range
            self.startTime = startTime
            self.endTime = endTime
        }
    }

    public struct Result: Sendable {
        public let graphemes: String
        public let phonemes: String
        public let audio: [Float]
        public let sampleRate: Int
        public let timestamps: [WordTimestamp]?

        public init(
            graphemes: String,
            phonemes: String,
            audio: [Float],
            sampleRate: Int,
            timestamps: [WordTimestamp]? = nil
        ) {
            self.graphemes = graphemes
            self.phonemes = phonemes
            self.audio = audio
            self.sampleRate = sampleRate
            self.timestamps = timestamps
        }
    }

    public let engine: any ProsodiaActorBackend
    public let voices: VoiceLoader
    public let sampleRate: Int
    public let langCode: String
    private var g2p: (any ProsodiaG2PProcessor)?

    public init(
        engine: any ProsodiaActorBackend,
        voices: VoiceLoader,
        sampleRate: Int = 24_000,
        langCode: String = "en-us",
        customG2P: (any ProsodiaG2PProcessor)? = nil
    ) {
        self.engine = engine
        self.voices = voices
        self.sampleRate = sampleRate
        self.langCode = langCode
        self.g2p = customG2P
    }

    public func setCustomG2P(_ processor: any ProsodiaG2PProcessor) {
        self.g2p = processor
    }

    /// Pre-warms the synthesis engine by executing a silent, minimal inference pass.
    /// This warms up the LiteRT interpreter/delegates to eliminate cold-start latencies.
    public func prewarm(voice: String = "anchor_female_adult") async throws {
        _ = try await synthesize(text: "a", voice: voice)
    }

    /// Reclaims system memory by unloading loaded neural models from the backend engine.
    public func reclaimMemory() {
        engine.reclaimMemory()
    }

    /// Synchronous synthesis (all at once)
    public func synthesize(
        text: String,
        voice: String,
        speed: Float = 1.0,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) async throws -> Result {
        let phonemized = try resolveG2P().phonemize(text, langCode: langCode)
        let phonemes = phonemized.phonemes
        return try await synthesizeResolved(
            graphemes: text,
            phonemes: phonemes,
            voice: voice,
            speed: speed,
            durationScales: durationScales,
            f0Bias: f0Bias
        )
    }

    /// Synthesizes speech from markup containing `<prosody rate="..." pitch="...">` tags.
    public func synthesizeMarkup(
        _ markupText: String,
        voice: String,
        speed: Float = 1.0
    ) async throws -> Result {
        guard speed.isFinite, speed > 0 else {
            throw StyleTTS2Error.invalidSpeed(speed)
        }

        // 1. Parse the markup text
        let parsed = ProsodyMarkupParser.parse(markupText)
        let cleanText = parsed.cleanText
        let characterProsody = parsed.characterProsody

        // 2. Perform G2P text tokenization
        let g2pResult = try resolveG2P().phonemize(cleanText, langCode: langCode)
        let tokenChunks = chunkTokens(g2pResult.tokens, limit: 510)
        let vocab = engine.vocab

        var totalAudio: [Float] = []
        var wordTimestamps: [WordTimestamp] = []
        var audioTimeOffset = 0.0
        var charOffset = 0

        var tokenOffset = 0
        for chunk in tokenChunks {
            // Build the states for characters in the raw untrimmed chunk phonemes
            var rawPhonemes = ""
            var rawStates: [ProsodyState] = []

            var wordStartCharOffset = charOffset
            for token in chunk {
                let tokenState = (wordStartCharOffset < characterProsody.count) ? characterProsody[wordStartCharOffset] : ProsodyState()
                let phonemesAndSpace = (token.phonemes ?? "") + token.whitespace
                for char in phonemesAndSpace {
                    rawPhonemes.append(char)
                    rawStates.append(tokenState)
                }
                wordStartCharOffset += token.text.count + token.whitespace.count
            }

            // Trim leading/trailing whitespace to match engine behavior
            var startIndex = rawPhonemes.startIndex
            var endIndex = rawPhonemes.endIndex
            var leadingTrim = 0
            while startIndex < endIndex && rawPhonemes[startIndex].isWhitespace {
                startIndex = rawPhonemes.index(after: startIndex)
                leadingTrim += 1
            }
            var trailingTrim = 0
            while startIndex < endIndex {
                let prevIndex = rawPhonemes.index(before: endIndex)
                if rawPhonemes[prevIndex].isWhitespace {
                    endIndex = prevIndex
                    trailingTrim += 1
                } else {
                    break
                }
            }

            let chunkPhonemes = String(rawPhonemes[startIndex..<endIndex])
            guard !chunkPhonemes.isEmpty else {
                charOffset = wordStartCharOffset
                continue
            }

            let trimmedStates = Array(rawStates[leadingTrim..<(rawStates.count - trailingTrim)])

            // Filter states matching what gets tokenized by vocab
            var filteredStates: [ProsodyState] = []
            for (idx, char) in chunkPhonemes.enumerated() {
                if vocab[String(char)] != nil {
                    filteredStates.append(trimmedStates[idx])
                }
            }

            // finalStates includes SOS (index 0) and EOS (index last)
            let finalStates = [ProsodyState()] + filteredStates + [ProsodyState()]

            // Build durationScales and f0Bias arrays as standard Swift floats
            let durationScales = finalStates.map { Float(1.0 / $0.rate) }
            let f0Bias = finalStates.map { Float($0.pitch) }

            // Synthesize this chunk using the loaded style vector
            let style = try await voices.styleVectorAsync(for: voice, phonemeCount: chunkPhonemes.count)
            let output = try engine.forward(
                phonemes: chunkPhonemes,
                refS: style,
                speed: speed,
                durationScales: durationScales,
                f0Bias: f0Bias
            )

            // Assemble timestamps for the result
            let predDur = output.predDur
            var tokenIdx = 1
            let frameDuration = 512.0 / Double(sampleRate)
            var currentTime = Double(predDur[0]) * frameDuration

            for token in chunk {
                let wordText = token.text
                let wordPhonemes = token.phonemes ?? ""
                let whitespace = token.whitespace

                let wordStartCharOffset = charOffset
                charOffset += wordText.count + whitespace.count

                let wordStartTime = audioTimeOffset + currentTime

                for char in wordPhonemes {
                    if vocab[String(char)] != nil {
                        if tokenIdx < predDur.count - 1 {
                            currentTime += Double(predDur[tokenIdx]) * frameDuration
                            tokenIdx += 1
                        }
                    }
                }

                for char in whitespace {
                    if vocab[String(char)] != nil {
                        if tokenIdx < predDur.count - 1 {
                            currentTime += Double(predDur[tokenIdx]) * frameDuration
                            tokenIdx += 1
                        }
                    }
                }

                let wordEndTime = audioTimeOffset + currentTime

                let cleanWord = wordText.trimmingCharacters(in: .whitespacesAndNewlines)
                if !cleanWord.isEmpty && !wordPhonemes.isEmpty && token.tag != "." && token.tag != "," && token.tag != ":" {
                    let range = wordStartCharOffset..<charOffset
                    wordTimestamps.append(WordTimestamp(
                        word: cleanWord,
                        range: range,
                        startTime: wordStartTime,
                        endTime: wordEndTime
                    ))
                }
            }

            totalAudio.append(contentsOf: output.audio)
            audioTimeOffset += Double(output.audio.count) / Double(sampleRate)
            tokenOffset += chunk.count
        }

        let fullPhonemes = g2pResult.tokens.reduce(into: "") { partial, token in
            partial += (token.phonemes ?? "") + token.whitespace
        }.trimmingCharacters(in: .whitespacesAndNewlines)

        let limitedAudio = AudioWriter.limit(totalAudio)

        return Result(
            graphemes: cleanText,
            phonemes: fullPhonemes,
            audio: limitedAudio,
            sampleRate: sampleRate,
            timestamps: wordTimestamps
        )
    }


    /// Dynamic stream synthesis. Runs inference chunk-by-chunk and streams floating-point PCM audio buffers.
    public func synthesizeStream(
        text: String,
        voice: String,
        speed: Float = 1.0
    ) -> AsyncStream<[Float]> {
        let (stream, continuation) = AsyncStream<[Float]>.makeStream()

        Task {
            do {
                let phonemized = try resolveG2P().phonemize(text, langCode: langCode)
                let phonemes = phonemized.phonemes
                let chunks = ProsodiaActorPipeline.chunkPhonemes(phonemes, limit: 510)

                for chunk in chunks where !chunk.isEmpty {
                    let style = try await voices.styleVectorAsync(for: voice, phonemeCount: chunk.count)
                    let output = try engine.forward(phonemes: chunk, refS: style, speed: speed, durationScales: nil, f0Bias: nil)
                    continuation.yield(output.audio)
                }
                continuation.finish()
            } catch {
                continuation.finish()
            }
        }

        return stream
    }

    /// Dynamic stream synthesis with token-by-token style morphing (Phase 2 capability).
    /// Accepts a style transition closure or array of custom style blends per chunk.
    public func synthesizeStreamWithMorph(
        text: String,
        voiceBlends: [[CastingProfile]],
        speed: Float = 1.0
    ) -> AsyncStream<[Float]> {
        let (stream, continuation) = AsyncStream<[Float]>.makeStream()

        Task {
            do {
                let phonemized = try resolveG2P().phonemize(text, langCode: langCode)
                let phonemes = phonemized.phonemes
                let chunks = ProsodiaActorPipeline.chunkPhonemes(phonemes, limit: 510)

                for (idx, chunk) in chunks.enumerated() where !chunk.isEmpty {
                    // Pick the blend recipe for this chunk (fade between voices over chunks)
                    let blendRecipe = voiceBlends.indices.contains(idx) ? voiceBlends[idx] : (voiceBlends.last ?? [])
                    let style = try await voices.styleVectorAsync(for: blendRecipe, phonemeCount: chunk.count)
                    let output = try engine.forward(phonemes: chunk, refS: style, speed: speed, durationScales: nil, f0Bias: nil)
                    continuation.yield(output.audio)
                }
                continuation.finish()
            } catch {
                continuation.finish()
            }
        }

        return stream
    }

    /// Synthesizes speech and returns word-level timestamps alongside the synthesized audio.
    ///
    /// - Parameters:
    ///   - text: The input text passage to synthesize.
    ///   - voice: The voice string or blend description.
    ///   - speed: Global speed multiplier override.
    ///   - pitch: Global pitch offset in Hz.
    ///   - durationScales: Optional token-level duration scaling factors, one per G2P token.
    ///   - f0Bias: Optional token-level F0 pitch biases in Hz, one per G2P token.
    /// - Returns: A `Result` struct containing graphemes, phonemes, audio PCM frames, and word-level timestamps.
    public func synthesizeWithTimestamps(
        text: String,
        voice: String,
        speed: Float = 1.0,
        pitch: Float = 0.0,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) async throws -> Result {
        let g2pResult = try resolveG2P().phonemize(text, langCode: langCode)
        let voiceBlends = [String](repeating: voice, count: g2pResult.tokens.count)
        return try await synthesizeWithTimestamps(
            text: text,
            voiceBlends: voiceBlends,
            speed: speed,
            pitch: pitch,
            durationScales: durationScales,
            f0Bias: f0Bias
        )
    }

    /// Synthesizes speech with token-level voice blends and returns word-level timestamps.
    ///
    /// - Parameters:
    ///   - text: The input text passage to synthesize.
    ///   - voiceBlends: Array of voice strings representing the blend for each token.
    ///   - speed: Global speed multiplier override.
    ///   - pitch: Global pitch offset in Hz.
    ///   - durationScales: Optional token-level duration scaling factors, one per G2P token.
    ///   - f0Bias: Optional token-level F0 pitch biases in Hz, one per G2P token.
    /// - Returns: A `Result` struct containing graphemes, phonemes, audio PCM frames, and word-level timestamps.
    public func synthesizeWithTimestamps(
        text: String,
        voiceBlends: [String],
        speed: Float = 1.0,
        pitch: Float = 0.0,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) async throws -> Result {
        guard speed.isFinite, speed > 0 else {
            throw StyleTTS2Error.invalidSpeed(speed)
        }

        let g2pResult = try resolveG2P().phonemize(text, langCode: langCode)
        let tokenChunks = chunkTokens(g2pResult.tokens, limit: 510)
        let vocab = engine.vocab
        let frameDuration = 512.0 / Double(sampleRate)
        
        var totalAudio: [Float] = []
        var wordTimestamps: [WordTimestamp] = []
        var audioTimeOffset = 0.0
        var charOffset = 0
        
        var tokenOffset = 0
        for chunk in tokenChunks {
            let chunkPhonemes = chunk.reduce(into: "") { partial, token in
                partial += (token.phonemes ?? "") + token.whitespace
            }.trimmingCharacters(in: .whitespacesAndNewlines)
            
            guard !chunkPhonemes.isEmpty else { continue }
            
            let chunkCastingProfiles = Array(voiceBlends.suffix(from: min(tokenOffset, voiceBlends.count - 1)).prefix(chunk.count))
            let tokenPhonemesList = chunk.map { TokenPhonemes(phonemes: $0.phonemes ?? "", whitespace: $0.whitespace) }
            let style = try await voices.styleMatrixAsync(
                for: tokenPhonemesList,
                voiceBlends: chunkCastingProfiles.isEmpty ? [""] : chunkCastingProfiles,
                vocab: vocab
            )
            
            let ids = try engine.tokenize(chunkPhonemes)

            var chunkDurationScales: [Float]? = nil
            if let durationScales = durationScales {
                var scales = [Float](repeating: 1.0, count: ids.count)
                var pIdx = 1
                for (tIdx, token) in chunk.enumerated() {
                    let globalTokenIdx = tokenOffset + tIdx
                    let scale = globalTokenIdx < durationScales.count ? durationScales[globalTokenIdx] : 1.0
                    
                    let wordPhonemes = token.phonemes ?? ""
                    for char in wordPhonemes {
                        if vocab[String(char)] != nil {
                            if pIdx < ids.count - 1 {
                                scales[pIdx] = scale
                                pIdx += 1
                            }
                        }
                    }
                    for char in token.whitespace {
                        if vocab[String(char)] != nil {
                            if pIdx < ids.count - 1 {
                                scales[pIdx] = scale
                                pIdx += 1
                            }
                        }
                    }
                }
                chunkDurationScales = scales
            }

            let resolvedF0Bias: [Float]?
            if let f0Bias = f0Bias {
                var biases = [Float](repeating: 0.0, count: ids.count)
                var pIdx = 1
                for (tIdx, token) in chunk.enumerated() {
                    let globalTokenIdx = tokenOffset + tIdx
                    let bias = globalTokenIdx < f0Bias.count ? f0Bias[globalTokenIdx] : 0.0
                    
                    let wordPhonemes = token.phonemes ?? ""
                    for char in wordPhonemes {
                        if vocab[String(char)] != nil {
                            if pIdx < ids.count - 1 {
                                biases[pIdx] = bias
                                pIdx += 1
                            }
                        }
                    }
                    for char in token.whitespace {
                        if vocab[String(char)] != nil {
                            if pIdx < ids.count - 1 {
                                biases[pIdx] = bias
                                pIdx += 1
                            }
                        }
                    }
                }
                resolvedF0Bias = biases
            } else if pitch != 0.0 {
                resolvedF0Bias = [Float](repeating: pitch, count: ids.count)
            } else {
                resolvedF0Bias = nil
            }
            
            let output = try engine.forward(
                phonemes: chunkPhonemes,
                refS: style,
                speed: speed,
                durationScales: chunkDurationScales,
                f0Bias: resolvedF0Bias
            )
            
            let predDur = output.predDur
            var tokenIdx = 1
            var currentTime = Double(predDur[0]) * frameDuration
            
            for token in chunk {
                let wordText = token.text
                let wordPhonemes = token.phonemes ?? ""
                let whitespace = token.whitespace
                
                let wordStartCharOffset = charOffset
                charOffset += wordText.count + whitespace.count
                
                let wordStartTime = audioTimeOffset + currentTime
                
                for char in wordPhonemes {
                    if vocab[String(char)] != nil {
                        if tokenIdx < predDur.count - 1 {
                            currentTime += Double(predDur[tokenIdx]) * frameDuration
                            tokenIdx += 1
                        }
                    }
                }
                
                for char in whitespace {
                    if vocab[String(char)] != nil {
                        if tokenIdx < predDur.count - 1 {
                            currentTime += Double(predDur[tokenIdx]) * frameDuration
                            tokenIdx += 1
                        }
                    }
                }
                
                let wordEndTime = audioTimeOffset + currentTime
                
                let cleanWord = wordText.trimmingCharacters(in: .whitespacesAndNewlines)
                if !cleanWord.isEmpty && !wordPhonemes.isEmpty && token.tag != "." && token.tag != "," && token.tag != ":" {
                    let range = wordStartCharOffset..<charOffset
                    wordTimestamps.append(WordTimestamp(
                        word: cleanWord,
                        range: range,
                        startTime: wordStartTime,
                        endTime: wordEndTime
                    ))
                }
            }
            
            totalAudio.append(contentsOf: output.audio)
            audioTimeOffset += Double(output.audio.count) / Double(sampleRate)
            tokenOffset += chunk.count
        }
        
        let fullPhonemes = g2pResult.tokens.reduce(into: "") { partial, token in
            partial += (token.phonemes ?? "") + token.whitespace
        }.trimmingCharacters(in: .whitespacesAndNewlines)

        let limitedAudio = AudioWriter.limit(totalAudio)

        return Result(
            graphemes: text,
            phonemes: fullPhonemes,
            audio: limitedAudio,
            sampleRate: sampleRate,
            timestamps: wordTimestamps
        )
    }

    private func chunkTokens(_ tokens: [MToken], limit: Int = 510) -> [[MToken]] {
        var chunks: [[MToken]] = []
        var currentChunk: [MToken] = []
        var currentLen = 0
        
        for token in tokens {
            let tokenLen = (token.phonemes ?? "").count + token.whitespace.count
            if currentLen + tokenLen > limit && !currentChunk.isEmpty {
                chunks.append(currentChunk)
                currentChunk = [token]
                currentLen = tokenLen
            } else {
                currentChunk.append(token)
                currentLen += tokenLen
            }
        }
        if !currentChunk.isEmpty {
            chunks.append(currentChunk)
        }
        return chunks
    }

    private func synthesizeResolved(
        graphemes: String,
        phonemes: String,
        voice: String,
        speed: Float,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) async throws -> Result {
        guard speed.isFinite, speed > 0 else {
            throw StyleTTS2Error.invalidSpeed(speed)
        }

        let normalizedPhonemes = phonemes.trimmingCharacters(in: .whitespacesAndNewlines)
        let chunks = Self.chunkPhonemes(normalizedPhonemes, limit: 510)
        var audio: [Float] = []

        for chunk in chunks where !chunk.isEmpty {
            let style = try await voices.styleVectorAsync(for: voice, phonemeCount: chunk.count)
            let output = try engine.forward(
                phonemes: chunk,
                refS: style,
                speed: speed,
                durationScales: durationScales,
                f0Bias: f0Bias
            )
            audio.append(contentsOf: output.audio)
        }

        let limitedAudio = AudioWriter.limit(audio)

        return Result(
            graphemes: graphemes,
            phonemes: normalizedPhonemes,
            audio: limitedAudio,
            sampleRate: sampleRate
        )
    }

    private func resolveG2P() throws -> any ProsodiaG2PProcessor {
        if let g2p {
            return g2p
        }

        let normalized = Self.normalizedLangCode(langCode)
        switch normalized {
        case "en-us":
            let defaultG2P = try G2P(british: false, unk: "")
            self.g2p = defaultG2P
            return defaultG2P
        case "en-gb":
            let defaultG2P = try G2P(british: true, unk: "")
            self.g2p = defaultG2P
            return defaultG2P
        default:
            throw StyleTTS2Error.unsupportedLanguageCode(langCode)
        }
    }

    private static func normalizedLangCode(_ rawValue: String) -> String {
        switch rawValue
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: "_", with: "-")
        {
        case "a", "en", "en-us":
            return "en-us"
        case "b", "en-gb", "en-uk":
            return "en-gb"
        default:
            return rawValue
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased()
                .replacingOccurrences(of: "_", with: "-")
        }
    }

    public static func chunkPhonemes(_ phonemes: String, limit: Int = 510) -> [String] {
        let trimmed = phonemes.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count > limit else {
            return trimmed.isEmpty ? [] : [trimmed]
        }

        let breakCharacters = Set([" ", ".", ",", ";", ":", "!", "?", "—", "…"])
        let characters = Array(trimmed)
        var chunks: [String] = []
        var start = 0

        while start < characters.count {
            let endLimit = min(start + limit, characters.count)
            if endLimit == characters.count {
                chunks.append(String(characters[start..<endLimit]).trimmingCharacters(in: .whitespaces))
                break
            }

            var splitIndex = endLimit
            var cursor = endLimit - 1
            while cursor > start + (limit / 2) {
                if breakCharacters.contains(String(characters[cursor])) {
                    splitIndex = cursor + 1
                    break
                }
                cursor -= 1
            }

            chunks.append(String(characters[start..<splitIndex]).trimmingCharacters(in: .whitespaces))
            start = splitIndex
            while start < characters.count, characters[start].isWhitespace {
                start += 1
            }
        }

        return chunks.filter { !$0.isEmpty }
    }
}
