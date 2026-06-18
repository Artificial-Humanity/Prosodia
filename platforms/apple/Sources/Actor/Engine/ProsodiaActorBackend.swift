import Foundation
import Kit

public struct ActorEngineOutput: Sendable {
    public let audio: [Float]
    public let predDur: [Int]

    public init(audio: [Float], predDur: [Int]) {
        self.audio = audio
        self.predDur = predDur
    }
}

public protocol ProsodiaActorBackend: AnyObject, Sendable {
    func forward(
        phonemeIds: [Int32],
        refS: StyleVector,
        speed: Float,
        vat: [Float]?,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> ActorEngineOutput

    func reclaimMemory()

    func isMatcha() -> Bool
    func getTokenLimit() -> Int32
}

public extension ProsodiaActorBackend {
    func reclaimMemory() {}
    func isMatcha() -> Bool { false }
    func getTokenLimit() -> Int32 { 510 }
}
