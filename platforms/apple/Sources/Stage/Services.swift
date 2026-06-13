import Foundation
import ProsodiaSwiftCore

// MARK: - Shared Value Types

/// A lightweight handle to a book on disk. The engine resolves the concrete
/// parser (txt / pdf / epub / mobi) from `fileURL` at ingestion time.
public struct BookReference: Sendable, Equatable {
    /// The unique identifier of the book.
    public let id: UUID
    /// The local file URL pointing to the book on disk.
    public let fileURL: URL

    /// Initializes a new BookReference.
    ///
    /// - Parameters:
    ///   - id: The unique identifier (defaults to a random UUID).
    ///   - fileURL: The local URL pointing to the file.
    public init(id: UUID = UUID(), fileURL: URL) {
        self.id = id
        self.fileURL = fileURL
    }
}

// MARK: - Playback Position Sync

/// A word-level timestamp indicating the relative start/end seconds and character offset within a sentence.
public struct TokenTimestamp: Sendable, Codable, Equatable {
    /// The word text.
    public let text: String
    /// Character offset of the word start, relative to the sentence plain text.
    public let characterOffset: Int
    /// Timing in seconds, relative to the start of the sentence audio buffer.
    public let startSeconds: Double
    /// Timing in seconds, relative to the start of the sentence audio buffer.
    public let endSeconds: Double

    /// Initializes a new TokenTimestamp.
    ///
    /// - Parameters:
    ///   - text: The word text.
    ///   - characterOffset: Character offset.
    ///   - startSeconds: Start time in seconds.
    ///   - endSeconds: End time in seconds.
    public init(text: String, characterOffset: Int, startSeconds: Double, endSeconds: Double) {
        self.text = text
        self.characterOffset = characterOffset
        self.startSeconds = startSeconds
        self.endSeconds = endSeconds
    }
}

// MARK: - Playback Control Plane

/// Lifecycle events emitted by an active ``PlaybackController``.
public enum PlaybackEvent: Sendable {
    /// Synthesis and scheduling has started for the sentence at `index` in the
    /// payload stream (zero-based, within the current render session).
    case sentenceBegan(index: Int)
    /// The audio buffer for sentence `index` has been fully scheduled into
    /// `AVAudioEngine` (playback may still be in progress).
    case sentenceScheduled(index: Int, timestamps: [TokenTimestamp])
    /// Real-time playback progress update with active character offset.
    case playbackProgress(index: Int, characterOffset: Int)
    /// All sentences have been scheduled; the stream has finished.
    case finished
    /// A non-fatal error occurred while processing a sentence. Playback continues.
    case segmentFailed(index: Int, Error)
}

/// A handle returned by ``VocalActor/render(stream:)`` that lets callers
/// pause, resume, and stop an active render session, and observe lifecycle events.
///
/// The controller is valid for the lifetime of the render session. After
/// ``stop()`` or a ``PlaybackEvent/finished`` event, further control calls are
/// no-ops.
public protocol PlaybackController: Sendable {
    /// Pauses the audio player node. In-flight synthesis continues buffering so
    /// resumption is gapless.
    func pause() async
    /// Resumes a paused session.
    func resume() async
    /// Cancels the session immediately, releasing audio resources.
    func stop() async
    /// Async sequence of lifecycle events for this session.
    var events: AsyncStream<PlaybackEvent> { get }
}

public extension PlaybackController {
    /// Suspends until the session emits ``PlaybackEvent/finished`` or the task
    /// is cancelled. Useful in tests and simple callers that don't need to
    /// react to intermediate events.
    func awaitFinished() async {
        for await event in events {
            if case .finished = event { break }
        }
    }
}

// MARK: - Service Protocols (Director–Actor Flow)

/// The mode in which book dialogue and narration are performed.
public enum NarrationMode: String, Codable, CaseIterable, Sendable {
    /// Solo Narrator Mode: A single primary narrator voice is maintained, with minor pitch offsets
    /// and small voice blends applied during dialogue to color character voices.
    case solo = "solo"
    
