import Foundation
import Kit

#if canImport(MLX)
public struct MlxVocalActorProvider: VocalActorProvider {
    public init() {}
    
    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        // MLX model files end in .safetensors (like styletts2_lite.safetensors)
        if ext == "safetensors" || modelURL.path.hasSuffix(".safetensors") {
            return true
        }
        // CoreML models/directories
        if ext == "mlmodelc" || modelURL.path.hasSuffix(".mlmodelc") {
            return true
        }
        let isDir = (try? modelURL.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
        if isDir {
            let configURL = modelURL.appendingPathComponent("config.json")
            let styletts2URL = modelURL.appendingPathComponent("styletts2_lite.mlmodelc")
            return FileManager.default.fileExists(atPath: configURL.path) && FileManager.default.fileExists(atPath: styletts2URL.path)
        }
        return false
    }
    
    public func makeActor(modelURL: URL, voiceDirectoryURL: URL?) -> any VocalActor {
        let voiceDir = voiceDirectoryURL ?? modelURL.deletingLastPathComponent()
        return MlxVocalActor(modelPath: modelURL, voiceDirectory: voiceDir)
    }
}
#endif

#if canImport(CLlama)
public struct GgufVocalActorProvider: VocalActorProvider {
    public init() {}
    
    public func canHandle(modelURL: URL) -> Bool {
        let ext = modelURL.pathExtension.lowercased()
        // GGUF text-to-speech models (OuteTTS format)
        return ext == "gguf" || modelURL.path.hasSuffix(".gguf")
    }
    
    public func makeActor(modelURL: URL, voiceDirectoryURL: URL?) -> any VocalActor {
        // GgufVocalActor expects vocoderPath as its second URL parameter
        let vocoderURL = voiceDirectoryURL ?? modelURL.deletingLastPathComponent().appendingPathComponent("vocoder.gguf")
        return GgufVocalActor(modelPath: modelURL, vocoderPath: vocoderURL)
    }
}
#endif

/// Entry point to register all compiled vocal actor backends with the registry.
public func registerProsodiaActors() {
    #if canImport(MLX)
    VocalActorRegistry.shared.register(provider: MlxVocalActorProvider())
    #endif
    
    #if canImport(CLlama)
    VocalActorRegistry.shared.register(provider: GgufVocalActorProvider())
    #endif
}
