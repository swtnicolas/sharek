import CoreGraphics

enum SoftShadow {
    static func boxRadii(sigma: CGFloat) -> [Int] {
        let ideal = (12 * sigma * sigma / 3 + 1).squareRoot()
        var lower = Int(ideal.rounded(.down))
        if lower % 2 == 0 { lower -= 1 }
        let upper = lower + 2
        let base = CGFloat(lower)
        let split = Int(((12 * sigma * sigma - 3 * base * base - 12 * base - 9) / (-4 * base - 4)).rounded())
        return (0..<3).map { max((($0 < split ? lower : upper) - 1) / 2, 0) }
    }

    static func blur(_ data: UnsafeMutablePointer<UInt8>, scratch: UnsafeMutablePointer<UInt8>, sums: UnsafeMutablePointer<UInt32>, width: Int, height: Int, sigma: CGFloat) {
        guard sigma >= 0.5, width > 0, height > 0 else { return }
        for radius in boxRadii(sigma: sigma) {
            rows(data, scratch, width: width, height: height, radius: radius)
            columns(scratch, data, sums: sums, width: width, height: height, radius: radius)
        }
    }

    private static func reciprocal(_ radius: Int) -> UInt32 {
        UInt32((1 << 24) / (2 * radius + 1) + 1)
    }

    private static func rows(_ source: UnsafeMutablePointer<UInt8>, _ target: UnsafeMutablePointer<UInt8>, width: Int, height: Int, radius: Int) {
        let scale = reciprocal(radius)
        let last = width - 1
        for y in 0..<height {
            let input = source + y * width
            let output = target + y * width
            var sum: UInt32 = 0
            for k in 0...(2 * radius) {
                sum += UInt32(input[min(max(k - radius, 0), last)])
            }
            for x in 0..<width {
                output[x] = UInt8(truncatingIfNeeded: (sum &* scale) >> 24)
                sum = sum + UInt32(input[min(x + radius + 1, last)]) - UInt32(input[max(x - radius, 0)])
            }
        }
    }

    private static func columns(_ source: UnsafeMutablePointer<UInt8>, _ target: UnsafeMutablePointer<UInt8>, sums: UnsafeMutablePointer<UInt32>, width: Int, height: Int, radius: Int) {
        let scale = reciprocal(radius)
        let last = height - 1
        for x in 0..<width { sums[x] = 0 }
        for k in 0...(2 * radius) {
            let line = source + min(max(k - radius, 0), last) * width
            for x in 0..<width { sums[x] += UInt32(line[x]) }
        }
        for y in 0..<height {
            let leaving = source + max(y - radius, 0) * width
            let entering = source + min(y + radius + 1, last) * width
            let output = target + y * width
            for x in 0..<width {
                output[x] = UInt8(truncatingIfNeeded: (sums[x] &* scale) >> 24)
                sums[x] = sums[x] + UInt32(entering[x]) - UInt32(leaving[x])
            }
        }
    }

    static func composite(
        coverage: UnsafeMutablePointer<UInt8>, width: Int, height: Int, reduction: Int,
        into target: UnsafeMutableRawPointer, stride: Int, clip: CGRect, originX: Int, originY: Int
    ) {
        let columns = width * reduction
        let first = max(Int(clip.minX) - originX, 0)
        let last = min(Int(clip.maxX) - originX, columns)
        guard first < last else { return }
        var near = [UInt16](repeating: 0, count: columns)
        var far = [UInt16](repeating: 0, count: columns)

        func widen(_ row: Int, into line: inout [UInt16]) {
            let source = coverage + row * width
            for x in 0..<width {
                let value = UInt16(source[x])
                if reduction == 1 {
                    line[x] = value * 4
                } else {
                    line[2 * x] = value * 3 + UInt16(source[max(x - 1, 0)])
                    line[2 * x + 1] = value * 3 + UInt16(source[min(x + 1, width - 1)])
                }
            }
        }

        for y in 0..<(height * reduction) {
            let row = originY + y
            guard row >= Int(clip.minY), row < Int(clip.maxY) else { continue }
            let source = y / reduction
            let neighbor = reduction == 1 ? source : y % 2 == 0 ? max(source - 1, 0) : min(source + 1, height - 1)
            widen(source, into: &near)
            widen(neighbor, into: &far)
            let line = (target + row * stride + (originX + first) * 4).assumingMemoryBound(to: UInt8.self)
            for index in first..<last {
                let weight = UInt32(near[index]) * 3 + UInt32(far[index])
                guard weight != 0 else { continue }
                let alpha = (weight + 16) / 32
                let keep = 255 - alpha
                let pixel = line + (index - first) * 4
                pixel[0] = UInt8((UInt32(pixel[0]) * keep + 127) / 255)
                pixel[1] = UInt8((UInt32(pixel[1]) * keep + 127) / 255)
                pixel[2] = UInt8((UInt32(pixel[2]) * keep + 127) / 255)
                pixel[3] = UInt8(alpha + (UInt32(pixel[3]) * keep + 127) / 255)
            }
        }
    }
}
