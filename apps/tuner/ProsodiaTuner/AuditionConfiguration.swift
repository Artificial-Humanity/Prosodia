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
    var vocalEnergy: Double = 1.0
    var strainOrRasp: Double = 0.0

    enum CodingKeys: String, CodingKey {
        case id, name, valence, arousal, tension, speed, volume, pitch, ageProfile, masculinity, vocalEnergy, strainOrRasp
    }

    init(id: UUID = UUID(), name: String, valence: Double, arousal: Double, tension: Double, speed: Double, volume: Double, pitch: Double = 0.0, ageProfile: Double = 0.0, masculinity: Double = 0.0, vocalEnergy: Double = 1.0, strainOrRasp: Double = 0.0) {
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
        self.vocalEnergy = vocalEnergy
        self.strainOrRasp = strainOrRasp
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
        self.vocalEnergy = try container.decodeIfPresent(Double.self, forKey: .vocalEnergy) ?? 1.0
        self.strainOrRasp = try container.decodeIfPresent(Double.self, forKey: .strainOrRasp) ?? 0.0
    }

    var emotion: EmotionVector {
        EmotionVector(valence: valence, arousal: arousal, tension: tension)
    }

    var acoustics: ProsodyAcoustics {
        let cp = CastingProfile(
            ageProfile: ageProfile,
            masculinity: masculinity,
            strainOrRasp: strainOrRasp
        )
        return ProsodyAcoustics(
            speedMultiplier: speed,
            speedBias: nil,
            gainMultiplier: volume,
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

    var directive: ProsodyDirective {
        ProsodyDirective(emotion: emotion, acoustics: acoustics)
    }

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
            vocalEnergy: 1.0,
            strainOrRasp: 0.0
        )
    }
}

@MainActor
@Observable
final class AuditionPresetStore {
    nonisolated static var availableVoices: [String] {
        let voiceDir = ProductionRunner.resolvedVoiceDirectory
        
        var voices: [String] = []
        if let contents = try? FileManager.default.contentsOfDirectory(at: voiceDir, includingPropertiesForKeys: nil) {
            for file in contents {
                let ext = file.pathExtension.lowercased()
                if ext == "safetensors" || ext == "npy" {
                    let name = file.deletingPathExtension().lastPathComponent
                    if !name.contains("epochs_") {
                        voices.append(name)
                    }
                }
            }
        }
        
        if !voices.isEmpty {
            return voices.sorted()
        }
        
        return ["narrator", "sad", "whisper", "happy", "angry"]
    }

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
            return "Load a preset and tweak it with sliders. Sliders adjust Valence/Arousal/Tension continuously without auto-saving to the preset database, or you can save adjustments as a new preset."
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
    var mlxBaseVoice: String? = nil
    var mlxNarrationMode: Stage.NarrationMode = .solo

    init() {
        applyConfigToStageAndActors(ProsodiaConfigManager.shared.config)
    }

    /// Builds the Director implementation for the current settings.
    func makeDirector(model: DirectorModel?) -> any DirectorInference {
        switch emotionMode {
        case .preset:
            return StubDirectorInference(directive: activePreset.directive)
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
