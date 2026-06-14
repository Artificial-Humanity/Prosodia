import Foundation
import Kit

// MARK: - EmotionPreset Extensions

extension EmotionPreset: CaseIterable {
    public static var allCases: [EmotionPreset] {
        return [
            .baseline, .soft, .somber, .excited, .tense, .tender,
            .technical, .angry, .cold, .tired, .distraught,
            .theatrical, .stern, .pleading
        ]
    }
}

extension EmotionPreset: RawRepresentable {
    public typealias RawValue = String
    
    public init?(rawValue: String) {
        switch rawValue.lowercased() {
        case "baseline": self = .baseline
        case "soft": self = .soft
        case "somber": self = .somber
        case "excited": self = .excited
        case "tense": self = .tense
        case "tender": self = .tender
        case "technical": self = .technical
        case "angry": self = .angry
        case "cold": self = .cold
        case "tired": self = .tired
        case "distraught": self = .distraught
        case "theatrical": self = .theatrical
        case "stern": self = .stern
        case "pleading": self = .pleading
        default: return nil
        }
    }
    
    public var rawValue: String {
        switch self {
        case .baseline: return "baseline"
        case .soft: return "soft"
        case .somber: return "somber"
        case .excited: return "excited"
        case .tense: return "tense"
        case .tender: return "tender"
        case .technical: return "technical"
        case .angry: return "angry"
        case .cold: return "cold"
        case .tired: return "tired"
        case .distraught: return "distraught"
        case .theatrical: return "theatrical"
        case .stern: return "stern"
        case .pleading: return "pleading"
        }
    }
}

public extension EmotionPreset {
    var vector: EmotionVector {
        switch self {
        case .baseline: return EmotionVector(valence: 0.0, arousal: 0.0, tension: 0.0)
        case .soft: return EmotionVector(valence: 0.2, arousal: -0.6, tension: 0.1)
        case .somber: return EmotionVector(valence: -0.7, arousal: -0.5, tension: 0.1)
        case .excited: return EmotionVector(valence: 0.8, arousal: 0.8, tension: 0.0)
        case .tense: return EmotionVector(valence: -0.3, arousal: 0.1, tension: 0.9)
        case .tender: return EmotionVector(valence: -0.3, arousal: -0.3, tension: 0.1)
        case .technical: return EmotionVector(valence: 0.1, arousal: -0.6, tension: 0.0)
        case .angry: return EmotionVector(valence: -0.95, arousal: 0.95, tension: 0.95)
        case .cold: return EmotionVector(valence: -0.4, arousal: -0.5, tension: 0.7)
        case .tired: return EmotionVector(valence: -0.2, arousal: -0.8, tension: 0.1)
        case .distraught: return EmotionVector(valence: -0.9, arousal: 0.6, tension: 0.95)
        case .theatrical: return EmotionVector(valence: 0.6, arousal: 0.7, tension: 0.3)
        case .stern: return EmotionVector(valence: -0.5, arousal: 0.0, tension: 0.85)
        case .pleading: return EmotionVector(valence: -0.3, arousal: 0.4, tension: 0.8)
        }
    }
}

// MARK: - ProsodyDirective Extensions

public extension ProsodyDirective {
    init(preset: EmotionPreset, acoustics: ProsodyAcoustics? = nil) {
        self.init(emotion: preset.vector, acoustics: acoustics)
    }
    
    var speedMultiplier: Double {
        return Kit.directiveSpeedMultiplier(directive: self)
    }
    
    var gainMultiplier: Double {
        return Kit.directiveGainMultiplier(directive: self)
    }
}

// MARK: - ProsodyPhraser Extensions

public extension ProsodyPhraser {
    func resolveSpans(overall: EmotionVector, decoded: [ProsodySpan]) -> [ProsodySpan] {
        return Kit.resolveSpans(phraser: self, overall: overall, decoded: decoded)
    }
}

// MARK: - DirectorOutput

public struct DirectorOutput {
    public static func neutral(for passage: String) -> String {
        return Kit.neutralPayloadForPassage(passage: passage)
    }
    
    public static func payload(from raw: String, passage: String) -> String {
        return Kit.payloadFromRaw(raw: raw, passage: passage)
    }
    
    public static func wordsMatch(_ a: String, _ b: String) -> Bool {
        let canonA = a.filter { $0.isLetter || $0.isNumber }.lowercased()
        let canonB = b.filter { $0.isLetter || $0.isNumber }.lowercased()
        return canonA == canonB
    }
}

// MARK: - AcousticMatrix & PhrasePause Configuration Wrappers

public enum AcousticMatrix {
    public static var expressiveness: Double {
        get { Kit.getAcousticMatrix().expressiveness }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.expressiveness = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var speedArousalGain: Double {
        get { Kit.getAcousticMatrix().speedArousalGain }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.speedArousalGain = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var speedTensionGain: Double {
        get { Kit.getAcousticMatrix().speedTensionGain }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.speedTensionGain = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var speedValenceGain: Double {
        get { Kit.getAcousticMatrix().speedValenceGain }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.speedValenceGain = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var speedRange: ClosedRange<Double> {
        get {
            let cfg = Kit.getAcousticMatrix()
            return cfg.speedRangeMin...cfg.speedRangeMax
        }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.speedRangeMin = newValue.lowerBound
            cfg.speedRangeMax = newValue.upperBound
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var gainArousalGain: Double {
        get { Kit.getAcousticMatrix().gainArousalGain }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.gainArousalGain = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var gainValenceGain: Double {
        get { Kit.getAcousticMatrix().gainValenceGain }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.gainValenceGain = newValue
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static var gainRange: ClosedRange<Double> {
        get {
            let cfg = Kit.getAcousticMatrix()
            return cfg.gainRangeMin...cfg.gainRangeMax
        }
        set {
            var cfg = Kit.getAcousticMatrix()
            cfg.gainRangeMin = newValue.lowerBound
            cfg.gainRangeMax = newValue.upperBound
            Kit.setAcousticMatrix(config: cfg)
        }
    }
    
    public static func speed(for emotion: EmotionVector) -> Double {
        return Kit.speedForEmotion(emotion: emotion)
    }
    
    public static func gain(for emotion: EmotionVector) -> Double {
        return Kit.gainForEmotion(emotion: emotion)
    }
    
    public static func pitch(for emotion: EmotionVector) -> Double {
        return Kit.pitchForEmotion(emotion: emotion)
    }
}

public enum PhrasePause {
    public static var sentence: Double {
        get { Kit.getPhrasePause().sentence }
        set {
            var cfg = Kit.getPhrasePause()
            cfg.sentence = newValue
            Kit.setPhrasePause(config: cfg)
        }
    }
    
    public static var clause: Double {
        get { Kit.getPhrasePause().clause }
        set {
            var cfg = Kit.getPhrasePause()
            cfg.clause = newValue
            Kit.setPhrasePause(config: cfg)
        }
    }
    
    public static func after(_ text: String) -> Double {
        return Kit.pauseAfter(text: text)
    }
}

// MARK: - ProsodySpan Extensions

public extension ProsodySpan {
    var directive: ProsodyDirective {
        return ProsodyDirective(emotion: self.emotion, acoustics: self.acoustics)
    }
    
    var speed: Double {
        return directive.speedMultiplier
    }
    
    var gain: Double {
        return directive.gainMultiplier
    }
    
    var pitch: Double {
        return self.acoustics?.pitch ?? AcousticMatrix.pitch(for: self.emotion)
    }
}


