//
//  TunerDemo.swift
//  ProsodiaTuner
//
//  End-to-end exercise of the ProsodiaStage pipeline for tuning and A/B work.
//

import Foundation
import Observation
import Kit
import Stage

// MARK: - ProductionRunner

@MainActor
@Observable
final class ProductionRunner {
    private(set) var segments: [StubVocalActor.RenderedSegment] = []
    private(set) var isRunning = false
    private(set) var isSpeaking = false
    private(set) var activeModel: DirectorModel?
    private var activePlaybackController: (any PlaybackController)?
    private var activePreviewController: (any PlaybackController)?
    private var cachedActor: (any Stage.VocalActor)?
    
    private var cachedDirector: (any Stage.DirectorInference)?
    private var cachedDirectorModel: DirectorModel?
    private var cachedDirectorEmotionMode: EmotionSourceMode?
    private var cachedDirectorNarrationMode: Stage.NarrationMode?

    /// Resolves the real StyleTTS2 actor, or `nil` when its model files are missing.
    ///
    /// Returns `nil` rather than falling back to a placeholder renderer: a missing
    /// production model must surface as a disabled "Speak" affordance (see ``canSpeak``),
    /// never as the stub's audible 440 Hz test tone masquerading as synthesized speech.
    /// Only a genuinely resolved actor is cached, so dropping the model into `Models/`
    /// and re-triggering Speak picks it up without an app relaunch.
    private func getActor() -> (any Stage.VocalActor)? {
        if let cached = cachedActor {
            return cached
        }
        let modelFile = Self.resolvedModelPath
        let voiceDir = Self.resolvedVoiceDirectory

        guard let resolved = VocalActorRegistry.shared.makeActor(for: modelFile, voiceDirectoryURL: voiceDir) else {
            return nil
        }
        cachedActor = resolved
        return resolved
    }

    private func getDirector(config: AuditionConfiguration, model: DirectorModel?) -> any Stage.DirectorInference {
        if let cached = cachedDirector,
           cachedDirectorModel == model,
           cachedDirectorEmotionMode == config.emotionMode,
           cachedDirectorNarrationMode == config.mlxNarrationMode {
            return cached
        }
        
        let rawDirector = config.makeDirector(model: model)
        let director: any Stage.DirectorInference
        if config.emotionMode == .director, let model = model {
            director = CachingDirectorEngine(base: rawDirector, modelId: model.id, narrationMode: config.mlxNarrationMode)
        } else {
            director = rawDirector
        }
        
        cachedDirector = director
        cachedDirectorModel = model
        cachedDirectorEmotionMode = config.emotionMode
        cachedDirectorNarrationMode = config.mlxNarrationMode
        return director
    }

    func reclaimDirectorMemory() async {
        if let director = cachedDirector {
            await director.reclaimMemory()
            cachedDirector = nil
            cachedDirectorModel = nil
            cachedDirectorEmotionMode = nil
            cachedDirectorNarrationMode = nil
        }
    }

    func reclaimMemory() async {
        await reclaimDirectorMemory()
        if let actor = cachedActor {
            await actor.reclaimMemory()
            cachedActor = nil
        }
    }

    /// Refreshes segment metadata (VAD, speed, voice blend) using the stub Actor.
    func preview(config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isRunning, !isSpeaking else { return }
        isRunning = true
        defer { isRunning = false }

        let document = InMemoryBookDocument(chapters: SamplePassageStore.shared.passages)
        let director = getDirector(config: config, model: model)
        let renderer = StubVocalActor(isSilent: true)

        let controller = await Stage.StageCoordinator.run(
            document: document,
            director: director,
            actor: renderer,
            lookahead: 5
        )
        activePreviewController = controller
        await controller.awaitFinished()
        activePreviewController = nil
        segments = await renderer.snapshot()
    }

    // MARK: - Real audio (macOS, model files required)

    nonisolated static var projectRoot: URL {
        #if os(macOS)
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // TunerDemo.swift parent (ProsodiaTuner)
            .deletingLastPathComponent() // ProsodiaTuner (outer)
            .deletingLastPathComponent() // apps (outer)
            .deletingLastPathComponent() // Project Root (Prosodia)
        #else
        URL(fileURLWithPath: "/dev/null")
        #endif
    }

