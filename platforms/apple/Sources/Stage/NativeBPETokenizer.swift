import Foundation

/// A zero-dependency, single-file Swift implementation of a Byte-Pair Encoding (BPE) tokenizer.
/// It loads pre-compiled `.pvocab` binary files containing vocabulary mappings and merge rules.
public struct NativeBPETokenizer: Sendable {
    
    public struct Pair: Hashable, Sendable {
        public let first: String
        public let second: String
        
        public init(_ first: String, _ second: String) {
            self.first = first
            self.second = second
        }
    }
    
    /// Mapping from token strings to token IDs.
    public let vocab: [String: Int]
    
    /// Mapping from token IDs to token strings.
    public let inverseVocab: [Int: String]
    
    /// Mapping from adjacent subword pairs to their merge rank.
    public let merges: [Pair: Int]
    
    /// Whether to treat input at the byte-level (like GPT-2, GPT-4, Qwen) or raw character-level.
    public let byteLevel: Bool
    
    /// Optional token ID to fall back on when a subword is out-of-vocabulary.
    public let unknownTokenId: Int?
    
    // GPT-2/Qwen byte-to-unicode translation maps
    private let byteToUnicode: [UInt8: Character]
    private let unicodeToByte: [Character: UInt8]
    
    /// Initializes a tokenizer by loading a binary `.pvocab` file from the specified URL.
    public init(contentsOf url: URL, byteLevel: Bool = true, unknownTokenId: Int? = nil) throws {
        let data = try Data(contentsOf: url)
        try self.init(data: data, byteLevel: byteLevel, unknownTokenId: unknownTokenId)
    }
    
    /// Initializes a tokenizer by parsing binary `.pvocab` data.
    public init(data: Data, byteLevel: Bool = true, unknownTokenId: Int? = nil) throws {
        // Read header (Magic: 4 bytes, Version: 2 bytes, Vocab size: 4 bytes, Merges size: 4 bytes)
        guard data.count >= 14 else {
            throw NSError(
                domain: "NativeBPETokenizer",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Invalid file size for .pvocab format."]
            )
        }
        
        let magic = data.subdata(in: 0..<4)
        guard magic == Data("PVOC".utf8) else {
            throw NSError(
                domain: "NativeBPETokenizer",
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Invalid magic header. Expected 'PVOC'."]
            )
        }
        
        var offset = 4
        let version = Self.readUInt16(from: data, at: offset)
        offset += 2
        guard version == 1 else {
            throw NSError(
                domain: "NativeBPETokenizer",
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: "Unsupported version: \(version)"]
            )
        }
        
        let vocabCount = Int(Self.readUInt32(from: data, at: offset))
        offset += 4
        
        let mergesCount = Int(Self.readUInt32(from: data, at: offset))
        offset += 4
        
        var vocab: [String: Int] = [:]
        var inverseVocab: [Int: String] = [:]
        
