//
//  TunerContentView.swift
//  ProsodiaTuner
//

import SwiftUI
import UniformTypeIdentifiers
import Kit
import Stage
#if canImport(AppKit)
import AppKit
#endif
#if canImport(UIKit)
import UIKit
#endif

struct TunerContentView: View {
    @State private var runner = ProductionRunner()
    @State private var store = DirectorModelStore()
    @State private var presetStore = AuditionPresetStore()
    @State private var config = AuditionConfiguration()
    @State private var showImporter = false
    @State private var feedbackSegment: StubVocalActor.RenderedSegment?
    @State private var showAddPassageSheet = false
    @State private var passageStore = SamplePassageStore.shared
    @State private var didCopyConfig = false
    @State private var previewTask: Task<Void, Never>? = nil

    private var footerText: String {
        if runner.canSpeak {
            if config.canUseMlx {
                let name = store.selected?.displayName ?? "add a Director model below"
                return "Preview = metadata only. Speak = StyleTTS2 actor audio via \(name)."
            }
            return "Preview = metadata only. Speak = StyleTTS2 actor audio with the emotion above."
        }
        return "Preview shows VAD and voice blends. Add the StyleTTS2 actor model under /Models to enable Speak."
    }

    var body: some View {
        @Bindable var store = store
        @Bindable var presetStore = presetStore
        @Bindable var config = config

        NavigationStack {
            List {
                emotionSection(config: config, presetStore: presetStore)
                if config.emotionMode == .director {
                    directorModelSection(store: store)
                }
                globalKnobsSection
                segmentsSection
            }
            .navigationTitle("Prosody Harness")
            .fileImporter(isPresented: $showImporter, allowedContentTypes: [.folder]) { result in
                if case .success(let url) = result { store.add(directory: url) }
            }
            .toolbar { toolbarContent(store: store, config: config) }
            .task {
                config.activePreset = presetStore.selected
                config.loadedPresetID = presetStore.selectedID
                await runner.preview(config: config, model: store.selected)
            }
            .onChange(of: presetStore.selectedID) { _, _ in
                config.activePreset = presetStore.selected
                config.loadedPresetID = presetStore.selectedID
                Task {
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: config.emotionMode) { _, newMode in
                Task {
                    if newMode == .preset {
                        await runner.reclaimDirectorMemory()
                    }
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: store.selectedID) { _, _ in
                CachingDirectorEngine.clearCache()
                Task {
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: config.activePreset) { _, _ in
                Task {
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: config.mlxNarrationMode) { _, _ in
                Task {
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: config.mlxBaseVoice) { _, _ in
                Task {
                    await runner.preview(config: config, model: store.selected)
                }
            }
            .onChange(of: config.globalConfig) { _, newConfig in
                // Sync to the thread-safe config manager immediately
                ProsodiaConfigManager.shared.config = newConfig
                
                // Debounce pipeline preview updates by 150ms to ensure smooth drag rendering
                previewTask?.cancel()
                if !runner.isSpeaking {
                    previewTask = Task {
                        try? await Task.sleep(for: .milliseconds(150))
                        guard !Task.isCancelled else { return }
                        await runner.preview(config: config, model: store.selected)
                    }
                }
            }
            .sheet(item: Binding(
                get: { feedbackSegment.map { IdentifiableSegment(segment: $0) } },
                set: { feedbackSegment = $0?.segment }
            )) { identifiable in
                FeedbackSheet(
                    segment: identifiable.segment,
                    store: store,
                    config: config
                ) {
                    feedbackSegment = nil
                }
            }
            .sheet(isPresented: $showAddPassageSheet) {
                AddPassageSheet(
                    store: passageStore,
                    runner: runner,
                    config: config,
                    model: store.selected
                )
            }
        }
    }

    @ViewBuilder
    private func emotionSection(
        config: AuditionConfiguration,
        presetStore: AuditionPresetStore
    ) -> some View {
        Section {
            Picker("Emotion", selection: $config.emotionMode) {
                ForEach(EmotionSourceMode.allCases) { mode in
                    Text(mode.rawValue).tag(mode)
                }
            }
            .pickerStyle(.menu)

            switch config.emotionMode {
            case .preset:
                Picker("Preset", selection: $presetStore.selectedID) {
                    ForEach(presetStore.presets) { preset in
                        Text(preset.name).tag(preset.id)
                    }
                }
                presetEditor(config: config, presetStore: presetStore)

            case .director:
                @Bindable var config = config
                Picker("Base Voice", selection: $config.mlxBaseVoice) {
                    Text("None (Dynamic Blend Only)").tag(nil as String?)
                    ForEach(AuditionPresetStore.availableVoices, id: \.self) { voice in
                        Text(voice).tag(voice as String?)
                    }
                }
                .pickerStyle(.menu)

                Picker("Narration Mode", selection: $config.mlxNarrationMode) {
                    ForEach(NarrationMode.allCases, id: \.self) { mode in
                        Text(mode == .solo ? "Solo Narrator (Caricature)" : "Full Cast (Replacement)").tag(mode)
                    }
                }
                .pickerStyle(.menu)
            }
        } header: {
            Text("Audition settings")
        } footer: {
            Text(config.emotionMode.helpText)
        }
    }

    @ViewBuilder
    private func presetEditor(config: AuditionConfiguration, presetStore: AuditionPresetStore) -> some View {
        @Bindable var config = config
        let preset = config.activePreset
        
        let speedMin = min(config.globalConfig.speedMin, config.globalConfig.speedMax)
        let speedMax = max(config.globalConfig.speedMin, config.globalConfig.speedMax)
        let speedRange = speedMin...speedMax

        let gainMin = min(config.globalConfig.gainMin, config.globalConfig.gainMax)
        let gainMax = max(config.globalConfig.gainMin, config.globalConfig.gainMax)
        let gainRange = gainMin...gainMax

        TextField("Preset name", text: $config.activePreset.name)

        presetSlider("Valence", value: $config.activePreset.valence, range: -1.0...1.0)
        presetSlider("Arousal", value: $config.activePreset.arousal, range: -1.0...1.0)
        presetSlider("Tension", value: $config.activePreset.tension, range: 0.0...1.0)
        presetSlider(
            "Speed",
            value: Binding(
                get: { min(max(config.activePreset.speed, speedMin), speedMax) },
                set: { config.activePreset.speed = $0 }
            ),
            range: speedRange
        )
        presetSlider(
            "Volume",
            value: Binding(
                get: { min(max(config.activePreset.volume, gainMin), gainMax) },
                set: { config.activePreset.volume = $0 }
            ),
            range: gainRange
        )
        presetSlider("Pitch (Tone)", value: $config.activePreset.pitch, range: -20.0...20.0, step: 0.5)
        presetSlider("Age Profile", value: $config.activePreset.ageProfile, range: -1.0...1.0, step: 0.05)
        presetSlider("Masculinity", value: $config.activePreset.masculinity, range: -1.0...1.0, step: 0.05)
        presetSlider("Vocal Energy", value: $config.activePreset.vocalEnergy, range: 0.0...2.0, step: 0.05)
        presetSlider("Strain/Rasp", value: $config.activePreset.strainOrRasp, range: 0.0...1.0, step: 0.05)

        HStack(spacing: 12) {
            Button {
                presetStore.saveAsNewPreset(config.activePreset)
            } label: {
                Label("Save New", systemImage: "square.and.arrow.down")
            }

            Button {
                copyPresetToClipboard(config.activePreset)
                withAnimation {
                    didCopyConfig = true
                }
                Task {
                    try? await Task.sleep(for: .seconds(1.5))
                    withAnimation {
                        didCopyConfig = false
                    }
                }
            } label: {
                if didCopyConfig {
                    Label("Copied!", systemImage: "checkmark")
                        .foregroundStyle(.green)
                } else {
                    Label("Copy Preset", systemImage: "doc.on.doc")
                }
            }

            Spacer()

            Button(role: .destructive) {
                presetStore.deletePreset(presetStore.selected)
            } label: {
                Label("Delete State", systemImage: "trash")
            }
            .disabled(presetStore.presets.count <= 1)

            Button {
                config.activePreset = presetStore.selected
            } label: {
                Label("Reset Defaults", systemImage: "arrow.counterclockwise")
            }
        }

        blendSummary(for: preset.emotion, acoustics: preset.acoustics)
    }

    private func presetSlider(
        _ title: String,
        value: Binding<Double>,
        range: ClosedRange<Double>,
        step: Double = 0.05
    ) -> some View {
        HStack {
            Text(title)
                .frame(width: 70, alignment: .leading)
            Slider(value: value, in: range, step: step)
            Text(value.wrappedValue, format: .number.precision(.fractionLength(2)))
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .frame(width: 45, alignment: .trailing)
        }
    }
    
    @ViewBuilder
    private func directorModelSection(store: DirectorModelStore) -> some View {
        Section("Director model") {
            if store.models.isEmpty {
                Text("No models registered — add one with ＋ in the toolbar.")
                    .foregroundStyle(.secondary)
            } else {
                Picker("Model", selection: modelSelectionBinding(store: store)) {
                    ForEach(store.models) { model in
                        Text(model.displayName + (model.isAvailable ? "" : "  (missing)"))
                            .tag(model.id)
                    }
                }
                .pickerStyle(.menu)
            }
        }
    }

    /// Non-optional binding so the List picker always has a valid tag on macOS.
    private func modelSelectionBinding(store: DirectorModelStore) -> Binding<String> {
        Binding(
            get: {
                if let id = store.selectedID, store.models.contains(where: { $0.id == id }) {
                    return id
                }
                return store.models.first?.id ?? ""
            },
            set: { newID in
                guard !newID.isEmpty else { return }
                store.selectedID = newID
            }
        )
    }

    @ViewBuilder
    private var segmentsSection: some View {
        Section {
            if runner.segments.isEmpty && !runner.isRunning {
                ContentUnavailableView("No segments yet", systemImage: "waveform")
                    .listRowBackground(Color.clear)
            } else {
                ForEach(Array(runner.segments.enumerated()), id: \.offset) { _, segment in
                    SegmentRow(
                        segment: segment,
                        canSpeak: runner.canSpeak,
                        isBusy: runner.isRunning || runner.isSpeaking,
                        config: config,
                        speak: {
                            Task { await runner.speakPassage(segment.text, config: config, model: store.selected) }
                        },
                        feedback: {
                            feedbackSegment = segment
                        }
                    )
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            deletePassage(segment.text)
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .contextMenu {
                        Button(role: .destructive) {
                            deletePassage(segment.text)
                        } label: {
                            Label("Delete Passage", systemImage: "trash")
                        }
                    }
                }
            }
        } header: {
            HStack {
                Text("Sample passages")
                if runner.isRunning {
                    ProgressView()
                        .controlSize(.small)
                        .padding(.leading, 4)
                }
                Spacer()
                Button {
                    showAddPassageSheet = true
                } label: {
                    Label("Add Passage", systemImage: "plus")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.blue)
                .disabled(runner.isRunning || runner.isSpeaking)
            }
        } footer: {
            Text(footerText)
        }
    }

    @ToolbarContentBuilder
    private func toolbarContent(store: DirectorModelStore, config: AuditionConfiguration) -> some ToolbarContent {
        if runner.canSpeak {
            ToolbarItem(placement: .primaryAction) {
                modelMenu
            }
        }

        if runner.canSpeak {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    Task { await runner.speak(config: config, model: store.selected) }
                } label: {
                    if runner.isSpeaking {
                        ProgressView()
                    } else {
                        Label("Speak", systemImage: "speaker.wave.2.fill")
                    }
                }
                .disabled(
                    runner.isSpeaking
                    || runner.isRunning
                    || (config.canUseMlx && store.selected?.isAvailable != true)
                )
            }
        }

        ToolbarItem(placement: .primaryAction) {
            Button {
                Task { await runner.stopActive() }
            } label: {
                Label("Stop", systemImage: "stop.fill")
            }
            .disabled(!runner.isRunning && !runner.isSpeaking)
        }
    }

    /// Director-model menu: restored from the pre-split app. The selector is
    /// available whenever Director/Speak is available, regardless of emotion mode.
    private var modelMenu: some View {
        let selection = Binding(get: { store.selectedID }, set: { store.selectedID = $0 })
        return Menu {
            if !store.models.isEmpty {
                Picker("Director model", selection: selection) {
                    ForEach(store.models) { model in
                        Text(model.displayName + (model.isAvailable ? "" : "  (missing)"))
                            .tag(Optional(model.id))
                    }
                }
                Divider()
            }
            Button { showImporter = true } label: {
                Label("Add Model…", systemImage: "plus")
            }
            if let selected = store.selected {
                Button(role: .destructive) { store.remove(selected) } label: {
                    Label("Remove “\(selected.displayName)”", systemImage: "trash")
                }
            }
        } label: {
            Label(store.selected?.displayName ?? "Director model", systemImage: "cpu")
        }
        .disabled(runner.isSpeaking)
    }

    private func vadSlider(_ title: String, value: Binding<Double>, range: ClosedRange<Double>) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title)
                Spacer()
                Text(value.wrappedValue, format: .number.precision(.fractionLength(2)))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
            Slider(value: value, in: range, step: 0.05)
        }
    }

