import Foundation
import ProsodiaActor

#if canImport(MLX)
import MLX
#endif

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
        var backendStr = "coreml"
        
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
            case "--backend", "-b":
                if i + 1 < args.count {
                    backendStr = args[i + 1].lowercased()
                    i += 2
                } else {
                    print("Error: Missing value for --backend")
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
        print("Backend selected: \(backendStr.uppercased())")
        print("Voice selected: \(voice)")
        print("Text to synthesize: \"\(textToSynthesize)\"")
        
        do {
            let backend: any ProsodiaActorBackend
            
            if backendStr == "mlx" {
                #if canImport(MLX)
                let configURL = modelsDir.appendingPathComponent("config.json")
                let weightsURL = modelsDir.appendingPathComponent("StyleTTS2/Models/LibriTTS/epochs_2nd.pth")
                backend = try ProsodiaActorEngine(configURL: configURL, weightsURL: weightsURL)
                #else
                print("Error: MLX backend is not compiled or supported on this architecture/build configuration.")
                return
                #endif
            } else if backendStr == "coreml" {
                backend = try CoreMlProsodiaActorEngine(modelsDirectory: modelsDir)
            } else {
                print("Error: Invalid backend '\(backendStr)'. Must be either 'coreml' or 'mlx'.")
                return
            }
            
            let loader = VoiceLoader(baseDirectory: modelsDir)
            let pipeline = ProsodiaActorPipeline(engine: backend, voices: loader)
            
            print("Synthesizing audio samples...")
            let result = try await pipeline.synthesize(text: textToSynthesize, voice: voice, speed: speed)
            
            let outputURL = URL(fileURLWithPath: outputPath)
            print("Writing synthesized audio to '\(outputURL.path)'...")
            try AudioWriter.writeWAV(samples: result.audio, to: outputURL, sampleRate: result.sampleRate)
            
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
          -b, --backend <name>    [Optional] Synthesis backend to use: 'coreml' or 'mlx'. (Default: coreml)
          -h, --help              Show this help menu.
        """)
    }
}
