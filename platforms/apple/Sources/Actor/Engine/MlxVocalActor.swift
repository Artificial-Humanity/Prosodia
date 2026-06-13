import Foundation
import MLX
import Misaki
import ProsodiaStage

// MARK: - Language

/// Narration language (default US English).
public enum Language: String, CaseIterable, Sendable {
    case enUS = "en-us"
    case enGB = "en-gb"
}

// MARK: - Errors

// MARK: - Errors

/// Errors thrown or emitted during MLX-based audio rendering.
public enum MlxActorError: Error, Sendable {
    /// The specified voice is not loaded or known.
    /// - Parameter voice: The name of the missing voice.
    case unknownVoice(String)
    /// Synthesis engine failed to generate audio.
    case synthesisFailed
}

// MARK: - MlxVocalActor

/// An ``VocalActor`` backed by an MLX TTS engine (like StyleTTS2 82M) via ProsodiaActor.
///
/// ``render(stream:)`` starts synthesis in a background task and returns a
/// ``MlxVocalActorPlaybackController`` immediately, so the caller can pause, resume, or
/// stop the session while audio plays. The controller emits ``PlaybackEvent``s as
/// each sentence is scheduled into `AVAudioEngine`.
public actor MlxVocalActor: VocalActor {
    /// The URL to the compiled MLX weights or CoreML model folder.
    private let modelPath: URL
    /// The directory URL where voice files are located.
    private let voiceDirectory: URL
    /// The loader handling local style-vector files.
    private var voices: VoiceLoader?
    /// The underlying Misaki-based audio synthesis pipeline.
    private var pipeline: ProsodiaActorPipeline?
    /// The narration language used for tokenization.
    private let language: Language
    /// The phrase boundaries splitter.
    private let phraser: any ProsodyPhraser
    /// The mapping mechanism to translate VAD vectors to voice weights.
    private let voiceMap: any ActorVoiceMap
    /// The output audio playback sink handling AVAudioEngine.
    private let sink: StageAudioSink
    /// A global multiplier applying on top of per-phrase speeds.
    private var speedMultiplier: Double = 1.0
    /// The aggregate count of audio samples scheduled.
    private var totalSamplesScheduled: Int64 = 0
    /// Tracks active background progress reporting tasks.
    private var progressTasks: [UUID: Task<Void, Never>] = [:]
    /// The primary base voice to blend on top of the dynamic matrix blend.
    private var baseVoice: String? = nil
    /// Tracks the voice blend used in the last resolved phrase chunk to apply temporal smoothing/coherence.
    private var lastResolvedBlend: [ProsodiaStage.CastingProfile]? = nil

    #if DEBUG
    /// Internal access to last resolved voice blend for testing.
    var testLastResolvedBlend: [ProsodiaStage.CastingProfile]? {
        lastResolvedBlend
    }
    /// Resets the resolved blend history for testing.
    func testResetLastResolvedBlend() {
        lastResolvedBlend = nil
    }
    #endif

    /// Resets the scheduled sample count and cancels all progress tasks.
    private func resetScheduledSamples() {
        totalSamplesScheduled = 0
        for task in progressTasks.values {
            task.cancel()
        }
        progressTasks.removeAll()
        lastResolvedBlend = nil
    }

    /// Removes a finished progress reporting task.
    /// - Parameter id: The UUID identifier of the progress task to remove.
    private func removeProgressTask(_ id: UUID) {
        progressTasks.removeValue(forKey: id)
    }

    /// Initializes a new MLX actor renderer.
    ///
    /// - Parameters:
    ///   - modelPath: URL to `styletts2_lite.safetensors` or CoreML model directory.
    ///   - voiceDirectory: Directory holding the `anchor_*.safetensors` voice packs.
    ///   - language: Narration language (default US English).
    ///   - phraser: Fallback splitter for chunks the Director didn't phrase.
    ///     Defaults to ``SentencePhraser``; the Director supplies real intra-sentence phrasing over the wire when it can.
    ///   - voiceMap: Emotion→voice mapping for chunks the Director didn't pin a voice on.
    ///     Defaults to ``StyleVoiceMatrix`` (the `anchor_*` Gaussian blend).
    public init(
        modelPath: URL,
        voiceDirectory: URL,
        language: Language = .enUS,
        phraser: any ProsodyPhraser = SentencePhraser(),
        voiceMap: (any ActorVoiceMap)? = nil
    ) {
        self.modelPath = modelPath
        self.voiceDirectory = voiceDirectory
        self.language = language
        self.phraser = phraser
        
        var voices: [String] = []
        if let contents = try? FileManager.default.contentsOfDirectory(at: voiceDirectory, includingPropertiesForKeys: nil) {
            for file in contents {
                let ext = file.pathExtension.lowercased()
                if ext == "safetensors" || ext == "npy" {
                    let name = file.deletingPathExtension().lastPathComponent
                    if name != "styletts2_lite" && name != "epochs_2nd" && !name.contains("epochs_") {
                        voices.append(name)
                    }
                }
            }
        }
        
        self.voiceMap = voiceMap ?? MlxVoiceMatrix(availableVoices: voices.isEmpty ? nil : voices)
        self.sink = StageAudioSink(sampleRate: 24000.0)
        self.voices = nil
        self.pipeline = nil
    }

    /// Ensures that the backend speech synthesis pipeline is fully loaded and initialized.
    /// This runs asynchronously inside the actor's context to prevent blocking the caller (like the Main UI thread).
    @discardableResult
    private func ensurePipelineInitialized() async throws -> ProsodiaActorPipeline {
        if let existing = pipeline {
            return existing
        }
        
        let downloader = VoiceDownloader()
        let voicesLoader = VoiceLoader(baseDirectory: voiceDirectory, downloader: downloader)
        self.voices = voicesLoader
        
        let fileManager = FileManager.default
        var isDir: ObjCBool = false
        let exists = fileManager.fileExists(atPath: modelPath.path, isDirectory: &isDir)
        let isDirectory = exists && isDir.boolValue
        
        let isTflite = modelPath.path.hasSuffix(".tflite")
        let isCoreML = !modelPath.path.hasSuffix(".safetensors") && !isTflite
        
        let backend: any ProsodiaActorBackend
        if isTflite {
            let configURL = modelPath.deletingLastPathComponent().appendingPathComponent("config.json")
            
            // Download and compile models and configurations if missing
            let assetManager = ModelAssetManager()
            try await assetManager.ensureConfigReady(inDirectory: modelPath.deletingLastPathComponent())
            let modelName = modelPath.deletingPathExtension().lastPathComponent
            try await assetManager.ensureTfliteModelReady(named: modelName, inDirectory: modelPath.deletingLastPathComponent())
            
            backend = try LiteRtActorEngine(modelPath: modelPath, configURL: configURL)
        } else if isCoreML {
            let coreMlDir = isDirectory ? modelPath : modelPath.deletingLastPathComponent()
            
            // Download and compile models and configurations if missing
            let assetManager = ModelAssetManager()
            try await assetManager.ensureConfigReady(inDirectory: coreMlDir)
            try await assetManager.ensureModelReady(named: "styletts2_lite", inDirectory: coreMlDir)
            
            backend = try CoreMlProsodiaActorEngine(modelsDirectory: coreMlDir)
        } else {
            let configURL = modelPath.deletingLastPathComponent().appendingPathComponent("config.json")
            backend = try ProsodiaActorEngine(configURL: configURL, weightsURL: modelPath)
        }
        
        let langStr = language.rawValue
        let newPipeline = ProsodiaActorPipeline(engine: backend, voices: voicesLoader, sampleRate: 24000, langCode: langStr)
        self.pipeline = newPipeline
        return newPipeline
    }


    // MARK: - VocalActor

    /// Starts rendering and returns a controller for the session. Synthesis and
    /// scheduling proceed in a background task; the controller gives the caller
    /// pause / resume / stop and a stream of ``PlaybackEvent``s.
    ///
    /// - Parameter stream: An asynchronous stream of wire-format payload strings from the Director.
    /// - Returns: A ``PlaybackController`` to manage the rendering playback session.
    public nonisolated func render(stream: AsyncStream<String>) -> any PlaybackController {
        let (eventStream, eventContinuation) = AsyncStream<PlaybackEvent>.makeStream()
        let renderTask = Task { [self] in
            do {
                _ = try await self.ensurePipelineInitialized()
            } catch {
                eventContinuation.yield(.segmentFailed(index: 0, error))
                eventContinuation.finish()
                return
            }
            await self.resetScheduledSamples()
            var index = 0
            for await payload in stream {
                if Task.isCancelled {
                    break
                }
                guard let decoded = ProsodyPayload.decodeSpans(payload) else { continue }
                let spans = self.phraser.resolveSpans(overall: decoded.overall, decoded: decoded.spans)
                do {
                    eventContinuation.yield(.sentenceBegan(index: index))
                    let timestamps = try await self.renderChunk(
                        overall: decoded.overall,
                        acoustics: decoded.acoustics,
                        spans: spans,
                        index: index,
                        eventContinuation: eventContinuation
                    )
                    eventContinuation.yield(.sentenceScheduled(index: index, timestamps: timestamps))
                } catch {
                    eventContinuation.yield(.segmentFailed(index: index, error))
                }
                index += 1
            }
            eventContinuation.yield(.finished)
            eventContinuation.finish()
        }
        return MlxVocalActorPlaybackController(renderer: self, renderTask: renderTask, events: eventStream)
    }

    /// Synthesizes and schedules a single segment. Exposed for the interruption
    /// engine, which drives rendering one directive at a time. The phraser shapes
    /// the text into spans first, so even a single-emotion segment gets
    /// intra-sentence pacing where it applies.
    ///
    /// - Parameters:
    ///   - directive: The prosody directive containing VAD and acoustic options.
    ///   - text: The text of the segment to render.
    /// - Returns: An array of ``TokenTimestamp``s indicating word boundaries.
    /// - Throws: An error if synthesis or scheduling fails.
    @discardableResult
    public func renderSegment(directive: ProsodyDirective, text: String) async throws -> [TokenTimestamp] {
        _ = try await ensurePipelineInitialized()
        let spans = directive.acoustics == nil
            ? phraser.spans(for: text, emotion: directive.emotion)
            : [ProsodySpan(text: text, emotion: directive.emotion, acoustics: directive.acoustics)]
        return try await renderChunk(overall: directive.emotion, acoustics: directive.acoustics, spans: spans)
    }

    /// Helper to resolve character-level ranges of tokens inside text.
    ///
    /// - Parameters:
    ///   - tokens: The list of raw token structures returned by Misaki.
    ///   - text: The original source text string.
    /// - Returns: An array of NSRanges mapping each token back to the source text.
    public static func resolveTokenRanges(tokens: [MToken], text: String) -> [NSRange] {
        var ranges: [NSRange] = []
        var searchStart = text.startIndex
        
        for token in tokens {
            if token.text.isEmpty {
                ranges.append(NSRange(location: 0, length: 0))
                continue
            }
            
            if let range = text.range(of: token.text, options: [], range: searchStart..<text.endIndex) {
                let location = text.distance(from: text.startIndex, to: range.lowerBound)
                let length = text.distance(from: range.lowerBound, to: range.upperBound)
                ranges.append(NSRange(location: location, length: length))
                searchStart = range.upperBound
            } else {
                ranges.append(NSRange(location: 0, length: 0))
            }
        }
        return ranges
    }

    /// Renders a chunk span by span. The `overall` emotion fixes the voice for the
    /// whole chunk (so timbre stays continuous across the sentence), while each
    /// span's own emotion drives its speed and volume — a higher-arousal phrase is
    /// faster and louder, a calmer one slower and quieter.
    ///
    /// Each phrase is a separate synthesis, so we trim the synthesis engine's padding silence
    /// from every fragment and insert one controlled beat between phrases (from the
    /// preceding phrase's punctuation, or an explicit ``ProsodySpan/leadingPause``).
    /// Without this, the stacked padding makes phrase joins gap unnaturally.
    ///
    /// - Parameters:
    ///   - overall: The length-weighted average emotion vector of the chunk.
    ///   - acoustics: Any chunk-level acoustics configurations.
    ///   - spans: The array of prosody spans to render.
    ///   - index: The current sentence index for progress tracing.
    ///   - eventContinuation: Continuation to yield events during rendering.
    /// - Returns: An array of ``TokenTimestamp``s for word alignment.
    /// - Throws: An error if the pipeline synthesis fails.
    @discardableResult
    private func renderChunk(
        overall: EmotionVector,
        acoustics: ProsodyAcoustics?,
        spans: [ProsodySpan],
        index: Int = 0,
        eventContinuation: AsyncStream<PlaybackEvent>.Continuation? = nil
    ) async throws -> [TokenTimestamp] {
        let chunkInitialCompleted = self.totalSamplesScheduled
        let resolvedBlend: [ProsodiaStage.CastingProfile]
        if let acoustics = acoustics, !acoustics.voiceBlend.isEmpty {
            resolvedBlend = acoustics.voiceBlend
            self.lastResolvedBlend = nil // Reset history on explicit character switch
        } else if let acoustics = acoustics, let lock = acoustics.speakerLock {
            resolvedBlend = [ProsodiaStage.CastingProfile(voice: lock, fraction: 1.0)]
            self.lastResolvedBlend = nil // Reset history on explicit character switch
        } else {
            let rawBlend = voiceMap.voiceBlend(for: overall)
            if let baseVoice = self.baseVoice {
                resolvedBlend = MlxVoiceMatrix.applyBaseVoice(baseVoice, to: rawBlend)
                self.lastResolvedBlend = resolvedBlend
            } else {
                if let lastBlend = self.lastResolvedBlend {
                    // Perform Exponential Moving Average (EMA) smoothing to prevent abrupt jumps in timbre
                    let alpha = 0.4
                    var blendedMap: [String: Double] = [:]
                    
                    // Blend current raw values
                    for entry in rawBlend {
                        blendedMap[entry.voice, default: 0.0] += alpha * entry.fraction
                    }
                    // Blend last values
                    for entry in lastBlend {
                        blendedMap[entry.voice, default: 0.0] += (1.0 - alpha) * entry.fraction
                    }
                    
                    // Filter out negligible contributors (< blendMinimumFraction) and renormalize
                    let minFraction = MlxVoiceMatrix.blendMinimumFraction
                    var filtered = blendedMap.filter { $0.value >= minFraction }
                    if filtered.isEmpty {
                        if let best = blendedMap.max(by: { $0.value < $1.value }) {
                            filtered = [best.key: 1.0]
                        } else {
                            filtered = ["anchor_female_adult": 1.0]
                        }
                    }
                    
                    let total = filtered.reduce(0.0) { $0 + $1.value }
                    resolvedBlend = filtered.map { ProsodiaStage.CastingProfile(voice: $0.key, fraction: $0.value / total) }
                        .sorted { $0.fraction > $1.fraction }
                } else {
                    resolvedBlend = rawBlend
                }
                self.lastResolvedBlend = resolvedBlend
            }
        }

        let mappedBlend = resolvedBlend.map { ProsodiaActor.CastingProfile(voice: $0.voice, fraction: $0.fraction) }
        let voiceString = mappedBlend.map { "\($0.voice):\($0.fraction)" }.joined(separator: ",")

        var allTimestamps: [TokenTimestamp] = []
        var chunkTimeOffset = 0.0
        var spanCharOffset = 0

        let phrases = spans.filter { !$0.text.isEmpty }
        
        // Pre-synthesize all spans of the chunk first to avoid synthesis latency gaps during playback
        var synthesizedSpans: [(result: ProsodiaActorPipeline.Result, cleanText: String, span: ProsodySpan)] = []
        for (spanIndex, span) in phrases.enumerated() {
            if Task.isCancelled {
                break
            }
            let rawText = span.acoustics?.pronunciationOverride ?? span.text
            let textToSynthesize = cleanTextForSynthesis(rawText, isLast: spanIndex == phrases.count - 1)
            
            let durationScales = span.acoustics?.tokenDurationScales?.map { Float($0) }
            let f0Bias = span.acoustics?.tokenF0Biases?.map { Float($0) }
            
            let pipeline = try await ensurePipelineInitialized()
            let result = try await pipeline.synthesizeWithTimestamps(
                text: textToSynthesize,
                voice: voiceString,
                speed: Float(span.speed * speedMultiplier),
                pitch: Float(span.pitch),
                durationScales: durationScales,
                f0Bias: f0Bias
            )
            synthesizedSpans.append((result, textToSynthesize, span))
        }
        
        // Play the pre-synthesized spans sequentially
        for (spanIndex, item) in synthesizedSpans.enumerated() {
            if Task.isCancelled {
                break
            }
            let result = item.result
            let span = item.span
            
            let derivedPause = spanIndex == 0 ? 0 : PhrasePause.after(phrases[spanIndex - 1].text)
            let multiplier = span.acoustics?.pauseMultiplier ?? acoustics?.pauseMultiplier ?? 1.0
            let pause = max(span.leadingPause, derivedPause * multiplier)
            if pause > 0 {
                try await sink.scheduleSilence(seconds: pause)
                chunkTimeOffset += pause
                let pauseSamples = Int64((pause * 24000.0).rounded())
                self.totalSamplesScheduled += pauseSamples
            }
            
            let trimThreshold: Float = 0.005
            let trimmed = AudioShaping.trimmingSilence(result.audio, threshold: trimThreshold)
            let trimmedLeadingSamples = result.audio.firstIndex(where: { abs($0) > trimThreshold }) ?? 0
            let trimmedLeadingSeconds = Double(trimmedLeadingSamples) / 24000.0
            
            var currentPhraseTimestamps: [TokenTimestamp] = []
            if let wordTimestamps = result.timestamps {
                for ts in wordTimestamps {
                    let adjustedStart = max(0.0, ts.startTime - trimmedLeadingSeconds)
                    let adjustedEnd = max(0.0, ts.endTime - trimmedLeadingSeconds)
                    let tokenTs = TokenTimestamp(
                        text: ts.word,
                        characterOffset: spanCharOffset + ts.range.lowerBound,
                        startSeconds: chunkTimeOffset + adjustedStart,
                        endSeconds: chunkTimeOffset + adjustedEnd
                    )
                    currentPhraseTimestamps.append(tokenTs)
                }
            }
            allTimestamps.append(contentsOf: currentPhraseTimestamps)

            let gain = span.gain
            let shaped = gain == 1.0
                ? trimmed
                : trimmed.map { max(-1.0, min(1.0, $0 * Float(gain))) }

            self.totalSamplesScheduled += Int64(shaped.count)
            try await sink.schedule(samples: shaped)
            
            chunkTimeOffset += Double(trimmed.count) / 24000.0
            spanCharOffset += span.text.count
        }
        
        if let eventContinuation = eventContinuation {
            let taskId = UUID()
            let progressTask = Task { [self, sink, eventContinuation, chunkInitialCompleted, allTimestamps, chunkTimeOffset] in
                var lastReportedOffset: Int? = nil
                while !Task.isCancelled {
                    do {
                        try await Task.sleep(nanoseconds: 50_000_000) // 50ms
                    } catch {
                        break
                    }
                    let elapsed = await sink.getElapsedSeconds(relativeTo: chunkInitialCompleted)
                    if elapsed >= chunkTimeOffset {
                        break
                    }
                    if let matching = allTimestamps.last(where: { elapsed >= $0.startSeconds }) {
                        if lastReportedOffset != matching.characterOffset {
                            eventContinuation.yield(.playbackProgress(index: index, characterOffset: matching.characterOffset))
                            lastReportedOffset = matching.characterOffset
                        }
                    }
                }
                self.removeProgressTask(taskId)
            }
            self.progressTasks[taskId] = progressTask
        }
        
        return allTimestamps
    }

    /// Cleans trailing whitespace and non-speech symbols off intermediate spans to prevent synthesis engine silence
    /// while preserving critical prosodic punctuation (like periods and commas) that guide natural intonation.
    ///
    /// - Parameters:
    ///   - text: The input span text.
    ///   - isLast: A boolean indicating whether this is the final span in the chunk.
    /// - Returns: The cleaned text string.
    private func cleanTextForSynthesis(_ text: String, isLast: Bool) -> String {
        if isLast {
            return text
        }
        var clean = text
        let trimChars = CharacterSet(charactersIn: " \n\t)]}»”’\"'")
        while let last = clean.last, String(last).rangeOfCharacter(from: trimChars) != nil {
            clean.removeLast()
        }
        return clean
    }

    // MARK: - Transport

    /// Pauses audio playback in the audio sink.
    func pausePlayback() async { await sink.pause() }
    /// Resumes audio playback in the audio sink.
    func resumePlayback() async { await sink.resume() }
    /// Stops audio playback and resets the scheduled sample state.
    func stopPlayback() async {
        await sink.stop()
        resetScheduledSamples()
    }

    /// Reclaims memory consumed by the synthesis pipeline and loaded style vectors.
    public func reclaimMemory() async {
        if pipeline != nil {
            await pipeline?.reclaimMemory()
            pipeline = nil
            voices?.clearCache()
            #if canImport(MLX)
            MLX.Memory.clearCache()
            #endif
        }
    }

    /// Updates the global speed multiplier used across all synthesis operations.
    ///
    /// - Parameter speed: The new speed multiplier value.
    public func updateSpeedMultiplier(_ speed: Double) {
        self.speedMultiplier = speed
    }

    /// Sets the base voice override.
    ///
    /// - Parameter voice: The identifier of the base voice to use, or `nil` to clear.
    public func setBaseVoice(_ voice: String?) {
        self.baseVoice = voice
    }
}

// MARK: - MlxVocalActorPlaybackController

/// The ``PlaybackController`` returned by ``MlxVocalActor/render(stream:)``.
public struct MlxVocalActorPlaybackController: PlaybackController, @unchecked Sendable {
    /// The associated actor audio renderer.
    private let renderer: MlxVocalActor
    /// The background rendering and scheduling task.
    private let renderTask: Task<Void, Never>
    /// The stream of playback events.
    public let events: AsyncStream<PlaybackEvent>

    /// Initializes a new playback controller.
    ///
    /// - Parameters:
    ///   - renderer: The actor audio renderer.
    ///   - renderTask: The task driving synthesis.
    ///   - events: The event stream.
    init(renderer: MlxVocalActor,
         renderTask: Task<Void, Never>,
         events: AsyncStream<PlaybackEvent>) {
        self.renderer = renderer
        self.renderTask = renderTask
        self.events = events
    }

    /// Pauses the current playback session.
    public func pause()  async { await renderer.pausePlayback() }
    /// Resumes the current playback session.
    public func resume() async { await renderer.resumePlayback() }
    /// Stops the current playback session and cancels synthesis.
    public func stop()   async {
        renderTask.cancel()
        await renderer.stopPlayback()
    }
}
