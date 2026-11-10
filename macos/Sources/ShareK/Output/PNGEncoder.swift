import CoreGraphics
import Foundation
import zlib

enum PNGEncoder {
    private static let signature: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    private static let outputChunk = 1 << 16

    static func encode(_ pixels: Pixels, scale: CGFloat?, fast: Bool) -> Data? {
        let width = pixels.width
        let height = pixels.height
        let opaque = pixels.isOpaque
        let channels = opaque ? 3 : 4
        let rowLength = width * channels

        var header = Data()
        header.append(bigEndian: UInt32(width))
        header.append(bigEndian: UInt32(height))
        header.append(contentsOf: [8, opaque ? 2 : 6, 0, 0, 0])

        let space = pixels.colorSpace
        var profile: (type: String, body: Data)?
        if (space.name as String?) == (CGColorSpace.sRGB as String) {
            profile = ("sRGB", Data([0]))
        } else if let icc = space.copyICCData() as Data?, let compressed = deflated(icc) {
            var body = Data("ICC Profile".utf8)
            body.append(contentsOf: [0, 0])
            body.append(compressed)
            profile = ("iCCP", body)
        }

        var density: Data?
        if let scale {
            let perMeter = UInt32((72 * scale / 0.0254).rounded())
            var body = Data()
            body.append(bigEndian: perMeter)
            body.append(bigEndian: perMeter)
            body.append(1)
            density = body
        }

        var stream = z_stream()
        guard deflateInit_(&stream, fast ? Z_BEST_SPEED : Z_DEFAULT_COMPRESSION, ZLIB_VERSION, Int32(MemoryLayout<z_stream>.size)) == Z_OK else {
            return nil
        }
        defer { deflateEnd(&stream) }

        let bound = Int(deflateBound(&stream, uLong((rowLength + 1) * height)))
        let capacity = 1024 + (profile?.body.count ?? 0) + bound + 12 * (height + bound / outputChunk + 2)

        let previous = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLength)
        let current = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLength)
        let filtered = UnsafeMutablePointer<UInt8>.allocate(capacity: 4 * (rowLength + 1))
        let compressed = UnsafeMutablePointer<UInt8>.allocate(capacity: outputChunk)
        defer {
            previous.deallocate()
            current.deallocate()
            filtered.deallocate()
            compressed.deallocate()
        }
        previous.initialize(repeating: 0, count: rowLength)
        current.initialize(repeating: 0, count: rowLength)
        filtered.initialize(repeating: 0, count: 4 * (rowLength + 1))

        let stride = pixels.format.bytesPerRow
        let source = pixels.data
        return MappedData.make(capacity: capacity) { start in
            var writer = ChunkWriter(start: start, capacity: capacity)
            writer.append(Data(signature))
            writer.chunk("IHDR", header)
            if let profile { writer.chunk(profile.type, profile.body) }
            if let density { writer.chunk("pHYs", density) }
            let succeeded = source.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
                guard let base = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return false }
                for row in 0..<height {
                    convert(base + row * stride, into: current, width: width, opaque: opaque)
                    let best = filter(current, previous: previous, length: rowLength, channels: channels, into: filtered)
                    stream.next_in = filtered + best * (rowLength + 1)
                    stream.avail_in = uInt(rowLength + 1)
                    let flush = row == height - 1 ? Z_FINISH : Z_NO_FLUSH
                    while true {
                        stream.next_out = compressed
                        stream.avail_out = uInt(outputChunk)
                        let status = deflate(&stream, flush)
                        guard status == Z_OK || status == Z_STREAM_END || status == Z_BUF_ERROR else { return false }
                        let produced = outputChunk - Int(stream.avail_out)
                        if produced > 0 { writer.chunk("IDAT", UnsafeRawBufferPointer(start: compressed, count: produced)) }
                        if (flush == Z_FINISH) ? (status == Z_STREAM_END) : (stream.avail_out > 0) { break }
                    }
                    previous.update(from: current, count: rowLength)
                }
                return true
            }
            writer.chunk("IEND", Data())
            return succeeded && !writer.overflow ? writer.count : nil
        }
    }

    private static func convert(_ source: UnsafePointer<UInt8>, into target: UnsafeMutablePointer<UInt8>, width: Int, opaque: Bool) {
        if opaque {
            for x in 0..<width {
                let pixel = source + x * 4
                let out = target + x * 3
                out[0] = pixel[2]
                out[1] = pixel[1]
                out[2] = pixel[0]
            }
            return
        }
        for x in 0..<width {
            let pixel = source + x * 4
            let out = target + x * 4
            let alpha = Int(pixel[3])
            switch alpha {
            case 255:
                out[0] = pixel[2]
                out[1] = pixel[1]
                out[2] = pixel[0]
            case 0:
                out[0] = 0
                out[1] = 0
                out[2] = 0
            default:
                out[0] = UInt8(min(255, (Int(pixel[2]) * 255 + alpha / 2) / alpha))
                out[1] = UInt8(min(255, (Int(pixel[1]) * 255 + alpha / 2) / alpha))
                out[2] = UInt8(min(255, (Int(pixel[0]) * 255 + alpha / 2) / alpha))
            }
            out[3] = UInt8(alpha)
        }
    }

    private static func filter(
        _ row: UnsafeMutablePointer<UInt8>, previous: UnsafeMutablePointer<UInt8>,
        length: Int, channels: Int, into output: UnsafeMutablePointer<UInt8>
    ) -> Int {
        var best = 0
        var bestScore = Int.max
        for type in 0..<4 {
            let target = output + type * (length + 1)
            target[0] = UInt8(type == 3 ? 4 : type)
            var score = 0
            for index in 0..<length {
                let left = index >= channels ? row[index - channels] : 0
                let up = previous[index]
                let value: UInt8
                switch type {
                case 0: value = row[index]
                case 1: value = row[index] &- left
                case 2: value = row[index] &- up
                default:
                    let corner = index >= channels ? previous[index - channels] : 0
                    value = row[index] &- paeth(left, up, corner)
                }
                target[index + 1] = value
                score += value < 128 ? Int(value) : 256 - Int(value)
            }
            if score < bestScore {
                bestScore = score
                best = type
            }
        }
        return best
    }

    private static func paeth(_ left: UInt8, _ up: UInt8, _ corner: UInt8) -> UInt8 {
        let estimate = Int(left) + Int(up) - Int(corner)
        let toLeft = abs(estimate - Int(left))
        let toUp = abs(estimate - Int(up))
        let toCorner = abs(estimate - Int(corner))
        if toLeft <= toUp, toLeft <= toCorner { return left }
        return toUp <= toCorner ? up : corner
    }

    private static func deflated(_ data: Data) -> Data? {
        var length = compressBound(uLong(data.count))
        var result = Data(count: Int(length))
        let status = result.withUnsafeMutableBytes { target -> Int32 in
            data.withUnsafeBytes { source -> Int32 in
                compress2(
                    target.baseAddress?.assumingMemoryBound(to: Bytef.self), &length,
                    source.baseAddress?.assumingMemoryBound(to: Bytef.self), uLong(data.count), Z_DEFAULT_COMPRESSION
                )
            }
        }
        guard status == Z_OK else { return nil }
        return result.prefix(Int(length))
    }
}

