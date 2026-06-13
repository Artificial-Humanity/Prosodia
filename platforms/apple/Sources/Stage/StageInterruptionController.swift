import Foundation
import NaturalLanguage

// MARK: - NarrationPosition

/// Represents a unique, absolute reading position within the book's chapter spine and character offset.
public struct NarrationPosition: Sendable, Codable, Equatable {
    public let spineIndex: Int
    public let characterOffset: Int

    public init(spineIndex: Int, characterOffset: Int) {
        self.spineIndex = spineIndex
        self.characterOffset = max(0, characterOffset)
    }
}

// MARK: - ConversationalContextPacket

/// A spoiler-safe context packet used by the chat interface to understand the story
/// up to the exact point of interruption.
public struct ConversationalContextPacket: Sendable, Codable, Equatable {
    /// The ID of the book being read.
    public let bookID: UUID
    /// The absolute position where the reading was interrupted.
    public let position: NarrationPosition
    /// Optional title of the chapter where the interruption occurred.
    public let chapterTitle: String?
    /// The exact text of the current chapter read *up to* the interruption offset.
    /// This guarantees that the assistant can never see spoilers beyond the current moment!
    public let preInterruptionText: String
    /// An optional summary or baseline synopsis of the book provided by the system.
    public let bookSummary: String?

    public init(
        bookID: UUID,
        position: NarrationPosition,
        chapterTitle: String?,
        preInterruptionText: String,
        bookSummary: String? = nil
    ) {
        self.bookID = bookID
        self.position = position
        self.chapterTitle = chapterTitle
        self.preInterruptionText = preInterruptionText
        self.bookSummary = bookSummary
    }
}

// MARK: - InterruptionSession

/// Represents an active conversational interruption session, freezing narration playback
/// and exposing the narrative context packet.
public struct InterruptionSession: Sendable, Equatable {
    /// The exact position in the book where the user interrupted narration.
    public let interruptedPosition: NarrationPosition
    /// The spoiler-safe context packet of the story up to the interruption.
    public let context: ConversationalContextPacket

    public init(interruptedPosition: NarrationPosition, context: ConversationalContextPacket) {
        self.interruptedPosition = interruptedPosition
        self.context = context
    }
}

// MARK: - StageInterruptionControllerState

/// The operational state of the Interruption Engine.
public enum StageInterruptionControllerState: Sendable, Equatable {
    case idle
    case playing(position: NarrationPosition)
    case paused(position: NarrationPosition)
    case interrupted(session: InterruptionSession)
}

// MARK: - StageInterruptionControllerEvent

/// Rich high-level events emitted by the `StageInterruptionController`.
public enum StageInterruptionControllerEvent: Sendable {
    case sentenceBegan(text: String, position: NarrationPosition)
    case finished
    case error(Error)
}

// MARK: - StageInterruptionController

