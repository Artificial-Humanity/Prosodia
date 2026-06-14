import Foundation
import Stage

/// The only supported director backend: Gemma 4 via LiteRT-LM (``LiteRtLmDirector``).
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
    DirectorRegistry.shared.register(provider: LiteRtLmDirectorProvider())
}