    private func blendSummary(for emotion: EmotionVector, acoustics: ProsodyAcoustics? = nil) -> some View {
        let speed = acoustics?.speedMultiplier ?? AcousticMatrix.speed(for: emotion)
        let gain = acoustics?.gainMultiplier ?? AcousticMatrix.gain(for: emotion)
        let pitch = acoustics?.pitch ?? AcousticMatrix.pitch(for: emotion)
        return VStack(alignment: .leading, spacing: 4) {
            Text(String(format: "V %.2f  A %.2f  T %.2f", emotion.valence, emotion.arousal, emotion.tension))
                .font(.caption.monospaced())
            Text(String(format: "Speed ×%.2f | Vol ×%.2f | Pitch %.1f", speed, gain, pitch))
                .font(.caption2)
                .foregroundStyle(.secondary)
            if let cp = acoustics?.castingProfile {
                Text(String(format: "Age: %.2f  Masc: %.2f  Strain: %.2f", cp.ageProfile, cp.masculinity, cp.strainOrRasp))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
    }

    private func deletePassage(_ text: String) {
        passageStore.delete(text)
        Task {
            await runner.preview(config: config, model: store.selected)
        }
    }

    private func copyPresetToClipboard(_ preset: AuditionPreset) {
        let summary = """
        Preset: \(preset.name)
        Valence: \(String(format: "%.2f", preset.valence))
        Arousal: \(String(format: "%.2f", preset.arousal))
        Tension: \(String(format: "%.2f", preset.tension))
        Speed: \(String(format: "%.2f", preset.speed))
        Volume: \(String(format: "%.2f", preset.volume))
        Pitch: \(String(format: "%.2f", preset.pitch))
        Age Profile: \(String(format: "%.2f", preset.ageProfile))
        Masculinity: \(String(format: "%.2f", preset.masculinity))
        Vocal Energy: \(String(format: "%.2f", preset.vocalEnergy))
        Strain/Rasp: \(String(format: "%.2f", preset.strainOrRasp))
        """
        #if canImport(AppKit)
        let pasteboard = NSPasteboard.general
        pasteboard.declareTypes([.string], owner: nil)
        pasteboard.setString(summary, forType: .string)
        #elseif canImport(UIKit)
        UIPasteboard.general.string = summary
        #endif
    }

    private func globalKnobSlider(
        _ title: String,
        value: Binding<Double>,
        range: ClosedRange<Double>,
        step: Double = 0.05
    ) -> some View {
        HStack {
            Text(title)
                .frame(width: 170, alignment: .leading)
            Slider(value: value, in: range, step: step)
            Text(value.wrappedValue, format: .number.precision(.fractionLength(2)))
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .frame(width: 45, alignment: .trailing)
        }
    }

    private var globalKnobsSection: some View {
        Section {
            globalKnobSlider(
                "Expressiveness",
                value: Binding(
                    get: {
                        if config.emotionMode == .preset {
                            let maxCoord = max(
                                abs(config.activePreset.valence),
                                abs(config.activePreset.arousal),
                                abs(config.activePreset.tension)
                            )
                            let maxEffective = maxCoord > 0 ? (1.0 / maxCoord) : 10.0
                            return min(config.globalConfig.expressiveness, maxEffective)
                        } else {
                            return config.globalConfig.expressiveness
                        }
                    },
                    set: { newValue in
                        if config.emotionMode == .preset {
                            let maxCoord = max(
                                abs(config.activePreset.valence),
                                abs(config.activePreset.arousal),
                                abs(config.activePreset.tension)
                            )
                            let maxEffective = maxCoord > 0 ? (1.0 / maxCoord) : 10.0
                            config.globalConfig.expressiveness = min(newValue, maxEffective)
                        } else {
                            config.globalConfig.expressiveness = newValue
                        }
                    }
                ),
                range: 0.5...10.0
            )

            globalKnobSlider(
                "Voice Blend Sigma",
                value: Binding(
                    get: { min(max(config.globalConfig.blendSigma, 0.05), 1.0) },
                    set: { config.globalConfig.blendSigma = $0 }
                ),
                range: 0.05...1.0
            )

            globalKnobSlider(
                "Min Voice Blend %",
                value: Binding(
                    get: { min(max(config.globalConfig.blendMinimumFraction * 100, 0.0), 30.0) },
                    set: { config.globalConfig.blendMinimumFraction = $0 / 100.0 }
                ),
                range: 0.0...30.0,
                step: 1.0
            )

            globalKnobSlider(
                "Blend Proximity Threshold",
                value: Binding(
                    get: { min(max(config.globalConfig.blendProximityThreshold, 0.0), 0.5) },
                    set: { config.globalConfig.blendProximityThreshold = $0 }
                ),
                range: 0.0...0.5,
                step: 0.01
            )

            globalKnobSlider(
                "Speed Arousal Gain",
                value: Binding(
                    get: { min(max(config.globalConfig.speedArousalGain, 0.0), 1.0) },
                    set: { config.globalConfig.speedArousalGain = $0 }
                ),
                range: 0.0...1.0
            )

            globalKnobSlider(
                "Speed Tension Gain",
                value: Binding(
                    get: { min(max(config.globalConfig.speedTensionGain, 0.0), 1.0) },
                    set: { config.globalConfig.speedTensionGain = $0 }
                ),
                range: 0.0...1.0
            )

            globalKnobSlider(
                "Speed Valence Gain",
                value: Binding(
                    get: { min(max(config.globalConfig.speedValenceGain, 0.0), 0.75) },
                    set: { config.globalConfig.speedValenceGain = $0 }
                ),
                range: 0.0...0.75
            )

            globalKnobSlider(
                "Speed Min Limit",
                value: Binding(
                    get: { min(max(config.globalConfig.speedMin, 0.1), 1.2) },
                    set: { config.globalConfig.speedMin = min($0, config.globalConfig.speedMax) }
                ),
                range: 0.1...1.2
            )

            globalKnobSlider(
                "Speed Max Limit",
                value: Binding(
                    get: { min(max(config.globalConfig.speedMax, 1.0), 4.0) },
                    set: { config.globalConfig.speedMax = max($0, config.globalConfig.speedMin) }
                ),
                range: 1.0...4.0
            )

            globalKnobSlider(
                "Volume Arousal Gain",
                value: Binding(
                    get: { min(max(config.globalConfig.gainArousalGain, 0.0), 1.0) },
                    set: { config.globalConfig.gainArousalGain = $0 }
                ),
                range: 0.0...1.0
            )

            globalKnobSlider(
                "Volume Valence Gain",
                value: Binding(
                    get: { min(max(config.globalConfig.gainValenceGain, 0.0), 0.75) },
                    set: { config.globalConfig.gainValenceGain = $0 }
                ),
                range: 0.0...0.75
            )

            globalKnobSlider(
                "Volume Min Limit",
                value: Binding(
                    get: { min(max(config.globalConfig.gainMin, 0.1), 1.2) },
                    set: { config.globalConfig.gainMin = min($0, config.globalConfig.gainMax) }
                ),
                range: 0.1...1.2
            )

            globalKnobSlider(
                "Volume Max Limit",
                value: Binding(
                    get: { min(max(config.globalConfig.gainMax, 1.0), 4.0) },
                    set: { config.globalConfig.gainMax = max($0, config.globalConfig.gainMin) }
                ),
                range: 1.0...4.0
            )

            globalKnobSlider(
                "Pause sentence (sec)",
                value: Binding(
                    get: { min(max(config.globalConfig.pauseSentence, 0.0), 2.0) },
                    set: { config.globalConfig.pauseSentence = $0 }
                ),
                range: 0.0...2.0
            )

            globalKnobSlider(
                "Pause clause (sec)",
                value: Binding(
                    get: { min(max(config.globalConfig.pauseClause, -0.5), 2.0) },
                    set: { config.globalConfig.pauseClause = $0 }
                ),
                range: -0.5...2.0
            )

            HStack(spacing: 16) {
                Button {
                    let encoder = JSONEncoder()
                    encoder.outputFormatting = .prettyPrinted
                    if let data = try? encoder.encode(config.globalConfig),
                       let jsonString = String(data: data, encoding: .utf8) {
                        #if canImport(AppKit)
                        let pasteboard = NSPasteboard.general
                        pasteboard.declareTypes([.string], owner: nil)
                        pasteboard.setString(jsonString, forType: .string)
                        #elseif canImport(UIKit)
                        UIPasteboard.general.string = jsonString
                        #endif
                        
                        withAnimation {
                            didCopyConfig = true
                        }
                        Task {
                            try? await Task.sleep(for: .seconds(1.5))
                            withAnimation {
                                didCopyConfig = false
                            }
                        }
                    }
                } label: {
                    if didCopyConfig {
                        Label("Copied!", systemImage: "checkmark")
                            .foregroundStyle(.green)
                    } else {
                        Label("Copy Config", systemImage: "doc.on.doc")
                    }
                }
                .buttonStyle(.borderless)

                Button {
                    CachingDirectorEngine.clearCache()
                    ProsodiaConfigManager.shared.load()
                    config.globalConfig = ProsodiaConfigManager.shared.config
                } label: {
                    Label("Reload Config", systemImage: "arrow.clockwise")
                }
                .buttonStyle(.borderless)

                Spacer()

                Button(role: .destructive) {
                    CachingDirectorEngine.clearCache()
                    let newDefault = ProsodiaConfig()
                    config.globalConfig = newDefault
                    ProsodiaConfigManager.shared.config = newDefault
                } label: {
                    Label("Reset Defaults", systemImage: "arrow.counterclockwise")
                }
                .buttonStyle(.borderless)
            }
            .padding(.vertical, 4)
        } header: {
            Text("Global Acoustic & Blending Knobs")
        }
    }
}

struct IdentifiableSegment: Identifiable {
    let id = UUID()
    let segment: StubVocalActor.RenderedSegment
}

struct FeedbackSheet: View {
    let segment: StubVocalActor.RenderedSegment
    let store: DirectorModelStore
    let config: AuditionConfiguration
    let onDismiss: () -> Void

    @State private var rating = 4
    @State private var comment = ""
    @State private var didSave = false
    
    private var editorBackgroundColor: Color {
        #if os(macOS)
        return Color(NSColor.controlBackgroundColor)
        #else
        return Color(UIColor.secondarySystemBackground)
        #endif
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Passage Details
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Passage Details")
                            .font(.subheadline.weight(.bold))
                            .foregroundStyle(.secondary)
                        
                        Text(segment.text)
                            .font(.body)
                            .foregroundStyle(.primary)
                            .textSelection(.enabled)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(editorBackgroundColor.opacity(0.6))
                            .cornerRadius(6)
                            .overlay(
                                RoundedRectangle(cornerRadius: 6)
                                    .stroke(Color.secondary.opacity(0.15), lineWidth: 1)
                            )
                    }
                    
                    // Acoustics Details (Grid)
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Acoustics & Voice Blend")
                            .font(.subheadline.weight(.bold))
                            .foregroundStyle(.secondary)
                        
                        Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 10) {
                            GridRow {
                                Text("Parameters")
                                    .foregroundStyle(.secondary)
                                    .font(.body.weight(.medium))
                                let e = segment.directive.emotion
                                Text(String(format: "V %.2f  A %.2f  T %.2f", e.valence, e.arousal, e.tension))
                                    .font(.body.monospaced())
                            }
                            GridRow {
                                Text("Modulation")
                                    .foregroundStyle(.secondary)
                                    .font(.body.weight(.medium))
                                Text(String(format: "Speed ×%.2f | Vol ×%.2f", segment.speedMultiplier, segment.directive.acoustics?.gainMultiplier ?? AcousticMatrix.gain(for: segment.directive.emotion)))
                            }
                            GridRow {
                                Text("Casting Profile")
                                    .foregroundStyle(.secondary)
                                    .font(.body.weight(.medium))
                                if let cp = segment.directive.acoustics?.castingProfile {
                                    Text(String(format: "Age: %.2f | Masc: %.2f | Strain: %.2f", cp.ageProfile, cp.masculinity, cp.strainOrRasp))
                                        .font(.body)
                                } else {
                                    Text("Default")
                                        .font(.body)
                                }
                            }
                        }
                        .padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(editorBackgroundColor.opacity(0.4))
                        .cornerRadius(6)
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .stroke(Color.secondary.opacity(0.1), lineWidth: 1)
                        )
                    }
                    
                    // Subjective Audition Review
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Subjective Audition Review")
                            .font(.subheadline.weight(.bold))
                            .foregroundStyle(.secondary)
                        
                        HStack {
                            Text("Rating")
                                .font(.body.weight(.medium))
                            Spacer()
                            Picker("Rating", selection: $rating) {
                                Text("⭐ (1/5 - Terrible)").tag(1)
                                Text("⭐⭐ (2/5 - Poor)").tag(2)
                                Text("⭐⭐⭐ (3/5 - Fair)").tag(3)
                                Text("⭐⭐⭐⭐ (4/5 - Good)").tag(4)
                                Text("⭐⭐⭐⭐⭐ (5/5 - Perfect!)").tag(5)
                            }
                            .labelsHidden()
                            .pickerStyle(.menu)
                        }
                        
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Audition Notes")
                                .font(.caption.weight(.medium))
                                .foregroundStyle(.secondary)
                            TextEditor(text: $comment)
                                .font(.body)
                                .frame(height: 100)
                                .padding(4)
                                .background(editorBackgroundColor)
                                .cornerRadius(4)
                                .overlay(
                                    RoundedRectangle(cornerRadius: 4)
                                        .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                                )
                                .overlay(alignment: .topLeading) {
                                    if comment.isEmpty {
                                        Text("Add notes about emotional conveyance, transition quality, pacing, or robotic/unnatural voice blend behaviors...")
                                            .foregroundStyle(.tertiary)
                                            .font(.body)
                                            .padding(.top, 10)
                                            .padding(.leading, 8)
                                            .allowsHitTesting(false)
                                    }
                                }
                        }
                    }
                    
                    if didSave {
                        HStack(spacing: 8) {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(.green)
                            Text("Saved to TuningFeedback.md & Copied to clipboard!")
                                .foregroundStyle(.green)
                                .font(.subheadline.weight(.semibold))
                        }
                        .frame(maxWidth: .infinity, alignment: .center)
                        .padding(.vertical, 8)
                    }
                }
                .padding(20)
            }
            .navigationTitle("Audition Feedback")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { onDismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save & Log") {
                        let blend: String
                        if let cp = segment.directive.acoustics?.castingProfile {
                            blend = String(format: "Age: %.2f, Masc: %.2f, Strain: %.2f", cp.ageProfile, cp.masculinity, cp.strainOrRasp)
                        } else {
                            blend = "Default"
                        }

                        TuningFeedbackLogger.log(
                            text: segment.text,
                            rating: rating,
                            emotion: segment.directive.emotion,
                            speed: segment.speedMultiplier,
                            volume: segment.directive.acoustics?.gainMultiplier ?? AcousticMatrix.gain(for: segment.directive.emotion),
                            voiceBlend: blend,
                            comment: comment,
                            spans: segment.spans,
                            mode: config.emotionMode.rawValue,
                            modelName: config.emotionMode == .director ? store.selected?.displayName : nil,
                            globalConfig: config.globalConfig
                        )

                        didSave = true
                        Task {
                            try? await Task.sleep(for: .seconds(1.2))
                            onDismiss()
                        }
                    }
                }
            }
        }
        .frame(minWidth: 500, minHeight: 480)
    }
}

