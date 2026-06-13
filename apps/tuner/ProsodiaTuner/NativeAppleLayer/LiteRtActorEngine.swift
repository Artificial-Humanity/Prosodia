import Foundation
import os
import Kit

// MARK: - TensorFlow Lite C-API Swift Bindings

@_silgen_name("TfLiteModelCreateFromFile")
private func TfLiteModelCreateFromFile(_ model_path: UnsafePointer<CChar>) -> OpaquePointer?

@_silgen_name("TfLiteModelDelete")
private func TfLiteModelDelete(_ model: OpaquePointer?)

@_silgen_name("TfLiteInterpreterOptionsCreate")
private func TfLiteInterpreterOptionsCreate() -> OpaquePointer?

@_silgen_name("TfLiteInterpreterOptionsDelete")
private func TfLiteInterpreterOptionsDelete(_ options: OpaquePointer?)

@_silgen_name("TfLiteInterpreterOptionsSetNumThreads")
private func TfLiteInterpreterOptionsSetNumThreads(_ options: OpaquePointer?, _ num_threads: Int32)

@_silgen_name("TfLiteInterpreterCreate")
private func TfLiteInterpreterCreate(_ model: OpaquePointer?, _ options: OpaquePointer?) -> OpaquePointer?

@_silgen_name("TfLiteInterpreterDelete")
private func TfLiteInterpreterDelete(_ interpreter: OpaquePointer?)

@_silgen_name("TfLiteInterpreterAllocateTensors")
private func TfLiteInterpreterAllocateTensors(_ interpreter: OpaquePointer?) -> Int32

@_silgen_name("TfLiteInterpreterInvoke")
private func TfLiteInterpreterInvoke(_ interpreter: OpaquePointer?) -> Int32

@_silgen_name("TfLiteInterpreterGetInputTensorCount")
private func TfLiteInterpreterGetInputTensorCount(_ interpreter: OpaquePointer?) -> Int32

@_silgen_name("TfLiteInterpreterGetInputTensor")
private func TfLiteInterpreterGetInputTensor(_ interpreter: OpaquePointer?, _ input_index: Int32) -> OpaquePointer?

@_silgen_name("TfLiteInterpreterGetOutputTensorCount")
private func TfLiteInterpreterGetOutputTensorCount(_ interpreter: OpaquePointer?) -> Int32

@_silgen_name("TfLiteInterpreterGetOutputTensor")
private func TfLiteInterpreterGetOutputTensor(_ interpreter: OpaquePointer?, _ output_index: Int32) -> OpaquePointer?

@_silgen_name("TfLiteInterpreterResizeInputTensor")
private func TfLiteInterpreterResizeInputTensor(
    _ interpreter: OpaquePointer?,
    _ input_index: Int32,
    _ dims: UnsafePointer<Int32>?,
    _ dims_count: Int32
) -> Int32

@_silgen_name("TfLiteTensorCopyFromBuffer")
private func TfLiteTensorCopyFromBuffer(
    _ tensor: OpaquePointer?,
    _ input_data: UnsafeRawPointer?,
    _ input_data_size: Int
) -> Int32

@_silgen_name("TfLiteTensorCopyToBuffer")
private func TfLiteTensorCopyToBuffer(
    _ tensor: OpaquePointer?,
    _ output_data: UnsafeMutableRawPointer?,
    _ output_data_size: Int
) -> Int32

@_silgen_name("TfLiteTensorByteSize")
private func TfLiteTensorByteSize(_ tensor: OpaquePointer?) -> Int

@_silgen_name("TfLiteTensorName")
private func TfLiteTensorName(_ tensor: OpaquePointer?) -> UnsafePointer<CChar>?

@_silgen_name("TfLiteTensorType")
private func TfLiteTensorType(_ tensor: OpaquePointer?) -> Int32

@_silgen_name("TfLiteTensorNumDims")
private func TfLiteTensorNumDims(_ tensor: OpaquePointer?) -> Int32

@_silgen_name("TfLiteTensorDim")
private func TfLiteTensorDim(_ tensor: OpaquePointer?, _ dim_index: Int32) -> Int32

// MARK: - LiteRtActorEngine

/// A ``ProsodiaActorBackend`` powered by the Google LiteRT (TensorFlow Lite) runtime.
///
/// It loads a compiled StyleTTS2-Lite `.tflite` model, maps phoneme, style, and VAT
/// inputs to their corresponding input tensors, invokes inference, and extracts PCM outputs.
public final class LiteRtActorEngine: @unchecked Sendable, ProsodiaActorBackend {

