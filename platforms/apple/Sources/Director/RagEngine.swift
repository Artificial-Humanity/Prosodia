import Foundation
import MLX
import MLXEmbedders
import Tokenizers
import ProsodiaStage

/// A single chunk in the embedding index, representing a part of a chapter's text.
public struct EmbeddedChunk: Sendable, Codable, Equatable {
    /// Spine index of the chapter.
    public let spineIndex: Int
    /// Character offset of the chunk start.
    public let characterOffset: Int
    /// Raw clean plain text of the chunk.
    public let text: String
    /// The generated embedding vector.
    public let vector: [Float]

    public init(spineIndex: Int, characterOffset: Int, text: String, vector: [Float]) {
        self.spineIndex = spineIndex
        self.characterOffset = characterOffset
        self.text = text
        self.vector = vector
    }
}

/// An on-device RAG embedding and retrieval index for audiobook narration context and book-grounded Q&A.
/// Supports spoiler-safe querying up to a specific reading position (`PlaybackBookmark`).
public actor BookEmbeddingIndex {
    
    private let modelDirectory: URL
    private var container: EmbedderModelContainer?
    private var chunks: [EmbeddedChunk] = []

    public init(modelDirectory: URL) {
        self.modelDirectory = modelDirectory
    }

    /// Load the model container lazily on first embedding request.
    private func loadedContainer() async throws -> EmbedderModelContainer {
        if let existing = container { return existing }
        let c = try await EmbedderModelFactory.shared.loadContainer(
            from: modelDirectory,
            using: HuggingFaceTokenizerLoader()
        )
        container = c
        return c
    }

    /// Clears the index (e.g. when loading a new book).
    public func clear() {
        chunks = []
    }

    /// Returns the currently stored chunks.
    public func getChunks() -> [EmbeddedChunk] {
        return chunks
    }

    /// Load chunks from an existing serialized index (e.g. stored in SwiftData / Documents).
    public func load(chunks: [EmbeddedChunk]) {
        self.chunks = chunks
    }

    /// Segment and embed a `BookDocument` in a single pass.
    /// Chunks are generated with a target size and overlap.
    public func ingest(document: any BookDocument, chunkSize: Int = 600, overlap: Int = 100) async throws {
        clear()
        let c = try await loadedContainer()
        
        for index in 0..<document.chapterCount {
            guard let chapter = try? await document.chapter(at: index) else { continue }
            let chapterText = chapter.text
            guard !chapterText.isEmpty else { continue }
            
            var textChunks: [(offset: Int, text: String)] = []
            var start = 0
            
            while start < chapterText.count {
                let end = min(start + chunkSize, chapterText.count)
                let rangeStart = chapterText.index(chapterText.startIndex, offsetBy: start)
                let rangeEnd = chapterText.index(chapterText.startIndex, offsetBy: end)
                let chunkText = String(chapterText[rangeStart..<rangeEnd])
                
                textChunks.append((offset: start, text: chunkText))
                
                if end == chapterText.count { break }
                start += chunkSize - overlap
            }
            
            // Embed this chapter's chunks
            for item in textChunks {
                let vector = try await embed(text: item.text, using: c)
                let embedded = EmbeddedChunk(
                    spineIndex: index,
                    characterOffset: item.offset,
                    text: item.text,
                    vector: vector
                )
                chunks.append(embedded)
            }
        }
    }

    /// Query the index with spoiler-safe constraints: only searches up to the specified `currentSpineIndex`
    /// and `currentCharacterOffset`.
    public func query(
        _ queryString: String,
        currentSpineIndex: Int,
        currentCharacterOffset: Int,
        topK: Int = 3
    ) async throws -> [EmbeddedChunk] {
        guard !chunks.isEmpty else { return [] }
        let c = try await loadedContainer()
        let queryVector = try await embed(text: queryString, using: c)
        
        // Filter chunks to prevent spoilers
        let safeChunks = filterSafeChunks(chunks, currentSpineIndex: currentSpineIndex, currentCharacterOffset: currentCharacterOffset)
        
        guard !safeChunks.isEmpty else { return [] }
        
        // Calculate similarity scores
        let scored = safeChunks.map { chunk -> (chunk: EmbeddedChunk, score: Float) in
            let score = cosineSimilarity(queryVector, chunk.vector)
            return (chunk, score)
        }
        
        // Sort and return top-k
        let sorted = scored.sorted { $0.score > $1.score }
        let result = sorted.prefix(topK).map { $0.chunk }
        return Array(result)
    }

    /// Single string embedding utility.
    private func embed(text: String, using container: EmbedderModelContainer) async throws -> [Float] {
        let result: [Float] = await container.perform { context in
            let model = context.model
            let tokenizer = context.tokenizer
            let pooling = context.pooling
            
            let tokens = tokenizer.encode(text: text, addSpecialTokens: true)
            let padded = stacked([MLXArray(tokens)])
            let eosId = tokenizer.eosTokenId ?? 0
            let mask = padded .!= eosId
            let tokenTypes = MLXArray.zeros(like: padded)
            
            let output = model(padded, positionIds: nil, tokenTypeIds: tokenTypes, attentionMask: mask)
            let pooled = pooling(output, normalize: true, applyLayerNorm: true)
            pooled.eval()
            
            return pooled[0].asArray(Float.self)
        }
        return result
    }

    /// Filter chunks to prevent spoilers. Exposed for testing.
    public nonisolated func filterSafeChunks(_ chunks: [EmbeddedChunk], currentSpineIndex: Int, currentCharacterOffset: Int) -> [EmbeddedChunk] {
        return chunks.filter { chunk in
            if chunk.spineIndex < currentSpineIndex {
                return true
            } else if chunk.spineIndex == currentSpineIndex {
                return chunk.characterOffset <= currentCharacterOffset
            }
            return false
        }
    }

    /// Helper for cosine similarity.
    private func cosineSimilarity(_ a: [Float], _ b: [Float]) -> Float {
        guard a.count == b.count, !a.isEmpty else { return 0.0 }
        var dotProduct: Float = 0.0
        var normA: Float = 0.0
        var normB: Float = 0.0
        for i in 0..<a.count {
            dotProduct += a[i] * b[i]
            normA += a[i] * a[i]
            normB += b[i] * b[i]
        }
        guard normA > 0 && normB > 0 else { return 0.0 }
        return dotProduct / (sqrt(normA) * sqrt(normB))
    }
}