private struct SegmentRow: View {
    let segment: StubVocalActor.RenderedSegment
    let canSpeak: Bool
    let isBusy: Bool
    let config: AuditionConfiguration
    let speak: () -> Void
    let feedback: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text(segment.text)
                    .font(.body)
                    .textSelection(.enabled)
                Spacer()
                if canSpeak {
                    HStack(spacing: 12) {
                        Button {
                            feedback()
                        } label: {
                            Label("Feedback", systemImage: "bubble.left.and.bubble.right.fill")
                        }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .disabled(isBusy)

                        Button {
                            speak()
                        } label: {
                            Label("Speak Passage", systemImage: "play.fill")
                        }
                        .labelStyle(.iconOnly)
                        .disabled(isBusy)
                    }
                }
            }

            HStack(spacing: 8) {
                Text(vadDescription)
                    .font(.caption.weight(.semibold).monospaced())
                    .padding(.horizontal, 8)
                    .padding(.vertical, 2)
                    .background(.tint.opacity(0.15), in: Capsule())

                Spacer()

                Text("× \(segment.speedMultiplier, format: .number.precision(.fractionLength(3)))")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }

            Text(blendDescription)
                .font(.caption2)
                .foregroundStyle(.tertiary)

            if segment.spans.count > 1 {
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(Array(segment.spans.enumerated()), id: \.offset) { _, span in
                        SpanRow(span: span)
                    }
                }
                .padding(.top, 2)
            }
        }
        .padding(.vertical, 4)
    }

    private var vadDescription: String {
        let e = segment.directive.emotion
        return String(format: "V %.2f  A %.2f  T %.2f", e.valence, e.arousal, e.tension)
    }

    private var blendDescription: String {
        guard let cp = segment.directive.acoustics?.castingProfile else { return "Auto Voice" }
        return String(format: "Age: %.2f, Masc: %.2f, Strain: %.2f", cp.ageProfile, cp.masculinity, cp.strainOrRasp)
    }
}

