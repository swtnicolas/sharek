import CoreGraphics
import Foundation
import Testing
@testable import ShareK

private func source(width: Int, height: Int) -> Pixels {
    var bytes = Data()
    for index in 0..<(width * height) {
        bytes.append(contentsOf: [UInt8(index + 1), 0, 0, 255])
    }
    return Pixels(format: PixelFormat(width: width, height: height, bytesPerRow: width * 4, colorSpace: Pixels.sRGB), data: bytes)!
}

private func blues(_ pixels: Pixels) -> [[UInt8]] {
    (0..<pixels.height).map { row in
        (0..<pixels.width).map { column in pixels.data[row * pixels.format.bytesPerRow + column * 4] }
    }
}

@Test func aBlitOfTheSameSizeCopiesThePixelsToThatPlace() throws {
    let image = source(width: 2, height: 2)
    let canvas = try #require(Pixels.drawable(width: 4, height: 4, colorSpace: nil))
    image.blit(from: image.bounds, into: canvas.context, at: CGRect(x: 1, y: 2, width: 2, height: 2))
    #expect(blues(canvas.pixels) == [[0, 0, 0, 0], [0, 0, 0, 0], [0, 1, 2, 0], [0, 3, 4, 0]])
}

@Test func aLargerTargetRepeatsEachPixel() throws {
    let image = source(width: 2, height: 2)
    let canvas = try #require(Pixels.drawable(width: 4, height: 4, colorSpace: nil))
    image.blit(from: image.bounds, into: canvas.context, at: CGRect(x: 0, y: 0, width: 4, height: 4))
    #expect(blues(canvas.pixels) == [[1, 1, 2, 2], [1, 1, 2, 2], [3, 3, 4, 4], [3, 3, 4, 4]])
}

@Test func aBlitTakesOnlyTheRequestedAreaAndStaysInsideTheBuffer() throws {
    let image = source(width: 3, height: 2)
    let canvas = try #require(Pixels.drawable(width: 2, height: 2, colorSpace: nil))
    image.blit(from: CGRect(x: 1, y: 0, width: 2, height: 2), into: canvas.context, at: CGRect(x: -1, y: -1, width: 2, height: 2))
    #expect(blues(canvas.pixels) == [[6, 0], [0, 0]])
}
