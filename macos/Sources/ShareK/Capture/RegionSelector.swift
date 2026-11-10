import AppKit
import QuartzCore

struct FrozenScreen {
    let screen: NSScreen
    let image: SurfaceImage
}

struct RegionSelection: @unchecked Sendable {
    let screen: NSScreen
    let rect: CGRect
    let pixels: Pixels?

    var scale: CGFloat { screen.backingScaleFactor }

    var topLeftRect: CGRect {
        CGRect(x: rect.minX, y: screen.frame.height - rect.maxY, width: rect.width, height: rect.height)
    }

    var globalRect: CGRect {
        rect.offsetBy(dx: screen.frame.minX, dy: screen.frame.minY)
    }
}

@MainActor
final class RegionSelector {
    private var windows: [NSWindow] = []
    private var completion: ((RegionSelection?) -> Void)?

    func begin(screens: [FrozenScreen], windowFrames: [CGRect], measuring: Bool, crops: Bool, completion: @escaping (RegionSelection?) -> Void) {
        self.completion = completion
        for frozen in screens {
            let frame = frozen.screen.frame
            let localRects = measuring ? [] : windowFrames.compactMap { rect -> CGRect? in
                let local = rect.offsetBy(dx: -frame.minX, dy: -frame.minY)
                    .intersection(CGRect(origin: .zero, size: frame.size))
                return local.isEmpty ? nil : local
            }
            let window = OverlayWindow(screen: frozen.screen)
            window.contentView = RegionSelectionView(
                size: frame.size,
                image: frozen.image,
                scale: frozen.screen.backingScaleFactor,
                windowRects: localRects,
                measuring: measuring
            ) { [weak self] rect in
                self?.finish(rect.map { rect in
                    let scale = frozen.screen.backingScaleFactor
                    let source = CGRect(x: rect.minX * scale, y: (frame.height - rect.maxY) * scale, width: rect.width * scale, height: rect.height * scale)
                    return RegionSelection(screen: frozen.screen, rect: rect, pixels: crops ? frozen.image.pixels(in: source) : nil)
                })
            }
            windows.append(window)
        }
        NSApp.activate()
        for window in windows { window.orderFrontRegardless() }
        let keyWindow = windows.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) } ?? windows.first
        keyWindow?.makeKey()
        NSCursor.crosshair.set()
    }

    private func finish(_ selection: RegionSelection?) {
        for window in windows { window.orderOut(nil) }
        windows.removeAll()
        NSCursor.arrow.set()
        let completion = completion
        self.completion = nil
        completion?(selection)
    }
}

