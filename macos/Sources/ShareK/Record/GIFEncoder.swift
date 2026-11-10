import Foundation

final class GIFEncoder {
    let width: Int
    let height: Int

    private let handle: FileHandle
    private var reference: [UInt32]
    private var hasReference = false
    private var pending: Data?
    private var pendingTime: Double = 0

    private let quantizer = PaletteQuantizer()
    private let lzw = LZWCompressor()
    private var indices: [UInt8] = []
    private var changed: [Bool] = []

    var isEmpty: Bool { !hasReference }

    init(url: URL, width: Int, height: Int) throws {
        self.width = width
        self.height = height
        reference = [UInt32](repeating: 0, count: width * height)
        FileManager.default.createFile(atPath: url.path, contents: nil)
        handle = try FileHandle(forWritingTo: url)

        var header = Data("GIF89a".utf8)
        header.appendLE16(width)
        header.appendLE16(height)
        header.append(contentsOf: [0x70, 0, 0])
        header.append(contentsOf: [0x21, 0xFF, 0x0B])
        header.append(contentsOf: Array("NETSCAPE2.0".utf8))
        header.append(contentsOf: [0x03, 0x01, 0x00, 0x00, 0x00])
        try handle.write(contentsOf: header)
    }

    func append(bgra pixels: UnsafeRawPointer, bytesPerRow: Int, time: Double) throws {
        guard let bounds = changedBounds(pixels, bytesPerRow: bytesPerRow) else { return }
        try flushPending(until: time)

        let frameWidth = bounds.maxX - bounds.minX + 1
        let frameHeight = bounds.maxY - bounds.minY + 1
        let count = frameWidth * frameHeight
        if changed.count < count {
            changed = [Bool](repeating: false, count: count)
            indices = [UInt8](repeating: 0, count: count)
        }

        quantizer.begin()
        reference.withUnsafeMutableBufferPointer { reference in
            changed.withUnsafeMutableBufferPointer { changed in
                for y in 0..<frameHeight {
                    let row = (pixels + (bounds.minY + y) * bytesPerRow).assumingMemoryBound(to: UInt32.self)
                    let referenceRow = (bounds.minY + y) * width
                    for x in 0..<frameWidth {
                        let pixel = row[bounds.minX + x] | 0xFF00_0000
                        let differs = !hasReference || reference[referenceRow + bounds.minX + x] != pixel
                        changed[y * frameWidth + x] = differs
                        if differs { quantizer.add(pixel) }
                    }
                }
            }
        }
        let palette = quantizer.build(maxColors: 255)
        let transparent = UInt8(palette.count)

        reference.withUnsafeMutableBufferPointer { reference in
            indices.withUnsafeMutableBufferPointer { indices in
                changed.withUnsafeBufferPointer { changed in
                    for y in 0..<frameHeight {
                        let row = (pixels + (bounds.minY + y) * bytesPerRow).assumingMemoryBound(to: UInt32.self)
                        let referenceRow = (bounds.minY + y) * width
                        for x in 0..<frameWidth {
                            let offset = y * frameWidth + x
                            if changed[offset] {
                                let pixel = row[bounds.minX + x] | 0xFF00_0000
                                indices[offset] = quantizer.index(of: pixel)
                                reference[referenceRow + bounds.minX + x] = pixel
                            } else {
                                indices[offset] = transparent
                            }
                        }
                    }
                }
            }
        }
        quantizer.reset()
        hasReference = true

        var tableBits = 1
        while (1 << tableBits) < palette.count + 1 { tableBits += 1 }

        var frame = Data()
        frame.append(contentsOf: [0x21, 0xF9, 0x04, (1 << 2) | 1, 0, 0, transparent, 0])
        frame.append(0x2C)
        frame.appendLE16(bounds.minX)
        frame.appendLE16(bounds.minY)
        frame.appendLE16(frameWidth)
        frame.appendLE16(frameHeight)
        frame.append(0x80 | UInt8(tableBits - 1))
        for color in palette {
            frame.append(UInt8(truncatingIfNeeded: color >> 16))
            frame.append(UInt8(truncatingIfNeeded: color >> 8))
            frame.append(UInt8(truncatingIfNeeded: color))
        }
        frame.append(contentsOf: [UInt8](repeating: 0, count: ((1 << tableBits) - palette.count) * 3))
        indices.withUnsafeBufferPointer { indices in
            lzw.compress(UnsafeBufferPointer(rebasing: indices[0..<count]), minimumCodeSize: max(2, tableBits), into: &frame)
        }
        pending = frame
        pendingTime = time
    }

    func finish(at time: Double) throws {
        try flushPending(until: max(time, pendingTime + 0.1))
        try handle.write(contentsOf: Data([0x3B]))
        try handle.close()
    }

