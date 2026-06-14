import Foundation
import os
@preconcurrency import LiteRTLM
import Stage

/// A ``DirectorInference`` backed by a LiteRT-LM model (e.g. Gemma 4 E2B-IT or E4B-IT).
///
/// The model loads lazily on the first call to ``annotate(chapterStream:)`` and
/// stays resident for the lifetime of the actor. For each passage, the engine
/// initializes a fresh conversation session with the mode-specific system prompt,
/// runs a streaming inference request, and yields tag-annotated wire payload.
public actor LiteRtLmDirector: DirectorInference {

    /// Logger instance used to output telemetry.
    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "LiteRtLmDirector")

    /// Local path to the `.litertlm` model file.
    private let modelPath: URL
    /// The loaded LiteRT-LM engine.
    private var engine: Engine?
    /// A history of recently processed passage strings for rolling context.
    private var priorPassages: [String] = []
    /// The active narration mode.
    private var narrationMode: NarrationMode = .solo

    /// Initializes a new instance of the LiteRT-LM Director.
    ///
    /// - Parameters:
    ///   - modelPath: The local path to the `.litertlm` model file.
    ///   - narrationMode: The initial narration mode (defaults to .solo).
    public init(modelPath: URL, narrationMode: NarrationMode = .solo) {
        self.modelPath = modelPath
        self.narrationMode = narrationMode
    }

    /// Sets the active narration mode for the engine.
    public func setNarrationMode(_ mode: NarrationMode) {
        self.narrationMode = mode
    }

    /// Reclaims memory by releasing the loaded engine and clearing context.
    public func reclaimMemory() {
        self.engine = nil
        self.priorPassages = []
        Self.log.info("LiteRT-LM engine memory reclaimed.")
    }

    // MARK: - DirectorInference

    /// Process a stream of chapters, returning a stream of annotated wire-format payloads.
    ///
    /// - Parameter chapterStream: An asynchronous stream of raw text passages/chapters.
    /// - Returns: An asynchronous stream of annotated payloads.
    public func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String> {
        AsyncStream { continuation in
            let task = Task { [self] in
                do {
                    let activeEngine = try await self.getOrInitializeEngine()
                    self.resetPriorPassages()
                    for await passage in chapterStream {
                        if Task.isCancelled { break }
                        let payload = await self.tag(passage: passage, using: activeEngine)
                        continuation.yield(payload)
                    }
                } catch {
                    Self.log.error("""
                    LiteRT-LM engine failed to load from \
                    \(self.modelPath.lastPathComponent, privacy: .public): \
                    \(error.localizedDescription, privacy: .public). \
                    Falling back to neutral narration.
                    """)
                    for await passage in chapterStream {
                        if Task.isCancelled { break }
                        continuation.yield(DirectorOutput.neutral(for: passage))
                    }
                }
                continuation.finish()
            }
            continuation.onTermination = { @Sendable _ in task.cancel() }
        }
    }

    /// Resets the rolling narrative context history.
    private func resetPriorPassages() {
        priorPassages = []
    }

    // MARK: - Loading

    /// Returns the cached engine or loads it lazily.
    ///
    /// - Returns: The initialized ``Engine``.
    /// - Throws: An error if the engine fails to load or compile Metal kernels.
    private func getOrInitializeEngine() async throws -> Engine {
        if let existing = engine { return existing }
        
        Self.log.info("Loading LiteRT-LM model: \(self.modelPath.path, privacy: .public)")
        
        // We use .gpu backend for Metal execution
        let config = try EngineConfig(
            modelPath: modelPath.path,
            backend: .gpu
        )
        let newEngine = Engine(engineConfig: config)
        try await newEngine.initialize()
        self.engine = newEngine
        return newEngine
    }

    // MARK: - Per-sentence annotation

    /// Annotates a single passage using a fresh conversation context.
    ///
    /// - Parameters:
    ///   - passage: The passage text.
    ///   - engine: The LiteRT-LM engine runner.
    /// - Returns: An annotated payload string.
    private func tag(passage: String, using engine: Engine) async -> String {
        // Prepend rolling narrative context if available
        let contextPrefix = priorPassages.isEmpty ? "" : "CONTEXT (Prior story paragraphs for narrative context):\n" + priorPassages.joined(separator: "\n\n") + "\n\n---"
        let fullUserMessage = contextPrefix.isEmpty ? passage : "\(contextPrefix)\n\nCURRENT PASSAGE TO ANNOTATE:\n\(passage)"

        let systemPrompt = directorPrompt(for: narrationMode)

        do {
            let samplerConfig = try SamplerConfig(
                topK: 40,
                topP: 0.95,
                temperature: 0.3
            )
            let config = ConversationConfig(
                systemMessage: Message(systemPrompt),
                samplerConfig: samplerConfig
            )
            
            // Create a fresh conversation to avoid context window overflow across passages
            let conversation = try await engine.createConversation(with: config)
            let message = Message(fullUserMessage)
            
            var raw = ""
            for try await chunk in conversation.sendMessageStream(message) {
                raw += chunk.toString
            }

            // Append current passage to history, keeping a rolling window of 3 prior paragraphs
            priorPassages.append(passage)
            if priorPassages.count > 3 {
                priorPassages.removeFirst()
            }

            return DirectorOutput.payload(from: raw, passage: passage)
        } catch {
            Self.log.error("LiteRT-LM tag generation failed: \(error.localizedDescription, privacy: .public)")
            return DirectorOutput.neutral(for: passage)
        }
    }
}
