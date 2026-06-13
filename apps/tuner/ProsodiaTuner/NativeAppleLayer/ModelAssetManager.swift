import Foundation
import CoreML

/// A thread-safe helper class that manages dynamic downloading and on-device
/// compilation of CoreML models for Prosodia.
public final class ModelAssetManager: Sendable {
    /// The base URL of the remote server hosting the uncompiled model assets.
    public let remoteBaseURL: URL
    
    /// The URLSession used to download model files.
    private let session: URLSession

    /// Initializes a new ModelAssetManager with a remote base URL.
    /// - Parameter remoteBaseURL: The CDN URL hosting model files.
    public init(remoteBaseURL: URL = URL(string: "https://huggingface.co/McFarlin-Technologies/ProsodiaModels/resolve/main/")!) {
        self.remoteBaseURL = remoteBaseURL
        
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 60.0
        configuration.timeoutIntervalForResource = 600.0
        self.session = URLSession(configuration: configuration)
    }

    /// Ensures that the specified CoreML model is present as a compiled `.mlmodelc` bundle.
    /// If only the uncompiled `.mlmodel` exists locally, it compiles it.
    /// If neither exists, it downloads the `.mlmodel` and compiles it.
    ///
    /// - Parameters:
    ///   - modelName: The name of the model, e.g. "styletts2_lite"
    ///   - localDirectory: The directory where models are stored.
    /// - Returns: The URL to the compiled `.mlmodelc` bundle.
    @discardableResult
    public func ensureModelReady(named modelName: String, inDirectory localDirectory: URL) async throws -> URL {
        let fileManager = FileManager.default
        let compiledURL = localDirectory.appendingPathComponent("\(modelName).mlmodelc")
        
        // 1. Check if the compiled model already exists
        if fileManager.fileExists(atPath: compiledURL.path) {
            return compiledURL
        }
        
        // Ensure local directory exists
        try fileManager.createDirectory(at: localDirectory, withIntermediateDirectories: true)
        
        // 2. Check if the uncompiled model exists locally
        let uncompiledURL = localDirectory.appendingPathComponent("\(modelName).mlmodel")
        if fileManager.fileExists(atPath: uncompiledURL.path) {
            print("[ModelAssetManager] Found local uncompiled model at \(uncompiledURL.path). Compiling...")
            return try await compileModel(at: uncompiledURL, to: compiledURL)
        }
        
        // 3. Download and compile the model
        let remoteURL = remoteBaseURL.appendingPathComponent("\(modelName).mlmodel")
        print("[ModelAssetManager] Model not found locally. Downloading from \(remoteURL.absoluteString)...")
        let tempDownloadURL = try await downloadFile(from: remoteURL)
        
        defer {
            // Clean up temporary download
            try? fileManager.removeItem(at: tempDownloadURL)
        }
        
        print("[ModelAssetManager] Compiling downloaded model...")
        return try await compileModel(at: tempDownloadURL, to: compiledURL)
    }

    /// Ensures that the specified LiteRT model (.tflite) is present locally.
    /// If missing, downloads it from the remote base URL.
    ///
    /// - Parameters:
    ///   - modelName: The name of the model without extension, e.g. "styletts2_lite"
    ///   - localDirectory: The directory where models are stored.
    /// - Returns: The URL to the local `.tflite` model.
    @discardableResult
    public func ensureTfliteModelReady(named modelName: String, inDirectory localDirectory: URL) async throws -> URL {
        let fileManager = FileManager.default
        let localURL = localDirectory.appendingPathComponent("\(modelName).tflite")
        
        // Check if the model already exists locally
        if fileManager.fileExists(atPath: localURL.path) {
            return localURL
        }
        
        // Ensure local directory exists
        try fileManager.createDirectory(at: localDirectory, withIntermediateDirectories: true)
        
        // Download the model
        let remoteURL = remoteBaseURL.appendingPathComponent("\(modelName).tflite")
        print("[ModelAssetManager] LiteRT model not found locally. Downloading from \(remoteURL.absoluteString)...")
        let tempDownloadURL = try await downloadFile(from: remoteURL)
        
        if fileManager.fileExists(atPath: localURL.path) {
            try? fileManager.removeItem(at: localURL)
        }
        
        try fileManager.moveItem(at: tempDownloadURL, to: localURL)
        print("[ModelAssetManager] Successfully placed LiteRT model at \(localURL.path)")
        return localURL
    }
    
    /// Ensures that the required configuration files (config.json, vocab_index.json) are present.
    /// If missing, downloads them from the remote base URL.
    /// - Parameter localDirectory: The directory where configuration files are stored.
    public func ensureConfigReady(inDirectory localDirectory: URL) async throws {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: localDirectory, withIntermediateDirectories: true)
        
        let files = ["config.json", "vocab_index.json"]
        for file in files {
            let localURL = localDirectory.appendingPathComponent(file)
            if !fileManager.fileExists(atPath: localURL.path) {
                let remoteURL = remoteBaseURL.appendingPathComponent(file)
                print("[ModelAssetManager] Downloading config file \(file) from \(remoteURL.absoluteString)...")
                do {
                    let tempURL = try await downloadFile(from: remoteURL)
                    if fileManager.fileExists(atPath: localURL.path) {
                        try fileManager.removeItem(at: localURL)
                    }
                    try fileManager.moveItem(at: tempURL, to: localURL)
                } catch {
                    print("[ModelAssetManager] Warning: Failed to download config file \(file): \(error.localizedDescription)")
                }
            }
        }
    }
    
    private func downloadFile(from url: URL) async throws -> URL {
        let (tempURL, response) = try await session.download(from: url)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else {
            let code = (response as? HTTPURLResponse)?.statusCode ?? 404
            throw NSError(
                domain: "ModelAssetManager",
                code: code,
                userInfo: [NSLocalizedDescriptionKey: "Failed to download asset from \(url.absoluteString) (Status: \(code))."]
            )
        }
        return tempURL
    }
    
    private func compileModel(at srcURL: URL, to dstURL: URL) async throws -> URL {
        let fileManager = FileManager.default
        let tempCompiledURL = try await Task.detached(priority: .userInitiated) {
            try MLModel.compileModel(at: srcURL)
        }.value
        
        if fileManager.fileExists(atPath: dstURL.path) {
            try fileManager.removeItem(at: dstURL)
        }
        
        try fileManager.moveItem(at: tempCompiledURL, to: dstURL)
        print("[ModelAssetManager] Successfully compiled model to \(dstURL.path)")
        return dstURL
    }
}