    private func flushPending(until time: Double) throws {
        guard var frame = pending else { return }
        let centiseconds = max(2, min(65535, Int(((time - pendingTime) * 100).rounded())))
        frame[4] = UInt8(centiseconds & 0xFF)
        frame[5] = UInt8(centiseconds >> 8)
        try handle.write(contentsOf: frame)
        pending = nil
    }

    private func changedBounds(_ pixels: UnsafeRawPointer, bytesPerRow: Int) -> (minX: Int, minY: Int, maxX: Int, maxY: Int)? {
        guard hasReference else { return (0, 0, width - 1, height - 1) }
        var minX = width, minY = height, maxX = -1, maxY = -1
        reference.withUnsafeBufferPointer { reference in
            for y in 0..<height {
                let row = (pixels + y * bytesPerRow).assumingMemoryBound(to: UInt32.self)
                let referenceRow = y * width
                var x = 0
                while x < width, reference[referenceRow + x] == row[x] | 0xFF00_0000 { x += 1 }
                guard x < width else { continue }
                var last = width - 1
                while last > x, reference[referenceRow + last] == row[last] | 0xFF00_0000 { last -= 1 }
                minX = min(minX, x)
                maxX = max(maxX, last)
                minY = min(minY, y)
                maxY = y
            }
        }
        return maxY < 0 ? nil : (minX, minY, maxX, maxY)
    }
}

private final class PaletteQuantizer {
    private static let bins = 1 << 18
    private var counts = [UInt32](repeating: 0, count: bins)
    private var sumR = [UInt32](repeating: 0, count: bins)
    private var sumG = [UInt32](repeating: 0, count: bins)
    private var sumB = [UInt32](repeating: 0, count: bins)
    private var lookup = [UInt8](repeating: 0, count: bins)
    private var touched: [Int32] = []

    func begin() {
        touched.removeAll(keepingCapacity: true)
    }

    @inline(__always)
    private static func bin(_ pixel: UInt32) -> Int {
        let r = Int((pixel >> 16) & 0xFF) >> 2
        let g = Int((pixel >> 8) & 0xFF) >> 2
        let b = Int(pixel & 0xFF) >> 2
        return (r << 12) | (g << 6) | b
    }

    @inline(__always)
    func add(_ pixel: UInt32) {
        let bin = Self.bin(pixel)
        if counts[bin] == 0 { touched.append(Int32(bin)) }
        counts[bin] &+= 1
        sumR[bin] &+= (pixel >> 16) & 0xFF
        sumG[bin] &+= (pixel >> 8) & 0xFF
        sumB[bin] &+= pixel & 0xFF
    }

    @inline(__always)
    func index(of pixel: UInt32) -> UInt8 {
        lookup[Self.bin(pixel)]
    }

    func build(maxColors: Int) -> [UInt32] {
        guard !touched.isEmpty else { return [0] }
        var boxes: [Range<Int>] = [0..<touched.count]
        var splittable = true
        while boxes.count < maxColors, splittable {
            splittable = false
            var best = -1
            var bestScore = 0
            var bestChannel = 0
            for (index, box) in boxes.enumerated() where box.count > 1 {
                let (channel, range, population) = measure(box)
                guard range > 0 else { continue }
                let score = range * range * Int(Double(population).squareRoot().rounded(.up))
                if score > bestScore {
                    bestScore = score
                    best = index
                    bestChannel = channel
                }
            }
            guard best >= 0 else { break }
            let box = boxes[best]
            let shift = (2 - bestChannel) * 6
            touched[box].sort { (($0 >> shift) & 63) < (($1 >> shift) & 63) }
            let total = touched[box].reduce(UInt32(0)) { $0 &+ counts[Int($1)] }
            var running: UInt32 = 0
            var cut = box.lowerBound + 1
            for position in box {
                running &+= counts[Int(touched[position])]
                if running >= total / 2 {
                    cut = min(max(position + 1, box.lowerBound + 1), box.upperBound - 1)
                    break
                }
            }
            boxes[best] = box.lowerBound..<cut
            boxes.append(cut..<box.upperBound)
            splittable = true
        }

        var palette: [UInt32] = []
        palette.reserveCapacity(boxes.count)
        for box in boxes {
            var population: UInt64 = 0, r: UInt64 = 0, g: UInt64 = 0, b: UInt64 = 0
            for position in box {
                let bin = Int(touched[position])
                population += UInt64(counts[bin])
                r += UInt64(sumR[bin])
                g += UInt64(sumG[bin])
                b += UInt64(sumB[bin])
            }
            let divisor = max(population, 1)
            palette.append(UInt32((r + divisor / 2) / divisor) << 16 | UInt32((g + divisor / 2) / divisor) << 8 | UInt32((b + divisor / 2) / divisor))
        }

        let channels = palette.map { (Int($0 >> 16), Int(($0 >> 8) & 0xFF), Int($0 & 0xFF)) }
        for bin in touched {
            let index = Int(bin)
            let population = max(counts[index], 1)
            let r = Int(sumR[index] / population)
            let g = Int(sumG[index] / population)
            let b = Int(sumB[index] / population)
            var best = 0
            var bestDistance = Int.max
            for (candidate, color) in channels.enumerated() {
                let dr = r - color.0, dg = g - color.1, db = b - color.2
                let distance = dr * dr * 3 + dg * dg * 4 + db * db * 2
                if distance < bestDistance {
                    bestDistance = distance
                    best = candidate
                    if distance == 0 { break }
                }
            }
            lookup[index] = UInt8(best)
        }
        return palette
    }

