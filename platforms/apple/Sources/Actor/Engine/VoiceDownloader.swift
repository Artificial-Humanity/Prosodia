import Foundation

public final class VoiceDownloader: Sendable {
    public let remoteBaseURL: URL
    private let session: URLSession

    public init(remoteBaseURL: URL = URL(string: "https://huggingface.co/artificial-humanity/StyleTTS2-Lite/resolve/main/voices/")!) {
        self.remoteBaseURL = remoteBaseURL
        
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 30.0
        configuration.timeoutIntervalForResource = 300.0
        self.session = URLSession(configuration: configuration)
    }

    /// Downloads the requested voice files dynamically, supporting both safetensors and npy extensions.
    /// - Parameters:
    ///   - voiceName: The voice name, e.g. "anchor_female_adult"
    ///   - localDirectory: The directory where downloaded voice files are stored
    /// - Returns: The local URL of the downloaded voice file.
    public func downloadVoice(named voiceName: String, toDirectory localDirectory: URL) async throws -> URL {
        // Try safetensors first, fallback to npy if it fails
        let extensions = ["safetensors", "npy"]
        var lastError: Error?

        for ext in extensions {
            let filename = "\(voiceName).\(ext)"
            let localURL = localDirectory.appendingPathComponent(filename, isDirectory: false)
            
            // Check if already exists locally
            if FileManager.default.fileExists(atPath: localURL.path) {
                return localURL
            }

            let remoteURL = remoteBaseURL.appendingPathComponent(filename, isDirectory: false)
            do {
                let (tempURL, response) = try await session.download(from: remoteURL)
                guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else {
                    continue // Try next extension
                }
                
                // Ensure target directory exists
                try FileManager.default.createDirectory(at: localDirectory, withIntermediateDirectories: true)
                
                // Move item from temporary down path
                if FileManager.default.fileExists(atPath: localURL.path) {
                    try FileManager.default.removeItem(at: localURL)
                }
                try FileManager.default.moveItem(at: tempURL, to: localURL)
                return localURL
            } catch {
                lastError = error
            }
        }

        throw lastError ?? NSError(domain: "VoiceDownloader", code: 404, userInfo: [
            NSLocalizedDescriptionKey: "Failed to download voice pack '\(voiceName)' in either safetensors or npy formats."
        ])
    }
}
