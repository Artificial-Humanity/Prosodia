//
//  AuditionConfiguration.swift
//  ProsodiaTuner
//

import Foundation
import Observation
import Kit
import Stage

// Harness directive:
// Tune the models for their roles. The Director directs arbitrary prose; the Actor
// acts out that direction. Keep this harness to explicit audition controls, saved states,
// and instrumentation. Do not add keyword rules or book-specific emotion guesses
// in app code.



/// What the selected actor role's model takes, from its path and its `conditioning`
/// block. Decides which VAT values the Tuner sends and how far Volume may go.
struct ActorRoleCapabilities: Equatable, Sendable {
    /// The role's path is a split-model directory.
    var isSplit: Bool
    /// The role has a `conditioning` block in prosodia_models.json.
    var hasConditioning: Bool
    /// VAT channels the role's model trained: 0 valence, 1 Energy (arousal), 2 tension.
    var trainedVat: Set<UInt32>

    /// A monolith (today's `actor` role has no `vat` input): every slider stays live
    /// and moves only the AcousticMatrix-derived speed and volume.
    static let monolith = ActorRoleCapabilities(isSplit: false, hasConditioning: false, trainedVat: [])

    /// Volume on split roles: [−12, +6] dB as `G:` writes it, to 3 decimals
    /// (20·log10(0.251) = −12.007 dB would be refused).
    static let splitVolumeBounds: ClosedRange<Double> = 0.252...1.995

    /// Whether VAT channel `channel` is sent as set. On a split role only trained
    /// channels are; a split role with no block trains none (fail closed).
    func sends(_ channel: UInt32) -> Bool {
        !isSplit || trainedVat.contains(channel)
    }

    /// Whether the model itself changes loudness with Energy.
    var trainsEnergy: Bool { isSplit && trainedVat.contains(1) }

    /// The Volume range the role accepts, or nil for no bound.
    var volumeBounds: ClosedRange<Double>? { isSplit ? Self.splitVolumeBounds : nil }

    /// The Volume range sent and shown: `limits` (the global gain limits) inside the
    /// role's ``volumeBounds``.
    func volumeRange(within limits: ClosedRange<Double>) -> ClosedRange<Double> {
        guard let bounds = volumeBounds else { return limits }
        return limits.clamped(to: bounds)
    }
}

struct AuditionPreset: Codable, Identifiable, Hashable, Sendable {
    var id: UUID = UUID()
    var name: String
    var valence: Double
    var arousal: Double
    var tension: Double
    var speed: Double
    var volume: Double
    var pitch: Double = 0.0
    var ageProfile: Double = 0.0
    var masculinity: Double = 0.0
    var strainOrRasp: Double = 0.0
    /// True while `volume` is `AcousticMatrix.gain` of the preset's emotion
    /// (built by ``from(_:)``); false once the user sets Volume.
    var volumeIsDerived: Bool = false

    enum CodingKeys: String, CodingKey {
        case id, name, valence, arousal, tension, speed, volume, pitch, ageProfile, masculinity, strainOrRasp, volumeIsDerived
    }

    init(id: UUID = UUID(), name: String, valence: Double, arousal: Double, tension: Double, speed: Double, volume: Double, pitch: Double = 0.0, ageProfile: Double = 0.0, masculinity: Double = 0.0, strainOrRasp: Double = 0.0, volumeIsDerived: Bool = false) {
        self.id = id
        self.name = name
        self.valence = valence
        self.arousal = arousal
        self.tension = tension
        self.speed = speed
        self.volume = volume
        self.pitch = pitch
        self.ageProfile = ageProfile
        self.masculinity = masculinity
        self.strainOrRasp = strainOrRasp
        self.volumeIsDerived = volumeIsDerived
    }

    init(from decoder: Swift.Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.id = try container.decodeIfPresent(UUID.self, forKey: .id) ?? UUID()
        self.name = try container.decode(String.self, forKey: .name)
        self.valence = try container.decode(Double.self, forKey: .valence)
        self.arousal = try container.decode(Double.self, forKey: .arousal)
        self.tension = try container.decode(Double.self, forKey: .tension)
        self.speed = try container.decode(Double.self, forKey: .speed)
        self.volume = try container.decode(Double.self, forKey: .volume)
        self.pitch = try container.decodeIfPresent(Double.self, forKey: .pitch) ?? 0.0
        self.ageProfile = try container.decodeIfPresent(Double.self, forKey: .ageProfile) ?? 0.0
        self.masculinity = try container.decodeIfPresent(Double.self, forKey: .masculinity) ?? 0.0
        self.strainOrRasp = try container.decodeIfPresent(Double.self, forKey: .strainOrRasp) ?? 0.0
        // Saved presets predate the flag; their Volume is the one the user saved.
        self.volumeIsDerived = try container.decodeIfPresent(Bool.self, forKey: .volumeIsDerived) ?? false
    }

