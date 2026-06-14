import Foundation
import Kit

// MARK: - BoundedChannel

/// A single-producer / single-consumer async channel with a fixed capacity.
///
/// When the buffer is full, ``send(_:)`` suspends the producer until the consumer
/// calls ``next()`` and frees a slot. This is the backpressure primitive that
/// prevents the Director from racing ahead and annotating the entire book while
/// the Actor is playing sentence 1.
///
/// Not general-purpose: exactly one concurrent ``send(_:)`` and one concurrent
/// ``next()`` at a time. Multiple concurrent producers or consumers will corrupt
/// the internal state.
actor BoundedChannel<T: Sendable> {
    private var buffer: [T] = []
    private let capacity: Int
    private var producerWaiter: CheckedContinuation<Void, Never>?
    private var consumerWaiter: CheckedContinuation<T?, Never>?
    private var done = false

    init(capacity: Int) {
        precondition(capacity > 0, "BoundedChannel capacity must be > 0")
        self.capacity = capacity
    }

    /// Sends a value. Suspends if the buffer is at capacity until the consumer
    /// takes an item. No-op if the channel has been finished.
    func send(_ value: T) async {
        guard !done else { return }
        if let waiter = consumerWaiter {
            // Consumer is waiting for a value — deliver directly.
            consumerWaiter = nil
            waiter.resume(returning: value)
        } else if buffer.count < capacity {
            buffer.append(value)
        } else {
            // Buffer full — suspend until consumer frees a slot.
            await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
                producerWaiter = c
            }
            buffer.append(value)
        }
    }

    /// Returns the next value, suspending if the buffer is empty. Returns `nil`
    /// when the channel is finished and the buffer has been fully drained.
    func next() async -> T? {
        if !buffer.isEmpty {
            let value = buffer.removeFirst()
            // Wake a waiting producer if there is one.
            if let waiter = producerWaiter {
                producerWaiter = nil
                waiter.resume()
            }
            return value
        }
        if done { return nil }
        // Buffer empty and not done — wait for the producer.
        return await withCheckedContinuation { (c: CheckedContinuation<T?, Never>) in
            consumerWaiter = c
        }
    }

    /// Signals that no more values will be sent. Wakes any waiting consumer or
    /// producer so they can observe the terminal condition.
    func finish() {
        done = true
        producerWaiter?.resume()
        producerWaiter = nil
        consumerWaiter?.resume(returning: nil)
        consumerWaiter = nil
    }

    /// Cancels the channel immediately, emptying the buffer and waking waiters.
    func cancel() {
        done = true
        buffer.removeAll()
        producerWaiter?.resume()
        producerWaiter = nil
        consumerWaiter?.resume(returning: nil)
        consumerWaiter = nil
    }
}

// MARK: - StageCoordinator

/// Connects a ``BookDocument`` → ``DirectorInference`` → ``VocalActor``
/// with a bounded lookahead buffer between the Director and the Actor, then returns
/// a ``PlaybackController`` for the running session.
///
/// The bounded buffer (default: 5 chunks) means:
/// - The Director runs ahead of playback by up to `lookahead` paragraph-sized chunks,
///   keeping the Actor's input queue full so there are no gaps between passages.
/// - The Director does not annotate the entire book upfront — it blocks once the
///   buffer is full and resumes as the Actor consumes.
///
/// ```swift
/// let controller = await StageCoordinator.run(
///     document: document,
///     director: LiteRtLmDirector(modelPath: modelURL),
///     actor:    LiteRtActorEngine(modelPath: modelURL, configURL: configURL)
/// )
/// await controller.awaitFinished()
/// ```
public enum StageCoordinator {

    /// Starts the full narration pipeline and returns a controller for the session.
    ///
    /// - Parameters:
    ///   - document: the parsed book.
    ///   - segmenter: sentence tokenizer (default: English NLTokenizer).
    ///   - grouping: how sentences are batched into chunks (default: paragraph ~400 chars).
    ///   - director: annotates each chunk with VAD prosody spans.
    ///   - actor: synthesises and renders the annotated stream.
    ///   - lookahead: how many annotated chunks the Director may buffer ahead
    ///     of the Actor. Minimum 1, recommended 3–5.
    public static func run(
        document: any BookDocument,
        segmenter: SentenceSegmenter = SentenceSegmenter(),
        grouping: NarrationGrouping = .paragraph(),
        director: any DirectorInference,
        actor: any VocalActor,
        lookahead: Int = 5
    ) async -> any PlaybackController {
        let channel = BoundedChannel<String>(capacity: max(1, lookahead))

        // Director task: annotates paragraph-grouped chunks and fills the bounded channel.
        let directorTask = Task {
            let chunks = document.narrationStream(segmenter: segmenter, grouping: grouping)
            let annotated = await director.annotate(chapterStream: chunks)
            for await payload in annotated {
                guard !Task.isCancelled else { break }
                await channel.send(payload)
            }
            await channel.finish()
        }

        // Bridge the channel into an AsyncStream for the Actor.
        let stream = AsyncStream<String> { continuation in
            let bridgeTask = Task {
                while !Task.isCancelled, let payload = await channel.next() {
                    continuation.yield(payload)
                }
                continuation.finish()
            }
            continuation.onTermination = { @Sendable _ in bridgeTask.cancel() }
        }

        // Start rendering; wrap the inner controller so stop() also cancels the
        // Director task and drains the channel.
        let inner = actor.render(stream: stream)
        return CoordinatedPlaybackController(
            inner: inner,
            directorTask: directorTask,
            channel: channel
        )
    }
}

// MARK: - CoordinatedPlaybackController

/// A ``PlaybackController`` that wraps the Actor's controller and additionally
/// cancels the Director task and drains the channel on ``stop()``.
struct CoordinatedPlaybackController: PlaybackController {
    private let inner: any PlaybackController
    private let directorTask: Task<Void, Never>
    private let channel: BoundedChannel<String>

    init(
        inner: any PlaybackController,
        directorTask: Task<Void, Never>,
        channel: BoundedChannel<String>
    ) {
        self.inner = inner
        self.directorTask = directorTask
        self.channel = channel
    }

    var events: AsyncStream<PlaybackEvent> { inner.events }

    func pause() async  { await inner.pause() }
    func resume() async { await inner.resume() }

    func stop() async {
        directorTask.cancel()
        await channel.cancel()  // empties buffer and unblocks suspended send() or next()
        await inner.stop()
    }
}