    private static let log = Logger(subsystem: "com.mcfarlin.ProsodiaStage", category: "LiteRtActorEngine")

    public let vocab: [String: Int]
    private let modelPath: URL
    private let lock = NSLock()

    // Loaded model resource handles
    private var model: OpaquePointer?
    private var options: OpaquePointer?
    private var interpreter: OpaquePointer?

    // Last known dimensions to prevent redundant tensor reallocation
    private var lastPhonemeLength: Int = 0

    /// Initializes a new LiteRT Actor engine.
    ///
    /// - Parameters:
    ///   - modelPath: Local URL to the `.tflite` model file.
    ///   - configURL: Local URL to the configuration `config.json` containing vocab mapping.
    public init(modelPath: URL, configURL: URL) throws {
        self.modelPath = modelPath

        let data = try Data(contentsOf: configURL)
        let config = try JSONDecoder().decode(StyleTTS2Config.self, from: data)
        self.vocab = config.vocab
    }

    deinit {
        cleanup()
    }

    /// Reclaims memory by releasing the loaded interpreter and model structures.
    public func reclaimMemory() {
        lock.lock()
        defer { lock.unlock() }
        cleanup()
        Self.log.info("LiteRT Actor engine memory reclaimed.")
    }

    private func cleanup() {
        if let interpreter = interpreter {
            TfLiteInterpreterDelete(interpreter)
            self.interpreter = nil
        }
        if let options = options {
            TfLiteInterpreterOptionsDelete(options)
            self.options = nil
        }
        if let model = model {
            TfLiteModelDelete(model)
            self.model = nil
        }
        lastPhonemeLength = 0
    }

    // MARK: - Tokenization

    public func tokenize(_ phonemes: String) throws -> [Int] {
        let ids = phonemes.compactMap { vocab[String($0)] }
        // 0-bound padding identical to standard StyleTTS2 G2P tokenizer format
        return [0] + ids + [0]
    }

    // MARK: - Inference Execution

    private func getOrInitializeInterpreter() throws -> OpaquePointer {
        if let existing = interpreter { return existing }

        Self.log.info("Loading LiteRT Actor model: \(self.modelPath.path, privacy: .public)")
        guard let model = TfLiteModelCreateFromFile(modelPath.path) else {
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Failed to load TFLite model from \(modelPath.path)"]
            )
        }
        self.model = model

        let options = TfLiteInterpreterOptionsCreate()
        TfLiteInterpreterOptionsSetNumThreads(options, 4)
        self.options = options