private struct ChunkWriter {
    let start: UnsafeMutableRawPointer
    let capacity: Int
    private(set) var count = 0
    private(set) var overflow = false

    init(start: UnsafeMutableRawPointer, capacity: Int) {
        self.start = start
        self.capacity = capacity
    }

    mutating func append(_ bytes: UnsafeRawBufferPointer) {
        guard !overflow, count + bytes.count <= capacity else {
            overflow = true
            return
        }
        if let base = bytes.baseAddress { (start + count).copyMemory(from: base, byteCount: bytes.count) }
        count += bytes.count
    }

    mutating func append(_ data: Data) {
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in append(raw) }
    }

    mutating func append(bigEndian value: UInt32) {
        Swift.withUnsafeBytes(of: value.bigEndian) { append($0) }
    }

    mutating func chunk(_ type: String, _ body: UnsafeRawBufferPointer) {
        let name = Array(type.utf8)
        append(bigEndian: UInt32(body.count))
        name.withUnsafeBytes { append($0) }
        append(body)
        var checksum = name.withUnsafeBytes { crc32(0, $0.baseAddress?.assumingMemoryBound(to: Bytef.self), uInt($0.count)) }
        if body.count > 0 {
            checksum = crc32(checksum, body.baseAddress?.assumingMemoryBound(to: Bytef.self), uInt(body.count))
        }
        append(bigEndian: UInt32(truncatingIfNeeded: checksum))
    }

    mutating func chunk(_ type: String, _ body: Data) {
        body.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in chunk(type, raw) }
    }
}

private extension Data {
    mutating func append(bigEndian value: UInt32) {
        Swift.withUnsafeBytes(of: value.bigEndian) { append(contentsOf: $0) }
    }
}