    func reset() {
        for bin in touched {
            let index = Int(bin)
            counts[index] = 0
            sumR[index] = 0
            sumG[index] = 0
            sumB[index] = 0
        }
        touched.removeAll(keepingCapacity: true)
    }

    private func measure(_ box: Range<Int>) -> (channel: Int, range: Int, population: UInt32) {
        var low = [63, 63, 63]
        var high = [0, 0, 0]
        var population: UInt32 = 0
        for position in box {
            let bin = Int(touched[position])
            population &+= counts[bin]
            let components = [(bin >> 12) & 63, (bin >> 6) & 63, bin & 63]
            for channel in 0..<3 {
                low[channel] = min(low[channel], components[channel])
                high[channel] = max(high[channel], components[channel])
            }
        }
        let ranges = (0..<3).map { high[$0] - low[$0] }
        let channel = ranges.firstIndex(of: ranges.max()!)!
        return (channel, ranges[channel], population)
    }
}

private final class LZWCompressor {
    private static let tableSize = 8192
    private var keys = [Int32](repeating: -1, count: tableSize)
    private var codes = [UInt16](repeating: 0, count: tableSize)

    func compress(_ input: UnsafeBufferPointer<UInt8>, minimumCodeSize: Int, into output: inout Data) {
        output.append(UInt8(minimumCodeSize))
        let clear = 1 << minimumCodeSize
        let end = clear + 1
        var codeSize = minimumCodeSize + 1
        var nextCode = end + 1

        var block = [UInt8]()
        block.reserveCapacity(255)
        var bitBuffer: UInt32 = 0
        var bitCount = 0

        func emit(_ code: Int) {
            bitBuffer |= UInt32(code) << UInt32(bitCount)
            bitCount += codeSize
            while bitCount >= 8 {
                block.append(UInt8(truncatingIfNeeded: bitBuffer))
                bitBuffer >>= 8
                bitCount -= 8
                if block.count == 255 {
                    output.append(255)
                    output.append(contentsOf: block)
                    block.removeAll(keepingCapacity: true)
                }
            }
        }

        func finishBlocks() {
            if bitCount > 0 {
                block.append(UInt8(truncatingIfNeeded: bitBuffer))
                bitBuffer = 0
                bitCount = 0
            }
            if !block.isEmpty {
                output.append(UInt8(block.count))
                output.append(contentsOf: block)
            }
            output.append(0)
        }

        func resetTable() {
            keys.withUnsafeMutableBufferPointer { $0.update(repeating: -1) }
            codeSize = minimumCodeSize + 1
            nextCode = end + 1
        }

        resetTable()
        emit(clear)
        guard let first = input.first else {
            emit(end)
            finishBlocks()
            return
        }

        var prefix = Int(first)
        for position in 1..<input.count {
            let symbol = Int(input[position])
            let key = Int32((prefix << 8) | symbol)
            var slot = ((symbol << 5) ^ prefix) & (Self.tableSize - 1)
            var found = false
            while keys[slot] != -1 {
                if keys[slot] == key {
                    prefix = Int(codes[slot])
                    found = true
                    break
                }
                slot = (slot + 1) & (Self.tableSize - 1)
            }
            if found { continue }

            emit(prefix)
            if nextCode < 4096 {
                keys[slot] = key
                codes[slot] = UInt16(nextCode)
                nextCode += 1
                if nextCode > (1 << codeSize), codeSize < 12 { codeSize += 1 }
            } else {
                emit(clear)
                resetTable()
            }
            prefix = symbol
        }
        emit(prefix)
        if nextCode >= (1 << codeSize), codeSize < 12 { codeSize += 1 }
        emit(end)
        finishBlocks()
    }
}

private extension Data {
    mutating func appendLE16(_ value: Int) {
        append(UInt8(value & 0xFF))
        append(UInt8((value >> 8) & 0xFF))
    }
}
