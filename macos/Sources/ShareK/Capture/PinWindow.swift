import AppKit
import UniformTypeIdentifiers

@MainActor
enum PinManager {
    private static var pins: [PinWindow] = []

    static var hasPins: Bool { !pins.isEmpty }

    static func pin(_ pixels: Pixels, scale: CGFloat, at globalRect: CGRect? = nil) {
        let size = CGSize(width: CGFloat(pixels.width) / scale, height: CGFloat(pixels.height) / scale)
        let frame: CGRect
        if let globalRect {
            frame = globalRect
        } else {
            let visible = (NSScreen.underMouse ?? NSScreen.main)?.visibleFrame ?? CGRect(x: 0, y: 0, width: 1280, height: 800)
            let fit = min(1, visible.width * 0.7 / size.width, visible.height * 0.7 / size.height)
            let fitted = CGSize(width: size.width * fit, height: size.height * fit)
            let offset = CGFloat(pins.count % 8) * 24
            frame = CGRect(x: visible.midX - fitted.width / 2 + offset, y: visible.midY - fitted.height / 2 - offset, width: fitted.width, height: fitted.height)
        }
        let window = PinWindow(pixels: pixels, scale: scale, baseSize: size, frame: frame)
        window.onClose = { closed in
            pins.removeAll { $0 === closed }
            Memory.releaseSoon()
        }
        pins.append(window)
        window.orderFrontRegardless()
        window.makeKey()
    }

    static func closeAll() {
        for pin in pins { pin.close() }
    }
}

@MainActor
final class PinWindow: NSPanel {
    let pixels: Pixels
    let scale: CGFloat
    var onClose: ((PinWindow) -> Void)?

    private let baseSize: CGSize

    init(pixels: Pixels, scale: CGFloat, baseSize: CGSize, frame: CGRect) {
        self.pixels = pixels
        self.scale = scale
        self.baseSize = baseSize
        super.init(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        level = .floating
        isFloatingPanel = true
        hidesOnDeactivate = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        isReleasedWhenClosed = false
        animationBehavior = .utilityWindow
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        contentView = PinView(pixels: pixels)
    }

    override var canBecomeKey: Bool { true }

    override func close() {
        super.close()
        onClose?(self)
        onClose = nil
    }

    func zoom(by factor: CGFloat, anchor: CGPoint) {
        let current = frame
        let width = min(max(current.width * factor, 24), baseSize.width * 8)
        let height = width * baseSize.height / baseSize.width
        let relativeX = (anchor.x - current.minX) / current.width
        let relativeY = (anchor.y - current.minY) / current.height
        setFrame(CGRect(x: anchor.x - width * relativeX, y: anchor.y - height * relativeY, width: width, height: height), display: true)
    }

    func resetSize() {
        let current = frame
        setFrame(CGRect(x: current.minX, y: current.maxY - baseSize.height, width: baseSize.width, height: baseSize.height), display: true)
    }

    func adjustOpacity(by delta: CGFloat) {
        alphaValue = min(1, max(0.15, alphaValue + delta))
    }
}

@MainActor
private final class PinView: NSView {
    private let pixels: Pixels
    private let frames = FrameBuffers()

    init(pixels: Pixels) {
        self.pixels = pixels
        super.init(frame: .zero)
        wantsLayer = true
        frames.onFallback = { [weak self] in self?.show() }
        layer?.contentsGravity = .resize
        layer?.magnificationFilter = .nearest
        layer?.minificationFilter = .linear
        layer?.borderWidth = 1
        layer?.borderColor = NSColor.black.withAlphaComponent(0.25).cgColor
    }

    required init?(coder: NSCoder) { nil }

    private var pin: PinWindow? { window as? PinWindow }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        show()
    }

    private func show() {
        guard let layer, let window else { return }
        let space = pixels.colorSpace
        if let tagged = NSColorSpace(cgColorSpace: space) { window.colorSpace = tagged }
        frames.present(width: pixels.width, height: pixels.height, colorSpace: space, on: layer) { context in
            pixels.blit(from: pixels.bounds, into: context, at: pixels.bounds)
        }
    }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 2 {
            pin?.close()
            return
        }
        window?.makeKey()
        window?.performDrag(with: event)
    }

    override func keyDown(with event: NSEvent) {
        switch Int(event.keyCode) {
        case 53, 51, 117: pin?.close()
        default: super.keyDown(with: event)
        }
    }

    override func scrollWheel(with event: NSEvent) {
        guard let pin else { return }
        let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY * 0.005 : event.scrollingDeltaY * 0.05
        guard delta != 0 else { return }
        if event.modifierFlags.contains(.option) {
            pin.adjustOpacity(by: delta)
        } else {
            pin.zoom(by: 1 + delta, anchor: NSEvent.mouseLocation)
        }
    }

    override func magnify(with event: NSEvent) {
        pin?.zoom(by: 1 + event.magnification, anchor: NSEvent.mouseLocation)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()
        add("Copiar", #selector(copyImage), to: menu)
        add("Abrir en el editor", #selector(openInEditor), to: menu)
        add("Guardar como…", #selector(saveAs), to: menu)
        add("Extraer texto (OCR)", #selector(recognizeText), to: menu)
        menu.addItem(.separator())
        let opacity = NSMenuItem(title: "Opacidad", action: nil, keyEquivalent: "")
        let levels = NSMenu()
        for level in [100, 75, 50, 25] {
            let item = add("\(level) %", #selector(setOpacity(_:)), to: levels)
            item.tag = level
            item.state = Int(((pin?.alphaValue ?? 1) * 100).rounded()) == level ? .on : .off
        }
        opacity.submenu = levels
        menu.addItem(opacity)
        add("Tamaño original", #selector(originalSize), to: menu)
        menu.addItem(.separator())
        add("Cerrar", #selector(closePin), to: menu)
        add("Cerrar todas", #selector(closeAll), to: menu)
        return menu
    }

    @discardableResult
    private func add(_ title: String, _ action: Selector, to menu: NSMenu) -> NSMenuItem {
        let item = menu.addItem(withTitle: title, action: action, keyEquivalent: "")
        item.target = self
        return item
    }

    @objc private func copyImage() {
        guard let pin else { return }
        ClipboardManager.shared.copy(CaptureID.next(), pixels: pin.pixels, scale: pin.scale).encode()
        EditorProcess.shared.syncClipboardOwner()
    }

    @objc private func openInEditor() {
        guard let pin else { return }
        EditorProcess.shared.open(pin.pixels, id: CaptureID.next(), scale: pin.scale, created: Date(), saved: nil)
    }

    @objc private func saveAs() {
        guard let pin else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.png, .jpeg]
        panel.directoryURL = Settings.saveDirectory
        panel.nameFieldStringValue = ImageWriter.uniqueURL(in: Settings.saveDirectory, date: Date()).lastPathComponent
        NSApp.activate()
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try ImageWriter.write(pin.pixels, scale: pin.scale, to: url)
            History.shared.record(url)
        } catch {
            Toast.show("No se pudo guardar", detail: error.localizedDescription)
        }
    }

    @objc private func recognizeText() {
        guard let pin else { return }
        TextRecognizer.recognizeAndCopy(pin.pixels)
    }

    @objc private func setOpacity(_ sender: NSMenuItem) {
        pin?.alphaValue = CGFloat(sender.tag) / 100
    }

    @objc private func originalSize() {
        pin?.resetSize()
    }

    @objc private func closePin() {
        pin?.close()
    }

    @objc private func closeAll() {
        PinManager.closeAll()
    }
}
