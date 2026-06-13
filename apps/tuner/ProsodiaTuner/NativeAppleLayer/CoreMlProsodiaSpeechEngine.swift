import Foundation
import CoreML

public final class CoreMlProsodiaActorEngine: @unchecked Sendable, ProsodiaActorBackend {
    public let vocab: [String: Int]
    private let modelsDirectory: URL
    private let config: MLModelConfiguration
    private let hasSplitModels: Bool

    // Thread-safe lazy stores for CoreML models
    private let e2eModelStore = Locked<MLModel?>(nil)
    private let textEncoderStore = Locked<MLModel?>(nil)
    private let prosodyPredictorStore = Locked<MLModel?>(nil)
    private let vitsDecoderStore = Locked<MLModel?>(nil)

    public init(modelsDirectory: URL) throws {
        self.modelsDirectory = modelsDirectory
        
        let fileManager = FileManager.default
        
        // Load vocabulary mapping from either vocab_index.json or config.json
        let vocabURL = modelsDirectory.appendingPathComponent("vocab_index.json")
        let configURL = modelsDirectory.appendingPathComponent("config.json")
        
        var loadedVocab: [String: Int] = [:]
        if fileManager.fileExists(atPath: vocabURL.path),
           let data = try? Data(contentsOf: vocabURL),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let vocabDict = json["vocab"] as? [String: Int] {
            loadedVocab = vocabDict
        } else if fileManager.fileExists(atPath: configURL.path),
                    let data = try? Data(contentsOf: configURL),
                    let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                    let vocabDict = json["vocab"] as? [String: Int] {
            loadedVocab = vocabDict
        }
        self.vocab = loadedVocab
        
        let config = MLModelConfiguration()
        config.computeUnits = .all // Leverage Apple Neural Engine (ANE)
        self.config = config
        
        let textEncoderURL = modelsDirectory.appendingPathComponent("TextEncoder.mlmodelc")
        let prosodyPredictorURL = modelsDirectory.appendingPathComponent("ProsodyPredictor.mlmodelc")
        let vitsDecoderURL = modelsDirectory.appendingPathComponent("VitsDecoder.mlmodelc")
        
        if fileManager.fileExists(atPath: textEncoderURL.path) &&
           fileManager.fileExists(atPath: prosodyPredictorURL.path) &&
           fileManager.fileExists(atPath: vitsDecoderURL.path) {
            self.hasSplitModels = true
        } else {
            self.hasSplitModels = false
        }
    }

    private func getTextEncoderModel() throws -> MLModel {
        try textEncoderStore.withLock { store in
            if let model = store { return model }
            let url = modelsDirectory.appendingPathComponent("TextEncoder.mlmodelc")
            let model = try MLModel(contentsOf: url, configuration: config)
            store = model
            return model
        }
    }
    
    private func getProsodyPredictorModel() throws -> MLModel {
        try prosodyPredictorStore.withLock { store in
            if let model = store { return model }
            let url = modelsDirectory.appendingPathComponent("ProsodyPredictor.mlmodelc")
            let model = try MLModel(contentsOf: url, configuration: config)
            store = model
            return model
        }
    }
    
    private func getVitsDecoderModel() throws -> MLModel {
        try vitsDecoderStore.withLock { store in
            if let model = store { return model }
            let url = modelsDirectory.appendingPathComponent("VitsDecoder.mlmodelc")
            let model = try MLModel(contentsOf: url, configuration: config)
            store = model
            return model
        }
    }
    
    private func getE2EModel() throws -> MLModel {
        try e2eModelStore.withLock { store in
            if let model = store { return model }
            let url = modelsDirectory.appendingPathComponent("styletts2_lite.mlmodelc")
            guard FileManager.default.fileExists(atPath: url.path) else {
                throw NSError(
                    domain: "CoreMlProsodiaActorEngine",
                    code: 404,
                    userInfo: [
                        NSLocalizedDescriptionKey: "CoreML model components not found in directory '\(modelsDirectory.path)'."
                    ]
                )
            }
            let model = try MLModel(contentsOf: url, configuration: config)
            store = model
            return model
        }
    }

