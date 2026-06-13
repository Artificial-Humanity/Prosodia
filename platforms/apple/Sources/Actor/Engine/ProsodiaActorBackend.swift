import Foundation

public struct ActorEngineOutput: Sendable {
    public let audio: [Float]
    public let predDur: [Int]

    public init(audio: [Float], predDur: [Int]) {
        self.audio = audio
        self.predDur = predDur
    }
}

public protocol ProsodiaActorBackend: AnyObject, Sendable {
    var vocab: [String: Int] { get }

    func tokenize(_ phonemes: String) throws -> [Int]

    func forward(
        phonemes: String,
        refS: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> ActorEngineOutput

    func reclaimMemory()
}

public extension ProsodiaActorBackend {
    func tokenize(_ phonemes: String) throws -> [Int] {
        return []
    }
    func reclaimMemory() {}
}