    /// Full Cast Mode: The narrator reads the prose, but characters are completely replaced by
    /// distinct voices (100% replacement) during dialogue.
    case fullCast = "fullCast"
}

// Stage 1 — Ingestion lives in `Ingestion.swift` (`BookParsing` / `BookDocument`).

/// Stage 2 — Director. Reads incoming passage chunks and emits one or more
/// `[V: … A: … T: …]` prosody blocks per phrase (see ``ProsodyPayload``).
public protocol DirectorInference: Sendable {
    /// Processes a stream of chapter chunks, annotating each with VAD vectors.
    ///
    /// - Parameter chapterStream: The input stream of raw text passages.
    /// - Returns: A stream of annotated payload strings.
    func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String>

    /// Sets the active narration mode for the engine.
    func setNarrationMode(_ mode: NarrationMode) async

    /// Reclaims memory/resources consumed by the director engine.
    func reclaimMemory() async
}

/// A provider that can handle initialization of a specific ``DirectorInference`` engine.
public protocol DirectorProvider: Sendable {
    /// Returns true if this provider is capable of creating a director for the given model.
    ///
    /// - Parameter modelURL: The local URL path of the model.
    /// - Returns: True if supported, false otherwise.
    func canHandle(modelURL: URL) -> Bool

    /// Instantiates the specific director inference engine.
    ///
    /// - Parameters:
    ///   - modelURL: The local URL path of the model.
    ///   - narrationMode: The active narration mode.
    /// - Returns: A model conforming to ``DirectorInference``.
    func makeDirector(for modelURL: URL, narrationMode: NarrationMode) -> any DirectorInference
}

/// A thread-safe registry to store and resolve ``DirectorInference`` providers dynamically.
public final class DirectorRegistry: @unchecked Sendable {
    /// Shared registry instance.
    public static let shared = DirectorRegistry()

    private let lock = NSLock()
    private var providers: [any DirectorProvider] = []

    private init() {}

    /// Registers a provider with the dynamic resolver.
    ///
    /// - Parameter provider: The provider implementation.
    public func register(provider: any DirectorProvider) {
        lock.withLock {
            providers.append(provider)
        }
    }

    /// Resolves and instantiates the correct director implementation for the given model.
    ///
    /// - Parameters:
    ///   - modelURL: The local URL path of the model.
    ///   - narrationMode: The active narration mode.
    /// - Returns: An initialized ``DirectorInference`` engine, or nil if no provider supports the model.
    public func makeDirector(for modelURL: URL, narrationMode: NarrationMode) -> (any DirectorInference)? {
        lock.withLock {
            for provider in providers {
                if provider.canHandle(modelURL: modelURL) {
                    return provider.makeDirector(for: modelURL, narrationMode: narrationMode)
                }
            }
            return nil
        }
    }
}

public extension DirectorInference {
    /// Default empty implementation for engines that do not require mode adjustment.
    func setNarrationMode(_ mode: NarrationMode) async {}

    /// Default empty implementation for reclaiming memory.
    func reclaimMemory() async {}
}

/// Stage 3 — Actor. Consumes the annotated payload stream, applies the acoustic
/// matrix, and renders audio. Returns a ``PlaybackController`` immediately;
/// rendering proceeds in a background task so the caller can interact with the
/// session while it runs.
public protocol VocalActor: Sendable {
    /// Renders an annotated payload stream to audio.
    ///
    /// - Parameter stream: An asynchronous stream of annotated wire-format payloads.
    /// - Returns: A playback controller to manage the playback lifecycle.
    @discardableResult
    func render(stream: AsyncStream<String>) -> any PlaybackController
    
