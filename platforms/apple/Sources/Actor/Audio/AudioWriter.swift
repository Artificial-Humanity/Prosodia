import Foundation

public enum AudioWriter {
    public static func limit(_ samples: [Float]) -> [Float] {
        let threshold: Float = 0.85
        let ceiling: Float = 0.98
        let diff = ceiling - threshold
        
        return samples.map { x in
            let absX = abs(x)
            if absX <= threshold {
                return x
            } else {
                let sign: Float = x >= 0 ? 1.0 : -1.0
                return sign * (threshold + diff * tanh((absX - threshold) / diff))
            }
        }
    }

    public static func wavData(samples: [Float], sampleRate: Int = 24_000) -> Data {
        let limited = limit(samples)
        let pcm = limited.map { Int16((max(-1.0, min(1.0, $0)) * Float(Int16.max)).rounded()) }

        let bytesPerSample = MemoryLayout<Int16>.size
        let dataSize = pcm.count * bytesPerSample
        let riffSize = 36 + dataSize

        var data = Data()
        data.append("RIFF".data(using: .ascii)!)
        data.append(littleEndian(UInt32(riffSize)))
        data.append("WAVE".data(using: .ascii)!)

        data.append("fmt ".data(using: .ascii)!)
        data.append(littleEndian(UInt32(16)))
        data.append(littleEndian(UInt16(1)))
        data.append(littleEndian(UInt16(1)))
        data.append(littleEndian(UInt32(sampleRate)))
        data.append(littleEndian(UInt32(sampleRate * bytesPerSample)))
        data.append(littleEndian(UInt16(bytesPerSample)))
        data.append(littleEndian(UInt16(16)))

        data.append("data".data(using: .ascii)!)
        data.append(littleEndian(UInt32(dataSize)))

        pcm.withUnsafeBufferPointer { buffer in
            data.append(contentsOf: UnsafeRawBufferPointer(buffer))
        }

        return data
    }

    @discardableResult
    public static func writeWAV(
        samples: [Float],
        to url: URL,
        sampleRate: Int = 24_000
    ) throws -> URL {
        try wavData(samples: samples, sampleRate: sampleRate).write(to: url)
        return url
    }

    private static func littleEndian(_ value: UInt16) -> Data {
        withUnsafeBytes(of: value.littleEndian) { Data($0) }
    }

    private static func littleEndian(_ value: UInt32) -> Data {
        withUnsafeBytes(of: value.littleEndian) { Data($0) }
    }
}
