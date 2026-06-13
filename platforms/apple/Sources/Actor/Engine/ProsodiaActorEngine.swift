#if canImport(MLX)
import Foundation
import MLX
import MLXNN

public final class ProsodiaActorEngine: Module, @unchecked Sendable, ProsodiaActorBackend {

    public let config: StyleTTS2Config
    public let vocab: [String: Int]
    public let contextLength: Int

    @ModuleInfo(key: "bert") var bert: CustomAlbert
    @ModuleInfo(key: "bert_encoder") var bertEncoder: Linear
    @ModuleInfo(key: "predictor") var predictor: ProsodyPredictor
    @ModuleInfo(key: "text_encoder") var textEncoder: TextEncoder
    @ModuleInfo(key: "decoder") var decoder: Decoder

    public init(
        configURL: URL,
        weightsURL: URL? = nil,
        disableComplex: Bool = false
    ) throws {
        let data = try Data(contentsOf: configURL)
        let config = try JSONDecoder().decode(StyleTTS2Config.self, from: data)
        self.config = config
        self.vocab = config.vocab
        self.contextLength = config.plbert.maxPositionEmbeddings

        _bert.wrappedValue = CustomAlbert(config: config.plbert, vocabSize: config.nToken)
        _bertEncoder.wrappedValue = Linear(config.plbert.hiddenSize, config.hiddenDim, bias: true)
        _predictor.wrappedValue = ProsodyPredictor(
            styleDim: config.styleDim,
            hiddenDim: config.hiddenDim,
            layers: config.nLayer,
            maxDur: config.maxDur
        )
        _textEncoder.wrappedValue = TextEncoder(
            channels: config.hiddenDim,
            kernelSize: config.textEncoderKernelSize,
            depth: config.nLayer,
            symbols: config.nToken
        )
        _decoder.wrappedValue = Decoder(
            dimIn: config.hiddenDim,
            styleDim: config.styleDim,
            dimOut: config.nMels,
            resblockKernelSizes: config.istftnet.resblockKernelSizes,
            upsampleRates: config.istftnet.upsampleRates,
            upsampleInitialChannel: config.istftnet.upsampleInitialChannel,
            resblockDilationSizes: config.istftnet.resblockDilationSizes,
            upsampleKernelSizes: config.istftnet.upsampleKernelSizes,
            genIstftNFFT: config.istftnet.genIstftNFFT,
            genIstftHopSize: config.istftnet.genIstftHopSize,
            disableComplex: disableComplex
        )
        super.init()

        if let weightsURL {
            try loadWeights(from: weightsURL)
        }
    }

    public func loadWeights(from url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw StyleTTS2Error.missingWeights(url)
        }
        let (rawWeights, _) = try loadArraysAndMetadata(url: url)
        

        // Remap flat weight keys to match the Swift MLXNN.Module nested namespace
        var weights: [String: MLXArray] = [:]
        for (key, value) in rawWeights {
            if key.hasPrefix("bert.pooler.") {
                continue
            }
            
            var remappedKey = key
            
            // 1. Remap text_encoder.cnn.X.Y flat keys to conv/norm namespaces
            if key.hasPrefix("text_encoder.cnn.") {
                let parts = key.split(separator: ".")
                if parts.count == 5 {
                    let index = parts[2]
                    let layerType = parts[3] // "0" (conv) or "1" (norm)
                    let param = parts[4]
                    if layerType == "0" {
                        remappedKey = "text_encoder.cnn.\(index).conv.\(param)"
                    } else if layerType == "1" {
                        remappedKey = "text_encoder.cnn.\(index).norm.\(param)"
                    }
                }
            }
            
            // 2. Remap decoder.asr_res.0 flat keys to conv namespace
            if key.hasPrefix("decoder.asr_res.") {
                let parts = key.split(separator: ".")
                if parts.count == 4 {
                    let param = parts[3]
                    remappedKey = "decoder.asr_res.conv.\(param)"
                }
            }
            
            // 3. Remap albert_layer_groups and albert_layers to camelCase
            if remappedKey.contains("albert_layer_groups") {
                remappedKey = remappedKey.replacingOccurrences(of: "albert_layer_groups", with: "albertLayerGroups")
            }
            if remappedKey.contains("albert_layers") {
                remappedKey = remappedKey.replacingOccurrences(of: "albert_layers", with: "albertLayers")
            }
            
            weights[remappedKey] = value
        }
        

