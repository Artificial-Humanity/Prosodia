import Foundation

/// Configuration format for StyleTTS2 models, specifying vocabulary maps and acoustic parameters.
public struct StyleTTS2Config: Codable, Sendable {
    /// Settings for the iSTFTNet spectrogram decoder.
    public struct IstftNetConfig: Codable, Sendable {
        public let resblockDilationSizes: [[Int]]
        public let upsampleKernelSizes: [Int]
        public let genIstftNFFT: Int
        public let genIstftHopSize: Int

        enum CodingKeys: String, CodingKey {
            case resblockDilationSizes = "resblock_dilation_sizes"
            case upsampleKernelSizes = "upsample_kernel_sizes"
            case genIstftNFFT = "gen_istft_nfft"
            case genIstftHopSize = "gen_istft_hop_size"
        }
    }

    /// The mapping of character/grapheme symbols to token IDs.
    public let vocab: [String: Int]
    /// The size of the style embedding dimension.
    public let styleDim: Int
    /// Configuration for the iSTFTNet decoder.
    public let istftnet: IstftNetConfig

    enum CodingKeys: String, CodingKey {
        case vocab
        case styleDim = "style_dim"
        case istftnet
    }
}

/// Errors raised during StyleTTS2 voice loading, tokenization, or inference.
public enum StyleTTS2Error: Error, LocalizedError, Sendable {
    case missingWeights(URL)
    case invalidPhonemeLength(Int)
    case unsupportedBatch(Int)
    case invalidSpeed(Float)
    case unsupportedLanguageCode(String)
    case missingVoice(String)
    case expected2DVoicePack(String, [Int])

    public var errorDescription: String? {
        switch self {
        case .missingWeights(let url):
            return "Missing model weight file at: \(url.path)"
        case .invalidPhonemeLength(let length):
            return "Phoneme token length \(length) exceeds context window limits."
        case .unsupportedBatch(let batchSize):
            return "Inference only supports a batch size of 1. Got: \(batchSize)"
        case .invalidSpeed(let speed):
            return "Invalid synthesis speed multiplier (must be finite and > 0): \(speed)"
        case .unsupportedLanguageCode(let code):
            return "Unsupported language code requested: \(code)"
        case .missingVoice(let name):
            return "Requested voice is not loaded or missing: \(name)"
        case .expected2DVoicePack(let name, let shape):
            return "Expected 2D matrix shape for voice pack '\(name)', but got: \(shape)"
        }
    }
}
