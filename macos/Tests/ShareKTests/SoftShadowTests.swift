import CoreGraphics
import Testing
@testable import ShareK

private let blurred: [UInt8] = [28, 56, 85, 56, 28, 0, 56, 113, 170, 113, 56, 0, 56, 113, 170, 113, 56, 0, 28, 56, 85, 56, 28, 0]

private func composited(width: Int, height: Int, fill: [UInt8], reduction: Int, clip: CGRect? = nil) -> [UInt8] {
    var target = [UInt8](repeating: 0, count: width * height * 4)
    for index in target.indices { target[index] = fill[index % 4] }
    var coverage = blurred
    coverage.withUnsafeMutableBufferPointer { data in
        target.withUnsafeMutableBytes { bytes in
            SoftShadow.composite(
                coverage: data.baseAddress!, width: 6, height: 4, reduction: reduction,
                into: bytes.baseAddress!, stride: width * 4, clip: clip ?? CGRect(x: 0, y: 0, width: width, height: height),
                originX: 1, originY: 1
            )
        }
    }
    return target
}

@Test func theShadowBlurGivesTheValuesTheLinuxEditorIsTestedAgainst() {
    var coverage: [UInt8] = [0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]
    var scratch = [UInt8](repeating: 0, count: 24)
    var sums = [UInt32](repeating: 0, count: 6)
    coverage.withUnsafeMutableBufferPointer { data in
        scratch.withUnsafeMutableBufferPointer { spare in
            sums.withUnsafeMutableBufferPointer { totals in
                SoftShadow.blur(data.baseAddress!, scratch: spare.baseAddress!, sums: totals.baseAddress!, width: 6, height: 4, sigma: 1)
            }
        }
    }
    #expect(coverage == blurred)
    let medium: [Int] = [2, 2, 2]
    let large: [Int] = [4, 4, 5]
    #expect(SoftShadow.boxRadii(sigma: 2.5) == medium)
    #expect(SoftShadow.boxRadii(sigma: 5) == large)
}

private func row(_ index: Int, of pixels: [UInt8], width: Int, channel: Int) -> [UInt8] {
    var values: [UInt8] = []
    for column in 0..<width {
        values.append(pixels[(index * width + column) * 4 + channel])
    }
    return values
}

@Test func theShadowDarkensWhatIsUnderItAsOnLinux() {
    let opaque = composited(width: 14, height: 10, fill: [200, 150, 100, 255], reduction: 2)
    let half: [UInt8] = [200, 180, 176, 166, 156, 147, 147, 156, 166, 176, 186, 195, 200, 200]
    #expect(row(3, of: opaque, width: 14, channel: 0) == half)
    let full = composited(width: 8, height: 6, fill: [200, 150, 100, 255], reduction: 1)
    let whole: [UInt8] = [200, 178, 155, 133, 155, 178, 200, 200]
    #expect(row(2, of: full, width: 8, channel: 0) == whole)
}

@Test func theShadowAddsItsOwnAlphaOverNothing() {
    let clear = composited(width: 14, height: 10, fill: [0, 0, 0, 0], reduction: 2)
    let alpha: [UInt8] = [0, 28, 35, 49, 64, 78, 78, 64, 49, 35, 21, 7, 0, 0]
    #expect(row(4, of: clear, width: 14, channel: 3) == alpha)
}

@Test func theShadowStaysInsideItsClip() {
    let clipped = composited(width: 14, height: 10, fill: [200, 150, 100, 255], reduction: 2, clip: CGRect(x: 5, y: 0, width: 9, height: 10))
    let expected: [UInt8] = [200, 200, 200, 200, 200, 147, 147, 156, 166, 176, 186, 195, 200, 200]
    #expect(row(3, of: clipped, width: 14, channel: 0) == expected)
}
