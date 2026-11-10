import AppKit
import UniformTypeIdentifiers

@MainActor
final class CapturePage: NSViewController, NSMenuItemValidation, AnnotationViewDelegate, EditorPage {
    static let chrome = CGSize(width: 80, height: 200)

    private let tab: CaptureTab
    private let canvas: CanvasView
    private let overlay: AnnotationView
    private let scrollView: EditorScrollView
    private var chrome: Chrome?
    private var hidden = false
    private var coalescing: (key: String, id: UUID, time: Date)?

    private var capture: CaptureDocument { tab.capture }

    @MainActor
    private final class Chrome {
        let toolbar = EditorToolbar()
        let options = OptionsPill()
        let zoomControl = ZoomControl()
        let hint = HintView()
        var optionsCenter: NSLayoutConstraint?
        var zoomBottom: NSLayoutConstraint?
    }

    init(tab: CaptureTab) {
        self.tab = tab
        canvas = CanvasView(size: tab.capture.pointSize)
        overlay = AnnotationView(document: tab.capture, canvas: canvas)
        scrollView = EditorScrollView(overlay: overlay)
        super.init(nibName: nil, bundle: nil)

        scrollView.documentView = canvas
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.allowsMagnification = true
        scrollView.minMagnification = 0.05
        scrollView.maxMagnification = 16
        scrollView.drawsBackground = true
        scrollView.backgroundColor = Theme.canvas
        view = layoutContent()
        installChrome()
        capture.isHidden = false
        overlay.delegate = self
        tab.onRestore = { [weak self] in self?.stateRestored() }

        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(viewportDidChange), name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
        center.addObserver(self, selector: #selector(viewportDidChange), name: NSView.frameDidChangeNotification, object: canvas)
        center.addObserver(self, selector: #selector(magnificationDidChange), name: NSScrollView.didEndLiveMagnifyNotification, object: scrollView)
        center.addObserver(self, selector: #selector(undoWillRun), name: .NSUndoManagerWillUndoChange, object: tab.undo)
        center.addObserver(self, selector: #selector(undoWillRun), name: .NSUndoManagerWillRedoChange, object: tab.undo)
        center.addObserver(self, selector: #selector(stylesDidChange), name: StyleStore.didChange, object: nil)

        let lastTool = UserDefaults.standard.string(forKey: "lastTool").flatMap(Tool.init(rawValue:)) ?? .rectangle
        select(lastTool)
        overlay.select(tab.selection)
    }

    required init?(coder: NSCoder) { nil }

    private func layoutContent() -> NSView {
        let root = FlippedView()
        root.onLayout = { [weak self] in self?.placeChrome() }
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.window.cgColor
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(scrollView)
        NSLayoutConstraint.activate([
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.topAnchor.constraint(equalTo: root.topAnchor, constant: EditorToolbar.height),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        return root
    }

    private func installChrome() {
        guard chrome == nil else { return }
        let root = view
        let chrome = Chrome()
        self.chrome = chrome
        let toolbar = chrome.toolbar
        let options = chrome.options
        let zoomControl = chrome.zoomControl
        toolbar.onSelect = { [weak self] tool in self?.select(tool) }
        toolbar.onAction = { [weak self] name, shift, anchor in self?.toolbarAction(name, shift: shift, anchor: anchor) }
        toolbar.onHint = { [weak self] title, body, anchor in self?.showHint(title, body, anchor) }
        toolbar.copyButton.onClick = { [weak self] in self?.copy(nil) }
        toolbar.copyButton.dragContents = { [weak self] in self?.dragContents() }
        options.onChange = { [weak self] key, change in self?.changeStyle(key: key, change) }
        options.onApplyCrop = { [weak self] in self?.overlay.applyCrop() }
        options.onCancelCrop = { [weak self] in self?.overlay.cancelCrop() }
        options.onPickColor = { [weak self] key in self?.startPicking(key) }
        zoomControl.onZoom = { [weak self] kind in self?.zoomAction(kind) }
        for view in [toolbar, options, zoomControl] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        root.addSubview(chrome.hint)
        let center = options.centerXAnchor.constraint(equalTo: scrollView.centerXAnchor)
        let bottom = zoomControl.bottomAnchor.constraint(equalTo: scrollView.bottomAnchor, constant: -20)
        NSLayoutConstraint.activate([
            toolbar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            toolbar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            toolbar.topAnchor.constraint(equalTo: root.topAnchor),
            toolbar.heightAnchor.constraint(equalToConstant: EditorToolbar.height),
            options.bottomAnchor.constraint(equalTo: scrollView.bottomAnchor, constant: -14),
            options.widthAnchor.constraint(lessThanOrEqualTo: scrollView.widthAnchor, constant: -24),
            zoomControl.trailingAnchor.constraint(equalTo: scrollView.trailingAnchor, constant: -14),
            center,
            bottom,
        ])
        chrome.optionsCenter = center
        chrome.zoomBottom = bottom
        toolbar.selected = overlay.tool
        refreshOptions()
        zoomControl.show(magnification: scrollView.magnification)
    }

    private func removeChrome() {
        guard let chrome else { return }
        self.chrome = nil
        for view in [chrome.toolbar, chrome.options, chrome.zoomControl, chrome.hint] as [NSView] {
            view.removeFromSuperview()
        }
    }

    private func placeChrome() {
        let width = scrollView.frame.width
        guard width > 0, let chrome, let optionsCenter = chrome.optionsCenter, let zoomBottom = chrome.zoomBottom else { return }
        let options = chrome.options
        let widths = options.widths
        let compact = widths.full > width - 24
        if options.compact != compact { options.compact = compact }
        let wanted = min(compact ? widths.compact : widths.full, width - 24)
        let room = chrome.zoomControl.fittingSize.width + 14 + 10
        let stacked = wanted > width - 12 - room
        let centered = stacked || wanted <= width - 2 * room
        let center = centered ? 0 : 12 + (width - room - 12 - wanted) / 2 + wanted / 2 - width / 2
        let bottom: CGFloat = stacked ? -(14 + 44 + 8) : -20
        if optionsCenter.constant != center { optionsCenter.constant = center }
        if zoomBottom.constant != bottom { zoomBottom.constant = bottom }
    }

    func attached() {
        view.window?.makeFirstResponder(overlay)
        overlay.tagWindow()
        guard !tab.autoFit, let viewport = tab.viewport else {
            fitToWindow()
            return
        }
        view.window?.contentView?.layoutSubtreeIfNeeded()
        scrollView.magnification = viewport.magnification
        scrollView.contentView.scroll(to: viewport.origin)
        scrollView.reflectScrolledClipView(scrollView.contentView)
        overlay.needsDisplay = true
        chrome?.zoomControl.show(magnification: scrollView.magnification)
    }

    func windowDidResize() {
        if tab.autoFit { fitToWindow() }
    }

    private func fitToWindow() {
        view.window?.contentView?.layoutSubtreeIfNeeded()
        let available = scrollView.contentSize
        guard available.width > 1, available.height > 1 else { return }
        let size = canvas.frame.size
        if size.width <= available.width, size.height <= available.height {
            scrollView.magnification = 1
        } else {
            scrollView.magnify(toFit: canvas.frame)
        }
        overlay.needsDisplay = true
        chrome?.zoomControl.show(magnification: scrollView.magnification)
    }

    private func showHint(_ title: String, _ body: String, _ anchor: NSView?) {
        guard let anchor else {
            chrome?.hint.isHidden = true
            return
        }
        let rect = view.convert(anchor.bounds, from: anchor)
        chrome?.hint.show(title: title, body: body, at: CGPoint(x: rect.minX, y: rect.maxY + 4), within: view.bounds)
    }

    private func select(_ tool: Tool) {
        overlay.tool = tool
        chrome?.toolbar.selected = tool
        if tool != .crop, tool != .cutOut {
            UserDefaults.standard.set(tool.rawValue, forKey: "lastTool")
        }
        refreshOptions()
        view.window?.makeFirstResponder(overlay)
    }

    private func refreshOptions() {
        let kind = overlay.selectedShape?.kind ?? overlay.tool.kind
        let style = overlay.selectedShape?.style ?? kind.map(StyleStore.style(for:))
        chrome?.options.show(tool: overlay.tool, kind: kind, style: style)
        placeChrome()
    }

    @objc private func stylesDidChange() {
        chrome?.toolbar.refresh()
        refreshOptions()
    }

    private func changeStyle(key: String, _ change: (inout ShapeStyle) -> Void) {
        guard let selected = overlay.selectedShape else {
            guard let kind = overlay.tool.kind else { return }
            var style = StyleStore.style(for: kind)
            change(&style)
            StyleStore.save(style, for: kind)
            overlay.restoreFocus()
            return
        }
        let before = capture.state
        overlay.updateSelected {
            change(&$0.style)
            if $0.kind.isText { TextMetrics.fit(&$0) }
        }
        let now = Date()
        if let coalescing, coalescing.key == key, coalescing.id == selected.id, now.timeIntervalSince(coalescing.time) < 1.5 {
        } else {
            tab.register(before, name: "Cambiar estilo")
        }
        coalescing = (key, selected.id, now)
        if let style = overlay.selectedShape?.style { StyleStore.save(style, for: selected.kind) }
        documentDidChange()
        overlay.restoreFocus()
    }

    private func startPicking(_ key: String) {
        overlay.picking = { [weak self] point in
            guard let self else { return }
            guard let color = capture.rendered()?.color(at: CGPoint(x: point.x * capture.scale, y: point.y * capture.scale)) else { return }
            let value = RGBA(r: color.r, g: color.g, b: color.b, a: 1)
            changeStyle(key: key) { style in
                switch key {
                case "text": style.text = value
                case "stroke": style.stroke = value
                default: style.fill = value
                }
            }
            refreshOptions()
        }
        view.window?.makeFirstResponder(overlay)
    }

    private func stateRestored() {
        overlay.stateDidReset()
        documentDidChange()
        refreshOptions()
        if tab.autoFit { fitToWindow() }
    }

    private func documentDidChange() {
        canvas.resize(to: capture.pointSize)
        overlay.needsDisplay = true
        EditorManager.shared.refreshTabs()
        EditorClipboard.documentDidChange(capture)
    }

    @objc private func viewportDidChange() {
        overlay.needsDisplay = true
        chrome?.zoomControl.show(magnification: scrollView.magnification)
    }

    @objc private func magnificationDidChange() {
        tab.autoFit = false
        chrome?.zoomControl.show(magnification: scrollView.magnification)
        overlay.needsDisplay = true
    }

    @objc private func undoWillRun() {
        overlay.endEditing(commit: false)
    }

    func annotationView(_ view: AnnotationView, commitFrom before: EditorState, actionName: String) {
        coalescing = nil
        tab.register(before, name: actionName)
        documentDidChange()
        refreshOptions()
    }

    func annotationViewSelectionDidChange(_ view: AnnotationView) {
        coalescing = nil
        refreshOptions()
    }

    func annotationView(_ view: AnnotationView, didPick tool: Tool) {
        select(tool)
    }

    func annotationView(_ view: AnnotationView, documentHistory redo: Bool) {
        if redo {
            if tab.undo.canRedo { tab.undo.redo() }
        } else if tab.undo.canUndo {
            tab.undo.undo()
        }
    }

    func resume() {
        guard hidden else { return }
        hidden = false
        capture.isHidden = false
        installChrome()
        view.layoutSubtreeIfNeeded()
        overlay.resumeBacking()
    }

    func suspend() {
        guard !hidden else { return }
        hidden = true
        SwatchPopover.dismiss()
        FramePopover.dismiss()
        removeChrome()
        overlay.releaseBacking()
        capture.isHidden = true
        Memory.releaseSoon()
        CATransaction.flush()
    }

    func close() {
        NotificationCenter.default.removeObserver(self)
        overlay.endDrag()
        overlay.endEditing()
        tab.onRestore = nil
        tab.selection = overlay.selectedShape?.id
        tab.viewport = CaptureTab.Viewport(magnification: scrollView.magnification, origin: scrollView.contentView.bounds.origin)
        SwatchPopover.dismiss()
        FramePopover.dismiss()
        removeChrome()
        overlay.releaseScene()
        capture.isHidden = true
    }

    var probeState: String {
        let frame = overlay.frameSize
        return "shown=\(!hidden) chrome=\(chrome != nil) shapes=\(capture.state.shapes.count) frame=\(frame.width)x\(frame.height) \(overlay.frameKind) zoom=\(scrollView.magnification)"
    }

    func probeDraw(_ tool: Tool) async {
        guard let window = view.window else { return }
        select(tool)
        let bounds = overlay.bounds
        func event(_ type: NSEvent.EventType, _ reach: CGFloat) -> NSEvent? {
            NSEvent.mouseEvent(
                with: type, location: overlay.convert(CGPoint(x: bounds.width * reach, y: bounds.height * reach), to: nil),
                modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: 1, pressure: 1
            )
        }
        if let down = event(.leftMouseDown, 0.3) { overlay.mouseDown(with: down) }
        for step in 1...30 {
            if let move = event(.leftMouseDragged, 0.3 + 0.4 * CGFloat(step) / 30) { overlay.mouseDragged(with: move) }
            try? await Task.sleep(for: .milliseconds(16))
        }
        if let up = event(.leftMouseUp, 0.7) { overlay.mouseUp(with: up) }
        overlay.endEditing()
    }

    func probeZoom(_ magnification: CGFloat) {
        tab.autoFit = false
        scrollView.magnification = magnification
        overlay.needsDisplay = true
    }

    func probeRedraw() {
        overlay.needsDisplay = true
    }

    func probeUndo() {
        tab.undo.undo()
    }

    func probePin() {
        guard let pixels = capture.rendered() else { return }
        PinManager.pin(pixels, scale: capture.scale)
    }

    func probeFrame(to url: URL) -> Bool {
        overlay.writeFrame(to: url)
    }

    func annotationView(_ view: AnnotationView, recognizeIn area: CGRect) {
        guard let window = self.view.window else { return }
        let scale = capture.scale
        let pixels = CGRect(x: area.minX * scale, y: area.minY * scale, width: area.width * scale, height: area.height * scale).integral
        guard let crop = capture.rendered()?.cropped(to: pixels) else { return }
        TextRecognizer.presentResult(of: crop, on: window)
    }

    private func toolbarAction(_ name: String, shift: Bool, anchor: NSView) {
        chrome?.hint.isHidden = true
        switch name {
        case "save": saveCapture(nil)
        case "save-as": saveCaptureAs(nil)
        case "ocr": recognizeText(nil)
        case "ocr-area":
            overlay.endEditing()
            overlay.selectingText = true
            view.window?.makeFirstResponder(overlay)
        case "pin": pinCapture(nil)
        case "undo": tab.undo.undo()
        case "redo": tab.undo.redo()
        case "rotate": rotate(clockwise: !shift)
        case "flip": flip(horizontal: !shift)
        case "frame": FramePopover.show(from: anchor) { [weak self] frame in self?.applyFrame(frame) }
        case "discard": discardCapture(nil)
        default: break
        }
    }

    private func replaceBase(_ name: String, _ build: (EditorState) -> EditorState?) {
        overlay.endEditing()
        overlay.cancelCrop()
        let before = capture.state
        guard let next = build(before) else { return }
        capture.state = next
        tab.register(before, name: name)
        overlay.stateDidReset()
        documentDidChange()
        refreshOptions()
        if tab.autoFit { fitToWindow() }
    }

    private static func moved(_ shape: Shape, _ map: (CGPoint) -> CGPoint) -> Shape {
        var moved = shape
        guard shape.kind.isText || shape.kind == .image else {
            moved.map(map)
            return moved
        }
        let rect = shape.rect
        let center = map(CGPoint(x: rect.midX, y: rect.midY))
        let dx = center.x - rect.midX
        let dy = center.y - rect.midY
        moved.map { CGPoint(x: $0.x + dx, y: $0.y + dy) }
        moved.tail = map(shape.tail)
        return moved
    }

    private func rotate(clockwise: Bool) {
        let scale = capture.scale
        replaceBase("Girar") { state in
            guard let base = state.source.current, let image = ImageOps.rotated(base, clockwise: clockwise) else { return nil }
            let width = CGFloat(base.width) / scale
            let height = CGFloat(base.height) / scale
            let map: (CGPoint) -> CGPoint = clockwise ? { CGPoint(x: height - $0.y, y: $0.x) } : { CGPoint(x: $0.y, y: width - $0.x) }
            return EditorState(base: image, shapes: state.shapes.map { Self.moved($0, map) })
        }
    }

    private func flip(horizontal: Bool) {
        let scale = capture.scale
        replaceBase("Voltear") { state in
            guard let base = state.source.current, let image = ImageOps.flipped(base, horizontal: horizontal) else { return nil }
            let width = CGFloat(base.width) / scale
            let height = CGFloat(base.height) / scale
            let map: (CGPoint) -> CGPoint = horizontal ? { CGPoint(x: width - $0.x, y: $0.y) } : { CGPoint(x: $0.x, y: height - $0.y) }
            return EditorState(base: image, shapes: state.shapes.map { Self.moved($0, map) })
        }
    }

    private func applyFrame(_ frame: ImageFrame) {
        let scale = capture.scale
        replaceBase("Marco") { state in
            guard let base = state.source.current, let result = ImageOps.framed(base, scale: scale, frame: frame) else { return nil }
            let offset = result.offset
            let shapes = state.shapes.map { shape -> Shape in
                var moved = shape
                moved.map { CGPoint(x: $0.x + offset, y: $0.y + offset) }
                return moved
            }
            return EditorState(base: result.pixels, shapes: shapes)
        }
    }

    private func dragContents() -> (png: Data, file: URL, preview: NSImage)? {
        overlay.endEditing()
        guard let pixels = capture.rendered(), let png = PNGEncoder.encode(pixels, scale: capture.scale, fast: true) else { return nil }
        let directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("ShareK/drag", isDirectory: true)
        try? FileManager.default.removeItem(at: directory)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let file = ImageWriter.uniqueURL(in: directory, date: capture.createdAt)
        guard (try? png.write(to: file)) != nil else { return nil }
        let factor = min(512 / CGFloat(pixels.width), 512 / CGFloat(pixels.height), 1)
        let small = pixels.scaled(width: max(Int(CGFloat(pixels.width) * factor), 1), height: max(Int(CGFloat(pixels.height) * factor), 1))
        let preview = small?.image.map { NSImage(cgImage: $0, size: capture.pointSize) } ?? NSImage(size: capture.pointSize)
        return (png, file, preview)
    }

    @objc func copy(_ sender: Any?) {
        overlay.endEditing()
        EditorClipboard.copy(capture)
    }

    @objc func paste(_ sender: Any?) {
        guard let image = NSImage(pasteboard: .general)?.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            Toast.show("El portapapeles no tiene una imagen")
            return
        }
        overlay.insert(image: image)
    }

    @objc func delete(_ sender: Any?) {
        overlay.deleteSelection()
    }

    @objc func saveCapture(_ sender: Any?) {
        overlay.endEditing()
        capture.save()
        EditorManager.shared.refreshTabs()
    }

    @objc func saveCaptureAs(_ sender: Any?) {
        guard let window = view.window else { return }
        overlay.endEditing()
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.png, .jpeg]
        panel.directoryURL = Settings.saveDirectory
        panel.nameFieldStringValue = ImageWriter.uniqueURL(in: Settings.saveDirectory, date: capture.createdAt).lastPathComponent
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            self.capture.save(to: url)
            EditorManager.shared.refreshTabs()
        }
    }

    @objc func pinCapture(_ sender: Any?) {
        overlay.endEditing()
        guard let pixels = capture.rendered() else { return }
        EditorClipboard.pin(pixels, scale: capture.scale)
    }

    @objc func recognizeText(_ sender: Any?) {
        guard let window = view.window else { return }
        overlay.endEditing()
        guard let rendered = capture.rendered() else { return }
        TextRecognizer.presentResult(of: rendered, on: window)
    }

    @objc func discardCapture(_ sender: Any?) {
        overlay.endEditing(commit: false)
        EditorManager.shared.closeCurrent(discarded: true)
    }

    @objc func zoomIn(_ sender: Any?) {
        zoom(by: 1.25)
    }

    @objc func zoomOut(_ sender: Any?) {
        zoom(by: 0.8)
    }

    @objc func zoomActualSize(_ sender: Any?) {
        tab.autoFit = false
        scrollView.magnification = 1
        overlay.needsDisplay = true
        chrome?.zoomControl.show(magnification: 1)
    }

    @objc func zoomToFit(_ sender: Any?) {
        tab.autoFit = true
        fitToWindow()
    }

    private func zoomAction(_ kind: String) {
        switch kind {
        case "in": zoomIn(nil)
        case "out": zoomOut(nil)
        default: zoomToFit(nil)
        }
    }

    private func zoom(by factor: CGFloat) {
        tab.autoFit = false
        let visible = scrollView.contentView.bounds
        scrollView.setMagnification(scrollView.magnification * factor, centeredAt: CGPoint(x: visible.midX, y: visible.midY))
        overlay.needsDisplay = true
        chrome?.zoomControl.show(magnification: scrollView.magnification)
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        switch menuItem.action {
        case #selector(paste(_:)):
            NSPasteboard.general.canReadObject(forClasses: [NSImage.self], options: nil)
        case #selector(delete(_:)):
            overlay.selectedShape != nil
        default:
            true
        }
    }
}

private final class FlippedView: NSView {
    var onLayout: (() -> Void)?

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        onLayout?()
    }
}
