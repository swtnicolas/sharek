import AppKit

extension NSColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
    }
}

@MainActor
enum Theme {
    static let window = NSColor(hex: 0x1b1d20)
    static let panel = NSColor(hex: 0x26292d)
    static let border = NSColor(hex: 0x3f444b)
    static let text = NSColor(hex: 0xeff0f1)
    static let secondary = NSColor(hex: 0xc3c8ce)
    static let tertiary = NSColor(hex: 0x8b939c)
    static let icon = NSColor(hex: 0xd5d9de)
    static let dim = NSColor(hex: 0x9aa1a9)
    static let accent = NSColor(hex: 0xff3d7f)
    static let accentHover = NSColor(hex: 0xff5a92)
    static let accentPressed = NSColor(hex: 0xe6336f)
    static let danger = NSColor(hex: 0xff6b6b)
    static let canvas = NSColor(hex: 0x131517)
    static let tooltip = NSColor(hex: 0x2f3338)

    static let pointer = NSColor(hex: 0xe8eaed)
    static let file = NSColor(hex: 0x5b9bff)
    static let recognize = NSColor(hex: 0x2ec4b6)
    static let pin = NSColor(hex: 0xff9f1c)
    static let history = NSColor(hex: 0xffb020)
    static let transform = NSColor(hex: 0x34c759)
    static let erase = NSColor(hex: 0xff7ab6)
    static let conceal = NSColor(hex: 0xaf7bff)
    static let folder = NSColor(hex: 0xffc94d)

    static func actionColor(_ name: String) -> NSColor {
        switch name {
        case "save", "save-as": file
        case "ocr", "ocr-area": recognize
        case "pin": pin
        case "undo", "redo": history
        case "rotate", "flip", "frame", "play": transform
        case "reveal": folder
        case "discard": danger
        default: icon
        }
    }

    private static func bright(_ value: RGBA) -> NSColor? {
        guard value.isVisible, 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b >= 0.18 else { return nil }
        return NSColor(srgbRed: value.r, green: value.g, blue: value.b, alpha: 1)
    }

    private static func legible(_ value: RGBA) -> NSColor? {
        guard value.isVisible else { return nil }
        return bright(value) ?? dim
    }

    static func iconColors(_ tool: Tool, _ style: ShapeStyle?) -> (stroke: NSColor, fill: NSColor) {
        switch tool {
        case .select: return (pointer, .clear)
        case .crop, .cutOut: return (transform, .clear)
        case .smartEraser: return (erase, .clear)
        case .blur, .pixelate, .spotlight: return (conceal, .clear)
        default: break
        }
        guard let style else { return (icon, .clear) }
        let fill: NSColor = style.fill.isVisible ? style.fill.nsColor : .clear
        let line = style.lineWidth > 0 ? legible(style.stroke) : nil
        switch tool {
        case .rectangle, .ellipse, .speechBalloon: return (line ?? legible(style.fill) ?? icon, fill)
        case .line, .arrow, .freehand, .magnify: return (legible(style.stroke) ?? icon, .clear)
        case .highlight: return (legible(style.fill) ?? icon, fill)
        case .textOutline: return (bright(style.text) ?? line ?? icon, .clear)
        case .textBackground, .step: return (legible(style.text) ?? icon, fill)
        default: return (icon, .clear)
        }
    }
}

@MainActor
final class HintView: NSView {
    private let title = NSTextField(labelWithString: "")
    private let body = NSTextField(wrappingLabelWithString: "")
    private let stack = NSStackView()
    private static let inset = NSEdgeInsets(top: 8, left: 10, bottom: 9, right: 10)
    private static let maximumWidth: CGFloat = 280

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        isHidden = true
        layer?.backgroundColor = Theme.tooltip.cgColor
        layer?.borderColor = Theme.border.cgColor
        layer?.borderWidth = 1
        layer?.cornerRadius = 7
        layer?.shadowColor = NSColor.black.cgColor
        layer?.shadowOpacity = 0.5
        layer?.shadowRadius = 10
        title.font = .systemFont(ofSize: 12, weight: .bold)
        title.textColor = Theme.text
        title.lineBreakMode = .byTruncatingTail
        body.font = .systemFont(ofSize: 11)
        body.textColor = Theme.secondary
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 3
        stack.addArrangedSubview(title)
        stack.addArrangedSubview(body)
        addSubview(stack)
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func show(title text: String, body detail: String, at anchor: CGPoint, within bounds: CGRect) {
        title.stringValue = text
        body.stringValue = detail
        body.isHidden = detail.isEmpty
        let inner = Self.maximumWidth - Self.inset.left - Self.inset.right
        let natural = max(title.intrinsicContentSize.width, detail.isEmpty ? 0 : (detail as NSString).size(withAttributes: [.font: body.font as Any]).width)
        let width = min(inner, ceil(natural))
        body.preferredMaxLayoutWidth = width
        let bodyHeight = detail.isEmpty ? 0 : body.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height + stack.spacing
        let height = ceil(title.intrinsicContentSize.height + bodyHeight)
        stack.frame = CGRect(x: Self.inset.left, y: Self.inset.top, width: width, height: height)
        let size = CGSize(width: width + Self.inset.left + Self.inset.right, height: height + Self.inset.top + Self.inset.bottom)
        let x = min(max(bounds.minX + 6, anchor.x - 6), bounds.maxX - size.width - 6)
        frame = CGRect(origin: CGPoint(x: x, y: anchor.y), size: size)
        isHidden = false
    }
}

