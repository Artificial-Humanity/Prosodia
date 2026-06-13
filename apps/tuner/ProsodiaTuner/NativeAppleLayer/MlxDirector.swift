#if canImport(MLXLLM)
import Foundation
import os
import MLXLLM
import MLXLMCommon
@preconcurrency import Tokenizers
import Kit
#if canImport(MLX)
import MLX
#endif

// The system prompt lives locally in `DirectorPrompt.swift` in this package.
// Output validation remains runtime-agnostic in `ProsodiaStage.DirectorOutput`.

// MARK: - MlxDirector

/// A ``DirectorInference`` backed by an MLX LLM model (e.g. Gemma 4 E2B-IT).
///
/// The model loads lazily on the first call to ``annotate(chapterStream:)`` and
/// stays resident for the lifetime of the actor. For each passage the engine
/// receives it runs a structured-output generation request; if the model output
/// doesn't parse or paraphrases the prose, the passage falls back to a single
/// neutral span rather than being dropped.
public actor MlxDirector: DirectorInference {

    /// Logger instance used to output telemetry.
    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "Director")

    /// Local path to the MLX model directory (any `LLMModelFactory`-supported
    /// architecture — Gemma, Qwen, Mistral, Llama, etc.; dense or MoE).
    private let modelDirectory: URL
    /// The loaded MLX LLM model container.
    private var container: ModelContainer?
    /// A history of recently processed passage strings for rolling context.
    private var priorPassages: [String] = []
    /// The active narration mode.
    private var narrationMode: NarrationMode = .solo

    /// Initializes a new instance of the MLX Director engine.
    ///
    /// - Parameters:
    ///   - modelDirectory: The local path to the MLX model directory.
    ///   - narrationMode: The initial narration mode (defaults to .solo).
    public init(modelDirectory: URL, narrationMode: NarrationMode = .solo) {
        self.modelDirectory = modelDirectory
        self.narrationMode = narrationMode
    }

    /// Sets the active narration mode for the engine.
    public func setNarrationMode(_ mode: NarrationMode) {
        self.narrationMode = mode
    }

    /// Reclaims memory by releasing the loaded model container.
    public func reclaimMemory() {
        if container != nil {
            self.container = nil
            #if canImport(MLX)
            MLX.Memory.clearCache()
            #endif
            Self.log.info("Director model memory reclaimed.")
        }
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
                    let c = try await self.loadedContainer()
                    self.resetPriorPassages()
                    for await passage in chapterStream {
                        if Task.isCancelled { break }
                        let payload = await self.tag(passage: passage, using: c)
                        continuation.yield(payload)
                    }
                } catch {
                    // Model failed to load (e.g. an architecture the pinned
                    // mlx-swift-lm doesn't support, or missing files) — narrate
                    // with neutral tags rather than failing, but surface why so a
                    // newly-added model that won't load isn't mistaken for flat AI.
                    Self.log.error("""
                    Director model failed to load from \
                    \(self.modelDirectory.lastPathComponent, privacy: .public): \
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

    /// Returns the cached model container or loads it lazily.
    ///
    /// - Returns: The loaded ``ModelContainer``.
    /// - Throws: An error if the model or tokenizer fails to load.
    private func loadedContainer() async throws -> ModelContainer {
        if let existing = container { return existing }
        let c = try await LLMModelFactory.shared.loadContainer(
            from: modelDirectory,
            using: HuggingFaceTokenizerLoader()
        )
        container = c
        Self.log.info("Director model loaded: \(self.modelDirectory.lastPathComponent, privacy: .public)")
        return c
    }

    // MARK: - Per-sentence annotation

    /// Annotates a single passage with continuous emotional VAD vectors.
    ///
    /// - Parameters:
    ///   - passage: The passage text.
    ///   - container: The model container to run inference on.
    /// - Returns: An annotated payload string.
    private func tag(passage: String, using container: ModelContainer) async -> String {
        // Prepend rolling narrative context if available
        let contextPrefix = priorPassages.isEmpty ? "" : "CONTEXT (Prior story paragraphs for narrative context):\n" + priorPassages.joined(separator: "\n\n") + "\n\n---"
        let fullUserMessage = contextPrefix.isEmpty ? passage : "\(contextPrefix)\n\nCURRENT PASSAGE TO ANNOTATE:\n\(passage)"

        let messages: [[String: any Sendable]] = [
            ["role": "system", "content": directorPrompt(for: narrationMode)],
            ["role": "user", "content": fullUserMessage],
        ]
        let userInput = UserInput(prompt: .messages(messages))

        do {
            let lmInput = try await container.prepare(input: userInput)
            // Budget enough output to reproduce the passage verbatim plus one
            // emotion block per phrase, with headroom — so a longer paragraph (or a
            // more verbose, higher-capability model) isn't truncated mid-phrasing,
            // which would fail validation and drop the chunk to a single-span read.
            // Scales with passage length; capped to bound worst-case latency.
            let tokenBudget = min(1024, max(256, passage.count / 2 + 96))
            var params = GenerateParameters(maxTokens: tokenBudget)
            params.temperature = 0.3
            let stream = try await container.generate(input: lmInput, parameters: params)

            var raw = ""
            for await generation in stream {
                if case .chunk(let text) = generation { raw += text }
            }

            // Append current passage to history, keeping a rolling window of e.g. 3 prior paragraphs
            priorPassages.append(passage)
            if priorPassages.count > 3 {
                priorPassages.removeFirst()
            }

            return DirectorOutput.payload(from: raw, passage: passage)
        } catch {
            return DirectorOutput.neutral(for: passage)
        }
    }
}

// MARK: - Tokenizer bridge

/// Loads a `Tokenizers.AutoTokenizer` from a local directory and bridges it to
/// `MLXLMCommon.Tokenizer`, satisfying the `TokenizerLoader` requirement without
/// needing the HuggingFace Hub or the `MLXHuggingFace` macro system.
struct HuggingFaceTokenizerLoader: MLXLMCommon.TokenizerLoader {
    /// Loads the tokenizer from the specified local directory.
    ///
    /// - Parameter directory: The directory URL where tokenizer config is located.
    /// - Returns: The bridged tokenizer.
    /// - Throws: An error if the tokenizer fails to load.
    func load(from directory: URL) async throws -> any MLXLMCommon.Tokenizer {
        let hfTokenizer = try await AutoTokenizer.from(modelFolder: directory)
        return TokenizerBridge(hfTokenizer)
    }
}

/// Bridges `any Tokenizers.Tokenizer` to `MLXLMCommon.Tokenizer`.
/// Marked `@unchecked Sendable` because `Tokenizers.Tokenizer` is not Sendable
/// but is used exclusively inside the model-loading actor.
private struct TokenizerBridge: MLXLMCommon.Tokenizer, @unchecked Sendable {
    /// The upstream non-sendable tokenizer instance.
    private let upstream: any Tokenizers.Tokenizer

    /// Initializes a new TokenizerBridge.
    ///
    /// - Parameter upstream: The upstream tokenizer to bridge.
    init(_ upstream: any Tokenizers.Tokenizer) {
        self.upstream = upstream
    }

    /// Encodes text into a list of token IDs.
    ///
    /// - Parameters:
    ///   - text: The input text string.
    ///   - addSpecialTokens: Whether to prepend/append special tokens.
    /// - Returns: An array of token IDs.
    func encode(text: String, addSpecialTokens: Bool) -> [Int] {
        upstream.encode(text: text, addSpecialTokens: addSpecialTokens)
    }

    /// Decodes a list of token IDs back into a text string.
    ///
    /// - Parameters:
    ///   - tokenIds: An array of token IDs.
    ///   - skipSpecialTokens: Whether to discard special tokens in the output string.
    /// - Returns: The decoded text string.
    func decode(tokenIds: [Int], skipSpecialTokens: Bool) -> String {
        upstream.decode(tokens: tokenIds, skipSpecialTokens: skipSpecialTokens)
    }

    /// Converts a token string to its token ID.
    ///
    /// - Parameter token: The token string.
    /// - Returns: The token ID, or `nil` if not found.
    func convertTokenToId(_ token: String) -> Int? {
        upstream.convertTokenToId(token)
    }

    /// Converts a token ID to its token string representation.
    ///
    /// - Parameter id: The token ID.
    /// - Returns: The token string, or `nil` if not found.
    func convertIdToToken(_ id: Int) -> String? {
        upstream.convertIdToToken(id)
    }

    /// The string representation of the beginning-of-sequence token, if defined.
    var bosToken: String? {
        guard let id = upstream.bosTokenId else { return nil }
        return upstream.convertIdToToken(id)
    }

    /// The string representation of the end-of-sequence token, if defined.
    var eosToken: String? {
        guard let id = upstream.eosTokenId else { return nil }
        return upstream.convertIdToToken(id)
    }

    /// The string representation of the unknown token, if defined.
    var unknownToken: String? {
        guard let id = upstream.unknownTokenId else { return nil }
        return upstream.convertIdToToken(id)
    }

    /// Formats the chat history into token IDs using the tokenizer's chat template.
    ///
    /// - Parameters:
    ///   - messages: The conversation message log.
    ///   - tools: Optional tools definitions (ignored).
    ///   - additionalContext: Optional template rendering context (ignored).
    /// - Returns: An array of formatted token IDs.
    /// - Throws: An error if chat templating fails.
    func applyChatTemplate(
        messages: [[String: any Sendable]],
        tools _: [[String: any Sendable]]?,
        additionalContext _: [String: any Sendable]?
    ) throws -> [Int] {
        let stringMessages: [[String: String]] = messages.map { dict in
            dict.compactMapValues { $0 as? String }
        }
        return try upstream.applyChatTemplate(messages: stringMessages)
    }
}
#endif // canImport(MLXLLM)
