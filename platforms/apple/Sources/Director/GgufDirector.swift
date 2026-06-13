#if canImport(CLlama)
import Foundation
import os
import CLlama
import ProsodiaStage

// MARK: - Model handle box

/// Owns the opaque llama.cpp model+context handle and frees it on deinit.
final class LlamaModelBox: @unchecked Sendable {
    let handle: OpaquePointer

    init(handle: OpaquePointer) {
        self.handle = handle
    }

    deinit {
        cllama_model_free(handle)
    }
}

// MARK: - GgufDirector

/// A ``DirectorInference`` backed by a **GGUF** model via the vendored
/// llama.cpp runtime (Metal). The GGUF analogue of ``MlxDirector``: it
/// uses the same system prompt and output validation
/// (``directorSystemPrompt`` from this package and ``DirectorOutput`` from the core package), so the two backends are
/// interchangeable behind the protocol and produce the same wire format.
public actor GgufDirector: DirectorInference {

    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "Director")

    /// Local path to a `.gguf` model file.
    private let modelPath: URL
    /// Layers to offload to Metal (999 = offload everything).
    private let gpuLayers: Int32
    /// Context window in tokens (0 = model default).
    private let contextTokens: Int32
    private var priorPassages: [String] = []

    /// Loaded model+context, boxed so its C-resource cleanup stays off the actor.
    private var model: LlamaModelBox?
    /// The active narration mode.
    private var narrationMode: NarrationMode = .solo

    public init(modelPath: URL, gpuLayers: Int32 = 999, contextTokens: Int32 = 8192, narrationMode: NarrationMode = .solo) {
        self.modelPath = modelPath
        self.gpuLayers = gpuLayers
        self.contextTokens = contextTokens
        self.narrationMode = narrationMode
    }

    /// Sets the active narration mode for the engine.
    public func setNarrationMode(_ mode: NarrationMode) {
        self.narrationMode = mode
    }

    /// Reclaims memory by releasing the loaded GGUF model box.
    public func reclaimMemory() {
        self.model = nil
        Self.log.info("GGUF Director model memory reclaimed.")
    }

    // MARK: - DirectorInference

    public func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String> {
        AsyncStream { continuation in
            let task = Task { [self] in
                let loaded = self.loadedModel()
                guard let loaded = loaded else {
                    Self.log.error("""
                    GGUF Director model failed to load from \
                    \(self.modelPath.lastPathComponent, privacy: .public). \
                    Falling back to neutral narration.
                    """)
                    for await passage in chapterStream {
                        if Task.isCancelled { break }
                        continuation.yield(DirectorOutput.neutral(for: passage))
                    }
                    continuation.finish()
                    return
                }

                self.resetPriorPassages()
                for await passage in chapterStream {
                    if Task.isCancelled { break }
                    continuation.yield(self.tag(passage: passage, handle: loaded.handle))
                }
                continuation.finish()
            }
            continuation.onTermination = { @Sendable _ in task.cancel() }
        }
    }

    private func resetPriorPassages() {
        priorPassages = []
    }

    // MARK: - Loading

    private func loadedModel() -> LlamaModelBox? {
        if let model = model { return model }
        guard let loaded = cllama_model_load(modelPath.path, gpuLayers, contextTokens) else {
            return nil
        }
        let box = LlamaModelBox(handle: loaded)
        model = box
        Self.log.info("GGUF Director model loaded: \(self.modelPath.lastPathComponent, privacy: .public)")
        return box
    }

    // MARK: - Per-passage annotation

    private func tag(passage: String, handle: OpaquePointer) -> String {
        // Prepend rolling narrative context if available
        let contextPrefix = priorPassages.isEmpty ? "" : "CONTEXT (Prior story paragraphs for narrative context):\n" + priorPassages.joined(separator: "\n\n") + "\n\n---"
        let fullUserMessage = contextPrefix.isEmpty ? passage : "\(contextPrefix)\n\nCURRENT PASSAGE TO ANNOTATE:\n\(passage)"

        let tokenBudget = Int32(min(1024, max(256, passage.count / 2 + 96)))

        let prompt = directorPrompt(for: narrationMode)
        guard let raw = cllama_generate(handle, prompt, fullUserMessage, tokenBudget, 0.3) else {
            return DirectorOutput.neutral(for: passage)
        }
        defer { cllama_string_free(raw) }

        priorPassages.append(passage)
        if priorPassages.count > 3 {
            priorPassages.removeFirst()
        }

        return DirectorOutput.payload(from: String(cString: raw), passage: passage)
    }
}
#endif // canImport(CLlama)
