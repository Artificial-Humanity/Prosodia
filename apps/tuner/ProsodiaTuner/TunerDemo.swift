//
//  TunerDemo.swift
//  ProsodiaTuner
//
//  End-to-end exercise of the ProsodiaStage pipeline for tuning and A/B work.
//

import Foundation
import Observation
import Kit
import Actor
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

    /// The actor role Speak renders with: `actor` or an `actor-*` key in
    /// prosodia_models.json. The model path, ``canSpeak`` and the conditioning follow it.
    private(set) var actorRole: String
    /// What the role's model takes: split or not, and its trained VAT channels.
    private(set) var roleCapabilities: ActorRoleCapabilities = .monolith
    /// The role's `conditioning` block, parsed by the Rust core; nil when the role has
    /// none or the config is the built-in fallback.
    private(set) var conditioning: RoleConditioning?
    /// Why the role cannot speak: a block that cannot be read, or an engine that
    /// refused the model or the block. Shown in the Actor section.
    private(set) var roleError: String?
    /// The selected speaker row; nil is the role's `defaultSpeaker`.
    private(set) var selectedSpeaker: UInt32?
    /// The actor build in flight, at most one: Speak and the warm-up both await it.
    /// `id` tells a finished build whether this entry is still its own.
    private var buildTask: (role: String, id: Int, task: Task<(any Stage.VocalActor)?, Never>)?
    private var buildCount = 0
    /// The last speaker change; each change waits for the one before it.
    private var speakerTask: Task<Void, Never>?
    /// Set by ``stopActive()`` so a Speak still waiting for its actor does not start.
    private var stopRequested = false

    private static let actorRoleKey = "harnessActorRole"

    /// The speaker as feedback and Copy Config record it: "LibriTTS-R <id>" for a
    /// picked row, "default (row N)" for the role's default, "—" when the role has
    /// no speaker table (no `conditioning` block).
    var speakerDescription: String {
        guard let conditioning = conditioning else { return "—" }
        guard let row = selectedSpeaker else { return "default (row \(conditioning.defaultSpeaker))" }
        let labels = conditioning.speakerLabels
        return Int(row) < labels.count ? "LibriTTS-R \(labels[Int(row)])" : "row \(row)"
    }

    init() {
        let stored = UserDefaults.standard.string(forKey: Self.actorRoleKey)
        let roles = ProsodiaModelsManager.shared.actorRoles
        actorRole = stored.flatMap { roles.contains($0) ? $0 : nil } ?? "actor"
        refreshRole()
    }

    /// Switches the actor role: re-reads its facts, resets the speaker to the new
    /// role's default (a row chosen for one model must never reach another), drops
    /// the cached actor and warms up the new one.
    func selectActorRole(_ role: String) {
        guard role != actorRole else { return }
        actorRole = role
        UserDefaults.standard.set(role, forKey: Self.actorRoleKey)
        selectedSpeaker = nil
        refreshRole()
        if let previous = cachedActor {
            cachedActor = nil
            Task { await previous.reclaimMemory() }
        }
        warmUpActor()
    }

    /// Selects a speaker row (nil = the role's default) and applies it to the cached
    /// actor; actors built later get it when they are built.
    func selectSpeaker(_ row: UInt32?) {
        selectedSpeaker = row
        applySpeaker()
    }

    /// Applies the current ``selectedSpeaker`` to the cached actor. Each call runs after
    /// the one before it and reads the speaker when it runs, so the latest choice wins.
    @discardableResult
    private func applySpeaker() -> Task<Void, Never> {
        let previous = speakerTask
        let task = Task {
            await previous?.value
            guard let actor = cachedActor else { return }
            await actor.setSpeaker(selectedSpeaker)
        }
        speakerTask = task
        return task
    }

    /// Re-reads the selected role: its conditioning block (parsed by Rust from the
    /// loaded prosodia_models.json text) and whether its path is a split model.
    private func refreshRole() {
        conditioning = nil
        roleError = nil
        if let text = ProsodiaModelsManager.shared.rawText {
            do {
                conditioning = try parseRoleConditioning(modelsJson: text, role: actorRole)
            } catch {
                roleError = "\(actorRole): its conditioning block cannot be read — \(error)"
            }
        }
        roleCapabilities = ActorRoleCapabilities(
            isSplit: LiteRtVocalActorProvider.isSplitModelDirectory(modelPath),
            hasConditioning: conditioning != nil,
            trainedVat: Set(conditioning?.trainedVat ?? [])
        )
    }

    /// Resolves the real actor for the selected role, or `nil` when its model files
    /// are missing or the engine refuses them (the reason goes to ``roleError``).
    ///
    /// Returns `nil` rather than falling back to a placeholder renderer: a missing
    /// production model must surface as a disabled "Speak" affordance (see ``canSpeak``),
    /// never as the stub's audible 440 Hz test tone masquerading as synthesized speech.
    /// Only a genuinely resolved actor is cached, so dropping the model into `Models/`
    /// and re-triggering Speak picks it up without an app relaunch.
    private func getActor() async -> (any Stage.VocalActor)? {
        if cachedActor == nil {
            _ = await actorBuild(priority: .userInitiated).value
        }
        guard let actor = cachedActor else { return nil }
        // The speaker is set before anything renders with this actor.
        await applySpeaker().value
        return actor
    }

    /// The build of the selected role's actor: the one in flight, or a new one.
    ///
    /// The engine is built off the main actor: with a conditioning block it loads and
    /// checks the graphs while it is built. The result is cached here, once, and only
    /// while the role is still selected and nothing is cached; a surplus actor is
    /// reclaimed. The task returns the role's cached actor, or `nil`.
    private func actorBuild(priority: TaskPriority) -> Task<(any Stage.VocalActor)?, Never> {
        let role = actorRole
        if let build = buildTask, build.role == role {
            return build.task
        }
        let modelFile = modelPath
        let voiceDir = Self.resolvedVoiceDirectory
        let roleConditioning = conditioning
        buildCount += 1
        let id = buildCount
        let task = Task { () async -> (any Stage.VocalActor)? in
            let built: (any Stage.VocalActor)?
            do {
                built = try await Task.detached(priority: priority) {
                    try VocalActorRegistry.shared.makeActor(for: modelFile, voiceDirectoryURL: voiceDir, conditioning: roleConditioning)
                }.value
            } catch {
                if buildTask?.id == id { buildTask = nil }
                if actorRole == role {
                    roleError = "\(role): the engine refused it — \(error)"
                }
                return nil
            }
            if buildTask?.id == id { buildTask = nil }
            guard let made = built else { return nil }
            guard actorRole == role, cachedActor == nil else {
                await made.reclaimMemory()
                return actorRole == role ? cachedActor : nil
            }
            cachedActor = made
            applySpeaker()
            return made
        }
        buildTask = (role: role, id: id, task: task)
        return task
    }

    /// Builds the actor in the background and runs one throwaway forward so the
    /// first Speak doesn't pay the model load and XNNPACK weight packing.
    /// No-op when the model is absent or an actor is already cached.
    func warmUpActor() {
        guard cachedActor == nil, canSpeak else { return }
        let build = actorBuild(priority: .utility)
        Task.detached(priority: .utility) {
            guard let actor = await build.value else { return }
            _ = actor.render(payload: encodeDirective(directive: ProsodyDirective(preset: .baseline), text: "Hi."))
        }
    }

    private func getDirector(config: AuditionConfiguration, model: DirectorModel?) -> any Stage.DirectorInference {
        // Preset mode: never cache. The stub director bakes in the directive at
        // construction, and the cache key below doesn't include it — so a cached
        // stub silently freezes preset edits (speed, VAD, volume, casting) made
        // after the first Speak. Building a stub is free; only the Gemma path
        // needs caching.
        guard config.emotionMode == .director else {
            return config.makeDirector(model: model, capabilities: roleCapabilities)
        }
        if let cached = cachedDirector,
           cachedDirectorModel == model,
           cachedDirectorEmotionMode == config.emotionMode,
           cachedDirectorNarrationMode == config.mlxNarrationMode {
            return cached
        }
        
        let rawDirector = config.makeDirector(model: model, capabilities: roleCapabilities)
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
            // Dropped before the await, so no Speak picks up an actor being reclaimed.
            cachedActor = nil
            await actor.reclaimMemory()
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

    /// The model path of an actor role.
    nonisolated static func modelPath(forRole role: String) -> URL {
        ProsodiaModelsManager.shared.url(forRole: role)
            ?? modelsBase.appendingPathComponent("actor.tflite")
    }

    /// The selected role's model path.
    var modelPath: URL {
        Self.modelPath(forRole: actorRole)
    }

    nonisolated static var resolvedVoiceDirectory: URL {
        ProsodiaModelsManager.shared.url(forRole: "voices") ?? modelsBase
    }

    /// Whether the selected role can speak: no ``roleError``, and a real actor model
    /// is present and resolvable.
    ///
    /// Gates every "Speak" control. When false, the harness still previews VAD/voice-blend
    /// metadata via the silent stub, and the section footer says why.
    var canSpeak: Bool {
        roleError == nil && VocalActorRegistry.shared.canMakeActor(for: modelPath)
    }

    /// Synthesizes one sample passage with the configured Director and Actor.
    /// (Whole-screen speak was removed 2026-07-11 — audio is auditioned per passage.)
    func speakPassage(_ text: String, config: AuditionConfiguration, model: DirectorModel?) async {
        guard !isSpeaking, canSpeak else { return }
        if config.canUseMlx {
            guard let model, model.isAvailable else { return }
        }
        // Speaking from here on, through the actor build, so a second Speak and the
        // role and speaker pickers wait until this one is done.
        isSpeaking = true
        stopRequested = false
        activeModel = model
        defer {
            isSpeaking = false
            Task {
                await preview(config: config, model: model)
            }
        }
        guard let actor = await getActor(), !stopRequested else { return }

        let document = InMemoryBookDocument(chapters: [text])
        let director = getDirector(config: config, model: model)

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
        stopRequested = true
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
            let result = base.annotate(passage: passage)
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

