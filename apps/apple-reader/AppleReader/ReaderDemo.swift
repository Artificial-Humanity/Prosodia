//
//  ReaderDemo.swift
//  AppleReader
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

    private func getActor() -> any Stage.VocalActor {
        if let cached = cachedActor {
            return cached
        }
        let actor: any Stage.VocalActor
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


    /// Loads the actor and runs one throwaway forward in the background so the
    /// first Speak doesn't pay the model load and XNNPACK weight packing.
    /// No-op when the model is absent or an actor is already cached.
    func warmUpActor() {
        guard cachedActor == nil, canSpeak else { return }
        let modelFile = Self.resolvedModelPath
        let voiceDir = Self.resolvedVoiceDirectory
        Task.detached(priority: .utility) {
            guard let resolved = VocalActorRegistry.shared.makeActor(for: modelFile, voiceDirectoryURL: voiceDir) else { return }
            _ = resolved.render(payload: encodeDirective(directive: ProsodyDirective(preset: .baseline), text: "Hi."))
            await MainActor.run { [weak self] in
                guard let self, self.cachedActor == nil else { return }
                self.cachedActor = resolved
            }
        }
    }

    private func getDirector(config: AuditionConfiguration, model: DirectorModel?) -> any Stage.DirectorInference {
        // Preset mode: never cache. The stub director bakes in the directive at
        // construction, and the cache key below doesn't include it — so a cached
        // stub silently freezes preset edits (speed, VAD, volume, casting) made
        // after the first Speak. Building a stub is free; only the Gemma path
        // needs caching.
        guard config.emotionMode == .director else {
            return config.makeDirector(model: model)
        }
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

    // Model locations resolve through prosodia_models.json (role-based; Debt F) —
    // no #filePath walks and no model filename literals in app source.

    nonisolated static var modelsBase: URL {
        ProsodiaModelsManager.shared.modelsBase
    }

    nonisolated static var resolvedModelPath: URL {
        ProsodiaModelsManager.shared.url(forRole: "actor")
            ?? modelsBase.appendingPathComponent("actor.tflite")
    }

    nonisolated static var resolvedVoiceDirectory: URL {
        ProsodiaModelsManager.shared.url(forRole: "voices") ?? modelsBase
    }

    var canSpeak: Bool {
        true
    }

    /// Synthesizes sample sentences with the configured Director and Actor.
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
    /// Role key from prosodia_models.json for config-seeded entries; nil for
    /// user-added models. The role — not the absolute path — is the durable
    /// identity, so Models/ restructures no longer strand persisted entries.
    var role: String? = nil

    var id: String { role ?? path }
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
        // Role-seeded entries re-resolve their path from prosodia_models.json on
        // every launch — the role key is the durable identity, so restructures
        // never strand them. Legacy path-persisted entries whose file is gone
        // adopt a role by filename match (one-time migration onto role keys);
        // user-added entries keep their explicit absolute paths.
        var migratedIDs: [String: String] = [:]
        for i in 0..<loadedModels.count {
            if loadedModels[i].role == nil, !loadedModels[i].isAvailable,
               let role = ProsodiaModelsManager.shared.role(
                   matchingFilename: loadedModels[i].directory.lastPathComponent) {
                migratedIDs[loadedModels[i].id] = role
                loadedModels[i].role = role
            }
            if let role = loadedModels[i].role,
               let url = ProsodiaModelsManager.shared.url(forRole: role) {
                loadedModels[i].path = url.standardizedFileURL.path
                loadedModels[i].name = ProsodiaModelsManager.shared.display(forRole: role)
            }
        }
        // Migration can converge on an id that is already listed — keep the first.
        var seenIDs = Set<String>()
        loadedModels.removeAll { !seenIDs.insert($0.id).inserted }
        models = loadedModels
        let storedSelection = UserDefaults.standard.string(forKey: Self.selectedKey)
        selectedID = storedSelection.map { migratedIDs[$0] ?? $0 }
        if models.isEmpty { seedDefaults() }
        else { save() }
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
        // Seed the Director roles configured in prosodia_models.json, in its
        // directorRoleOrder (the first available entry becomes the default).
        for role in ProsodiaModelsManager.shared.directorRoles {
            guard let url = ProsodiaModelsManager.shared.url(forRole: role) else { continue }
            let model = DirectorModel(
                name: ProsodiaModelsManager.shared.display(forRole: role),
                path: url.path,
                role: role
            )
            if model.isAvailable {
                models.append(model)
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
