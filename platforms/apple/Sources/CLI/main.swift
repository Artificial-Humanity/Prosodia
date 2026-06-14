import Foundation
import Actor
import Kit


class SwiftSpeechEngine: ProsodiaSpeechEngine {
    let backend: any ProsodiaActorBackend
    
    init(backend: any ProsodiaActorBackend) {
        self.backend = backend
    }
    
    func synthesize(input: PipelineOutput) -> Kit.ActorEngineOutput {
        fatalError("synthesize(input:) is deprecated, use forward instead")
    }
    
    func forward(
        phonemeIds: [Int32],
        style: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> Kit.ActorEngineOutput {
        let output = try backend.forward(
            phonemeIds: phonemeIds,
            refS: style,
            speed: speed,
            durationScales: durationScales,
            f0Bias: f0Bias
        )
        return Kit.ActorEngineOutput(audio: output.audio, predDur: output.predDur.map { Int32($0) })
    }
    
    func reclaimMemory() {
        backend.reclaimMemory()
    }
}

@main
struct ProsodiaCLI {
    static func main() async {
        let args = CommandLine.arguments
        
        if args.contains("-h") || args.contains("--help") || args.count < 2 {
            printHelp()
            return
        }
        
        var text: String?
        var voice = "narrator"
        var outputPath = "output.wav"
        var modelsPath = "StyleTTS2FineTune"
        var speed: Float = 1.0

        var i = 1
        while i < args.count {
            let arg = args[i]
            switch arg {
            case "--text", "-t":
                if i + 1 < args.count {
                    text = args[i + 1]
                    i += 2
                } else {
                    print("Error: Missing value for --text")
                    return
                }
            case "--voice", "-v":
                if i + 1 < args.count {
                    voice = args[i + 1]
                    i += 2
                } else {
                    print("Error: Missing value for --voice")
                    return
                }
            case "--output", "-o":
                if i + 1 < args.count {
                    outputPath = args[i + 1]
                    i += 2
                } else {
                    print("Error: Missing value for --output")
                    return
                }
            case "--models", "-m":
                if i + 1 < args.count {
                    modelsPath = args[i + 1]
                    i += 2
                } else {
                    print("Error: Missing value for --models")
                    return
                }
            case "--speed", "-s":
                if i + 1 < args.count, let s = Float(args[i + 1]) {
                    speed = s
                    i += 2
                } else {
                    print("Error: Invalid or missing value for --speed")
                    return
                }
            default:
                print("Error: Unknown argument: \(arg)")
                printHelp()
                return
            }
        }
        
        guard let textToSynthesize = text else {
            print("Error: --text parameter is required.")
            return
        }
        
        let modelsDir = URL(fileURLWithPath: modelsPath)
        
        print("Initializing Prosodia speech synthesis engine...")
        print("Models directory: \(modelsDir.path)")
        print("Backend: LiteRT (StyleTTS2)")
        print("Voice selected: \(voice)")
        print("Text to synthesize: \"\(textToSynthesize)\"")
 
        do {
            let modelURL = modelsDir.appendingPathComponent("styletts2_lite.tflite")
            let configURL = modelsDir.appendingPathComponent("config.json")
            
            let configData = try Data(contentsOf: configURL)
            guard let configJson = String(data: configData, encoding: .utf8) else {
                print("Error: Failed to read config.json as UTF-8 string.")
                return
            }
            
            let backend: any ProsodiaActorBackend = try LiteRtActorEngine(modelPath: modelURL, configURL: configURL)
            let speechEngine = SwiftSpeechEngine(backend: backend)
            
            let provider = DiskVoiceAssetProvider(baseDirectory: modelsDir)
            let voiceLoader = VoiceLoader(provider: provider)
            
            let g2p = ProsodiaSpeech()
            
            let pipeline = try ProsodiaActorPipeline(
                g2p: g2p,
                voiceLoader: voiceLoader,
                configJson: configJson,
                sampleRate: 24000,
                langCode: "en-us"
            )
            
            print("Synthesizing audio samples...")
            let result = try pipeline.synthesize(
                speechEngine: speechEngine,
                text: textToSynthesize,
                voice: voice,
                speed: speed,
                durationScales: nil as [Float]?,
                f0Bias: nil as [Float]?
            )
            
            let outputURL = URL(fileURLWithPath: outputPath)
            print("Writing synthesized audio to '\(outputURL.path)'...")
            try AudioWriter.writeWAV(samples: result.audio, to: outputURL, sampleRate: Int(result.sampleRate))
            
            print("Synthesis completed successfully! (Wrote \(result.audio.count) samples at \(result.sampleRate)Hz)")
        } catch {
            print("Error occurred during synthesis: \(error.localizedDescription)")
        }
    }
    
    static func printHelp() {
        print("""
ProsodiaCLI: Offline Command-Line Audio Auditing Tool

Usage:
  ProsodiaCLI --text "Hello world" --output output.wav [options]

Options:
  -t, --text <string>     [Required] The text string to synthesize.
  -o, --output <path>     [Optional] Output path for WAV file. (Default: output.wav)
  -m, --models <path>     [Optional] Path to the directory containing weights and configs. (Default: StyleTTS2FineTune)
  -v, --voice <name>      [Optional] Converted voice pack name or voice blend string. (Default: narrator)
  -s, --speed <float>     [Optional] Speed multiplier for synthesis rate. (Default: 1.0)
  -h, --help              Show this help menu.
""")
    }
}