private struct SpanRow: View {
    let span: ProsodySpan

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: "text.append")
                .font(.caption2)
                .foregroundStyle(.tertiary)
            Text("“\(span.text)”")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer(minLength: 4)
            Text(modulation)
                .font(.caption2.monospaced())
                .foregroundStyle(.tertiary)
        }
    }

    private var modulation: String {
        var parts: [String] = []
        if span.leadingPause > 0 {
            parts.append(String(format: "⏸%.2fs", span.leadingPause))
        }
        parts.append(String(format: "×%.2f", span.speed))
        parts.append(String(format: "vol %d%%", Int((span.gain * 100).rounded())))
        return parts.joined(separator: "  ")
    }
}

struct AddPassageSheet: View {
    let store: SamplePassageStore
    let runner: ProductionRunner
    let config: AuditionConfiguration
    let model: DirectorModel?
    
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @FocusState private var isTextFieldFocused: Bool

    private var editorBackgroundColor: Color {
        #if os(macOS)
        return Color(NSColor.controlBackgroundColor)
        #else
        return Color(UIColor.secondarySystemBackground)
        #endif
    }
    
    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text("Passage Text")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                
                TextEditor(text: $text)
                    .font(.body)
                    .focused($isTextFieldFocused)
                    .padding(.all, 8)
                    .background(editorBackgroundColor)
                    .cornerRadius(8)
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                    )
                    .frame(minHeight: 120)
                
                Text("Enter a new sentence or paragraph to audition custom prose at runtime without rebuilding.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(nil)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                
                Spacer()
            }
            .padding(.all, 20)
            .navigationTitle("Add Audition Passage")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        store.add(text)
                        Task {
                            await runner.preview(config: config, model: model)
                        }
                        dismiss()
                    }
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .onAppear {
                isTextFieldFocused = true
            }
        }
        .frame(minWidth: 450, minHeight: 280)
    }
}

#Preview {
    TunerContentView()
}