    nonisolated static var modelsBase: URL {
        projectRoot.appendingPathComponent("Models")
    }

    nonisolated static var resolvedModelPath: URL {
        modelsBase.appendingPathComponent("styletts2_lite.tflite")
    }

    nonisolated static var resolvedVoiceDirectory: URL {
        modelsBase
    }

    /// Whether a real StyleTTS2 actor model is present and resolvable.
    ///
    /// Gates every "Speak" control. When false, the harness still previews VAD/voice-blend
    /// metadata via the silent stub, and the section footer tells the user to add the model
    /// under `Models/` — instead of emitting a misleading placeholder tone.
    var canSpeak: Bool {
        VocalActorRegistry.shared.canMakeActor(for: Self.resolvedModelPath)
    }

    /// Synthesizes sample sentences with the configured Director and Actor.
    func speak(config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isSpeaking, canSpeak else { return }
        if config.canUseMlx {
            guard let model, model.isAvailable else { return }
        }
        guard let actor = getActor() else { return }

        isSpeaking = true
        activeModel = model
        defer {
            isSpeaking = false
            Task {
                await preview(config: config, model: model)
            }
        }

        let document = InMemoryBookDocument(chapters: SamplePassageStore.shared.passages)
        let director = getDirector(config: config, model: model)
        await actor.setBaseVoice(config.emotionMode == .director ? config.mlxBaseVoice : nil)

        let controller = await Stage.StageCoordinator.run(
            document: document,
            director: director,
            actor: actor,
            lookahead: 5
        )
        activePlaybackController = controller
        await controller.awaitFinished()
        activePlaybackController = nil
    }

    func speakPassage(_ text: String, config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isSpeaking, canSpeak else { return }
        if config.canUseMlx {
            guard let model, model.isAvailable else { return }
        }
        guard let actor = getActor() else { return }

        isSpeaking = true
        activeModel = model
        defer {
            isSpeaking = false
            Task {
                await preview(config: config, model: model)
            }
        }

        let document = InMemoryBookDocument(chapters: [text])
        let director = getDirector(config: config, model: model)
        await actor.setBaseVoice(config.emotionMode == .director ? config.mlxBaseVoice : nil)

        let controller = await Stage.StageCoordinator.run(
            document: document,
            director: director,
            actor: actor,
            lookahead: 1
        )
        activePlaybackController = controller
        await controller.awaitFinished()
        activePlaybackController = nil
    }

    func stopActive() async {
        await activePlaybackController?.stop()
        activePlaybackController = nil
        await activePreviewController?.stop()
        activePreviewController = nil
        
        await reclaimMemory()
    }
}

// MARK: - Director model selection (A/B evaluation harness)

struct DirectorModel: Codable, Identifiable, Hashable, Sendable {
    var name: String
    var path: String

    var id: String { path }
    var directory: URL { URL(fileURLWithPath: path) }
    var displayName: String { name }

    var isAvailable: Bool {
        let ext = directory.pathExtension
        let isFile = ext == "litertlm" || path.hasSuffix(".litertlm")
        if isFile {
            return FileManager.default.fileExists(atPath: path)
        }
        return FileManager.default.fileExists(atPath: directory.appendingPathComponent("config.json").path)
    }

    var menuTitle: String {
        displayName + (isAvailable ? "" : "  (missing)")
    }
}

@MainActor
@Observable
final class DirectorModelStore {
    private(set) var models: [DirectorModel]
    var selectedID: String? {
        didSet { UserDefaults.standard.set(selectedID, forKey: Self.selectedKey) }
    }

    private static let modelsKey = "harnessDirectorModels"
    private static let selectedKey = "harnessSelectedDirectorModel"

    init() {
        var loadedModels = Self.load()
        var pathChanged = false
        for i in 0..<loadedModels.count {
            if loadedModels[i].path.contains("apps/Models") {
                loadedModels[i].path = loadedModels[i].path.replacingOccurrences(of: "apps/Models", with: "Models")
                pathChanged = true
            }
        }
        models = loadedModels
        selectedID = UserDefaults.standard.string(forKey: Self.selectedKey)
        if models.isEmpty { seedDefaults() }
        else if pathChanged { save() }
        reconcileSelection()
    }

    var selected: DirectorModel? {
        models.first { $0.id == selectedID } ?? models.first
    }