    public func reclaimMemory() {
        textEncoderStore.withLock { $0 = nil }
        prosodyPredictorStore.withLock { $0 = nil }
        vitsDecoderStore.withLock { $0 = nil }
        e2eModelStore.withLock { $0 = nil }
    }

    public func tokenize(_ phonemes: String) throws -> [Int] {
        let ids = phonemes.compactMap { vocab[String($0)] }
        return [0] + ids + [0]
    }

    public func forward(
        phonemes: String,
        refS: StyleVector,
        speed: Float = 1.0,
        durationScales: [Float]? = nil,
        f0Bias: [Float]? = nil
    ) throws -> ActorEngineOutput {
        let ids = try tokenize(phonemes)
        
        // Use split-model pipeline if all three models are present
        if hasSplitModels {
            let encoder = try getTextEncoderModel()
            let predictor = try getProsodyPredictorModel()
            let decoder = try getVitsDecoderModel()
            
            // 1. Text Encoder Inference
            // Input shape: [1, 128]
            let tokensArray = try MLMultiArray(shape: [1, 128], dataType: .int32)
            let tokensPtr = tokensArray.dataPointer.assumingMemoryBound(to: Int32.self)
            for i in 0..<128 {
                tokensPtr[i] = i < ids.count ? Int32(ids[i]) : 0
            }
            
            let encoderInputs: [String: Any] = ["input_ids": tokensArray]
            let encoderInputProvider = try MLDictionaryFeatureProvider(dictionary: encoderInputs)
            let encoderOutput = try encoder.prediction(from: encoderInputProvider)
            
            let textEmbeddingsName = Array(encoder.modelDescription.outputDescriptionsByName.keys).first ?? "text_embeddings"
            guard let textEmbeddings = encoderOutput.featureValue(for: textEmbeddingsName)?.multiArrayValue else {
                throw NSError(
                    domain: "CoreMlProsodiaActorEngine",
                    code: 500,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to extract text embeddings from TextEncoder."]
                )
            }
            
            // 2. Prosody Predictor Inference
            // Input shape: text_embeddings [1, 128, 256], style_vector [1, 256]
            let styleArray = try MLMultiArray(shape: [1, 256], dataType: .float32)
            let styleFloats = refS.data
            let stylePtr = styleArray.dataPointer.assumingMemoryBound(to: Float.self)
            let limit = min(256, styleFloats.count)
            styleFloats.withUnsafeBufferPointer { srcPtr in
                if let srcBase = srcPtr.baseAddress {
                    stylePtr.initialize(from: srcBase, count: limit)
                }
            }
            if limit < 256 {
                for i in limit..<256 {
                    stylePtr[i] = 0.0
                }
            }
            
            let predictorInputs: [String: Any] = [
                "text_embeddings": textEmbeddings,
                "style_vector": styleArray
            ]
            let predictorInputProvider = try MLDictionaryFeatureProvider(dictionary: predictorInputs)
            let predictorOutput = try predictor.prediction(from: predictorInputProvider)
            
            let outputNames = Array(predictor.modelDescription.outputDescriptionsByName.keys)
            let durationLogitsName = outputNames.first(where: { $0.contains("duration") }) ?? "duration_logits"
            let f0Name = outputNames.first(where: { $0.contains("f0") }) ?? "f0"
            let energyName = outputNames.first(where: { $0.contains("n") || $0.contains("energy") }) ?? "n"
            
            guard let durationLogits = predictorOutput.featureValue(for: durationLogitsName)?.multiArrayValue,
                  let f0Array = predictorOutput.featureValue(for: f0Name)?.multiArrayValue,
                  let energyArray = predictorOutput.featureValue(for: energyName)?.multiArrayValue else {
                throw NSError(
                    domain: "CoreMlProsodiaActorEngine",
                    code: 500,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to extract prosody predictor outputs."]
                )
            }
            
            // Decode durations via sigmoid sum and apply speed & duration scales
            let maxDur = durationLogits.shape.count >= 3 ? durationLogits.shape[2].intValue : 10
            let durationLogitsPtr = durationLogits.dataPointer.assumingMemoryBound(to: Float.self)
            var predDur: [Int] = []
            var totalFrames = 0
            for i in 0..<ids.count {
                var sumSigmoid: Float = 0.0
                let offset = i * maxDur
                for d in 0..<maxDur {
                    let val = durationLogitsPtr[offset + d]
                    sumSigmoid += 1.0 / (1.0 + exp(-val))
                }
                var durVal = sumSigmoid / speed
                if let durationScales, i < durationScales.count {
                    durVal *= durationScales[i]
                }
                let dur = max(1, Int(durVal.rounded()))
                predDur.append(dur)
                totalFrames += dur
            }
            
            if totalFrames <= 0 {
                totalFrames = 128
            }
            
            // 3. Construct Aligned Inputs (VITS Durations Expansion)
            // aligned_text: [1, totalFrames, 256]
            // f0_curve: [1, totalFrames]
            // energy_curve: [1, totalFrames]
            let alignedText = try MLMultiArray(shape: [1, NSNumber(value: totalFrames), 256], dataType: .float32)
            let f0Curve = try MLMultiArray(shape: [1, NSNumber(value: totalFrames)], dataType: .float32)
            let energyCurve = try MLMultiArray(shape: [1, NSNumber(value: totalFrames)], dataType: .float32)
            
            let alignedTextPtr = alignedText.dataPointer.assumingMemoryBound(to: Float.self)
            let f0CurvePtr = f0Curve.dataPointer.assumingMemoryBound(to: Float.self)
            let energyCurvePtr = energyCurve.dataPointer.assumingMemoryBound(to: Float.self)
            
            let textEmbeddingsPtr = textEmbeddings.dataPointer.assumingMemoryBound(to: Float.self)
            let f0ArrayPtr = f0Array.dataPointer.assumingMemoryBound(to: Float.self)
            let energyArrayPtr = energyArray.dataPointer.assumingMemoryBound(to: Float.self)
            
            var frameIdx = 0
            for i in 0..<ids.count {
                let dur = predDur[i]
                var f0Val = f0ArrayPtr[i]
                if let f0Bias, i < f0Bias.count {
                    f0Val += f0Bias[i]
                }
                let energyVal = energyArrayPtr[i]
                
                let srcEmbeddingPtr = textEmbeddingsPtr.advanced(by: i * 256)
                
                for _ in 0..<dur {
                    if frameIdx >= totalFrames { break }
                    
                    let dstEmbeddingPtr = alignedTextPtr.advanced(by: frameIdx * 256)
                    dstEmbeddingPtr.initialize(from: srcEmbeddingPtr, count: 256)
                    
                    f0CurvePtr[frameIdx] = f0Val
                    energyCurvePtr[frameIdx] = energyVal
                    
                    frameIdx += 1
                }
            }
            
            while frameIdx < totalFrames {
                f0CurvePtr[frameIdx] = 0.0
                energyCurvePtr[frameIdx] = 0.0
                frameIdx += 1
            }
            
            // 4. VitsDecoder Inference
            let decoderInputs: [String: Any] = [
                "aligned_text": alignedText,
                "style_vector": styleArray,
                "f0_curve": f0Curve,
                "energy_curve": energyCurve
            ]
            let decoderInputProvider = try MLDictionaryFeatureProvider(dictionary: decoderInputs)
            let decoderOutput = try decoder.prediction(from: decoderInputProvider)
            
            let audioName = Array(decoder.modelDescription.outputDescriptionsByName.keys).first ?? "audio"
            guard let audioMultiArray = decoderOutput.featureValue(for: audioName)?.multiArrayValue else {
                throw NSError(
                    domain: "CoreMlProsodiaActorEngine",
                    code: 500,
                    userInfo: [NSLocalizedDescriptionKey: "Failed to extract audio output from Decoder."]
                )
            }
            
            let count = audioMultiArray.count
            var audioFrames = [Float](repeating: 0.0, count: count)
            let ptr = audioMultiArray.dataPointer.assumingMemoryBound(to: Float.self)
            audioFrames.withUnsafeMutableBufferPointer { destPtr in
                _ = destPtr.baseAddress?.initialize(from: ptr, count: count)
            }
            
            var trimIndex = count
            while trimIndex > 0 && abs(audioFrames[trimIndex - 1]) < 1e-4 {
                trimIndex -= 1
            }
            let trimmedAudio = Array(audioFrames[0..<max(1, trimIndex)])
            
            return ActorEngineOutput(audio: trimmedAudio, predDur: predDur)
        }
        
        // Fallback to single unified model (e2eModel)
        let e2eModel = try getE2EModel()

        let tokensArray = try MLMultiArray(shape: [1, 128], dataType: .int32)
        let tokensPtr = tokensArray.dataPointer.assumingMemoryBound(to: Int32.self)
        for i in 0..<128 {
            tokensPtr[i] = i < ids.count ? Int32(ids[i]) : 0
        }
        
        let attentionMask = try MLMultiArray(shape: [1, 128], dataType: .int32)
        let attentionMaskPtr = attentionMask.dataPointer.assumingMemoryBound(to: Int32.self)
        for i in 0..<128 {
            attentionMaskPtr[i] = i < ids.count ? 1 : 0
        }
        
        let randomPhases = try MLMultiArray(shape: [1, 9], dataType: .float32)
        let randomPhasesPtr = randomPhases.dataPointer.assumingMemoryBound(to: Float.self)
        for i in 0..<9 {
            randomPhasesPtr[i] = 0.0
        }
        
        // Shape of style is [1, 256]
        let styleArray = try MLMultiArray(shape: [1, 256], dataType: .float32)
        let styleFloats = refS.data
        let stylePtr = styleArray.dataPointer.assumingMemoryBound(to: Float.self)
        let limit = min(256, styleFloats.count)
        styleFloats.withUnsafeBufferPointer { srcPtr in
            if let srcBase = srcPtr.baseAddress {
                stylePtr.initialize(from: srcBase, count: limit)
            }
        }
        if limit < 256 {
            for i in limit..<256 {
                stylePtr[i] = 0.0
            }
        }
        
        // Shape of speed is [1]
        let speedArray = try MLMultiArray(shape: [1], dataType: .float32)
        let speedPtr = speedArray.dataPointer.assumingMemoryBound(to: Float.self)
        speedPtr[0] = speed

        // Run inference via MLFeatureProvider
        let inputDict: [String: Any] = [
            "input_ids": tokensArray,
            "ref_s": styleArray,
            "speed": speedArray,
            "attention_mask": attentionMask,
            "random_phases": randomPhases
        ]
        let inputProvider = try MLDictionaryFeatureProvider(dictionary: inputDict)
        let output = try e2eModel.prediction(from: inputProvider)
        
        guard let audioMultiArray = output.featureValue(for: "audio")?.multiArrayValue else {
            throw NSError(
                domain: "CoreMlProsodiaActorEngine",
                code: 500,
                userInfo: [
                    NSLocalizedDescriptionKey: "Failed to extract 'audio' output from CoreML prediction."
                ]
            )
        }
        
        let count = audioMultiArray.count
        var audioFrames = [Float](repeating: 0.0, count: count)
        let ptr = audioMultiArray.dataPointer.assumingMemoryBound(to: Float.self)
        audioFrames.withUnsafeMutableBufferPointer { destPtr in
            _ = destPtr.baseAddress?.initialize(from: ptr, count: count)
        }
        
        // Trailing silence trimming logic:
        var trimIndex = count
        while trimIndex > 0 && abs(audioFrames[trimIndex - 1]) < 1e-4 {
            trimIndex -= 1
        }
        let trimmedAudio = Array(audioFrames[0..<max(1, trimIndex)])
        
        // Extract predicted durations from the unified model output for correct timestamps
        var predDur: [Int] = []
        if let predDurMultiArray = output.featureValue(for: "pred_dur")?.multiArrayValue {
            let predDurPtr = predDurMultiArray.dataPointer.assumingMemoryBound(to: Float.self)
            for i in 0..<ids.count {
                let val = predDurPtr[i]
                predDur.append(max(1, Int(val.rounded())))
            }
        } else {
            predDur = [Int](repeating: 1, count: ids.count)
        }
        
        return ActorEngineOutput(audio: trimmedAudio, predDur: predDur)
    }
}