        let parameters = ModuleParameters.unflattened(weights)
        try update(parameters: parameters, verify: [.all])
        eval(self)
    }

    public func tokenize(_ phonemes: String) throws -> [Int] {
        let ids = phonemes.compactMap { vocab[String($0)] }
        if ids.count + 2 > contextLength {
            throw StyleTTS2Error.invalidPhonemeLength(ids.count)
        }
        return [0] + ids + [0]
    }

    public func forwardWithTokenIDs(
        _ inputIDs: MLXArray,
        refS: MLXArray,
        speed: Float = 1.0,
        durationScales: MLXArray? = nil,
        f0Bias: MLXArray? = nil
    ) throws -> ActorEngineOutput {
        try Self.validate(speed: speed)
        precondition(inputIDs.ndim == 2, "Expected [B, T] token ids.")
        let batch = inputIDs.dim(0)
        guard batch == 1 else {
            throw StyleTTS2Error.unsupportedBatch(batch)
        }

        let style = normalizeStyle(refS)
        let attentionMask = MLXArray.ones([1, inputIDs.dim(1)], dtype: .float32)

        let bertDur = bert(inputIDs, attentionMask: attentionMask)
        let dEn = bertEncoder(bertDur)

        let acousticStyle = style.ndim == 3 ? style[0..., 0..., 0..<config.styleDim] : style[0..., 0..<config.styleDim]
        let prosodyStyle = style.ndim == 3 ? style[0..., 0..., config.styleDim..<(config.styleDim * 2)] : style[0..., config.styleDim..<(config.styleDim * 2)]

        let durationEncoded = predictor.durationEncoding(dEn, style: prosodyStyle)
        let durationLogits = predictor.predictDurations(durationEncoded)
        var duration = sum(sigmoid(durationLogits), axis: -1) / speed

        if let durationScales {
            duration = duration * durationScales
        }

        let predDur = clip(duration.round(), min: 1).asType(.int32)
        let predDurValues = predDur.asArray(Int32.self).map(Int.init)

        let alignment = buildAlignment(durations: predDurValues)
        let prosodyAligned = matmul(durationEncoded.transposed(0, 2, 1), alignment).transposed(0, 2, 1)

        let prosodyStyleAligned = style.ndim == 3
            ? matmul(prosodyStyle.transposed(0, 2, 1), alignment).transposed(0, 2, 1)
            : prosodyStyle

        var (f0Pred, nPred) = predictor.F0Ntrain(prosodyAligned, style: prosodyStyleAligned)

        if let f0Bias {
            let alignedF0Bias = (f0Bias.ndim == 2 && f0Bias.dim(1) == inputIDs.dim(1))
                ? {
                    let aligned = matmul(f0Bias.expandedDimensions(axis: -1).transposed(0, 2, 1), alignment)
                        .transposed(0, 2, 1)
                        .squeezed(axis: -1)
                    let tFrames = aligned.dim(1)
                    return broadcast(aligned.expandedDimensions(axis: 2), to: [1, tFrames, 2])
                        .reshaped([1, tFrames * 2])
                }()
                : f0Bias
            f0Pred = f0Pred + alignedF0Bias
        }


        let textEncoded = textEncoder(inputIDs)
        let asr = matmul(textEncoded.transposed(0, 2, 1), alignment).transposed(0, 2, 1)

        let acousticStyleAligned = style.ndim == 3
            ? matmul(acousticStyle.transposed(0, 2, 1), alignment).transposed(0, 2, 1)
            : acousticStyle

        let audio = decoder(asr, F0Curve: f0Pred, N: nPred, style: acousticStyleAligned)
        eval(audio)
        return ActorEngineOutput(audio: audio.asArray(Float.self), predDur: predDurValues)
    }

    public func forward(
        phonemes: String,
        refS: StyleVector,
        speed: Float = 1.0,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) throws -> ActorEngineOutput {
        let ids = try tokenize(phonemes)
        let inputIDs = MLXArray(ids, [1, ids.count]).asType(.int32)
        let refSArray = MLXArray(refS.data, refS.shape)
        let durationScalesArray = durationScales.map { MLXArray($0, [1, $0.count]) }
        let f0BiasArray = f0Bias.map { MLXArray($0, [1, $0.count]) }
        return try forwardWithTokenIDs(
            inputIDs,
            refS: refSArray,
            speed: speed,
            durationScales: durationScalesArray,
            f0Bias: f0BiasArray
        )
    }

    private static func validate(speed: Float) throws {
        guard speed.isFinite, speed > 0 else {
            throw StyleTTS2Error.invalidSpeed(speed)
        }
    }

    private func normalizeStyle(_ refS: MLXArray) -> MLXArray {
        if refS.ndim == 3 {
            precondition(refS.dim(0) == 1 && refS.dim(2) == config.styleDim * 2)
            return refS
        }
        let style = if refS.ndim == 1 {
            refS.expandedDimensions(axis: 0)
        } else {
            refS
        }
        precondition(style.ndim == 2 && style.dim(1) == config.styleDim * 2)
        return style
    }

    private func buildAlignment(durations: [Int]) -> MLXArray {
        let tokenCount = durations.count
        let totalFrames = durations.reduce(0, +)
        var values = [Float](repeating: 0, count: tokenCount * totalFrames)
        var frame = 0
        for (tokenIndex, duration) in durations.enumerated() {
            for _ in 0 ..< duration {
                values[(tokenIndex * totalFrames) + frame] = 1
                frame += 1
            }
        }
        return MLXArray(values, [1, tokenCount, totalFrames])
    }
}
#endif

