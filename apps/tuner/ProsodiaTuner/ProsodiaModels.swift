import Foundation

/// One model-role binding from `prosodia_models.json`: a path relative to
/// `modelsBase` (or absolute), plus an optional human-readable display name.
public struct ModelRoleEntry: Codable, Equatable, Sendable {
    public var path: String
    public var display: String?

    public init(path: String, display: String? = nil) {
        self.path = path
        self.display = display
    }
}

/// Declarative role → artifact map (Debt F): the apps know model *roles*
/// ("actor", "director-light", …) and resolve every artifact through this
/// config — no model identities, filenames, or `#filePath` walks in app source.
/// Relocating `Models/` or renaming a checkpoint is a config edit, not a
/// recompile.
public struct ProsodiaModels: Codable, Equatable, Sendable {
    /// Base directory for relative role paths. When itself relative, it is
    /// resolved against the directory containing `prosodia_models.json` — the
    /// config file's own location anchors everything, wherever the repo lives.
    public var modelsBase: String
    public var roles: [String: ModelRoleEntry]
    /// Seeding/selection order for Director roles; the first available entry
    /// becomes the default Director.
    public var directorRoleOrder: [String]?

    /// Mirrors the committed `prosodia_models.json`, so a missing config file
    /// degrades to the standard shared-workspace layout instead of a dead app.
    public static let fallback = ProsodiaModels(
        modelsBase: "../Reference/Models",
        roles: [
            "actor": ModelRoleEntry(path: "sonora.tflite", display: "Sonora Actor (v1-ljspeech)"),
            "voices": ModelRoleEntry(path: ".", display: "Voice packs directory"),
            "director-light": ModelRoleEntry(path: "Google/gemma-4-E2B-it.litertlm", display: "Gemma 4 E2B"),
            "director-heavy": ModelRoleEntry(path: "Google/gemma-4-E4B-it.litertlm", display: "Gemma 4 E4B"),
        ],
        directorRoleOrder: ["director-light", "director-heavy"]
    )
}

/// Loads `prosodia_models.json` once and resolves role URLs against it.
/// Read-only by design — the file is edited by hand (or tooling), never by the
/// apps, so the manager carries no save path and is immutable after init.
public final class ProsodiaModelsManager: Sendable {
    public static let shared = ProsodiaModelsManager()

    public let config: ProsodiaModels
    /// Absolute, standardized base directory for relative role paths.
    public let modelsBase: URL
    /// Where the config was loaded from (nil when running on fallback).
    public let configFileURL: URL?

    private init() {
        let fileManager = FileManager.default

        // Discovery: explicit env override, then the known workspace layouts
        // (umbrella and standalone), newest first. No #filePath anchoring —
        // the config file's on-disk location is the anchor instead.
        var candidates: [URL] = []
        if let envPath = ProcessInfo.processInfo.environment["PROSODIA_MODELS_PATH"] {
            candidates.append(URL(fileURLWithPath: envPath))
        }
        let home = fileManager.homeDirectoryForCurrentUser
        candidates.append(home.appendingPathComponent("Projects/Artificial-Humanity/Prosodia/prosodia_models.json"))
        candidates.append(home.appendingPathComponent("Projects/Prosodia/prosodia_models.json"))
        if let bundled = Bundle.main.url(forResource: "prosodia_models", withExtension: "json") {
            candidates.append(bundled)
        }

        var loaded: ProsodiaModels?
        var loadedFrom: URL?
        for url in candidates where fileManager.fileExists(atPath: url.path) {
            do {
                let data = try Data(contentsOf: url)
                loaded = try JSONDecoder().decode(ProsodiaModels.self, from: data)
                loadedFrom = url
                print("[ProsodiaModelsManager] Loaded model roles from \(url.path)")
                break
            } catch {
                print("[ProsodiaModelsManager] Error loading \(url.path): \(error). Trying next candidate.")
            }
        }
        if loaded == nil {
            print("[ProsodiaModelsManager] No prosodia_models.json found — using fallback layout")
        }

        let config = loaded ?? .fallback
        self.config = config
        self.configFileURL = loadedFrom

        let baseString = config.modelsBase as NSString
        if baseString.isAbsolutePath {
            self.modelsBase = URL(fileURLWithPath: config.modelsBase).standardizedFileURL
        } else if let anchor = loadedFrom?.deletingLastPathComponent() {
            self.modelsBase = anchor.appendingPathComponent(config.modelsBase).standardizedFileURL
        } else {
            // Fallback-config case: no file to anchor to; assume the umbrella
            // workspace layout under the user's home.
            self.modelsBase = home
                .appendingPathComponent("Projects/Artificial-Humanity/Reference/Models")
                .standardizedFileURL
        }
    }

    /// Absolute URL for a role, or nil when the role is not configured.
    /// `"."` binds a role to `modelsBase` itself (e.g. the voices directory).
    public func url(forRole role: String) -> URL? {
        guard let entry = config.roles[role] else { return nil }
        if entry.path == "." { return modelsBase }
        let path = entry.path as NSString
        if path.isAbsolutePath { return URL(fileURLWithPath: entry.path).standardizedFileURL }
        return modelsBase.appendingPathComponent(entry.path).standardizedFileURL
    }

    /// Human-readable name for a role (falls back to the role key).
    public func display(forRole role: String) -> String {
        config.roles[role]?.display ?? role
    }

    /// Director roles in seeding order, restricted to configured entries.
    public var directorRoles: [String] {
        (config.directorRoleOrder ?? []).filter { config.roles[$0] != nil }
    }

    /// Role whose configured artifact has the given filename, if any — used to
    /// migrate legacy path-persisted UserDefaults entries onto durable role keys.
    public func role(matchingFilename filename: String) -> String? {
        config.roles.first { key, entry in
            key != "voices" && (entry.path as NSString).lastPathComponent == filename
        }?.key
    }
}