private final class OverlayWindow: NSWindow {
    init(screen: NSScreen) {
        super.init(contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
        setFrame(screen.frame, display: false)
        level = .screenSaver
        isOpaque = true
        backgroundColor = .black
        hasShadow = false
        isReleasedWhenClosed = false
        animationBehavior = .none
        acceptsMouseMovedEvents = true
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
}

private final class RegionSelectionView: NSView {
    private let image: SurfaceImage
    private let scale: CGFloat
    private let windowRects: [CGRect]
    private let measuring: Bool
    private let onFinish: (CGRect?) -> Void

    private let dimLayer = CAShapeLayer()
    private let outlineLayer = CAShapeLayer()
    private let dashLayer = CAShapeLayer()
    private let crosshairLayer = CAShapeLayer()
    private let sizeLabel = CATextLayer()
    private let magnifier = CALayer()
    private let magnifierCenter = CALayer()
    private let measureLine = CAShapeLayer()
    private let hintLabel = CATextLayer()

    private let magnifierSize: CGFloat = 132
    private let magnifierPixels = 21

    private var dragOrigin: CGPoint?
    private var measureStart: CGPoint?
    private var measureEnd: CGPoint?
    private var selection: CGRect?
    private var hovered: CGRect?
    private var finished = false

    init(size: CGSize, image: SurfaceImage, scale: CGFloat, windowRects: [CGRect], measuring: Bool, onFinish: @escaping (CGRect?) -> Void) {
        self.image = image
        self.scale = scale
        self.windowRects = windowRects
        self.measuring = measuring
        self.onFinish = onFinish
        super.init(frame: CGRect(origin: .zero, size: size))

        let root = CALayer()
        root.contents = image.surface
        root.contentsGravity = .resize
        layer = root
        wantsLayer = true

        dimLayer.fillRule = .evenOdd
        dimLayer.fillColor = NSColor.black.withAlphaComponent(0.4).cgColor

        outlineLayer.fillColor = nil
        outlineLayer.strokeColor = NSColor.black.withAlphaComponent(0.8).cgColor
        outlineLayer.lineWidth = 1
        dashLayer.fillColor = nil
        dashLayer.strokeColor = NSColor.white.cgColor
        dashLayer.lineWidth = 1
        dashLayer.lineDashPattern = [5, 4]

        crosshairLayer.strokeColor = NSColor.white.withAlphaComponent(0.35).cgColor
        crosshairLayer.lineWidth = 1

        sizeLabel.fontSize = 11
        sizeLabel.font = NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        sizeLabel.foregroundColor = NSColor.white.cgColor
        sizeLabel.backgroundColor = NSColor.black.withAlphaComponent(0.75).cgColor
        sizeLabel.cornerRadius = 4
        sizeLabel.alignmentMode = .center
        sizeLabel.contentsScale = scale
        sizeLabel.isHidden = true

        magnifier.magnificationFilter = .nearest
        magnifier.contentsGravity = .resize
        magnifier.borderColor = NSColor.white.cgColor
        magnifier.borderWidth = 1.5
        magnifier.cornerRadius = 6
        magnifier.masksToBounds = true
        magnifier.backgroundColor = NSColor.black.cgColor
        magnifier.bounds = CGRect(x: 0, y: 0, width: magnifierSize, height: magnifierSize)
        magnifier.isHidden = true
        let cell = magnifierSize / CGFloat(magnifierPixels)
        magnifierCenter.frame = CGRect(x: (magnifierSize - cell) / 2, y: (magnifierSize - cell) / 2, width: cell, height: cell)
        magnifierCenter.borderColor = NSColor.systemRed.cgColor
        magnifierCenter.borderWidth = 1
        magnifier.addSublayer(magnifierCenter)

        measureLine.strokeColor = NSColor.systemYellow.cgColor
        measureLine.lineWidth = 1.5
        measureLine.fillColor = NSColor.systemYellow.cgColor

        hintLabel.string = Self.labelText(" Arrastra para medir · ⌘C copia la medida · Esc cierra ")
        hintLabel.contentsScale = scale
        hintLabel.backgroundColor = NSColor.black.withAlphaComponent(0.75).cgColor
        hintLabel.cornerRadius = 6
        hintLabel.alignmentMode = .center
        let hintSize = Self.labelSize(hintLabel.string)
        hintLabel.frame = CGRect(x: (size.width - hintSize.width) / 2, y: size.height - hintSize.height - 48, width: hintSize.width, height: hintSize.height)
        hintLabel.isHidden = !measuring

        for sublayer in [dimLayer, crosshairLayer, outlineLayer, dashLayer, measureLine, sizeLabel, hintLabel, magnifier] {
            root.addSublayer(sublayer)
        }
        refresh(cursor: nil)
    }

    required init?(coder: NSCoder) { nil }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.activeAlways, .mouseMoved, .mouseEnteredAndExited, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) {
        window?.makeKey()
        NSCursor.crosshair.set()
    }

    override func mouseExited(with event: NSEvent) {
        guard dragOrigin == nil else { return }
        hovered = nil
        refresh(cursor: nil)
    }

    override func mouseMoved(with event: NSEvent) {
        NSCursor.crosshair.set()
        let point = location(of: event)
        hovered = windowRects.first { $0.contains(point) }
        refresh(cursor: point)
    }

    override func mouseDown(with event: NSEvent) {
        dragOrigin = measuring ? snapped(location(of: event)) : location(of: event)
        selection = nil
        measureStart = measuring ? dragOrigin : nil
        measureEnd = nil
    }

    override func mouseDragged(with event: NSEvent) {
        guard let origin = dragOrigin else { return }
        let point = measuring ? snapped(location(of: event)) : location(of: event)
        if measuring { measureEnd = point }
        selection = CGRect(
            x: min(origin.x, point.x), y: min(origin.y, point.y),
            width: abs(point.x - origin.x), height: abs(point.y - origin.y)
        )
        refresh(cursor: point)
    }

    override func mouseUp(with event: NSEvent) {
        defer { dragOrigin = nil }
        if measuring {
            if selection == nil { measureStart = nil }
            return
        }
        if let selection, selection.width >= 3, selection.height >= 3 {
            complete(with: selection)
        } else {
            complete(with: hovered ?? bounds)
        }
    }

    override func rightMouseDown(with event: NSEvent) {
        if selection != nil {
            selection = nil
            measureStart = nil
            measureEnd = nil
            dragOrigin = nil
            refresh(cursor: location(of: event))
        } else {
            cancel()
        }
    }

    override func keyDown(with event: NSEvent) {
        switch Int(event.keyCode) {
        case 53: cancel()
        case 36, 76: measuring ? cancel() : complete(with: selection ?? hovered ?? bounds)
        case 8 where measuring && event.modifierFlags.contains(.command): copyMeasurement()
        default: super.keyDown(with: event)
        }
    }

    private func location(of event: NSEvent) -> CGPoint {
        let point = convert(event.locationInWindow, from: nil)
        return CGPoint(x: min(max(point.x, 0), bounds.width), y: min(max(point.y, 0), bounds.height))
    }

    private func snapped(_ point: CGPoint) -> CGPoint {
        CGPoint(x: (point.x * scale).rounded() / scale, y: (point.y * scale).rounded() / scale)
    }

    private struct Measurement {
        let width: Int
        let height: Int
        let diagonal: Double
        let angle: Double
    }

    private func measurement() -> Measurement? {
        guard let start = measureStart, let end = measureEnd, let selection else { return nil }
        let width = Int((selection.width * scale).rounded())
        let height = Int((selection.height * scale).rounded())
        return Measurement(
            width: width,
            height: height,
            diagonal: hypot(Double(width), Double(height)),
            angle: atan2(Double(end.y - start.y), Double(end.x - start.x)) * 180 / .pi
        )
    }

    private func pixelRect(for rect: CGRect) -> CGRect {
        CGRect(
            x: rect.minX * scale,
            y: (bounds.height - rect.maxY) * scale,
            width: rect.width * scale,
            height: rect.height * scale
        ).integral
    }

    private func refresh(cursor: CGPoint?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }

        let active = measuring ? selection : (selection ?? (dragOrigin == nil ? hovered : nil))?.integral
        let dim = CGMutablePath()
        dim.addRect(bounds)
        if let active { dim.addRect(active) }
        dimLayer.path = dim

        let outline = active.map { CGPath(rect: $0.insetBy(dx: 0.5, dy: 0.5), transform: nil) }
        outlineLayer.path = outline
        dashLayer.path = outline

        if let start = measureStart, let end = measureEnd, let measurement = measurement() {
            let line = CGMutablePath()
            line.move(to: start)
            line.addLine(to: end)
            line.addEllipse(in: CGRect(x: start.x - 2.5, y: start.y - 2.5, width: 5, height: 5))
            line.addEllipse(in: CGRect(x: end.x - 2.5, y: end.y - 2.5, width: 5, height: 5))
            measureLine.path = line
            sizeLabel.string = Self.labelText(describe(measurement, separator: "\n"))
            let labelSize = Self.labelSize(sizeLabel.string)
            var origin = CGPoint(x: end.x + 14, y: end.y + 14)
            if origin.x + labelSize.width > bounds.width { origin.x = end.x - labelSize.width - 14 }
            if origin.y + labelSize.height > bounds.height { origin.y = end.y - labelSize.height - 14 }
            sizeLabel.frame = CGRect(origin: origin, size: labelSize)
            sizeLabel.isHidden = false
        } else if measuring {
            measureLine.path = nil
            sizeLabel.isHidden = true
        } else if let active {
            let pixels = pixelRect(for: active)
            sizeLabel.string = RegionLabel.text(
                width: Int(pixels.width), height: Int(pixels.height),
                screenWidth: Int(bounds.width * scale), screenHeight: Int(bounds.height * scale)
            )
            let labelSize = sizeLabel.preferredFrameSize()
            var origin = CGPoint(x: active.minX, y: active.maxY + 4)
            if origin.y + labelSize.height > bounds.height { origin.y = active.maxY - labelSize.height - 4 }
            origin.x = min(origin.x, bounds.width - labelSize.width)
            sizeLabel.frame = CGRect(origin: origin, size: labelSize)
            sizeLabel.isHidden = false
        } else {
            sizeLabel.isHidden = true
        }

        guard let cursor else {
            crosshairLayer.path = nil
            magnifier.isHidden = true
            return
        }

        let cross = CGMutablePath()
        cross.move(to: CGPoint(x: 0, y: cursor.y.rounded() + 0.5))
        cross.addLine(to: CGPoint(x: bounds.width, y: cursor.y.rounded() + 0.5))
        cross.move(to: CGPoint(x: cursor.x.rounded() + 0.5, y: 0))
        cross.addLine(to: CGPoint(x: cursor.x.rounded() + 0.5, y: bounds.height))
        crosshairLayer.path = cross

        let half = CGFloat(magnifierPixels / 2)
        let center = CGPoint(x: (cursor.x * scale).rounded(.down), y: ((bounds.height - cursor.y) * scale).rounded(.down))
        let source = CGRect(x: center.x - half, y: center.y - half, width: CGFloat(magnifierPixels), height: CGFloat(magnifierPixels))
        magnifier.contents = image.pixels(in: source)?.image
        var position = CGPoint(x: cursor.x + 24 + magnifierSize / 2, y: cursor.y - 24 - magnifierSize / 2)
        if position.x + magnifierSize / 2 > bounds.width { position.x = cursor.x - 24 - magnifierSize / 2 }
        if position.y - magnifierSize / 2 < 0 { position.y = cursor.y + 24 + magnifierSize / 2 }
        magnifier.position = position
        magnifier.isHidden = false
    }

