import Foundation
import Stage
import Kit

// Extend ProsodiaSpeech to conform to the UniFFI G2P interface
extension ProsodiaSpeech: Kit.ProsodiaG2pProcessor {}

/// A standard disk-based voice asset provider.
public class DiskVoiceAssetProvider: VoiceAssetProvider, @unchecked Sendable {
    public let baseDirectory: URL

    public init(baseDirectory: URL) {
        self.baseDirectory = baseDirectory
    }

    public func loadVoiceBytes(voiceName: String) -> Data? {
        let fileURL = baseDirectory.appendingPathComponent("\(voiceName).safetensors")
        return try? Data(contentsOf: fileURL)
    }
}

/// A wrapper class bridging the `ProsodiaActorBackend` to the UniFFI `ProsodiaSpeechEngine` interface.
class SwiftSpeechEngine: ProsodiaSpeechEngine {
    let backend: any ProsodiaActorBackend

    init(backend: any ProsodiaActorBackend) {
        self.backend = backend
    }

    func synthesize(input: PipelineOutput) -> Kit.ActorEngineOutput {
        fatalError("synthesize(input:) is deprecated, use forward instead")
    }

    func forward(
        phonemeIds: [Int32],
        style: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> Kit.ActorEngineOutput {
        let output = try backend.forward(
            phonemeIds: phonemeIds,
            refS: style,
            speed: speed,
            durationScales: durationScales,
            f0Bias: f0Bias
        )
        return Kit.ActorEngineOutput(audio: output.audio, predDur: output.predDur.map { Int32($0) })
    }

    func reclaimMemory() {
        backend.reclaimMemory()
    }
}

/// A ``VocalActor`` wrapped around ``ProsodiaActorEngine`` driving StyleTTS2 via LiteRT.
public actor LiteRtVocalActor: Stage.VocalActor {
    private let rustEngine: ProsodiaActorEngine

    public init(modelURL: URL, configURL: URL, voiceDirectoryURL: URL) throws {
        let provider = DiskVoiceAssetProvider(baseDirectory: voiceDirectoryURL)
        let voiceLoader = VoiceLoader(provider: provider)
        let g2p = ProsodiaSpeech()
        let configJson = try String(contentsOf: configURL, encoding: .utf8)

        let pipeline = try ProsodiaActorPipeline(
            g2p: g2p,
            voiceLoader: voiceLoader,
            configJson: configJson,
            sampleRate: Kit.getSampleRate(),
            langCode: "en-us"
        )
        let backend = try LiteRtActorEngine(modelPath: modelURL, configURL: configURL)
        let speechEngine = SwiftSpeechEngine(backend: backend)

        self.rustEngine = ProsodiaActorEngine(pipeline: pipeline, speechEngine: speechEngine)
    }

    public nonisolated func render(payload: String) -> [Float] {
        let semaphore = DispatchSemaphore(value: 0)
        var result: [Float] = []
        Task {
            result = await self.renderSingle(payload: payload)
            semaphore.signal()
        }
        semaphore.wait()
        return result
    }

    private func renderSingle(payload: String) async -> [Float] {
        guard let decoded = Kit.decodeSpans(payload: payload) else {
            return []
        }

        var totalAudio: [Float] = []
        for span in decoded.spans {
            let kitEmotion = Kit.EmotionVector(
                valence: span.emotion.valence,
                arousal: span.emotion.arousal,
                tension: span.emotion.tension
            )

            let kitAcoustics: Kit.ProsodyAcoustics?
            if let ac = span.acoustics {
                let kitCastingProfile: Kit.CastingProfile?
                if let cp = ac.castingProfile {
                    kitCastingProfile = Kit.CastingProfile(
                        ageProfile: cp.ageProfile,
                        masculinity: cp.masculinity,
                        strainOrRasp: cp.strainOrRasp
                    )
                } else {
                    kitCastingProfile = nil
                }

                kitAcoustics = Kit.ProsodyAcoustics(
                    speedMultiplier: ac.speedMultiplier,
                    speedBias: ac.speedBias,
                    gainMultiplier: ac.gainMultiplier,
                    gainBias: ac.gainBias,
                    castingProfile: kitCastingProfile,
                    speakerLock: ac.speakerLock,
                    pauseMultiplier: ac.pauseMultiplier,
                    pronunciationOverride: ac.pronunciationOverride,
                    pitch: ac.pitch,
                    tokenDurationScales: ac.tokenDurationScales,
                    tokenF0Biases: ac.tokenF0Biases
                )
            } else {
                kitAcoustics = nil
            }

            let kitSpan = Kit.ProsodySpan(
                text: span.text,
                emotion: kitEmotion,
                leadingPause: span.leadingPause,
                acoustics: kitAcoustics
            )

            let output = rustEngine.processAndSynthesize(span: kitSpan)
            totalAudio.append(contentsOf: output.audio)
        }

        return totalAudio
    }

    public nonisolated func render(stream: AsyncStream<String>) -> any PlaybackController {
        let (eventStream, eventContinuation) = AsyncStream<PlaybackEvent>.makeStream()
        Task { [self] in
            var index = 0
            for await payload in stream {
                _ = render(payload: payload)
                eventContinuation.yield(.sentenceBegan(index: index))
                eventContinuation.yield(.sentenceScheduled(index: index, timestamps: []))
                index += 1
            }
            eventContinuation.yield(.finished)
            eventContinuation.finish()
        }
        return StubPlaybackController(events: eventStream)
    }

    public func reclaimMemory() async {
        rustEngine.reclaimMemory()
    }
}

/// A provider that handles initialization of the LiteRT-backed vocal actor.
public struct LiteRtVocalActorProvider: VocalActorProvider {
    public init() {}

    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        let isTflite = ext == "tflite" || modelURL.path.hasSuffix(".tflite")
        let configURL = modelURL.deletingLastPathComponent().appendingPathComponent("config.json")
        return isTflite &&
               FileManager.default.fileExists(atPath: modelURL.path) &&
               FileManager.default.fileExists(atPath: configURL.path)
    }

    public func makeActor(modelURL: URL, voiceDirectoryURL: URL?) -> any Stage.VocalActor {
        let configURL = modelURL.deletingLastPathComponent().appendingPathComponent("config.json")
        let voiceDir = voiceDirectoryURL ?? modelURL.deletingLastPathComponent()
        return try! LiteRtVocalActor(modelURL: modelURL, configURL: configURL, voiceDirectoryURL: voiceDir)
    }
}

/// Global entry point to register all compiled vocal actor backends with the registry.
public func registerProsodiaActors() {
    VocalActorRegistry.shared.register(provider: LiteRtVocalActorProvider())
}