    /// The emotion sent on a role with `capabilities`. The only place V and T are
    /// zeroed: on a split role, a channel the role did not train is sent as 0. The
    /// Director's output is never passed through here.
    func sentEmotion(for capabilities: ActorRoleCapabilities) -> EmotionVector {
        EmotionVector(
            valence: capabilities.sends(0) ? valence : 0,
            arousal: capabilities.sends(1) ? arousal : 0,
            tension: capabilities.sends(2) ? tension : 0
        )
    }

    /// The Volume sent on a role with `capabilities`, which the Volume slider shows.
    /// A derived Volume is 1.0 where the model trains Energy, so loudness is not
    /// applied twice (model Energy plus mel Volume). The value is clamped to
    /// `volumeLimits` (the global gain limits) inside the role's bounds
    /// (``ActorRoleCapabilities/volumeRange(within:)``).
    func sentVolume(for capabilities: ActorRoleCapabilities, volumeLimits: ClosedRange<Double>) -> Double {
        let raw = volumeIsDerived && capabilities.trainsEnergy ? 1.0 : volume
        let range = capabilities.volumeRange(within: volumeLimits)
        return min(max(raw, range.lowerBound), range.upperBound)
    }

    func acoustics(for capabilities: ActorRoleCapabilities, volumeLimits: ClosedRange<Double>) -> ProsodyAcoustics {
        let cp = CastingProfile(
            ageProfile: ageProfile,
            masculinity: masculinity,
            strainOrRasp: strainOrRasp
        )
        return ProsodyAcoustics(
            speedMultiplier: speed,
            speedBias: nil,
            gainMultiplier: sentVolume(for: capabilities, volumeLimits: volumeLimits),
            gainBias: nil,
            castingProfile: cp,
            speakerLock: nil,
            pauseMultiplier: nil,
            pronunciationOverride: nil,
            pitch: pitch,
            tokenDurationScales: nil,
            tokenF0Biases: nil
        )
    }

    /// The directive for default presets, saved custom presets and the sliders alike.
    func directive(for capabilities: ActorRoleCapabilities, volumeLimits: ClosedRange<Double>) -> ProsodyDirective {
        ProsodyDirective(
            emotion: sentEmotion(for: capabilities),
            acoustics: acoustics(for: capabilities, volumeLimits: volumeLimits)
        )
    }

    static func from(_ preset: EmotionPreset) -> AuditionPreset {
        let emotion = preset
        return AuditionPreset(
            name: preset.rawValue.capitalized,
            valence: emotion.vector.valence,
            arousal: emotion.vector.arousal,
            tension: emotion.vector.tension,
            speed: AcousticMatrix.speed(for: emotion.vector),
            volume: AcousticMatrix.gain(for: emotion.vector),
            pitch: 0.0,
            ageProfile: 0.0,
            masculinity: 0.0,
            strainOrRasp: 0.0,
            volumeIsDerived: true
        )
    }
}

@MainActor
@Observable
final class AuditionPresetStore {
    var presets: [AuditionPreset]
    var selectedID: UUID {
        didSet { saveSelection() }
    }

    private static let presetsKey = "harnessAuditionPresets"
    private static let selectedKey = "harnessSelectedAuditionPreset"

    init() {
        let loaded = Self.loadPresets()
        let defaultCases = EmotionPreset.allCases
        let defaultNames = Set(defaultCases.map { $0.rawValue.lowercased() })
        
        // Separate custom user-created presets from standard default presets
        let userCustomPresets = loaded.filter { !defaultNames.contains($0.name.lowercased()) }
        
        // Re-generate all default presets fresh from their latest Swift EmotionPreset coordinate definitions
        let freshDefaults = defaultCases.map { AuditionPreset.from($0) }
        
        // Combine: fresh defaults first, followed by the user's custom presets
        let initialPresets = freshDefaults + userCustomPresets
        presets = initialPresets
        if let raw = UserDefaults.standard.string(forKey: Self.selectedKey),
           let id = UUID(uuidString: raw),
           initialPresets.contains(where: { $0.id == id }) {
            selectedID = id
        } else {
            selectedID = initialPresets.first?.id ?? UUID()
        }
        save()
    }

