#if canImport(CLlama) && canImport(AVFoundation)
import Foundation
import os
import AVFoundation
import Kit
import CLlama

// MARK: - Voice Map

/// The speaker-selection mapping for the GGUF Actor. GGUF TTS does not have a continuous
/// latent blend today.
public struct GgufActorVoiceMap: ActorVoiceMap {
    private let speaker: String

    public init(speaker: String = "default") {
        self.speaker = speaker
    }

    public func voiceBlend(for _: EmotionVector) -> [ProsodiaStage.CastingProfile] {
        [ProsodiaStage.CastingProfile(voice: speaker, fraction: 1.0)]
    }

    public func validate(_ id: String) -> Bool {
        id == speaker
    }
}

// MARK: - Errors

public enum GgufActorError: Error, Sendable {
    case synthesisNotImplemented
    case modelLoadFailed
}

// MARK: - LlamaTtsSessionBox

/// Box that owns the opaque C TTS session handle and frees it on deinit.
final class LlamaTtsSessionBox: @unchecked Sendable {
    let handle: OpaquePointer

    init(handle: OpaquePointer) {
        self.handle = handle
    }

    deinit {
        cllama_tts_session_free(handle)
    }
}

// MARK: - GgufVocalActor

/// An ``VocalActor`` backed by a **GGUF TTS** model (OuteTTS-style) via the
/// vendored llama.cpp runtime, the GGUF analogue of ``MlxVocalActor``.
public actor GgufVocalActor: VocalActor {

    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "Actor")

    /// WavTokenizer/OuteTTS native output sample rate.
    private static let sampleRate: Double = 24_000

    private let modelPath: URL
    private let vocoderPath: URL
    private let phraser: any ProsodyPhraser
    private let voiceMap: any ActorVoiceMap
    private let sink: StageAudioSink

    /// Loaded C TTS session box, lazily loaded.
    private var sessionBox: LlamaTtsSessionBox?

    /// - Parameters:
    ///   - modelPath: OuteTTS text-to-codes GGUF model.
    ///   - vocoderPath: WavTokenizer codes-to-speech GGUF vocoder.
    ///   - speaker: opaque speaker ID for ``GgufActorVoiceMap`` (reference voice).
    ///   - phraser: fallback splitter for chunks the Director didn't phrase.
    public init(
        modelPath: URL,
        vocoderPath: URL,
        speaker: String = "default",
        phraser: any ProsodyPhraser = SentencePhraser()
    ) {
        self.modelPath = modelPath
        self.vocoderPath = vocoderPath
        self.phraser = phraser
        self.voiceMap = GgufActorVoiceMap(speaker: speaker)
        self.sink = StageAudioSink(sampleRate: Self.sampleRate)
    }

    /// Reclaims memory by releasing the loaded GGUF TTS session box.
    public func reclaimMemory() async {
        self.sessionBox = nil
        Self.log.info("GGUF Actor model memory reclaimed.")
    }

    // MARK: - VocalActor

    public nonisolated func render(stream: AsyncStream<String>) -> any PlaybackController {
        let (eventStream, eventContinuation) = AsyncStream<PlaybackEvent>.makeStream()
        let renderTask = Task { [self] in
            var index = 0
            for await payload in stream {
                guard let decoded = ProsodyPayload.decodeSpans(payload) else { continue }
                let spans = self.phraser.resolveSpans(overall: decoded.overall, decoded: decoded.spans)
                do {
                    eventContinuation.yield(.sentenceBegan(index: index))
                    try await self.renderChunk(overall: decoded.overall, spans: spans)
                    eventContinuation.yield(.sentenceScheduled(index: index, timestamps: []))
                } catch {
                    eventContinuation.yield(.segmentFailed(index: index, error))
                }
                index += 1
            }
            eventContinuation.yield(.finished)
            eventContinuation.finish()
        }
        return GgufVocalActorPlaybackController(renderer: self, renderTask: renderTask, events: eventStream)
    }

    private func renderChunk(overall: EmotionVector, spans: [ProsodySpan]) async throws {
        _ = voiceMap.voiceBlend(for: overall)  // resolved speaker (single-voice today)

        let phrases = spans.filter { !$0.text.isEmpty }
        for (i, span) in phrases.enumerated() {
            let derivedPause = i == 0 ? 0 : PhrasePause.after(phrases[i - 1].text)
            let multiplier = span.acoustics?.pauseMultiplier ?? 1.0
            let pause = max(span.leadingPause, derivedPause * multiplier)
            if pause > 0 {
                try await sink.scheduleSilence(seconds: pause)
            }

            let textToSynthesize = span.acoustics?.pronunciationOverride ?? span.text
            let samples = try synthesize(text: textToSynthesize, speed: span.speed)
            let gain = span.gain
            let shaped = gain == 1.0
                ? samples
                : samples.map { max(-1.0, min(1.0, $0 * Float(gain))) }
            try await sink.schedule(samples: shaped)
        }
    }

    private func loadedSession() -> LlamaTtsSessionBox? {
        if let box = sessionBox { return box }
        guard let loaded = cllama_tts_session_load(modelPath.path, vocoderPath.path, 999) else {
            return nil
        }
        let box = LlamaTtsSessionBox(handle: loaded)
        sessionBox = box
        Self.log.info("GGUF Actor TTS session loaded: \(self.modelPath.lastPathComponent, privacy: .public) & \(self.vocoderPath.lastPathComponent, privacy: .public)")
        return box
    }

    private func synthesize(text: String, speed: Double) throws -> [Float] {
        guard let session = loadedSession() else {
            throw GgufActorError.modelLoadFailed
        }
        var size: Int32 = 0
        guard let pcmPointer = cllama_tts_synthesize(session.handle, text, Float(speed), &size), size > 0 else {
            throw GgufActorError.synthesisNotImplemented
        }
        defer { cllama_pcm_free(pcmPointer) }

        let pcmBuffer = UnsafeBufferPointer(start: pcmPointer, count: Int(size))
        return Array(pcmBuffer)
    }

    // MARK: - Transport

    func pausePlayback() async { await sink.pause() }
    func resumePlayback() async { await sink.resume() }
    func stopPlayback() async { await sink.stop() }

    public func updateSpeedMultiplier(_ speed: Double) {}
}

// MARK: - GgufVocalActorPlaybackController

/// The ``PlaybackController`` returned by ``GgufVocalActor/render(stream:)``.
public struct GgufVocalActorPlaybackController: PlaybackController, @unchecked Sendable {
    private let renderer: GgufVocalActor
    private let renderTask: Task<Void, Never>
    public let events: AsyncStream<PlaybackEvent>

    init(renderer: GgufVocalActor,
         renderTask: Task<Void, Never>,
         events: AsyncStream<PlaybackEvent>) {
        self.renderer = renderer
        self.renderTask = renderTask
        self.events = events
    }

    public func pause()  async { await renderer.pausePlayback() }
    public func resume() async { await renderer.resumePlayback() }
    public func stop()   async {
        renderTask.cancel()
        await renderer.stopPlayback()
    }
}
#endif // canImport(CLlama) && canImport(AVFoundation)
