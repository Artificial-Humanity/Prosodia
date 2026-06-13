import Foundation


#if os(macOS)
import IOKit.ps
#elseif os(iOS) || os(tvOS) || targetEnvironment(macCatalyst)
import UIKit
#endif

public final class SmartExecutionCoordinator: @unchecked Sendable, ProsodiaActorBackend {
    public let mlxEngine: (any ProsodiaActorBackend)?
    public let coreMlEngine: (any ProsodiaActorBackend)?
    
    private let provider = Locked<(@Sendable () -> BatteryStatus)?> (nil)
    
    public var batteryStatusProvider: (@Sendable () -> BatteryStatus)? {
        get { provider.withLock { $0 } }
        set { provider.withLock { $0 = newValue } }
    }
    
    public var vocab: [String: Int] {
        return (mlxEngine ?? coreMlEngine)?.vocab ?? [:]
    }
    
    public init(
        mlxEngine: (any ProsodiaActorBackend)? = nil,
        coreMlEngine: (any ProsodiaActorBackend)? = nil
    ) {
        precondition(mlxEngine != nil || coreMlEngine != nil, "At least one backend engine must be provided.")
        self.mlxEngine = mlxEngine
        self.coreMlEngine = coreMlEngine
    }
    
    public func forward(
        phonemes: String,
        refS: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> ActorEngineOutput {
        let preferredBackend = selectPreferredBackend()
        
        do {
            return try preferredBackend.forward(
                phonemes: phonemes,
                refS: refS,
                speed: speed,
                durationScales: durationScales,
                f0Bias: f0Bias
            )
        } catch {
            // Graceful fallback to the alternate backend
            let alternate = (preferredBackend === mlxEngine) ? coreMlEngine : mlxEngine
            guard let alternate else {
                throw error
            }
            print("[SmartExecutionCoordinator] Preferred backend failed. Falling back to alternate backend.")
            return try alternate.forward(
                phonemes: phonemes,
                refS: refS,
                speed: speed,
                durationScales: durationScales,
                f0Bias: f0Bias
            )
        }
    }
    
    public func tokenize(_ phonemes: String) throws -> [Int] {
        return try selectPreferredBackend().tokenize(phonemes)
    }

    public func reclaimMemory() {
        mlxEngine?.reclaimMemory()
        coreMlEngine?.reclaimMemory()
    }
    
    public func selectPreferredBackend() -> any ProsodiaActorBackend {
        guard let mlx = mlxEngine else {
            return coreMlEngine!
        }
        guard let coreMl = coreMlEngine else {
            return mlx
        }
        
        let status = fetchBatteryStatus()
        
        let shouldSavePower = status.isLowPowerMode || 
            ((status.level ?? 100) < 20 && !(status.isPluggedIn ?? false))
        
        if shouldSavePower {
            print("[SmartExecutionCoordinator] Routing to ANE (CoreML) to conserve power. (Low Power Mode: \(status.isLowPowerMode), Battery Level: \(status.level ?? -1)%)")
            return coreMl
        } else {
            return mlx
        }
    }
    
    public struct BatteryStatus: Sendable {
        public let level: Int?
        public let isPluggedIn: Bool?
        public let isLowPowerMode: Bool
        
        public init(level: Int?, isPluggedIn: Bool?, isLowPowerMode: Bool) {
            self.level = level
            self.isPluggedIn = isPluggedIn
            self.isLowPowerMode = isLowPowerMode
        }
    }
    
    private func fetchBatteryStatus() -> BatteryStatus {
        if let customProvider = batteryStatusProvider {
            return customProvider()
        }
        
        let isLowPower = ProcessInfo.processInfo.isLowPowerModeEnabled
        
        #if os(macOS)
        let snapshot = IOPSCopyPowerSourcesInfo()?.takeRetainedValue()
        let sources = snapshot.flatMap { IOPSCopyPowerSourcesList($0)?.takeRetainedValue() as? [CFTypeRef] } ?? []
        for source in sources {
            if let snapshotInfo = IOPSCopyPowerSourcesInfo()?.takeRetainedValue(),
               let info = IOPSGetPowerSourceDescription(snapshotInfo, source)?.takeUnretainedValue() as? [String: Any] {
                let state = info[kIOPSPowerSourceStateKey] as? String
                let current = info[kIOPSCurrentCapacityKey] as? Int
                let max = info[kIOPSMaxCapacityKey] as? Int
                
                let isPluggedIn = state != kIOPSBatteryPowerValue
                let level = (current != nil && max != nil && max! > 0) ? Int(Double(current!) / Double(max!) * 100) : nil
                return BatteryStatus(level: level, isPluggedIn: isPluggedIn, isLowPowerMode: isLowPower)
            }
        }
        #elseif os(iOS) || os(tvOS) || targetEnvironment(macCatalyst)
        let wasEnabled = UIDevice.current.isBatteryMonitoringEnabled
        if !wasEnabled {
            UIDevice.current.isBatteryMonitoringEnabled = true
        }
        let level = UIDevice.current.batteryLevel >= 0 ? Int(UIDevice.current.batteryLevel * 100) : nil
        let state = UIDevice.current.batteryState
        let isPluggedIn = state == .charging || state == .full
        if !wasEnabled {
            UIDevice.current.isBatteryMonitoringEnabled = false
        }
        return BatteryStatus(level: level, isPluggedIn: isPluggedIn, isLowPowerMode: isLowPower)
        #endif
        
        return BatteryStatus(level: nil, isPluggedIn: nil, isLowPowerMode: isLowPower)
    }
}