    var selected: AuditionPreset {
        get { presets.first { $0.id == selectedID } ?? presets[0] }
        set {
            guard let index = presets.firstIndex(where: { $0.id == selectedID }) else { return }
            presets[index] = newValue
            save()
        }
    }

    func saveAsNewPreset(_ preset: AuditionPreset) {
        var copy = preset
        copy.id = UUID()
        copy.name = uniqueName(base: copy.name)
        presets.append(copy)
        selectedID = copy.id
        save()
    }

    func deletePreset(_ preset: AuditionPreset) {
        guard presets.count > 1 else { return }
        if selectedID == preset.id {
            if let index = presets.firstIndex(where: { $0.id == preset.id }) {
                let nextIndex = index > 0 ? index - 1 : index + 1
                selectedID = presets[nextIndex].id
            }
        }
        presets.removeAll { $0.id == preset.id }
        save()
    }

    func resetToDefaults() {
        presets = Self.defaultPresets()
        selectedID = presets.first?.id ?? UUID()
        save()
    }

    private func uniqueName(base: String) -> String {
        var candidate = "\(base) Copy"
        var counter = 2
        while presets.contains(where: { $0.name == candidate }) {
            candidate = "\(base) Copy \(counter)"
            counter += 1
        }
        return candidate
    }

    private func save() {
        if let data = try? JSONEncoder().encode(presets) {
            UserDefaults.standard.set(data, forKey: Self.presetsKey)
        }
        saveSelection()
    }

    private func saveSelection() {
        UserDefaults.standard.set(selectedID.uuidString, forKey: Self.selectedKey)
    }

    private static func loadPresets() -> [AuditionPreset] {
        guard let data = UserDefaults.standard.data(forKey: presetsKey),
              let decoded = try? JSONDecoder().decode([AuditionPreset].self, from: data)
        else { return [] }
        return decoded
    }

    private static func defaultPresets() -> [AuditionPreset] {
        EmotionPreset.allCases.map { AuditionPreset.from($0) }
    }
}

/// How the harness chooses emotion for each sample sentence.
enum EmotionSourceMode: String, CaseIterable, Identifiable, Sendable {
    case preset = "Presets & Manual Tuning"
    case director = "Director Model"

    var id: String { rawValue }

    var helpText: String {
        switch self {
        case .preset:
            return "Load a preset and tweak it with sliders. Sliders adjust Valence/Energy/Tension continuously without auto-saving to the preset database, or you can save adjustments as a new preset."
        case .director:
            return "The Director model reads each sentence and dynamically guides the continuous emotional reading (VAD) block on the fly (Gemma 4 via LiteRT-LM)."
        }
    }
}

@MainActor
@Observable
final class AuditionConfiguration {
    var emotionMode: EmotionSourceMode = .preset
    var activePreset: AuditionPreset = .from(.tender)
    var loadedPresetID: UUID?
    var globalConfig: ProsodiaConfig = ProsodiaConfigManager.shared.config
    var mlxNarrationMode: Stage.NarrationMode = .solo

    init() {
        applyConfigToStageAndActors(ProsodiaConfigManager.shared.config)
    }

    /// The global gain limits ("Volume Min Limit" … "Volume Max Limit"), in order.
    var volumeLimits: ClosedRange<Double> {
        min(globalConfig.gainMin, globalConfig.gainMax)...max(globalConfig.gainMin, globalConfig.gainMax)
    }

    /// Builds the Director implementation for the current settings. In preset mode the
    /// directive is shaped for the selected actor role (`capabilities`); the Director's
    /// own output is never adjusted — the engine refuses what the role did not train.
    func makeDirector(model: DirectorModel?, capabilities: ActorRoleCapabilities) -> any Stage.DirectorInference {
        switch emotionMode {
        case .preset:
            return StubDirectorInference(directive: activePreset.directive(for: capabilities, volumeLimits: volumeLimits))
        case .director:
            guard let model else {
                return StubDirectorInference(directive: ProsodyDirective(preset: .baseline))
            }
            if let director = DirectorRegistry.shared.makeDirector(for: model.directory, narrationMode: mlxNarrationMode) {
                return director
            }
            return StubDirectorInference(directive: ProsodyDirective(preset: .baseline))
        }
    }

    var canUseMlx: Bool {
        emotionMode == .director
    }
}
