import Foundation
@preconcurrency import Kit
import Audio

// MARK: - NarrationSourceAdapter

class NarrationSourceAdapter: Kit.NarrationSource {
    private var iterator: AsyncStream<String>.Iterator

    init(document: any BookDocument, segmenter: SentenceSegmenter, grouping: NarrationGrouping) {
        let stream = document.narrationStream(segmenter: segmenter, grouping: grouping)
        self.iterator = stream.makeAsyncIterator()
    }

    func nextPassage() -> String? {
        let semaphore = DispatchSemaphore(value: 0)
        var result: String? = nil
        Task {
            result = await self.iterator.next()
            semaphore.signal()
        }
        semaphore.wait()
        return result
    }
}

// MARK: - StageCoordinator

/// Connects a ``BookDocument`` → ``DirectorInference`` → ``VocalActor``
/// via the high-performance Rust ``StageCoordinator`` (using a configurable
/// lookahead limit for background rendering), returning a ``PlaybackController``
/// to manage the active session.
public enum StageCoordinator {

    /// Starts the full narration pipeline and returns a controller for the session.
    public static func run(
        document: any BookDocument,
        segmenter: SentenceSegmenter = SentenceSegmenter(),
        grouping: NarrationGrouping = .paragraph(),
        director: any DirectorInference,
        actor: any VocalActor,
        lookahead: Int = 5 // Bounded lookahead limit (0 for synchronous/thread-free)
    ) async -> any PlaybackController {
        
        let sourceAdapter = NarrationSourceAdapter(
            document: document,
            segmenter: segmenter,
            grouping: grouping
        )
        
        let kitGrouping: Kit.NarrationGrouping
        switch grouping {
        case .sentence:
            kitGrouping = .sentence
        case .paragraph(let target):
            kitGrouping = .paragraph(targetCharacters: UInt32(target))
        }
        
        // Swift-side DirectorInference and VocalActor now inherit from FFI protocols directly
        let rustCoordinator = Kit.StageCoordinator.newWithLookahead(
            source: sourceAdapter,
            director: director,
            actor: actor,
            grouping: kitGrouping,
            sampleRate: Kit.getSampleRate(),
            lookaheadLimit: UInt32(lookahead)
        )
        
        let audioSink = StageAudioSink(sampleRate: Double(Kit.getSampleRate()))
        
        let controller = RustCoordinatedPlaybackController(
            coordinator: rustCoordinator,
            audioSink: audioSink
        )
        
        controller.start()
        
        return controller
    }
}

// MARK: - RustCoordinatedPlaybackController

class RustCoordinatedPlaybackController: PlaybackController {
    private let coordinator: Kit.StageCoordinator
    private let audioSink: StageAudioSink
    private let (eventStream, eventContinuation) = AsyncStream<PlaybackEvent>.makeStream()
    private var driveTask: Task<Void, Never>?
    private var paused = false

    init(coordinator: Kit.StageCoordinator, audioSink: StageAudioSink) {
        self.coordinator = coordinator
        self.audioSink = audioSink
    }

    var events: AsyncStream<PlaybackEvent> { eventStream }

    func start() {
        driveTask = Task {
            while !Task.isCancelled {
                if paused {
                    try? await Task.sleep(nanoseconds: 50_000_000) // 50ms
                    continue
                }
                
                // Fetch next chunk on background queue to keep cooperative thread pool free
                guard let chunk = await getNextChunk() else {
                    eventContinuation.yield(.finished)
                    break
                }
                
                eventContinuation.yield(.sentenceBegan(index: Int(chunk.index)))
                
                do {
                    try await audioSink.schedule(samples: chunk.audio)
                } catch {
                    eventContinuation.yield(.segmentFailed(index: Int(chunk.index), error))
                }
                
                eventContinuation.yield(.sentenceScheduled(index: Int(chunk.index), timestamps: []))
            }
            eventContinuation.finish()
        }
    }

    private func getNextChunk() async -> Kit.AudioChunk? {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let chunk = self.coordinator.nextChunk()
                continuation.resume(returning: chunk)
            }
        }
    }

    func pause() async {
        paused = true
        await audioSink.pause()
    }

    func resume() async {
        paused = false
        await audioSink.resume()
    }

    func stop() async {
        driveTask?.cancel()
        coordinator.stop()
        await audioSink.stop()
    }
}
