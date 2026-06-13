#if canImport(AVFoundation)
import Foundation
import AVFoundation

// MARK: - StageAudioSink

/// Shared `AVAudioEngine` output stage for Actor renderers.
///
/// Owns the engine and a single player node, started lazily on the first
/// scheduled buffer, and queues mono `Float` PCM buffers sequentially (no
/// `.interrupts`) so segments play gapless. Any ``VocalActor`` — an MLX Actor,
/// a GGUF Actor engine, … — schedules its synthesized samples through this sink, so
/// the transport and scheduling plumbing lives in one place rather than being
/// duplicated per engine.
public actor StageAudioSink {
    /// The underlying AVAudioEngine managing the audio graph.
    private let engine = AVAudioEngine()
    
    /// The player node used to schedule and play synthesized PCM buffers.
    private let player = AVAudioPlayerNode()
    
    /// The audio format expected by the player node (e.g. mono, 24kHz).
    private let format: AVAudioFormat
    
    /// Indicates whether the player node has been attached and connected to the engine.
    private var connected = false
    
    /// Tracks if the playback sink is currently paused.
    private var paused = false
    
    /// Total number of audio samples completed/rendered by the sink.
    private var totalSamplesCompleted: Int64 = 0
    
    /// The number of buffers currently scheduled and active in the player node queue.
    private var queuedBuffersCount = 0
    
    /// Continuations of tasks suspended due to bounded queue backpressure.
    private var waiters: [CheckedContinuation<Void, Never>] = []

    #if os(iOS) || targetEnvironment(macCatalyst) || os(tvOS) || os(watchOS)
    /// Background task monitoring system audio session interruptions (e.g., calls).
    private var interruptionTask: Task<Void, Never>?
    #endif

    /// Error states encountered during audio buffer scheduling.
    public enum SinkError: Error, Sendable {
        /// Thrown when the initialization of AVAudioPCMBuffer fails.
        case bufferAllocationFailed
    }

    /// Initializes a new StageAudioSink with the specified output sample rate.
    /// - Parameter sampleRate: The output sample rate in Hz (e.g., StyleTTS2's 24,000).
    public init(sampleRate: Double) {
        self.format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1)!
        #if os(iOS) || targetEnvironment(macCatalyst) || os(tvOS) || os(watchOS)
        Task {
            await setupAudioSession()
        }
        #endif
    }

    deinit {
        #if os(iOS) || targetEnvironment(macCatalyst) || os(tvOS) || os(watchOS)
        interruptionTask?.cancel()
        #endif
    }

    /// Schedules `seconds` of silence by queuing a zero-filled buffer, so a
    /// leading pause is gapless with the surrounding audio.
    public func scheduleSilence(seconds: Double) async throws {
        let frames = Int((seconds * format.sampleRate).rounded())
        guard frames > 0 else { return }
        try await schedule(samples: [Float](repeating: 0, count: frames))
    }

    /// Schedules one mono `Float` PCM buffer, starting the engine on first use.
    public func schedule(samples: [Float]) async throws {
        guard !samples.isEmpty else { return }
        guard let buffer = AVAudioPCMBuffer(
            pcmFormat: format, frameCapacity: AVAudioFrameCount(samples.count)
        ) else {
            throw SinkError.bufferAllocationFailed
        }
        buffer.frameLength = buffer.frameCapacity
        samples.withUnsafeBufferPointer { src in
            guard let base = src.baseAddress, let dst = buffer.floatChannelData?[0] else { return }
            dst.update(from: base, count: src.count)
        }

        if !connected {
            engine.attach(player)
            engine.connect(player, to: engine.mainMixerNode, format: format)
            try engine.start()
            player.play()
            connected = true
        }

        // Bounded queueing: if we already have 2 or more buffers queued on AVAudioPlayerNode,
        // suspend the caller until one finishes. This maintains gapless pipeline playback
        // (the player always has at least 1 next buffer queued) while keeping backpressure
        // and avoiding infinite queue growth.
        if queuedBuffersCount >= 2 {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                waiters.append(continuation)
            }
        }

        // Check if the sink was stopped or task was cancelled while suspended
        if !connected || Task.isCancelled {
            return
        }

        let frameCount = buffer.frameLength
        queuedBuffersCount += 1

        player.scheduleBuffer(buffer, completionHandler: {
            Task { [weak self] in
                guard let self else { return }
                await self.bufferFinished(frameCount: Int64(frameCount))
            }
        })
    }

    /// Callback triggered when a scheduled buffer finishes rendering.
    /// - Parameter frameCount: The length of the completed buffer in frames.
    private func bufferFinished(frameCount: Int64) {
        self.totalSamplesCompleted += frameCount
        self.queuedBuffersCount = max(0, self.queuedBuffersCount - 1)
        if self.queuedBuffersCount < 2 && !waiters.isEmpty {
            let waiter = waiters.removeFirst()
            waiter.resume()
        }
    }

    /// Retrieves the total number of audio samples completed during this playback session.
    /// - Returns: The total count of completed samples as an Int64.
    public func getTotalSamplesCompleted() -> Int64 {
        return totalSamplesCompleted
    }

    /// Calculates the elapsed seconds since the playback session started, relative to the provided base sample offset.
    /// - Parameter initialCompleted: The base sample offset to subtract.
    /// - Returns: The elapsed playback duration in seconds.
    public func getElapsedSeconds(relativeTo initialCompleted: Int64) -> Double {
        guard connected,
              let lastRenderTime = player.lastRenderTime,
              let playerTime = player.playerTime(forNodeTime: lastRenderTime) else {
            return 0.0
        }
        let elapsedSamples = max(0, playerTime.sampleTime - initialCompleted)
        return Double(elapsedSamples) / format.sampleRate
    }

    /// Pauses the player node. In-flight buffers remain queued so resumption is
    /// gapless.
    public func pause() {
        guard connected, !paused else { return }
        player.pause()
        paused = true
    }

    /// Resumes a paused sink.
    public func resume() {
        guard connected, paused else { return }
        player.play()
        paused = false
    }

    /// Stops playback and releases the engine.
    public func stop() {
        player.stop()
        engine.stop()
        connected = false
        paused = false
        totalSamplesCompleted = 0
        queuedBuffersCount = 0
        for waiter in waiters {
            waiter.resume()
        }
        waiters.removeAll()
    }

    #if os(iOS) || targetEnvironment(macCatalyst) || os(tvOS) || os(watchOS)
    /// Sets up the target AVAudioSession for background playback.
    private func setupAudioSession() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default, options: [.allowAirPlay])
            try session.setActive(true)
        } catch {
            // Ignore in simulators/unit tests
        }
        
        interruptionTask = Task { [weak self] in
            let notifications = NotificationCenter.default.notifications(named: AVAudioSession.interruptionNotification)
            for await notification in notifications {
                guard let self = self else { break }
                guard let userInfo = notification.userInfo,
                      let typeValue = userInfo[AVAudioSessionInterruptionTypeKey] as? UInt,
                      let type = AVAudioSession.InterruptionType(rawValue: typeValue) else {
                    continue
                }
                let shouldResume: Bool
                if let optionsValue = userInfo[AVAudioSessionInterruptionOptionKey] as? UInt {
                    shouldResume = AVAudioSession.InterruptionOptions(rawValue: optionsValue).contains(.shouldResume)
                } else {
                    shouldResume = false
                }
                await self.handleInterruption(type: type, shouldResume: shouldResume)
            }
        }
    }

    /// Handles interruptions sent by the iOS audio session manager (e.g. phone calls).
    /// - Parameters:
    ///   - type: The interruption type (began or ended).
    ///   - shouldResume: Whether playback should automatically resume when the interruption ends.
    private func handleInterruption(type: AVAudioSession.InterruptionType, shouldResume: Bool) {
        switch type {
        case .began:
            self.pause()
        case .ended:
            if shouldResume {
                self.resume()
            }
        @unknown default:
            break
        }
    }
    #endif
}
#endif // canImport(AVFoundation)
