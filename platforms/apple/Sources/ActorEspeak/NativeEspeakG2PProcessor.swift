import Foundation
import libespeak_ng
import ProsodiaActor
import Misaki

public final class NativeEspeakG2PProcessor: ProsodiaG2PProcessor, @unchecked Sendable {
    private let lock = NSLock()
    private var initialized = false

    public init() throws {
        // Initialize lazily or on instantiation.
        try ensureInitialized()
    }
    
    private func ensureInitialized() throws {
        lock.lock()
        defer { lock.unlock() }
        
        if initialized { return }
        
        let fileManager = FileManager.default
        var dataPath: String? = nil
        
        // Find the bundle dynamically
        let possibleNames = ["espeak-ng-data", "espeak-ng_data", "espeak_ng_data"]
        var bundleURL: URL? = nil
        for name in possibleNames {
            for bundle in Bundle.allBundles {
                if let path = bundle.path(forResource: name, ofType: nil) {
                    bundleURL = URL(fileURLWithPath: path)
                    break
                }
                if let path = bundle.path(forResource: name, ofType: "bundle") {
                    bundleURL = URL(fileURLWithPath: path)
                    break
                }
                if let resourceURL = bundle.resourceURL {
                    let checkURL = resourceURL.appendingPathComponent(name)
                    if fileManager.fileExists(atPath: checkURL.path) {
                        bundleURL = checkURL
                        break
                    }
                    let checkBundleURL = resourceURL.appendingPathComponent("\(name).bundle")
                    if fileManager.fileExists(atPath: checkBundleURL.path) {
                        bundleURL = checkBundleURL
                        break
                    }
                }
            }
            if bundleURL != nil { break }
        }
        
        if bundleURL == nil {
            if let resourcePath = Bundle.main.resourcePath {
                for name in possibleNames {
                    let checkURL = URL(fileURLWithPath: resourcePath).appendingPathComponent(name)
                    if fileManager.fileExists(atPath: checkURL.path) {
                        bundleURL = checkURL
                        break
                    }
                    let checkBundleURL = URL(fileURLWithPath: resourcePath).appendingPathComponent("\(name).bundle")
                    if fileManager.fileExists(atPath: checkBundleURL.path) {
                        bundleURL = checkBundleURL
                        break
                    }
                }
            }
        }
        
        // Try creating a symlink in the current working directory to help EspeakLib find the bundle if needed
        if let bundleURL = bundleURL {
            let cwd = fileManager.currentDirectoryPath
            let symlinkURL = URL(fileURLWithPath: cwd).appendingPathComponent("espeak-ng_data.bundle")
            if !fileManager.fileExists(atPath: symlinkURL.path) {
                try? fileManager.createSymbolicLink(at: symlinkURL, withDestinationURL: bundleURL)
            }
            dataPath = bundleURL.path
        }
        
        // Determine writable root directory for compiled resources (fallback to temp if needed)
        var espeakRootURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!.appendingPathComponent("espeak-ng", isDirectory: true)
        do {
            try fileManager.createDirectory(at: espeakRootURL, withIntermediateDirectories: true, attributes: nil)
        } catch {
            let tempDir = fileManager.temporaryDirectory.appendingPathComponent("espeak-ng", isDirectory: true)
            try? fileManager.createDirectory(at: tempDir, withIntermediateDirectories: true, attributes: nil)
            espeakRootURL = tempDir
        }
        
        // Install and compile bundle resources in root directory using EspeakLib
        do {
            try EspeakLib.ensureBundleInstalled(inRoot: espeakRootURL)
            dataPath = espeakRootURL.path
        } catch {
            print("[NativeEspeakG2PProcessor] EspeakLib failed to install bundle in root: \(error)")
        }
        
        // Initialize espeak-ng in synchronous mode (used purely for text-to-phonemes conversion).
        // Pass espeakINITIALIZE_DONT_EXIT to prevent espeak-ng from calling exit(1) on failure,
        // allowing us to catch the error in Swift instead.
        let result = espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, dataPath, Int32(espeakINITIALIZE_DONT_EXIT))
        guard result >= 0 else {
            throw NSError(
                domain: "NativeEspeakG2P",
                code: 1,
                userInfo: [
                    NSLocalizedDescriptionKey: "Failed to initialize espeak-ng engine. Data path: \(dataPath ?? "nil")"
                ]
            )
        }
        
        initialized = true
    }
    
    public func phonemize(_ text: String, langCode: String) throws -> (phonemes: String, tokens: [MToken]) {
        try ensureInitialized()
        
        lock.lock()
        defer { lock.unlock() }
        
        // Set the language voice name (e.g. "fr", "es", "en-us")
        let voiceResult = espeak_SetVoiceByName(langCode)
        guard voiceResult == EE_OK else {
            throw NSError(
                domain: "NativeEspeakG2P",
                code: 2,
                userInfo: [
                    NSLocalizedDescriptionKey: "Failed to set espeak-ng voice to: \(langCode)"
                ]
            )
        }
        
        var phonemesString = ""
        let remainingText = text
        
        // espeak_TextToPhonemes processes UTF-8 text and returns IPA phoneme arrays
        remainingText.withCString { cString in
            var currentPtr: UnsafePointer<Int8>? = cString
            
            while let ptr = currentPtr, ptr.pointee != 0 {
                // flags: 1 (espeakCHARS_UTF8) | 2 (espeakPHONEMES_IPA)
                let flags: Int32 = 1 | 2
                var addr: UnsafeRawPointer? = UnsafeRawPointer(ptr)
                
                let phonemeResultPtr = withUnsafeMutablePointer(to: &addr) { addrPtr in
                    espeak_TextToPhonemes(addrPtr, 1, flags)
                }
                
                if let phonemesCStr = phonemeResultPtr {
                    let part = String(cString: phonemesCStr)
                    if !phonemesString.isEmpty && !part.isEmpty {
                        phonemesString.append(" ")
                    }
                    phonemesString.append(part)
                } else {
                    break
                }
                
                if let updatedAddr = addr {
                    currentPtr = updatedAddr.assumingMemoryBound(to: Int8.self)
                } else {
                    break
                }
            }
        }
        
        let cleanedPhonemes = phonemesString.trimmingCharacters(in: .whitespacesAndNewlines)
        
        // Construct token metadata structure mapping to the input words
        let components = text.components(separatedBy: .whitespacesAndNewlines)
        let cleanPhonemeWords = cleanedPhonemes.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }
        
        var tokens: [MToken] = []
        for (idx, word) in components.enumerated() {
            guard !word.isEmpty else { continue }
            let wordPhonemes = idx < cleanPhonemeWords.count ? cleanPhonemeWords[idx] : word
            
            let isLast = idx == components.count - 1
            let whitespace = isLast ? "" : " "
            
            let token = MToken(
                text: word,
                tag: word.lowercased(),
                whitespace: whitespace,
                phonemes: wordPhonemes,
                startTS: nil,
                endTS: nil,
                underscore: MToken.Underscore(isHead: true, prespace: idx > 0)
            )
            tokens.append(token)
        }
        
        return (cleanedPhonemes, tokens)
    }
    
    deinit {
        if initialized {
            espeak_Terminate()
        }
    }
}
