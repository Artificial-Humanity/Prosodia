import Foundation
import os
import Kit
import Stage

/// A ``DirectorInference`` backed by a LiteRT-LM model executed via the Rust core `GemmaDirector`.
public actor LiteRtLmDirector: Stage.DirectorInference {

    /// Logger instance used to output telemetry.
    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "LiteRtLmDirector")

    /// The UniFFI Rust-backed GemmaDirector instance.
    private let rustDirector: GemmaDirector

    /// Initializes a new instance of the LiteRT-LM Director.
    ///
    /// - Parameters:
    ///   - modelPath: The local path to the `.litertlm` model file.
    ///   - narrationMode: The initial narration mode (defaults to .solo).
    public init(modelPath: URL, narrationMode: Stage.NarrationMode = .solo) {
        let rustMode: Kit.NarrationMode
        switch narrationMode {
        case .solo:
            rustMode = .solo
        case .fullCast:
            rustMode = .fullCast
        }
        self.rustDirector = GemmaDirector(
            modelPath: modelPath.path,
            contextTokens: 0, // 0 defaults to engine/model configuration size
            narrationMode: rustMode
        )
    }

    /// Sets the active narration mode for the engine.
    public func setNarrationMode(_ mode: Stage.NarrationMode) async {
        let rustMode: Kit.NarrationMode
        switch mode {
        case .solo:
            rustMode = .solo
        case .fullCast:
            rustMode = .fullCast
        }
        rustDirector.setNarrationMode(mode: rustMode)
    }

    /// Reclaims memory by releasing the loaded engine and clearing context.
    public func reclaimMemory() async {
        rustDirector.reclaimMemory()
        Self.log.info("LiteRT-LM engine memory reclaimed.")
    }

    // MARK: - DirectorInference (Swift)

    /// Process a stream of chapters, returning a stream of annotated wire-format payloads.
    ///
    /// - Parameter chapterStream: An asynchronous stream of raw text passages/chapters.
    /// - Returns: An asynchronous stream of annotated payloads.
    public func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String> {
        AsyncStream { continuation in
            let task = Task { [self] in
                for await passage in chapterStream {
                    if Task.isCancelled { break }
                    let payload = await self.annotateSingle(passage: passage)
                    continuation.yield(payload)
                }
                continuation.finish()
            }
            continuation.onTermination = { @Sendable _ in task.cancel() }
        }
    }

    private func annotateSingle(passage: String) async -> String {
        let raw = await rustDirector.tagPassage(passage: passage)
        return Kit.payloadFromRaw(raw: raw, passage: passage)
    }

    // MARK: - Kit.DirectorInference (Rust FFI Callback Interface)

    public nonisolated func annotate(passage: String) -> String {
        let semaphore = DispatchSemaphore(value: 0)
        var result = ""
        Task {
            result = await self.annotateSingle(passage: passage)
            semaphore.signal()
        }
        semaphore.wait()
        return result
    }
}