/// The main coordinator actor that manages reading position, handles conversational interruption,
/// locks state, builds spoiler-safe context packets, and manages resume-from-offset logic.
public actor StageInterruptionController {
    
    /// Metadata mapping the character-level start position of a chunk to its text.
    public struct SentMetadata: Sendable {
        /// The plain-text contents of this chunk.
        public let text: String
        /// The absolute position of the first character in the chunk.
        public let position: NarrationPosition
        
        /// Initializes a new SentMetadata instance.
        /// - Parameters:
        ///   - text: The text contents of the chunk.
        ///   - position: The absolute narration position inside the chapter spine.
        public init(text: String, position: NarrationPosition) {
            self.text = text
            self.position = position
        }
    }
    
    // Dependencies
    /// The UUID of the book currently being read.
    private let bookID: UUID
    /// The BookDocument providing the spine of chapters.
    private let document: any BookDocument
    /// The Director LLM engine supplying emotional annotations.
    private let director: any DirectorInference
    /// The Actor engine managing neural TTS synthesis and scheduling.
    private let actor: any VocalActor
    
    // Settings
    /// Parser that divides paragraph text into sentences.
    private let segmenter: SentenceSegmenter
    /// Policy grouping sentences into paragraphs.
    private let grouping: NarrationGrouping
    /// Number of upcoming paragraphs to pre-synthesize in parallel.
    private let lookahead: Int
    
    // State
    /// Tracks if the coordinator is active, paused, or interrupted.
    private(set) public var state: StageInterruptionControllerState = .idle
    /// The active playback session controller.
    private var activeController: (any PlaybackController)?
    /// Background task mapping event progress updates to character offsets.
    private var trackingTask: Task<Void, Never>?
    /// Synopsis of the book to pass to the conversational context packet.
    private var bookSummary: String?
    
    /// Metadata for dispatched sentences to map event indices to text & positions.
    private var dispatchedSentences: [SentMetadata] = []
    
    /// The index of the sentence chunk currently playing.
    private var currentPlayingIndex = 0
    /// The index of the next chunk to yield to the director.
    private var nextDispatchIndex = 0
    /// Suspended continuations waiting for lookahead window clearance.
    private var lookaheadWaiters: [CheckedContinuation<Void, Never>] = []
    
    // Event Streams continuations
    private var positionContinuation: AsyncStream<NarrationPosition>.Continuation?
    private var eventContinuation: AsyncStream<StageInterruptionControllerEvent>.Continuation?
    
    private lazy var positionStream: AsyncStream<NarrationPosition> = {
        AsyncStream { [weak self] continuation in
            guard let self = self else {
                continuation.finish()
                return
            }
            Task {
                await self.setPositionContinuation(continuation)
            }
        }
    }()
    
    private lazy var eventStream: AsyncStream<StageInterruptionControllerEvent> = {
        AsyncStream { [weak self] continuation in
            guard let self = self else {
                continuation.finish()
                return
            }
            Task {
                await self.setEventContinuation(continuation)
            }
        }
    }()
    
    /// Stream of fine-grained, real-time position updates during narration.
    public var positionUpdates: AsyncStream<NarrationPosition> { positionStream }
    
    /// High-level event stream for observing playback progress (e.g. sentence changes).
    public var events: AsyncStream<StageInterruptionControllerEvent> { eventStream }

    /// Initializes a new StageInterruptionController to manage dynamic book narration and context.
    /// - Parameters:
    ///   - bookID: The UUID of the book.
    ///   - document: The BookDocument providing chapters.
    ///   - director: The Director annotation engine.
    ///   - actor: The Actor TTS speech renderer.
    ///   - segmenter: The segmenter splitting text into sentences.
    ///   - grouping: The grouping policy for context batches.
    ///   - lookahead: The lookahead paragraph synthesis window.
    ///   - bookSummary: A brief overview/synopsis of the book.
    public init(
        bookID: UUID,
        document: any BookDocument,
        director: any DirectorInference,
        actor: any VocalActor,
        segmenter: SentenceSegmenter = SentenceSegmenter(),
        grouping: NarrationGrouping = .paragraph(),
        lookahead: Int = 5,
        bookSummary: String? = nil
    ) {
        self.bookID = bookID
        self.document = document
        self.director = director
        self.actor = actor
        self.segmenter = segmenter
        self.grouping = grouping
        self.lookahead = lookahead
        self.bookSummary = bookSummary
    }
    
    deinit {
        positionContinuation?.finish()
        eventContinuation?.finish()
    }
    
    private func setPositionContinuation(_ continuation: AsyncStream<NarrationPosition>.Continuation) {
        self.positionContinuation = continuation
    }
    
    private func setEventContinuation(_ continuation: AsyncStream<StageInterruptionControllerEvent>.Continuation) {
        self.eventContinuation = continuation
    }
    
    /// Starts dynamic narration of the book from a specific position.
    ///
    /// If narration is already active, this call stops the current session first.
    public func startNarration(from position: NarrationPosition) async throws {
        await stopActiveSession()
        
        state = .playing(position: position)
        dispatchedSentences = []
        
        // Prepare sliced chapter stream for the director
        let chapterStream = AsyncStream<String> { [weak self] continuation in
            guard let self = self else {
                continuation.finish()
                return
            }
            
            Task {
                let doc = self.document
                let seg = self.segmenter
                let grp = self.grouping
                let pos = position
                
                for index in pos.spineIndex..<doc.chapterCount {
                    guard let chapter = try? await doc.chapter(at: index) else { break }
                    var textToSegment = chapter.text
                    var baseOffset = 0
                    
                    // If it is the first chapter of the narration, slice from characterOffset
                    if index == pos.spineIndex && pos.characterOffset > 0 {
                        let startIndex = chapter.text.index(
                            chapter.text.startIndex,
                            offsetBy: min(pos.characterOffset, chapter.text.count)
                        )
                        textToSegment = String(chapter.text[startIndex...])
                        baseOffset = pos.characterOffset
                    }
                    
                    let sentences = seg.sentences(in: textToSegment)
                    
                    // Pre-calculate sentence metadata and ranges
                    var runningOffset = baseOffset
                    var sentenceMetas: [SentMetadata] = []
                    for sentence in sentences {
                        sentenceMetas.append(SentMetadata(
                            text: sentence,
                            position: NarrationPosition(spineIndex: index, characterOffset: runningOffset)
                        ))
                        runningOffset += sentence.count + 1 // Estimating single whitespace spacing join
                    }
                    
                    // Group the sentenceMetas into chunkMetas according to the grouping policy
                    var chunkMetas: [SentMetadata] = []
                    switch grp {
                    case .sentence:
                        chunkMetas = sentenceMetas
                    case .paragraph(let target):
                        var currentText = ""
                        var currentPos: NarrationPosition? = nil
                        for meta in sentenceMetas {
                            if currentText.isEmpty {
                                currentText = meta.text
                                currentPos = meta.position
                            } else {
                                currentText += " " + meta.text
                            }
                            if currentText.count >= target {
                                chunkMetas.append(SentMetadata(text: currentText, position: currentPos!))
                                currentText = ""
                                currentPos = nil
                            }
                        }
                        if !currentText.isEmpty {
                            chunkMetas.append(SentMetadata(text: currentText, position: currentPos!))
                        }
                    }
                    
                    await self.appendDispatched(chunkMetas)
                    
                    for chunk in grp.group(sentences) {
                        await self.waitForLookaheadWindow()
                        continuation.yield(chunk)
                        await self.incrementDispatchIndex()
                    }
                }
                continuation.finish()
            }
        }
        
        // Drive Director annotation and Actor rendering
        let annotated = await director.annotate(chapterStream: chapterStream)
        let controller = actor.render(stream: annotated)
        self.activeController = controller
        
        // Listen to the playback events and update the active position dynamically
        let trackingTask = Task { [weak self] in
            for await event in controller.events {
                guard let self = self else { break }
                
                switch event {
                case .sentenceBegan(let idx):
                    await self.updatePlayingIndex(idx)
                    if let meta = await self.lookupDispatched(index: idx) {
                        await self.updatePositionAndNotify(meta: meta)
                    }
                case .playbackProgress(let idx, let relativeOffset):
                    if let meta = await self.lookupDispatched(index: idx) {
                        let absoluteOffset = meta.position.characterOffset + relativeOffset
                        await self.reportPlaybackProgress(characterOffset: absoluteOffset)
                    }
                case .finished:
                    await self.handleNarrationFinished()
                case .segmentFailed(_, let error):
                    await self.handleNarrationError(error)
                case .sentenceScheduled(_, _):
                    break
                }
            }
        }
        self.trackingTask = trackingTask
    }
    
    /// Updates the current tracking position. Exposed to the audio coordinator/actor
    /// to report real-time character-level progress during playback.
    public func reportPlaybackProgress(characterOffset: Int) {
        switch state {
        case .playing(let current):
            let updated = NarrationPosition(spineIndex: current.spineIndex, characterOffset: characterOffset)
            state = .playing(position: updated)
            positionContinuation?.yield(updated)
        case .paused(let current):
            let updated = NarrationPosition(spineIndex: current.spineIndex, characterOffset: characterOffset)
            state = .paused(position: updated)
            positionContinuation?.yield(updated)
        default:
            break
        }
    }
    
    /// Freezes the current narration, cancels lookahead buffers immediately, locks the exact
    /// `characterOffset`, and returns a conversational interruption session.
    @discardableResult
    public func interrupt() async throws -> InterruptionSession {
        guard case .playing(let position) = state else {
            throw InterruptionError.notPlaying
        }
        
        // Stop audio playback immediately
        if let controller = activeController {
            await controller.stop()
        }
        trackingTask?.cancel()
        
        // Fetch current chapter information for the context packet
        let chapter = try await document.chapter(at: position.spineIndex)
        
        // Spoiler-safe text slicing: include text only up to the interrupted character offset
        let safeTextEnd = min(position.characterOffset, chapter.text.count)
        let sliceIndex = chapter.text.index(chapter.text.startIndex, offsetBy: safeTextEnd)
        let preInterruptionText = String(chapter.text[..<sliceIndex])
        
        let contextPacket = ConversationalContextPacket(
            bookID: bookID,
            position: position,
            chapterTitle: chapter.title,
            preInterruptionText: preInterruptionText,
            bookSummary: bookSummary
        )
        
        let session = InterruptionSession(
            interruptedPosition: position,
            context: contextPacket
        )
        
        state = .interrupted(session: session)
        return session
    }
    
    /// Resumes normal book narration from the exact position where it was interrupted.
    public func resumeNarration() async throws {
        guard case .interrupted(let session) = state else {
            throw InterruptionError.noActiveInterruption
        }
        
        try await startNarration(from: session.interruptedPosition)
    }
    
    /// Seeks to a completely new position in the book, cancelling any active interruption or narration.
    public func seek(to position: NarrationPosition) async throws {
        await stopActiveSession()
        state = .paused(position: position)
    }
    
    /// Pauses current playback.
    public func pause() async throws {
        if case .playing(let position) = state {
            if let controller = activeController {
                await controller.pause()
            }
            state = .paused(position: position)
        }
    }
    
    /// Resumes from a paused state without restarting the pipeline.
    public func resume() async throws {
        if case .paused(let position) = state {
            if let controller = activeController {
                await controller.resume()
            }
            state = .playing(position: position)
        }
    }
    
    // MARK: - Private Helpers
    
    /// Appends newly pre-calculated metadata chunks for lookahead mapping.
    /// - Parameter metas: The array of SentMetadata to append.
    private func appendDispatched(_ metas: [SentMetadata]) {
        dispatchedSentences.append(contentsOf: metas)
    }
    
    /// Looks up metadata corresponding to the given chunk/paragraph index.
    /// - Parameter index: The index of the chunk.
    /// - Returns: The SentMetadata if found, nil otherwise.
    private func lookupDispatched(index: Int) -> SentMetadata? {
        guard index >= 0, index < dispatchedSentences.count else { return nil }
        return dispatchedSentences[index]
    }
    
    /// Updates the narration position and fires began events to the listener.
    /// - Parameter meta: The SentMetadata for the chunk.
    private func updatePositionAndNotify(meta: SentMetadata) {
        state = .playing(position: meta.position)
        positionContinuation?.yield(meta.position)
        eventContinuation?.yield(.sentenceBegan(text: meta.text, position: meta.position))
    }
    
    /// Updates the playback speed multiplier dynamically on the active actor.
    /// - Parameter speed: The target speed multiplier.
    public func updateSpeedMultiplier(_ speed: Double) async {
        await actor.updateSpeedMultiplier(speed)
    }

    /// Stops the active speech controller and cancels any background tracking loops.
    private func stopActiveSession() async {
        if let controller = activeController {
            await controller.stop()
            activeController = nil
        }
        trackingTask?.cancel()
        trackingTask = nil
        
        clearLookaheadWaiters()
        currentPlayingIndex = 0
        nextDispatchIndex = 0
    }
    
    /// Transition engine to idle state and notifies listener of completion.
    private func handleNarrationFinished() {
        state = .idle
        eventContinuation?.yield(.finished)
    }
    
    /// Yields errors to the event subscriber and resets engine state to idle.
    /// - Parameter error: The error that occurred during synthesis/playback.
    private func handleNarrationError(_ error: Error) {
        eventContinuation?.yield(.error(error))
        state = .idle
    }
    
    // MARK: - Lookahead Backpressure Management
    
    /// Suspends the calling task if the number of pre-synthesized chunks exceeds the lookahead limit.
    private func waitForLookaheadWindow() async {
        while nextDispatchIndex - currentPlayingIndex >= lookahead {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                lookaheadWaiters.append(continuation)
            }
        }
    }
    
    /// Increments the count of chunks currently yielded to the director/actor pipeline.
    private func incrementDispatchIndex() {
        nextDispatchIndex += 1
    }
    
    /// Updates the index of the currently playing sentence chunk and resumes lookahead waiters if needed.
    private func updatePlayingIndex(_ index: Int) {
        currentPlayingIndex = index
        resumeWaitersIfNeeded()
    }
    
    /// Resumes as many suspended lookahead waiters as the current window permits.
    private func resumeWaitersIfNeeded() {
        while nextDispatchIndex - currentPlayingIndex < lookahead, !lookaheadWaiters.isEmpty {
            let waiter = lookaheadWaiters.removeFirst()
            waiter.resume()
        }
    }
    
    /// Resumes all suspended lookahead waiters to clean up resources during a stop or interruption.
    private func clearLookaheadWaiters() {
        for waiter in lookaheadWaiters {
            waiter.resume()
        }
        lookaheadWaiters.removeAll()
    }
}

// MARK: - InterruptionError

public enum InterruptionError: Error, Sendable, LocalizedError {
    case notPlaying
    case noActiveInterruption
    
    public var errorDescription: String? {
        switch self {
        case .notPlaying:
            return "Cannot interrupt narration because playback is not currently active."
        case .noActiveInterruption:
            return "No active conversational interruption to resume from."
        }
    }
}
