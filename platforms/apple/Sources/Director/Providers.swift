import Foundation
import ProsodiaStage

#if canImport(MLXLLM)
public struct MlxDirectorProvider: DirectorProvider {
    public init() {}
    
    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        // MLX models are directories (not .litertlm or .gguf files)
        return ext != "litertlm" && ext != "gguf" && !modelURL.path.hasSuffix(".litertlm") && !modelURL.path.hasSuffix(".gguf")
    }
    
    public func makeDirector(for modelURL: URL, narrationMode: NarrationMode) -> any DirectorInference {
        return MlxDirector(modelDirectory: modelURL, narrationMode: narrationMode)
    }
}
#endif

#if canImport(CLlama)
public struct GgufDirectorProvider: DirectorProvider {
    public init() {}
    
    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        return ext == "gguf" || modelURL.path.hasSuffix(".gguf")
    }
    
    public func makeDirector(for modelURL: URL, narrationMode: NarrationMode) -> any DirectorInference {
        return GgufDirector(modelPath: modelURL, narrationMode: narrationMode)
    }
}
#endif

public struct LiteRtLmDirectorProvider: DirectorProvider {
    public init() {}
    
    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        return ext == "litertlm" || modelURL.path.hasSuffix(".litertlm")
    }
    
    public func makeDirector(for modelURL: URL, narrationMode: NarrationMode) -> any DirectorInference {
        return LiteRtLmDirector(modelPath: modelURL, narrationMode: narrationMode)
    }
}

/// Global entry point to register all compiled director backends with the registry.
public func registerProsodiaDirectors() {
    #if canImport(MLXLLM)
    DirectorRegistry.shared.register(provider: MlxDirectorProvider())
    #endif
    
    #if canImport(CLlama)
    DirectorRegistry.shared.register(provider: GgufDirectorProvider())
    #endif
    
    DirectorRegistry.shared.register(provider: LiteRtLmDirectorProvider())
}
