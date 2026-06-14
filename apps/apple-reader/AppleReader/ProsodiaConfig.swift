import Foundation
import Kit
import Stage

public struct ProsodiaConfig: Codable, Equatable, Sendable {
    public var expressiveness: Double = 3.25
    public var speedArousalGain: Double = 0.08
    public var speedTensionGain: Double = 0.10
    public var speedValenceGain: Double = 0.05
    public var speedMin: Double = 0.65
    public var speedMax: Double = 1.12
    public var gainArousalGain: Double = 0.25
    public var gainValenceGain: Double = 0.08
    public var gainMin: Double = 0.60
    public var gainMax: Double = 1.20
    
    public var blendSigma: Double = 0.27
    public var blendMinimumFraction: Double = 0.05
    public var blendProximityThreshold: Double = 0.15
    
    public var pauseSentence: Double = 0.28
    public var pauseClause: Double = 0.25
    
    public init() {}
}

@MainActor
public func applyConfigToStageAndActors(_ config: ProsodiaConfig) {
    AcousticMatrix.expressiveness = config.expressiveness
    AcousticMatrix.speedArousalGain = config.speedArousalGain
    AcousticMatrix.speedTensionGain = config.speedTensionGain
    AcousticMatrix.speedValenceGain = config.speedValenceGain
    
    let sMin = min(config.speedMin, config.speedMax)
    let sMax = max(config.speedMin, config.speedMax)
    AcousticMatrix.speedRange = sMin...sMax
    
    AcousticMatrix.gainArousalGain = config.gainArousalGain
    AcousticMatrix.gainValenceGain = config.gainValenceGain
    
    let gMin = min(config.gainMin, config.gainMax)
    let gMax = max(config.gainMin, config.gainMax)
    AcousticMatrix.gainRange = gMin...gMax
    
    PhrasePause.sentence = config.pauseSentence
    PhrasePause.clause = config.pauseClause

    // Voice-blend tuning (blendSigma / blendMinimumFraction / blendProximityThreshold)
    // previously fed the removed MLX voice matrix. Rewire to the LiteRT StyleTTS2
    // voice-blend path once it lands.
}

public final class ProsodiaConfigManager: @unchecked Sendable {
    public static let shared = ProsodiaConfigManager()
    
    private let lock = NSLock()
    private var _config = ProsodiaConfig()
    
    public var config: ProsodiaConfig {
        get {
            lock.lock()
            defer { lock.unlock() }
            return _config
        }
        set {
            lock.lock()
            _config = newValue
            lock.unlock()
            save()
            DispatchQueue.main.async {
                applyConfigToStageAndActors(newValue)
            }
        }
    }
    
    private init() {
        load()
    }
    
    private var configURL: URL {
        if let envPath = ProcessInfo.processInfo.environment["PROSODIA_CONFIG_PATH"] {
            return URL(fileURLWithPath: envPath)
        }
        
        let devPath = "/Users/lmcfarlin/Projects/Prosodia/prosodia_config.json"
        let devDir = "/Users/lmcfarlin/Projects/Prosodia"
        if FileManager.default.isWritableFile(atPath: devDir) || FileManager.default.fileExists(atPath: devPath) {
            return URL(fileURLWithPath: devPath)
        }
        
        let paths = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)
        let docDir = paths.first ?? FileManager.default.temporaryDirectory
        return docDir.appendingPathComponent("prosodia_config.json")
    }
    
    public func load() {
        lock.lock()
        defer { lock.unlock() }
        
        let url = configURL
        do {
            if FileManager.default.fileExists(atPath: url.path) {
                let data = try Data(contentsOf: url)
                let decoded = try JSONDecoder().decode(ProsodiaConfig.self, from: data)
                self._config = decoded
                print("[ProsodiaConfigManager] Loaded config from \(url.path)")
                DispatchQueue.main.async {
                    applyConfigToStageAndActors(decoded)
                }
            } else {
                print("[ProsodiaConfigManager] Config file not found at \(url.path), using defaults")
                DispatchQueue.main.async {
                    applyConfigToStageAndActors(ProsodiaConfig())
                }
            }
        } catch {
            print("[ProsodiaConfigManager] Error loading config from \(url.path): \(error). Using defaults.")
            DispatchQueue.main.async {
                applyConfigToStageAndActors(ProsodiaConfig())
            }
        }
    }
    
    public func save() {
        lock.lock()
        let currentConfig = _config
        lock.unlock()
        
        let url = configURL
        do {
            let dir = url.deletingLastPathComponent()
            if !FileManager.default.fileExists(atPath: dir.path) {
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            }
            let encoder = JSONEncoder()
            encoder.outputFormatting = .prettyPrinted
            let data = try encoder.encode(currentConfig)
            try data.write(to: url, options: .atomic)
            print("[ProsodiaConfigManager] Saved config to \(url.path)")
        } catch {
            print("[ProsodiaConfigManager] Error saving config to \(url.path): \(error)")
        }
    }
}
