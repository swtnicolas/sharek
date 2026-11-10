import CoreGraphics
import Testing
@testable import ShareK

private let width = 64
private let scrollbar = 4

private func contentRow(_ index: Int) -> [UInt32] {
    if index % 7 == 3 { return [UInt32](repeating: 0xFFF0_F0F0, count: width) }
    var state = UInt32(truncatingIfNeeded: index) &* 2_654_435_761 &+ 12_345
    return (0..<width).map { _ in
        state = state &* 1_664_525 &+ 1_013_904_223
        return 0xFF00_0000 | (state >> 8)
    }
}

private func fixedRow(_ tag: Int) -> [UInt32] {
    (0..<width).map { 0xFF00_0000 | UInt32((tag * 37 + $0 * 11) & 0xFFFFFF) }
}

private func image(_ rows: [[UInt32]]) -> CGImage {
    var pixels = rows.flatMap { $0 }
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let context = pixels.withUnsafeMutableBytes { buffer in
        CGContext(data: buffer.baseAddress, width: width, height: rows.count, bitsPerComponent: 8, bytesPerRow: width * 4, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!.makeImage()!
    }
    return context
}

private func page(offset: Int, viewport: Int = 200, header: Int = 0, footer: Int = 0, scrollbarShade: UInt32 = 0) -> [[UInt32]] {
    var rows = (0..<header).map(fixedRow)
    for row in 0..<viewport {
        var line = contentRow(offset + row)
        for column in (width - scrollbar)..<width { line[column] = 0xFF00_0000 | scrollbarShade }
        rows.append(line)
    }
    rows += (0..<footer).map { fixedRow(100 + $0) }
    return rows
}

@Test func scrolledFramesAppendWhatMovedIn() {
    let stitcher = ScrollStitcher(ignoredTrailingColumns: scrollbar)
    #expect(stitcher.add(image(page(offset: 0, header: 20, footer: 30))) == .started)
    #expect(stitcher.add(image(page(offset: 137, header: 20, footer: 30, scrollbarShade: 0x404040))) == .appended(rows: 137))
    #expect(stitcher.add(image(page(offset: 274, header: 20, footer: 30, scrollbarShade: 0x808080))) == .appended(rows: 137))
    #expect(stitcher.compose()?.height == 20 + 274 + 200 + 30)
}

@Test func aSharedBlockWithoutAnAgreeingOverlapDoesNotMatch() {
    let block = (0..<24).map { contentRow(9000 + $0) }
    var previous = (0..<100).map(contentRow)
    previous.replaceSubrange(60..<84, with: block)
    var current = (0..<100).map { contentRow(5000 + $0) }
    current.replaceSubrange(20..<44, with: block)
    let stitcher = ScrollStitcher(ignoredTrailingColumns: scrollbar)
    _ = stitcher.add(image(previous))
    #expect(stitcher.add(image(current)) == .noMatch)
}

@Test func aFixedSidebarBesideTheScrollingContentStillMatches() {
    func sidebar(_ rows: [[UInt32]]) -> [[UInt32]] {
        rows.enumerated().map { index, row in Array(fixedRow(index).prefix(20)) + row.dropFirst(20) }
    }
    let stitcher = ScrollStitcher(ignoredTrailingColumns: scrollbar)
    _ = stitcher.add(image(sidebar(page(offset: 0))))
    #expect(stitcher.add(image(sidebar(page(offset: 70)))) == .appended(rows: 70))
}

@Test func aFrameThatDiffersInOneRowIsUnchanged() {
    let stitcher = ScrollStitcher(ignoredTrailingColumns: scrollbar)
    _ = stitcher.add(image(page(offset: 0)))
    var blinking = page(offset: 0)
    blinking[50] = [UInt32](repeating: 0xFF07_0707, count: width)
    #expect(stitcher.add(image(blinking)) == .unchanged)
}