        guard let interpreter = TfLiteInterpreterCreate(model, options) else {
            cleanup()
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Failed to create TFLite interpreter."]
            )
        }
        self.interpreter = interpreter

        let status = TfLiteInterpreterAllocateTensors(interpreter)
        guard status == 0 else {
            cleanup()
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: "Failed to allocate TFLite tensors (status: \(status))."]
            )
        }

        return interpreter
    }

    public func forward(
        phonemes: String,
        refS: StyleVector,
        speed: Float,
        durationScales: [Float]?,
        f0Bias: [Float]?
    ) throws -> ActorEngineOutput {
        lock.lock()
        defer { lock.unlock() }

        let interpreter = try getOrInitializeInterpreter()

        // 1. Tokenize phoneme text
        let tokenIds = try tokenize(phonemes)
        let tokenCount = tokenIds.count
        var inputIds = tokenIds.map { Int32($0) }

        // 2. Identify input tensor indices by matching names
        let inputCount = TfLiteInterpreterGetInputTensorCount(interpreter)
        var phonemesIndex: Int32 = -1
        var styleIndex: Int32 = -1
        var speedIndex: Int32 = -1
        var vatIndex: Int32 = -1

        for i in 0..<inputCount {
            guard let tensor = TfLiteInterpreterGetInputTensor(interpreter, i) else { continue }
            let name = String(cString: TfLiteTensorName(tensor) ?? UnsafePointer<CChar>(bitPattern: 0)!).lowercased()

            if name.contains("phone") || name.contains("input_ids") || name.contains("text") {
                phonemesIndex = i
            } else if name.contains("style") || name.contains("ref") {
                styleIndex = i
            } else if name.contains("speed") || name.contains("tempo") {
                if !name.contains("vat") {
                    speedIndex = i
                }
            } else if name.contains("vat") || name.contains("emotion") || name.contains("control") {
                vatIndex = i
            }
        }

        guard phonemesIndex != -1 else {
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 4,
                userInfo: [NSLocalizedDescriptionKey: "LiteRT actor model lacks expected phonemes input tensor."]
            )
        }

        // 3. Dynamically resize phonemes tensor if text length changed
        if tokenCount != lastPhonemeLength {
            let dims: [Int32] = [1, Int32(tokenCount)]
            let status = TfLiteInterpreterResizeInputTensor(interpreter, phonemesIndex, dims, 2)
            guard status == 0 else {
                throw NSError(
                    domain: "LiteRtActorEngine",
                    code: 5,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to resize TFLite phoneme tensor to \(tokenCount) (status: \(status))."]
                )
            }
            let allocStatus = TfLiteInterpreterAllocateTensors(interpreter)
            guard allocStatus == 0 else {
                throw NSError(
                    domain: "LiteRtActorEngine",
                    code: 6,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to re-allocate TFLite tensors after resize (status: \(allocStatus))."]
                )
            }
            lastPhonemeLength = tokenCount
        }

        // 4. Copy data into matching input tensors
        // A. Phonemes IDs
        if let tensor = TfLiteInterpreterGetInputTensor(interpreter, phonemesIndex) {
            let size = inputIds.count * MemoryLayout<Int32>.size
            let status = TfLiteTensorCopyFromBuffer(tensor, &inputIds, size)
            guard status == 0 else {
                throw NSError(
                    domain: "LiteRtActorEngine",
                    code: 7,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to copy phoneme IDs to TFLite input (status: \(status))."]
                )
            }
        }

        // B. Style Vectors
        if styleIndex != -1, let tensor = TfLiteInterpreterGetInputTensor(interpreter, styleIndex) {
            var styleData = refS.data
            let size = styleData.count * MemoryLayout<Float>.size
            TfLiteTensorCopyFromBuffer(tensor, &styleData, size)
        }

        // C. Speed
        if speedIndex != -1, let tensor = TfLiteInterpreterGetInputTensor(interpreter, speedIndex) {
            var speedVal = speed
            TfLiteTensorCopyFromBuffer(tensor, &speedVal, MemoryLayout<Float>.size)
        }

        // D. Emotion VAT (Valence, Arousal, Tempo) — dummy values of [0.5, 0.5, 0.5] if not specified downstream
        if vatIndex != -1, let tensor = TfLiteInterpreterGetInputTensor(interpreter, vatIndex) {
            var vatData: [Float] = [0.5, 0.5, 0.5]
            TfLiteTensorCopyFromBuffer(tensor, &vatData, vatData.count * MemoryLayout<Float>.size)
        }

        // 5. Invoke Inference
        let invokeStatus = TfLiteInterpreterInvoke(interpreter)
        guard invokeStatus == 0 else {
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 8,
                userInfo: [NSLocalizedDescriptionKey: "TFLite interpreter execution failed (status: \(invokeStatus))."]
            )
        }

        // 6. Extract output buffer PCM floats
        let outputCount = TfLiteInterpreterGetOutputTensorCount(interpreter)
        guard outputCount > 0, let outTensor = TfLiteInterpreterGetOutputTensor(interpreter, 0) else {
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 9,
                userInfo: [NSLocalizedDescriptionKey: "LiteRT model returned no output tensors."]
            )
        }

        let byteSize = TfLiteTensorByteSize(outTensor)
        let elementCount = byteSize / MemoryLayout<Float>.size
        var outputPcm = [Float](repeating: 0.0, count: elementCount)

        let copyStatus = TfLiteTensorCopyToBuffer(outTensor, &outputPcm, byteSize)
        guard copyStatus == 0 else {
            throw NSError(
                domain: "LiteRtActorEngine",
                code: 10,
                userInfo: [NSLocalizedDescriptionKey: "Failed to copy PCM data out of TFLite output tensor (status: \(copyStatus))."]
            )
        }

        // Return output, using a dummy prediction duration matching standard token count
        let dummyDurations = [Int](repeating: 8, count: tokenCount)
        return ActorEngineOutput(audio: outputPcm, predDur: dummyDurations)
    }
}
