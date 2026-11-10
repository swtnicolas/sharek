import AppKit
import Testing
@testable import ShareK

private func near(_ rect: CGRect, _ x: CGFloat, _ y: CGFloat, _ width: CGFloat, _ height: CGFloat) -> Bool {
    abs(rect.minX - x) < 0.01 && abs(rect.minY - y) < 0.01 && abs(rect.width - width) < 0.01 && abs(rect.height - height) < 0.01
}

@Test func anArcBulgesToTheSideItsSweepFlagNames() {
    let clockwise = IconPath.parse("M 0 5 A 5 5 0 0 1 5 0 Z")
    #expect(clockwise.contains(CGPoint(x: 2, y: 2)))
    #expect(!clockwise.contains(CGPoint(x: 3, y: 3)))
    let counterclockwise = IconPath.parse("M 0 5 A 5 5 0 0 0 5 0 Z")
    #expect(counterclockwise.contains(CGPoint(x: 3, y: 3)))
    #expect(!counterclockwise.contains(CGPoint(x: 2, y: 2)))
}

@Test func aLargeArcGoesTheLongWayAroundItsCircle() {
    #expect(near(IconPath.parse("M 0 5 A 5 5 0 1 1 5 0").boundingBoxOfPath, -5, -5, 10, 10))
    #expect(near(IconPath.parse("M 0 5 A 5 5 0 0 1 5 0").boundingBoxOfPath, 0, 0, 5, 5))
}

@Test func anIconFitsItsGridInsideItsSideLessTheStroke() {
    let icon = Icon(commands: "M 0 0 L 24 12")
    let down = icon.outline(at: CGPoint(x: 6, y: 6), flipped: true)?.boundingBoxOfPath ?? .null
    #expect(near(down, 6.95, 6.95, 20.1, 10.05))
    let up = icon.outline(at: CGPoint(x: 6, y: 6), flipped: false)?.boundingBoxOfPath ?? .null
    #expect(near(up, 6.95, 17, 20.1, 10.05))
}

@Test @MainActor func aShapeIconShowsItsOutlineAndFillColors() {
    var style = ShapeKind.rectangle.defaultStyle
    style.stroke = RGBA(r: 0, g: 132 / 255, b: 1, a: 1)
    style.fill = RGBA(r: 48 / 255, g: 209 / 255, b: 88 / 255, a: 1)
    let colors = Theme.iconColors(.rectangle, style)
    #expect(RGBA(colors.stroke) == style.stroke)
    #expect(RGBA(colors.fill) == style.fill)
    let step = ShapeKind.step.defaultStyle
    #expect(RGBA(Theme.iconColors(.step, step).stroke) == step.text)
    #expect(RGBA(Theme.iconColors(.step, step).fill) == step.fill)
}

@Test @MainActor func aToolWithoutAColorOfItsOwnTakesTheColorOfItsPurpose() {
    #expect(Theme.iconColors(.blur, ShapeKind.blur.defaultStyle).stroke == Theme.conceal)
    #expect(Theme.iconColors(.crop, nil).stroke == Theme.transform)
    #expect(Theme.iconColors(.select, nil).stroke == Theme.pointer)
}

@Test @MainActor func aDarkIconColorFallsBackToAVisibleGray() {
    var style = ShapeKind.arrow.defaultStyle
    style.stroke = .black
    #expect(Theme.iconColors(.arrow, style).stroke == Theme.dim)
}

@Test @MainActor func outlinedDarkTextShowsItsOutlineColorOnTheIcon() {
    var style = ShapeKind.textOutline.defaultStyle
    style.stroke = .yellow
    #expect(RGBA(Theme.iconColors(.textOutline, style).stroke) == .yellow)
    style.lineWidth = 0
    #expect(Theme.iconColors(.textOutline, style).stroke == Theme.icon)
}