    /// Reclaims memory/resources consumed by the renderer pipeline.
    func reclaimMemory() async
    /// Updates the global speed multiplier dynamically.
    ///
    /// - Parameter speed: The new speed multiplier.
    func updateSpeedMultiplier(_ speed: Double) async
    /// Sets the base voice name to blend on top of dynamic VAD-derived blends.
    ///
    /// - Parameter voice: The voice name, or `nil` to clear.
    func setBaseVoice(_ voice: String?) async
}

public extension VocalActor {
    /// Default empty implementation for re-claiming memory.
    func reclaimMemory() async {}
    /// Default empty implementation for speed multiplier changes.
    func updateSpeedMultiplier(_: Double) async {}
    /// Default empty implementation for base voice selection.
    func setBaseVoice(_: String?) async {}
}

/// A provider that can handle initialization of a specific ``VocalActor`` engine.
public protocol VocalActorProvider: Sendable {
    /// Returns true if this provider is capable of creating an actor for the given model.
    ///
    /// - Parameter modelURL: The local URL path of the model.
    /// - Returns: True if supported, false otherwise.
    func canHandle(modelURL: URL) -> Bool

    /// Instantiates the specific vocal actor engine.
    ///
    /// - Parameters:
    ///   - modelURL: The local URL path of the model.
    ///   - voiceDirectoryURL: The local URL path for voices or vocoders (optional).
    /// - Returns: A model conforming to ``VocalActor``.
    func makeActor(modelURL: URL, voiceDirectoryURL: URL?) -> any VocalActor
}

/// A thread-safe registry to store and resolve ``VocalActor`` providers dynamically.
public final class VocalActorRegistry: @unchecked Sendable {
    /// Shared registry instance.
    public static let shared = VocalActorRegistry()

    private let lock = NSLock()
    private var providers: [any VocalActorProvider] = []

    private init() {}

    /// Registers a provider with the dynamic resolver.
    ///
    /// - Parameter provider: The provider implementation.
    public func register(provider: any VocalActorProvider) {
        lock.withLock {
            providers.append(provider)
        }
    }

    /// Resolves and instantiates the correct vocal actor implementation for the given model.
    ///
    /// - Parameters:
    ///   - modelURL: The local URL path of the model.
    ///   - voiceDirectoryURL: The local URL path for voices or vocoders (optional).
    /// - Returns: An initialized ``VocalActor`` engine, or nil if no provider supports the model.
    public func makeActor(for modelURL: URL, voiceDirectoryURL: URL?) -> (any VocalActor)? {
        lock.withLock {
            for provider in providers {
                if provider.canHandle(modelURL: modelURL) {
                    return provider.makeActor(modelURL: modelURL, voiceDirectoryURL: voiceDirectoryURL)
                }
            }
            return nil
        }
    }
}

// MARK: - ActorVoiceMap

/// Maps a continuous ``EmotionVector`` onto a blend of an Actor engine's *native*
/// voice identifiers.
public protocol ActorVoiceMap: Sendable {
    /// The blend of native voice IDs to use for `emotion`, fractions summing to 1.
    ///
    /// - Parameter emotion: The target emotion vector.
    /// - Returns: An array of ``CastingProfile`` configurations.
    func voiceBlend(for emotion: EmotionVector) -> CastingProfile
    /// Whether `id` names a voice this engine recognizes.
    ///
    /// - Parameter id: The voice identifier string.
    /// - Returns: True if valid, false otherwise.
    func validate(_ id: String) -> Bool
}

// MARK: - Stub Implementations

