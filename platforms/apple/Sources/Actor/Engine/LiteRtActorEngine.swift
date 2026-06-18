import Foundation
import os
import Stage
import Kit

/// A ``ProsodiaActorBackend`` powered by the Google LiteRT (TensorFlow Lite) runtime executed in the Rust core.
public final class LiteRtActorEngine: @unchecked Sendable, ProsodiaActorBackend {

    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "LiteRtActorEngine")

    private let rustEngine: Kit.LiteRtActorEngine

    /// Initializes a new LiteRT Actor engine.
    ///
    /// - Parameters:
    ///   - modelPath: Local URL to the `.tflite` model file.
    ///   - configURL: Local URL to the configuration `config.json` containing vocab mapping (unused).
    public init(modelPath: URL, configURL: URL) throws {
        self.rustEngine = Kit.LiteRtActorEngine(modelPath: modelPath.path)
    }

    /// Reclaims memory by releasing the loaded interpreter and model structures.
    public func reclaimMemory() {
        rustEngine.reclaimMemory()
        Self.log.info("LiteRT Actor engine memory reclaimed via Rust.")
    }

    // MARK: - Inference Execution

    public func forward(
        phonemeIds: [Int32],
        refS: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> ActorEngineOutput {
        let output = try rustEngine.forward(
            phonemeIds: phonemeIds,
            style: refS,
            speed: speed,
            durationScales: durationScales,
            f0Bias: f0Bias
        )
        return ActorEngineOutput(
            audio: output.audio,
            predDur: output.predDur.map { Int($0) }
        )
    }

    public func isMatcha() -> Bool {
        return rustEngine.isMatcha()
    }

    public func getTokenLimit() -> Int32 {
        return rustEngine.getTokenLimit()
    }
}
