import CoreGraphics
import Foundation
import Testing
@testable import ShareK

@Test func closingATabBeforeTheShownOneKeepsThatTabShown() {
    #expect(EditorManager.selection(afterClosing: 0, from: 2, remaining: 3) == 1)
}

@Test func closingATabAfterTheShownOneKeepsThatTabShown() {
    #expect(EditorManager.selection(afterClosing: 3, from: 1, remaining: 3) == 1)
}

@Test func closingTheShownTabShowsTheOneThatFollowsIt() {
    #expect(EditorManager.selection(afterClosing: 1, from: 1, remaining: 3) == 1)
}

@Test func closingTheShownLastTabShowsTheOneBeforeIt() {
    #expect(EditorManager.selection(afterClosing: 3, from: 3, remaining: 3) == 2)
}

@Test func aFrameNarrowerThanItsPaddedRowsReadsBackAsDrawn() throws {
    var bytes = Data()
    for index in 0..<6 { bytes.append(contentsOf: [UInt8(index + 1), 0, 0, 255]) }
    let image = try #require(Pixels(format: PixelFormat(width: 3, height: 2, bytesPerRow: 12, colorSpace: Pixels.sRGB), data: bytes))
    let frame = try #require(MappedFrame(width: 3, height: 2, colorSpace: Pixels.sRGB))
    #expect(frame.draw { image.blit(from: image.bounds, into: $0, at: image.bounds) })
    let shown = try #require(frame.copyPixels())
    let rows: [[UInt8]] = (0..<2).map { row in
        (0..<3).map { column in shown.data[row * shown.format.bytesPerRow + column * 4] }
    }
    #expect(rows == [[1, 2, 3], [4, 5, 6]])
}
