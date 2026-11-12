import CoreGraphics
import Foundation

final class ScrollStitcher {
    enum Outcome: Equatable {
        case started
        case appended(rows: Int)
        case unchanged
        case reversed
        case noMatch
    }

    private struct Frame {
        let width: Int
        let height: Int
        let pixels: [UInt32]
    }

    private struct Rows {
        let hashes: [UInt64]
        let distinctive: [Bool]
        let tiles: [UInt64]
        let varied: [Bool]
        let perRow: Int
    }

    private typealias Counts = (compared: Int, agreed: Int)

    private static let window = 16
    private static let agreement = 90
    private static let sameFrame = 97
    private static let movingColumn = 2
    private static let tile = 8
    private static let disturbedShare = 3
    private static let toleratedBands = 2
    private static let candidateOffsets = 3
    private static let keptShare = 4
    private static let hashBasis: UInt64 = 0xCBF2_9CE4_8422_2325
    private static let hashPrime: UInt64 = 0x0000_0100_0000_01B3

    private let ignoredTrailingColumns: Int
    private var columns: Range<Int>?
    private var first: CGImage?
    private var last: CGImage?
    private var reference: Frame?
    private var previous: Rows?
    private var pieces: [Pixels] = []
    private var footer: Int?
    private(set) var height = 0
    var pointer: Int?

    init(ignoredTrailingColumns: Int) {
        self.ignoredTrailingColumns = ignoredTrailingColumns
    }

    func add(_ image: CGImage) -> Outcome {
        guard let frame = Self.frame(of: image) else { return .noMatch }
        let whole = 0..<max(1, frame.width - ignoredTrailingColumns)
        guard let reference, let previous else {
            first = image
            self.reference = frame
            self.previous = Self.rows(of: frame, columns: whole)
            height = frame.height
            return .started
        }
        guard frame.width == reference.width, frame.height == reference.height else { return .noMatch }
        let columns: Range<Int>
        if let known = self.columns {
            columns = known
        } else {
            if Self.isSameFrame(previous, Self.rows(of: frame, columns: whole)) { return .unchanged }
            guard let moving = Self.movingColumns(from: reference, to: frame, ignoring: ignoredTrailingColumns) else { return .unchanged }
            self.columns = moving
            self.previous = Self.rows(of: reference, columns: moving)
            columns = moving
        }
        guard let before = self.previous else { return .noMatch }
        let current = Self.rows(of: frame, columns: columns)
        if Self.isSameFrame(before, current) { return .unchanged }
        guard let moved = Self.offset(from: before, to: current, pointer: pointer) else {
            return Self.offset(from: current, to: before, pointer: pointer) != nil ? .reversed : .noMatch
        }

        let frameHeight = frame.height
        let footer = self.footer ?? max(Self.belowContent(before, current, moved: moved), max(8, frameHeight * 6 / 100))
        if moved + footer + Self.window > frameHeight { return .noMatch }
        if self.footer == nil {
            if let first, let body = ImageOps.crop(first, to: CGRect(x: 0, y: 0, width: first.width, height: frameHeight - footer)) {
                pieces.append(body)
            }
            self.footer = footer
        }
        if let piece = ImageOps.crop(image, to: CGRect(x: 0, y: frameHeight - footer - moved, width: image.width, height: moved)) {
            pieces.append(piece)
        }
        self.previous = current
        self.reference = frame
        last = image
        height = pieces.reduce(0) { $0 + $1.height } + footer
        return .appended(rows: moved)
    }