    func select(_ model: DirectorModel) {
        selectedID = model.id
    }

    /// Keeps ``selectedID`` aligned with ``models`` after load, seed, or remove.
    func reconcileSelection() {
        guard let id = selectedID, models.contains(where: { $0.id == id }) else {
            selectedID = models.first?.id
            return
        }
    }

    func add(directory url: URL) {
        let path = url.standardizedFileURL.path
        guard !models.contains(where: { $0.path == path }) else {
            selectedID = path
            return
        }
        let model = DirectorModel(name: url.lastPathComponent, path: path)
        models.append(model)
        models.sort { $0.name < $1.name }
        selectedID = model.id
        save()
        reconcileSelection()
    }

    func remove(_ model: DirectorModel) {
        models.removeAll { $0.id == model.id }
        reconcileSelection()
        save()
    }

    private func seedDefaults() {
        let base = ProductionRunner.modelsBase
        
        // Seed LiteRT-LM defaults: Gemma 4 (E2B is first/default), the only director backend.
        for file in ["gemma-4-E2B-it.litertlm", "gemma-4-E4B-it.litertlm"] {
            let url = base.appendingPathComponent(file)
            if FileManager.default.fileExists(atPath: url.path) {
                models.append(DirectorModel(name: file, path: url.path))
            }
        }

        if !models.isEmpty { save() }
    }

    private func save() {
        if let data = try? JSONEncoder().encode(models) {
            UserDefaults.standard.set(data, forKey: Self.modelsKey)
        }
    }

    private static func load() -> [DirectorModel] {
        guard let data = UserDefaults.standard.data(forKey: modelsKey),
              let decoded = try? JSONDecoder().decode([DirectorModel].self, from: data)
        else { return [] }
        return decoded
    }
}

// MARK: - CachingDirectorEngine

/// A wrapper around a `DirectorInference` that caches annotations in-memory
/// by model ID and passage text, preventing redundant LLM inference when adjusting
/// acoustic and voice blending sliders.
actor CachingDirectorEngine: Stage.DirectorInference {
    private let base: any Stage.DirectorInference
    private let modelId: String
    private var narrationMode: Stage.NarrationMode = .solo
    
    // In-memory cache shared across instances.
    private static var cache: [String: String] = [:]
    
    init(base: any Stage.DirectorInference, modelId: String, narrationMode: Stage.NarrationMode = .solo) {
        self.base = base
        self.modelId = modelId
        self.narrationMode = narrationMode
    }

    func setNarrationMode(_ mode: Stage.NarrationMode) async {
        self.narrationMode = mode
        await base.setNarrationMode(mode)
    }
    
    func reclaimMemory() async {
        await base.reclaimMemory()
    }
    
    func annotate(chapterStream: AsyncStream<String>) async -> AsyncStream<String> {
        AsyncStream { continuation in
            Task {
                for await passage in chapterStream {
                    let cacheKey = "\(modelId)::\(narrationMode.rawValue)::\(passage)"
                    if let cached = Self.cache[cacheKey] {
                        continuation.yield(cached)
                    } else {
                        // Pass single passage to base to annotate
                        let singleStream = AsyncStream<String> { c in
                            c.yield(passage)
                            c.finish()
                        }
                        let resultStream = await base.annotate(chapterStream: singleStream)
                        var result = ""
                        for await annotated in resultStream {
                            result = annotated
                        }
                        if !result.isEmpty {
                            Self.cache[cacheKey] = result
                        }
                        continuation.yield(result)
                    }
                }
                continuation.finish()
            }
        }
    }

    nonisolated func annotate(passage: String) -> String {
        let semaphore = DispatchSemaphore(value: 0)
        var result = ""
        Task {
            result = await self.annotateSingle(passage: passage)
            semaphore.signal()
        }
        semaphore.wait()
        return result
    }

    private func annotateSingle(passage: String) async -> String {
        let cacheKey = "\(modelId)::\(narrationMode.rawValue)::\(passage)"
        if let cached = Self.cache[cacheKey] {
            return cached
        } else {
            let result = await base.annotate(passage: passage)
            if !result.isEmpty {
                Self.cache[cacheKey] = result
            }
            return result
        }
    }
    
    static func clearCache() {
        cache.removeAll()
    }
}
