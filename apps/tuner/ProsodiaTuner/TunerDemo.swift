//
//  TunerDemo.swift
//  ProsodiaTuner
//
//  End-to-end exercise of the ProsodiaStage pipeline for tuning and A/B work.
//

import Foundation
import Observation
import Kit
#if canImport(MLX)
import MLX
#endif

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
    private var cachedActor: (any VocalActor)?
    
    private var cachedDirector: (any DirectorInference)?
    private var cachedDirectorModel: DirectorModel?
    private var cachedDirectorEmotionMode: EmotionSourceMode?
    private var cachedDirectorNarrationMode: NarrationMode?

    private func getActor() -> any VocalActor {
        if let cached = cachedActor {
            return cached
        }
        let actor: any VocalActor
        let modelFile = Self.resolvedModelPath
        let voiceDir = Self.resolvedVoiceDirectory
        
        if let resolved = VocalActorRegistry.shared.makeActor(for: modelFile, voiceDirectoryURL: voiceDir) {
            actor = resolved
        } else {
            actor = StubVocalActor()
        }
        cachedActor = actor
        return actor
    }

    private func getDirector(config: AuditionConfiguration, model: DirectorModel?) -> any DirectorInference {
        if let cached = cachedDirector,
           cachedDirectorModel == model,
           cachedDirectorEmotionMode == config.emotionMode,
           cachedDirectorNarrationMode == config.mlxNarrationMode {
            return cached
        }
        
        let rawDirector = config.makeDirector(model: model)
        let director: any DirectorInference
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
        #if canImport(MLX)
        MLX.Memory.clearCache()
        #endif
    }

    func reclaimMemory() async {
        await reclaimDirectorMemory()
        if let actor = cachedActor {
            await actor.reclaimMemory()
            cachedActor = nil
        }
        #if canImport(MLX)
        MLX.Memory.clearCache()
        #endif
    }

    /// Refreshes segment metadata (VAD, speed, voice blend) using the stub Actor.
    func preview(config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isRunning, !isSpeaking else { return }
        isRunning = true
        defer { isRunning = false }

        let document = InMemoryBookDocument(chapters: SamplePassageStore.shared.passages)
        let director = getDirector(config: config, model: model)
        let renderer = StubVocalActor()

        let controller = await StageCoordinator.run(
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
            .deletingLastPathComponent() // Project Root
        #else
        URL(fileURLWithPath: "/dev/null")
        #endif
    }

    nonisolated static var modelsBase: URL {
        projectRoot.appendingPathComponent("Models")
    }

    nonisolated static var mlxDirectory: URL { projectRoot.appendingPathComponent("StyleTTS2FineTune") }
    nonisolated static var mlxModelFile: URL { mlxDirectory.appendingPathComponent("StyleTTS2/Models/LibriTTS/epochs_2nd.pth") }

    nonisolated static var resolvedModelPath: URL {
        if FileManager.default.fileExists(atPath: mlxModelFile.path) {
            return mlxModelFile
        }
        let coreMlDir = modelsBase.appendingPathComponent("CoreML")
        let coreMlModel = coreMlDir.appendingPathComponent("kokoro_5s.mlmodelc")
        if FileManager.default.fileExists(atPath: coreMlModel.path) {
            return coreMlDir
        }
        return mlxModelFile
    }

    nonisolated static var resolvedVoiceDirectory: URL {
        if FileManager.default.fileExists(atPath: mlxModelFile.path) {
            return mlxDirectory
        }
        let coreMlDir = modelsBase.appendingPathComponent("CoreML")
        let coreMlModel = coreMlDir.appendingPathComponent("kokoro_5s.mlmodelc")
        if FileManager.default.fileExists(atPath: coreMlModel.path) {
            return coreMlDir
        }
        return mlxDirectory
    }

    var canSpeak: Bool {
        FileManager.default.fileExists(atPath: Self.mlxModelFile.path) ||
        FileManager.default.fileExists(atPath: Self.modelsBase.appendingPathComponent("CoreML/kokoro_5s.mlmodelc").path)
    }

    /// Synthesizes sample sentences with the configured Director and MLX Actor.
    func speak(config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isSpeaking, canSpeak else { return }
        if config.canUseMlx {
            guard let model, model.isAvailable else { return }
        }

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
        let actor = getActor()
        await actor.setBaseVoice(config.emotionMode == .director ? config.mlxBaseVoice : nil)

        let controller = await StageCoordinator.run(
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
        let actor = getActor()
        await actor.setBaseVoice(config.emotionMode == .director ? config.mlxBaseVoice : nil)

        let controller = await StageCoordinator.run(
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
        let isFile = ext == "litertlm" || ext == "gguf" || path.hasSuffix(".litertlm") || path.hasSuffix(".gguf")
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
        models = Self.load()
        selectedID = UserDefaults.standard.string(forKey: Self.selectedKey)
        if models.isEmpty { seedDefaults() }
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
        
        // Seed LiteRT-LM Defaults (Gemma 4 E2B LiteRT-LM is now first/default)
        for file in ["gemma-4-E2B-it.litertlm", "gemma-4-E4B-it.litertlm"] {
            let url = base.appendingPathComponent(file)
            if FileManager.default.fileExists(atPath: url.path) {
                models.append(DirectorModel(name: file, path: url.path))
            }
        }
        
        // Seed MLX Defaults
        for dir in ["gemma-4-e2b-it-4bit", "gemma-4-e4b-it-4bit"] {
            let url = base.appendingPathComponent(dir)
            if FileManager.default.fileExists(atPath: url.appendingPathComponent("config.json").path) {
                models.append(DirectorModel(name: dir, path: url.path))
            }
        }
        
        // Seed GGUF Defaults
        for dir in ["gemma-4-E2B-it-GGUF", "gemma-4-E4B-it-GGUF"] {
            let url = base.appendingPathComponent(dir)
            if let contents = try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: nil) {
                if let ggufFile = contents.first(where: { $0.pathExtension == "gguf" && !$0.lastPathComponent.hasPrefix("mmproj") }) {
                    models.append(DirectorModel(name: dir, path: ggufFile.path))
                }
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
actor CachingDirectorEngine: DirectorInference {
    private let base: any DirectorInference
    private let modelId: String
    private var narrationMode: NarrationMode = .solo
    
    // In-memory cache shared across instances.
    private static var cache: [String: String] = [:]
    
    init(base: any DirectorInference, modelId: String, narrationMode: NarrationMode = .solo) {
        self.base = base
        self.modelId = modelId
        self.narrationMode = narrationMode
    }

    func setNarrationMode(_ mode: NarrationMode) async {
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
    
    static func clearCache() {
        cache.removeAll()
    }
}