@MainActor
class IconButton: NSView {
    var icon: Icon { didSet { needsDisplay = true } }
    var color: NSColor { didSet { needsDisplay = true } }
    var fill: NSColor = .clear { didSet { needsDisplay = true } }
    var offset: CGFloat = 0 { didSet { needsDisplay = true } }
    var isSelected = false { didSet { needsDisplay = true } }
    var onClick: ((Bool) -> Void)?
    var onHover: ((Bool) -> Void)?

    private var hovered = false { didSet { needsDisplay = true } }
    private var pressed = false { didSet { needsDisplay = true } }

    init(icon: Icon, color: NSColor, size: CGSize = CGSize(width: 34, height: 34)) {
        self.icon = icon
        self.color = color
        super.init(frame: CGRect(origin: .zero, size: size))
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        let background: NSColor = isSelected ? NSColor(white: 1, alpha: 0.12)
            : pressed ? NSColor(white: 1, alpha: 0.15)
            : hovered ? NSColor(white: 1, alpha: 0.07)
            : .clear
        background.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 8, yRadius: 8).fill()
        icon.draw(centeredIn: bounds.offsetBy(dx: offset, dy: 0), flipped: isFlipped, stroke: color.cgColor, fill: fill.cgColor)
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = true
        onHover?(true)
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        onHover?(false)
    }

    override func mouseDown(with event: NSEvent) {
        pressed = true
        onHover?(false)
    }

    override func mouseUp(with event: NSEvent) {
        pressed = false
        if bounds.contains(convert(event.locationInWindow, from: nil)) {
            onClick?(event.modifierFlags.contains(.shift))
        }
    }
}

@MainActor
final class CopyButton: NSView, NSDraggingSource {
    var compact = false { didSet { needsDisplay = true } }
    var onClick: (() -> Void)?
    var onHover: ((Bool) -> Void)?
    var dragContents: (() -> (png: Data, file: URL, preview: NSImage)?)?

    private var hovered = false { didSet { needsDisplay = true } }
    private var pressed = false { didSet { needsDisplay = true } }
    private var pressPoint: CGPoint?

    static let fullWidth: CGFloat = 98
    static let compactWidth: CGFloat = 34

    init() {
        super.init(frame: CGRect(x: 0, y: 0, width: Self.fullWidth, height: 32))
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self))
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        (pressed ? Theme.accentPressed : hovered ? Theme.accentHover : Theme.accent).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 8, yRadius: 8).fill()
        let icon = Icon.copy.sized(18, lineWidth: 2)
        let x = compact ? (bounds.width - icon.side) / 2 : 11
        icon.draw(at: CGPoint(x: x, y: (bounds.height - icon.side) / 2), flipped: isFlipped, stroke: NSColor.white.cgColor)
        if !compact {
            let label = NSAttributedString(string: "Copiar", attributes: [.font: NSFont.systemFont(ofSize: 13, weight: .bold), .foregroundColor: NSColor.white])
            label.draw(at: CGPoint(x: x + icon.side + 7, y: (bounds.height - label.size().height) / 2))
        }
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = true
        onHover?(true)
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        onHover?(false)
    }

    override func mouseDown(with event: NSEvent) {
        pressed = true
        pressPoint = event.locationInWindow
        onHover?(false)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = pressPoint, hypot(event.locationInWindow.x - start.x, event.locationInWindow.y - start.y) > 4 else { return }
        pressPoint = nil
        pressed = false
        guard let contents = dragContents?() else { return }
        let item = NSPasteboardItem()
        item.setData(contents.png, forType: .png)
        item.setString(contents.file.absoluteString, forType: .fileURL)
        let dragging = NSDraggingItem(pasteboardWriter: item)
        let width: CGFloat = 120
        let height = contents.preview.size.width > 0 ? width * contents.preview.size.height / contents.preview.size.width : width
        let origin = convert(event.locationInWindow, from: nil)
        dragging.setDraggingFrame(CGRect(x: origin.x - width / 2, y: origin.y - height / 2, width: width, height: height), contents: contents.preview)
        beginDraggingSession(with: [dragging], event: event, source: self)
    }

    override func mouseUp(with event: NSEvent) {
        let clicked = pressPoint != nil && bounds.contains(convert(event.locationInWindow, from: nil))
        pressed = false
        pressPoint = nil
        if clicked { onClick?() }
    }

    nonisolated func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        .copy
    }
}