/// A ``DirectorInference`` that tags every chunk with a fixed directive.
/// Stands in for the Gemma-backed engine so downstream stages can be developed
/// and tested against a stable, well-formed payload stream.
public actor StubDirectorInference: DirectorInference {
    /// The fixed directive to tag every chunk with.
    private let directive: ProsodyDirective
    /// The active narration mode.
    private var narrationMode: NarrationMode = .solo

    /// Initializes a new instance of the stub director.
    ///
    /// - Parameters:
    ///   - directive: The fixed directive to use (defaults to baseline).
    ///   - narrationMode: The initial narration mode (defaults to .solo).
    public init(directive: ProsodyDirective = .init(preset: .baseline), narrationMode: NarrationMode = .solo) {
        self.directive = directive
        self.narrationMode = narrationMode
    }

    /// Sets the active narration mode for the stub engine.
    public func setNarrationMode(_ mode: NarrationMode) async {
        self.narrationMode = mode
    }

    /// Appends the stub directive to every chunk in the stream.
    ///
    /// - Parameter chapterStream: The input stream of raw text passages.
    /// - Returns: An annotated payload stream.
    public func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String> {
        let directive = self.directive
        return AsyncStream { continuation in
            Task {
                for await chunk in chapterStream {
                    continuation.yield(encodeDirective(directive: directive, text: chunk))
                }
                continuation.finish()
            }
        }
    }
}

/// An ``VocalActor`` that records what it would render rather than
/// producing audio. Returns a ``StubPlaybackController`` immediately; segments
/// are collected in a background task and readable via ``snapshot()`` once the
/// controller's ``PlaybackController/awaitFinished()`` resolves.
public actor StubVocalActor: VocalActor {
    /// A structured record of a segment the renderer was requested to play.
    public struct RenderedSegment: Sendable, Equatable {
        /// The prosody directive associated with the segment.
        public let directive: ProsodyDirective
        /// The segment text.
        public let text: String
        /// The playback speed multiplier.
        public let speedMultiplier: Double
        /// Intra-sentence phrasing the Actor would render.
        public let spans: [ProsodySpan]
    }

    /// The phrasing resolver.
    private let phraser: SentencePhraser
    /// The backing list of rendered segments.
    private(set) var renderedSegments: [RenderedSegment] = []

    /// Initializes a stub actor renderer.
    ///
    /// - Parameter phraser: The phrasing resolver to use.
    public init(phraser: SentencePhraser = SentencePhraser()) {
        self.phraser = phraser
    }

    /// Mocks rendering of the payload stream, capturing segments into an internal buffer.
    ///
    /// - Parameter stream: The input stream of annotated wire-format payloads.
    /// - Returns: A stub playback controller.
    public nonisolated func render(stream: AsyncStream<String>) -> any PlaybackController {
        let (eventStream, eventContinuation) = AsyncStream<PlaybackEvent>.makeStream()
        Task { [self] in
            var index = 0
            for await payload in stream {
                guard let decoded = decodeSpans(payload: payload) else { continue }
                let spans = self.phraser.resolveSpans(overall: decoded.overall, decoded: decoded.spans)
                let directive = ProsodyDirective(emotion: decoded.overall, acoustics: decoded.acoustics)
                await self.append(RenderedSegment(
                    directive: directive,
                    text: spans.map(\.text).joined(separator: " "),
                    speedMultiplier: directive.speedMultiplier,
                    spans: spans
                ))
                eventContinuation.yield(.sentenceBegan(index: index))
                eventContinuation.yield(.sentenceScheduled(index: index, timestamps: []))
                index += 1
            }
            eventContinuation.yield(.finished)
            eventContinuation.finish()
        }
        return StubPlaybackController(events: eventStream)
    }

    /// Appends a rendered segment to the internal array.
    private func append(_ segment: RenderedSegment) {
        renderedSegments.append(segment)
    }

    /// Returns a snapshot of all rendered segments recorded so far.
    public func snapshot() -> [RenderedSegment] {
        renderedSegments
    }
}

/// A no-op ``PlaybackController`` suitable for tests and stub renderers.
public struct StubPlaybackController: PlaybackController {
    /// The stream of events.
    public let events: AsyncStream<PlaybackEvent>
    
    /// Initializes a stub playback controller.
    ///
    /// - Parameter events: The event stream.
    public init(events: AsyncStream<PlaybackEvent>) { self.events = events }
    /// No-op pause.
    public func pause() async {}
    /// No-op resume.
    public func resume() async {}
    /// No-op stop.
    public func stop() async {}
}