    func compose() -> Pixels? {
        guard let first else { return nil }
        guard !pieces.isEmpty, let last else { return Pixels(first) }
        let footer = footer ?? 0
        var parts = pieces
        if footer > 0, let tail = ImageOps.crop(last, to: CGRect(x: 0, y: last.height - footer, width: last.width, height: footer)) {
            parts.append(tail)
        }
        let width = first.width
        guard parts.allSatisfy({ $0.width == width }) else { return nil }
        let rowBytes = width * 4
        let total = parts.reduce(0) { $0 + $1.height }
        guard let data = MappedData.make(count: rowBytes * total, { to in
            var offset = 0
            for part in parts {
                let stride = part.format.bytesPerRow
                part.data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
                    guard let from = raw.baseAddress else { return }
                    for row in 0..<part.height {
                        (to + offset).copyMemory(from: from + row * stride, byteCount: rowBytes)
                        offset += rowBytes
                    }
                }
            }
            return true
        }) else { return nil }
        return Pixels(format: parts[0].format.with(width: width, height: total, bytesPerRow: rowBytes), data: data)
    }

    static func settled(_ before: CGImage, _ after: CGImage) -> Bool {
        guard before.width == after.width, before.height == after.height, let first = frame(of: before), let second = frame(of: after) else { return false }
        let whole = 0..<first.width
        return isSameFrame(rows(of: first, columns: whole), rows(of: second, columns: whole))
    }

    private static func frame(of image: CGImage) -> Frame? {
        let width = image.width
        let height = image.height
        guard width > 0, height > 0, let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        var pixels = [UInt32](repeating: 0, count: width * height)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        return drawn ? Frame(width: width, height: height, pixels: pixels) : nil
    }

    private static func rows(of frame: Frame, columns: Range<Int>) -> Rows {
        let perRow = (columns.count + tile - 1) / tile
        var hashes = [UInt64](repeating: 0, count: frame.height)
        var distinctive = [Bool](repeating: false, count: frame.height)
        var tiles = [UInt64](repeating: 0, count: frame.height * perRow)
        var varied = [Bool](repeating: false, count: frame.height * perRow)
        frame.pixels.withUnsafeBufferPointer { pixels in
            for y in 0..<frame.height {
                let start = y * frame.width
                let rowFirst = pixels[start + columns.lowerBound]
                var rowHash = hashBasis
                var rowUniform = true
                for index in 0..<perRow {
                    let tileStart = columns.lowerBound + index * tile
                    let tileFirst = pixels[start + tileStart]
                    var tileHash = hashBasis
                    var tileUniform = true
                    for x in tileStart..<min(tileStart + tile, columns.upperBound) {
                        let pixel = pixels[start + x]
                        rowHash = (rowHash ^ UInt64(pixel)) &* hashPrime
                        tileHash = (tileHash ^ UInt64(pixel)) &* hashPrime
                        if pixel != rowFirst { rowUniform = false }
                        if pixel != tileFirst { tileUniform = false }
                    }
                    tiles[y * perRow + index] = tileHash
                    varied[y * perRow + index] = !tileUniform
                }
                hashes[y] = rowHash
                distinctive[y] = !rowUniform
            }
        }
        return Rows(hashes: hashes, distinctive: distinctive, tiles: tiles, varied: varied, perRow: perRow)
    }

    private static func movingColumns(from before: Frame, to after: Frame, ignoring trailing: Int) -> Range<Int>? {
        var changed = [Int](repeating: 0, count: before.width)
        for y in 0..<before.height {
            let start = y * before.width
            for x in 0..<before.width where before.pixels[start + x] != after.pixels[start + x] {
                changed[x] += 1
            }
        }
        let moving = { (column: Int) in changed[column] * 100 >= before.height * movingColumn }
        guard let start = (0..<before.width).first(where: moving), let last = (0..<before.width).last(where: moving) else { return nil }
        return start..<max(start + 1, last + 1 - trailing)
    }

    private static func isSameFrame(_ before: Rows, _ current: Rows) -> Bool {
        var compared = 0
        var same = 0
        for index in 0..<min(before.tiles.count, current.tiles.count) where before.varied[index] || current.varied[index] {
            compared += 1
            if before.tiles[index] == current.tiles[index] { same += 1 }
        }
        return same * 100 >= compared * sameFrame
    }

    private static func belowContent(_ before: Rows, _ current: Rows, moved: Int) -> Int {
        let height = before.hashes.count
        guard let row = (moved..<height).reversed().first(where: { before.distinctive[$0] && before.hashes[$0] == current.hashes[$0 - moved] }) else { return 0 }
        return height - 1 - row
    }

    private static func windows(_ rows: Rows) -> [UInt64?] {
        let count = rows.hashes.count - window + 1
        guard count > 0 else { return [] }
        return (0..<count).map { start in
            let slice = rows.hashes[start..<start + window]
            guard rows.distinctive[start..<start + window].contains(true), slice.contains(where: { $0 != slice.first }) else { return nil }
            return slice.reduce(UInt64(0x9E37_79B9_7F4A_7C15)) { ($0 ^ $1) &* 0x0000_0100_0000_01B3 }
        }
    }

    private static func offset(from previous: Rows, to current: Rows, pointer: Int?) -> Int? {
        var positions: [UInt64: [Int]] = [:]
        for (index, hash) in windows(previous).enumerated() {
            if let hash { positions[hash, default: []].append(index) }
        }
        var votes: [Int: Int] = [:]
        var candidates = 0
        for (index, hash) in windows(current).enumerated() {
            guard let hash, let matches = positions[hash], matches.count <= 3 else { continue }
            candidates += 1
            for match in matches where match > index {
                votes[match - index, default: 0] += 1
            }
        }
        let ranked = votes.filter { $0.value >= max(4, candidates / 5) }.sorted { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }
        return ranked.prefix(candidateOffsets).map(\.key).first { overlapAgrees(previous, current, moved: $0, pointer: pointer) }
    }

    private static func overlapAgrees(_ previous: Rows, _ current: Rows, moved: Int, pointer: Int?) -> Bool {
        let height = min(current.hashes.count, previous.hashes.count)
        let perRow = current.perRow
        let counts: [Counts] = (0..<max(0, height - moved)).map { row in
            var tally: Counts = (0, 0)
            let here = row * perRow
            let there = (row + moved) * perRow
            for tile in 0..<perRow {
                let now = current.tiles[here + tile]
                let before = previous.tiles[there + tile]
                guard current.varied[here + tile] || previous.varied[there + tile],
                      now != previous.tiles[here + tile], current.tiles[there + tile] != before else { continue }
                tally.compared += 1
                if now == before { tally.agreed += 1 }
            }
            return tally
        }
        let pointerRows = pointer.map { [$0, $0 - moved].filter { $0 >= 0 } } ?? []
        let disturbed = disturbedBands(counts, pointerRows: pointerRows)
        let total = counts.reduce(0) { $0 + $1.compared }
        var compared = 0
        var agreed = 0
        for (row, count) in counts.enumerated() where !disturbed.contains(where: { $0.contains(row) }) {
            compared += count.compared
            agreed += count.agreed
        }
        return compared >= 4 * window && compared * keptShare >= total && agreed * 100 >= compared * agreement
    }

    private static func disturbedBands(_ counts: [Counts], pointerRows: [Int]) -> [Range<Int>] {
        var bands: [(rows: Range<Int>, missed: Int)] = []
        for (row, count) in counts.enumerated() where count.agreed * 2 < count.compared {
            let missed = count.compared - count.agreed
            if let last = bands.last, row - last.rows.upperBound <= window {
                bands[bands.count - 1] = (last.rows.lowerBound..<row + 1, last.missed + missed)
            } else {
                bands.append((row..<row + 1, missed))
            }
        }
        let hovered = { (band: Range<Int>) in
            band.count * 2 <= counts.count && pointerRows.contains { band.lowerBound <= $0 + window && $0 < band.upperBound + window }
        }
        var disturbed = bands.map(\.rows).filter(hovered)
        let others = bands.filter { !hovered($0.rows) }.sorted { $0.missed == $1.missed ? $0.rows.lowerBound < $1.rows.lowerBound : $0.missed > $1.missed }
        var tolerated = 0
        for band in others.prefix(toleratedBands) {
            if (tolerated + band.rows.count) * disturbedShare <= counts.count {
                tolerated += band.rows.count
                disturbed.append(band.rows)
            }
        }
        return disturbed
    }
}
