import AppKit

@MainActor
final class TabBar: NSView {
    struct Item: Equatable {
        let title: String
        let edited: Bool
    }

    private struct Spot: Equatable {
        let tab: Int
        let close: Bool
    }

    static let height: CGFloat = 36
    private static let closeSide: CGFloat = 22

    var onSelect: ((Int) -> Void)?
    var onClose: ((Int) -> Void)?

    private var items: [Item] = []
    private var current = 0
    private var hovered: Spot? {
        didSet { if hovered != oldValue { needsDisplay = true } }
    }
    private var pressed: Spot?

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .activeInActiveApp, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { nil }

    override var wantsUpdateLayer: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    func show(_ items: [Item], current: Int) {
        guard items != self.items || current != self.current else { return }
        if items != self.items {
            hovered = nil
            pressed = nil
        }
        self.items = items
        self.current = current
        needsDisplay = true
    }

    func center(of index: Int) -> CGPoint {
        let tab = tabRect(index)
        return CGPoint(x: tab.minX + 40, y: tab.midY)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsDisplay = true
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        needsDisplay = true
    }

    private var tabWidth: CGFloat {
        min(230, (bounds.width - 16) / CGFloat(max(items.count, 1)))
    }

    private func tabRect(_ index: Int) -> CGRect {
        CGRect(x: 8 + CGFloat(index) * tabWidth, y: 0, width: tabWidth, height: bounds.height - 6)
    }

    private func closeRect(_ index: Int) -> CGRect {
        let tab = tabRect(index)
        return CGRect(x: tab.maxX - 6 - Self.closeSide, y: tab.midY - Self.closeSide / 2, width: Self.closeSide, height: Self.closeSide)
    }

    private func spot(_ event: NSEvent) -> Spot? {
        let point = convert(event.locationInWindow, from: nil)
        guard let index = items.indices.first(where: { tabRect($0).contains(point) }) else { return nil }
        return Spot(tab: index, close: closeRect(index).contains(point))
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = spot(event)
    }

    override func mouseMoved(with event: NSEvent) {
        hovered = spot(event)
    }

    override func mouseExited(with event: NSEvent) {
        hovered = nil
    }

    override func mouseDown(with event: NSEvent) {
        pressed = spot(event)
    }

    override func mouseUp(with event: NSEvent) {
        let released = spot(event)
        defer { pressed = nil }
        guard let released, released == pressed else { return }
        if released.close {
            onClose?(released.tab)
        } else {
            onSelect?(released.tab)
        }
    }

    override func updateLayer() {
        guard let layer else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.backgroundColor = Theme.window.cgColor
        layer.sublayers = items.indices.flatMap { layers(for: $0) }
        CATransaction.commit()
    }

    private func layers(for index: Int) -> [CALayer] {
        let item = items[index]
        let tab = tabRect(index)
        let active = index == current
        let over = hovered?.tab == index
        let overClose = over && hovered?.close == true
        var result: [CALayer] = []

        let back = CALayer()
        back.frame = CGRect(x: tab.minX + 1, y: tab.minY, width: tab.width - 2, height: tab.height)
        back.cornerRadius = 8
        back.maskedCorners = [.layerMinXMaxYCorner, .layerMaxXMaxYCorner]
        back.backgroundColor = (active ? Theme.panel : NSColor(white: 1, alpha: over ? 0.08 : 0.03)).cgColor
        result.append(back)

        if active {
            let line = CALayer()
            line.frame = CGRect(x: tab.minX + 10, y: tab.maxY - 2, width: tab.width - 20, height: 2)
            line.cornerRadius = 1
            line.backgroundColor = Theme.accent.cgColor
            result.append(line)
        }

        var left = tab.minX + 12
        if item.edited {
            let dot = CALayer()
            dot.frame = CGRect(x: left, y: tab.midY - 3.5, width: 7, height: 7)
            dot.cornerRadius = 3.5
            dot.backgroundColor = (active ? Theme.accent : Theme.tertiary).cgColor
            result.append(dot)
            left += 13
        }

        let close = closeRect(index)
        let font = NSFont.systemFont(ofSize: 12, weight: active ? .bold : .regular)
        let lineHeight = ceil(font.ascender - font.descender + font.leading)
        let title = CATextLayer()
        title.string = item.title
        title.font = font
        title.fontSize = 12
        title.foregroundColor = (active ? Theme.text : over ? Theme.secondary : Theme.tertiary).cgColor
        title.truncationMode = .end
        title.alignmentMode = .left
        title.contentsScale = window?.backingScaleFactor ?? 2
        title.frame = CGRect(x: left, y: tab.midY - lineHeight / 2, width: max(close.minX - 6 - left, 0), height: lineHeight)
        result.append(title)

        guard active || over else { return result }
        if overClose {
            let circle = CALayer()
            circle.frame = close
            circle.cornerRadius = Self.closeSide / 2
            circle.backgroundColor = NSColor(white: 1, alpha: 0.1).cgColor
            result.append(circle)
        }
        let cross = CAShapeLayer()
        cross.frame = CGRect(x: close.midX - 5, y: close.midY - 5, width: 10, height: 10)
        let path = CGMutablePath()
        path.move(to: CGPoint(x: 1.56, y: 1.56))
        path.addLine(to: CGPoint(x: 8.44, y: 8.44))
        path.move(to: CGPoint(x: 8.44, y: 1.56))
        path.addLine(to: CGPoint(x: 1.56, y: 8.44))
        cross.path = path
        cross.lineWidth = 1.4
        cross.fillColor = nil
        cross.strokeColor = (overClose ? Theme.text : Theme.secondary).cgColor
        result.append(cross)
        return result
    }
}