        // Parse Vocab Section
        for _ in 0..<vocabCount {
            guard offset + 6 <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 4,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed vocabulary section."]
                )
            }
            let tokenID = Int(Self.readUInt32(from: data, at: offset))
            offset += 4
            let len = Int(Self.readUInt16(from: data, at: offset))
            offset += 2
            
            guard offset + len <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 5,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed token string in vocabulary."]
                )
            }
            let tokenData = data.subdata(in: offset..<(offset + len))
            offset += len
            
            if let tokenStr = String(data: tokenData, encoding: .utf8) {
                vocab[tokenStr] = tokenID
                inverseVocab[tokenID] = tokenStr
            }
        }
        
        // Parse Merges Section
        var merges: [Pair: Int] = [:]
        for _ in 0..<mergesCount {
            guard offset + 8 <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 6,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed merges section."]
                )
            }
            let rank = Int(Self.readUInt32(from: data, at: offset))
            offset += 4
            
            let len1 = Int(Self.readUInt16(from: data, at: offset))
            offset += 2
            guard offset + len1 <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 7,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed merge rule part 1."]
                )
            }
            let firstData = data.subdata(in: offset..<(offset + len1))
            offset += len1
            guard let firstStr = String(data: firstData, encoding: .utf8) else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 8,
                    userInfo: [NSLocalizedDescriptionKey: "Invalid UTF-8 in merge rule part 1."]
                )
            }
            
            guard offset + 2 <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 9,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed merge rule part 2 length."]
                )
            }
            let len2 = Int(Self.readUInt16(from: data, at: offset))
            offset += 2
            guard offset + len2 <= data.count else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 10,
                    userInfo: [NSLocalizedDescriptionKey: "Malformed merge rule part 2."]
                )
            }
            let secondData = data.subdata(in: offset..<(offset + len2))
            offset += len2
            guard let secondStr = String(data: secondData, encoding: .utf8) else {
                throw NSError(
                    domain: "NativeBPETokenizer",
                    code: 11,
                    userInfo: [NSLocalizedDescriptionKey: "Invalid UTF-8 in merge rule part 2."]
                )
            }
            
            merges[Pair(firstStr, secondStr)] = rank
        }
        
        self.vocab = vocab
        self.inverseVocab = inverseVocab
        self.merges = merges
        self.byteLevel = byteLevel
        self.unknownTokenId = unknownTokenId
        
        let (b2u, u2b) = Self.createByteToUnicodeMaps()
        self.byteToUnicode = b2u
        self.unicodeToByte = u2b
    }
    
    // MARK: - Core API
    
    /// Encodes the input string into a list of token IDs.
    public func encode(_ text: String) -> [Int] {
        if text.isEmpty { return [] }
        
        let words = preTokenize(text)
        var tokenIDs: [Int] = []
        
        for word in words {
            if byteLevel {
                // Map word characters to UTF-8 bytes, then translate to unicode-byte characters
                let utf8Bytes = Array(word.utf8)
                var mappedWord = ""
                for byte in utf8Bytes {
                    if let char = byteToUnicode[byte] {
                        mappedWord.append(char)
                    }
                }
                
                let subwords = bpe(word: mappedWord)
                for subword in subwords {
                    if let id = vocab[subword] {
                        tokenIDs.append(id)
                    } else if let unkId = unknownTokenId {
                        tokenIDs.append(unkId)
                    }
                }
            } else {
                let subwords = bpe(word: word)
                for subword in subwords {
                    if let id = vocab[subword] {
                        tokenIDs.append(id)
                    } else if let unkId = unknownTokenId {
                        tokenIDs.append(unkId)
                    }
                }
            }
        }
        return tokenIDs
    }
    
    /// Decodes a list of token IDs back into a single string.
    public func decode(_ tokens: [Int]) -> String {
        var joinedString = ""
        for token in tokens {
            if let tokenStr = inverseVocab[token] {
                joinedString.append(tokenStr)
            }
        }
        
        if byteLevel {
            // Revert unicode character mappings back to original raw bytes
            var bytes: [UInt8] = []
            for char in joinedString {
                if let byte = unicodeToByte[char] {
                    bytes.append(byte)
                } else {
                    // Fallback to UTF-8 bytes of the character itself if it wasn't a mapped byte
                    bytes.append(contentsOf: String(char).utf8)
                }
            }
            return String(bytes: bytes, encoding: .utf8) ?? joinedString
        } else {
            return joinedString
        }
    }
    
    // MARK: - Private Helper Implementation
    
    /// Standard GPT-2/Qwen/GPT-4 regex pre-tokenizer.
    private func preTokenize(_ text: String) -> [String] {
        guard let regex = try? NSRegularExpression(
            pattern: "'s|'t|'re|'ve|'m|'ll|'d| ?\\p{L}+| ?\\p{N}+| ?[^\\s\\p{L}\\p{N}]+|\\s+(?!\\S)|\\s+",
            options: []
        ) else {
            return [text]
        }
        
        let nsString = text as NSString
        let results = regex.matches(in: text, options: [], range: NSRange(location: 0, length: nsString.length))
        return results.map { nsString.substring(with: $0.range) }
    }
    
    /// The greedy BPE merge loop.
    private func bpe(word: String) -> [String] {
        var parts = word.map { String($0) }
        if parts.isEmpty { return [] }
        
        while true {
            var bestPair: Pair? = nil
            var minRank = Int.max
            
            for i in 0..<(parts.count - 1) {
                let pair = Pair(parts[i], parts[i+1])
                if let rank = merges[pair] {
                    if rank < minRank {
                        minRank = rank
                        bestPair = pair
                    }
                }
            }
            
            guard let pairToMerge = bestPair else {
                break
            }
            
            // Perform merge of all adjacent matches in the array
            var newParts: [String] = []
            var i = 0
            while i < parts.count {
                if i < parts.count - 1 && parts[i] == pairToMerge.first && parts[i+1] == pairToMerge.second {
                    newParts.append(pairToMerge.first + pairToMerge.second)
                    i += 2
                } else {
                    newParts.append(parts[i])
                    i += 1
                }
            }
            parts = newParts
        }
        return parts
    }
    
    // MARK: - Binary IO Helpers
    
    private static func readUInt32(from data: Data, at offset: Int) -> UInt32 {
        let b0 = UInt32(data[offset])
        let b1 = UInt32(data[offset + 1])
        let b2 = UInt32(data[offset + 2])
        let b3 = UInt32(data[offset + 3])
        return b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }
    
    private static func readUInt16(from data: Data, at offset: Int) -> UInt16 {
        let b0 = UInt16(data[offset])
        let b1 = UInt16(data[offset + 1])
        return b0 | (b1 << 8)
    }
    
    /// Generates standard byte-to-unicode maps to prevent invalid UTF-8 byte sequences during tokenization.
    private static func createByteToUnicodeMaps() -> ([UInt8: Character], [Character: UInt8]) {
        var bs: [UInt8] = []
        // Range 1: '!' (33) to '~' (126)
        for b in UInt8(ascii: "!")...UInt8(ascii: "~") {
            bs.append(b)
        }
        // Range 2: '¡' (161) to '¬' (172)
        for b in 161...172 {
            bs.append(UInt8(b))
        }
        // Range 3: '®' (174) to 'ÿ' (255)
        for b in 174...255 {
            bs.append(UInt8(b))
        }
        
        var cs = bs.map { Int($0) }
        var n = 0
        for b in 0...255 {
            let u8 = UInt8(b)
            if !bs.contains(u8) {
                bs.append(u8)
                cs.append(256 + n)
                n += 1
            }
        }
        
        var byteToUnicode: [UInt8: Character] = [:]
        var unicodeToByte: [Character: UInt8] = [:]
        
        for (b, cVal) in zip(bs, cs) {
            if let scalar = UnicodeScalar(cVal) {
                let char = Character(scalar)
                byteToUnicode[b] = char
                unicodeToByte[char] = b
            }
        }
        
        return (byteToUnicode, unicodeToByte)
    }
}