    private func describe(_ measurement: Measurement, separator: String) -> String {
        var size = "\(measurement.width) × \(measurement.height) px"
        if scale != 1 {
            size += " (\(Self.format(Double(measurement.width) / scale)) × \(Self.format(Double(measurement.height) / scale)) pt)"
        }
        return [size, "Diagonal \(Self.format(measurement.diagonal)) px · \(Self.format(measurement.angle))°"].joined(separator: separator)
    }

    private static func format(_ value: Double) -> String {
        value.formatted(.number.precision(.fractionLength(0...1)).grouping(.never))
    }

    private static func labelText(_ text: String) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        let lines = text.split(separator: "\n").map { " \($0) " }.joined(separator: "\n")
        return NSAttributedString(string: lines, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: NSColor.white,
            .paragraphStyle: paragraph,
        ])
    }

    private static func labelSize(_ string: Any?) -> CGSize {
        guard let text = string as? NSAttributedString else { return .zero }
        let size = text.boundingRect(with: CGSize(width: 1000, height: 1000), options: [.usesLineFragmentOrigin]).size
        return CGSize(width: ceil(size.width) + 4, height: ceil(size.height) + 4)
    }

    private func copyMeasurement() {
        guard let measurement = measurement() else { return }
        let text = describe(measurement, separator: " · ")
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        Toast.show("Medida copiada", detail: text)
    }

    private func complete(with rect: CGRect) {
        guard !finished else { return }
        finished = true
        let clipped = rect.integral.intersection(bounds)
        onFinish(clipped.isEmpty ? nil : clipped)
    }

    private func cancel() {
        guard !finished else { return }
        finished = true
        onFinish(nil)
    }
}
