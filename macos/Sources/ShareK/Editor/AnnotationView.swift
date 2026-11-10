import AppKit

@MainActor
protocol AnnotationViewDelegate: AnyObject {
    func annotationView(_ view: AnnotationView, commitFrom before: EditorState, actionName: String)
    func annotationViewSelectionDidChange(_ view: AnnotationView)
    func annotationView(_ view: AnnotationView, didPick tool: Tool)
    func annotationView(_ view: AnnotationView, recognizeIn area: CGRect)
    func annotationView(_ view: AnnotationView, documentHistory redo: Bool)
}

@MainActor
final class AnnotationView: NSView, NSTextViewDelegate {
    weak var delegate: AnnotationViewDelegate?
    let document: CaptureDocument
    let canvas: CanvasView

    var tool: Tool = .select {
        didSet {
            endEditing()
            cancelCrop()
            selectingText = false
            picking = nil
            if tool != .select { select(nil) }
            window?.invalidateCursorRects(for: self)
        }
    }

    private(set) var selectedID: UUID? {
        didSet {
            guard oldValue != selectedID else { return }
            needsDisplay = true
            delegate?.annotationViewSelectionDidChange(self)
        }
    }

    private enum Drag {
        case create(UUID, origin: CGPoint)
        case move(UUID, last: CGPoint)
        case handle(UUID, Handle)
        case crop(origin: CGPoint)
        case cropHandle(Handle)
        case cut(start: CGPoint)
        case textArea(origin: CGPoint)
    }

    var selectingText = false {
        didSet {
            textArea = nil
            window?.invalidateCursorRects(for: self)
            needsDisplay = true
        }
    }

    var picking: ((CGPoint) -> Void)? {
        didSet { window?.invalidateCursorRects(for: self) }
    }

    private var textArea: CGRect?

    private var drag: Drag?
    private var before: EditorState?
    private var changed = false
    private(set) var pendingCrop: CGRect?
    private var cutPreview: (start: CGPoint, end: CGPoint)?
    private var textView: EditingTextView?
    private var editingID: UUID?
    private var editingBefore: EditorState?
    private var textUndo = UndoManager()
    private var typed = false
    private var scene: Scene?
    private var baseView: (key: BaseViewKey, pixels: Pixels)?
    static var probeBacking: CGFloat?
    static var probeKeepsWindowSpace = false
    private let frames = FrameBuffers()
    private var frameSpace: (source: ObjectIdentifier, space: CGColorSpace)?
    private var suspended = false

    private struct BaseViewKey: Equatable {
        let source: ObjectIdentifier
        let area: CGRect
        let width: Int
        let height: Int
    }

    private struct SceneKey: Equatable {
        let transform: CGAffineTransform
        let size: CGSize
        let backing: CGFloat
        let base: ObjectIdentifier
    }

    private struct Scene {
        let key: SceneKey
        let pixels: Pixels
        let context: CGContext
        var drawn: [Int]
    }

    init(document: CaptureDocument, canvas: CanvasView) {
        self.document = document
        self.canvas = canvas
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        frames.onFallback = { [weak self] in self?.needsDisplay = true }
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override var isOpaque: Bool { false }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    var selectedShape: Shape? {
        guard let selectedID else { return nil }
        return document.state.shapes.first { $0.id == selectedID }
    }

    private var scale: CGFloat { document.scale }
    private var magnification: CGFloat { (superview as? NSScrollView)?.magnification ?? 1 }
    private var tolerance: CGFloat { 5 / magnification }

    func select(_ id: UUID?) {
        selectedID = id
    }

    func updateSelected(_ body: (inout Shape) -> Void) {
        guard let selectedID else { return }
        update(selectedID, body)
        refreshTextEditor()
    }

    func stateDidReset() {
        endEditing(commit: false)
        cancelCrop()
        if let selectedID, !document.state.shapes.contains(where: { $0.id == selectedID }) { select(nil) }
        needsDisplay = true
    }

    func insert(image: CGImage) {
        endEditing()
        let before = document.state
        let visible = canvas.visibleRect.isEmpty ? canvas.bounds : canvas.visibleRect
        var size = CGSize(width: CGFloat(image.width) / scale, height: CGFloat(image.height) / scale)
        let fit = min(1, visible.width * 0.6 / size.width, visible.height * 0.6 / size.height)
        size = CGSize(width: size.width * fit, height: size.height * fit)
        let origin = CGPoint(x: visible.midX - size.width / 2, y: visible.midY - size.height / 2)
        var shape = Shape(kind: .image, style: StyleStore.style(for: .image), at: origin)
        shape.end = CGPoint(x: origin.x + size.width, y: origin.y + size.height)
        shape.image = image
        document.state.shapes.append(shape)
        select(shape.id)
        commit(from: before, name: "Pegar imagen")
    }


    override func resetCursorRects() {
        addCursorRect(bounds, cursor: tool == .select && !selectingText && picking == nil ? .arrow : .crosshair)
    }

    override func mouseDown(with event: NSEvent) {
        let point = documentPoint(event)
        if let pick = picking {
            picking = nil
            pick(point)
            return
        }
        if event.clickCount == 2,
           let hit = document.state.shapes.last(where: { $0.id != editingID && $0.kind.isText && ShapeGeometry.contains($0, point, tolerance: tolerance) }) {
            endEditing()
            window?.makeFirstResponder(self)
            select(hit.id)
            beginEditing(hit.id, before: document.state)
            return
        }
        if let textView, textView.frame.contains(point) {
            textView.mouseDown(with: event)
            return
        }
        endEditing()
        window?.makeFirstResponder(self)

        if selectingText {
            drag = .textArea(origin: clamp(point))
            textArea = nil
            return
        }
        if event.clickCount == 2, tool == .crop, let crop = pendingCrop, crop.contains(point) {
            applyCrop()
            return
        }

        switch tool {
        case .crop:
            if let crop = pendingCrop, let handle = handle(at: point, in: ShapeGeometry.handles(for: crop)) {
                drag = .cropHandle(handle)
            } else {
                pendingCrop = nil
                drag = .crop(origin: clamp(point))
            }
            needsDisplay = true
            return
        case .cutOut:
            drag = .cut(start: clamp(point))
            return
        default:
            break
        }

        before = document.state
        changed = false
        if let selected = selectedShape, let handle = handle(at: point, in: ShapeGeometry.handles(selected)) {
            drag = .handle(selected.id, handle)
            return
        }
        if let hit = topShape(at: point), tool == .select || hit.id == selectedID {
            select(hit.id)
            drag = .move(hit.id, last: point)
            return
        }
        guard let kind = tool.kind else {
            select(nil)
            before = nil
            return
        }
        create(kind, at: point)
    }

    override func mouseDragged(with event: NSEvent) {
        let point = documentPoint(event)
        let constrained = event.modifierFlags.contains(.shift)
        switch drag {
        case .create(let id, let origin):
            update(id) { shape in
                switch shape.kind {
                case .freehand:
                    if let last = shape.points.last, hypot(point.x - last.x, point.y - last.y) >= 1 / magnification {
                        shape.points.append(point)
                    }
                case .line, .arrow:
                    shape.end = constrained ? ShapeGeometry.snapAngle(point, from: origin) : point
                default:
                    shape.end = constrained ? ShapeGeometry.square(point, from: origin) : point
                }
            }
            changed = true
        case .move(let id, let last):
            let dx = point.x - last.x
            let dy = point.y - last.y
            update(id) { $0.map { CGPoint(x: $0.x + dx, y: $0.y + dy) } }
            drag = .move(id, last: point)
            changed = true
        case .handle(let id, let handle):
            update(id) { ShapeGeometry.apply(handle, point: point, to: &$0, constrained: constrained) }
            changed = true
        case .crop(let origin):
            let end = clamp(constrained ? ShapeGeometry.square(point, from: origin) : point)
            pendingCrop = CGRect(x: min(origin.x, end.x), y: min(origin.y, end.y), width: abs(end.x - origin.x), height: abs(end.y - origin.y))
        case .cropHandle(let handle):
            if let crop = pendingCrop { pendingCrop = ShapeGeometry.resize(crop, handle: handle, to: clamp(point)) }
        case .cut(let start):
            cutPreview = (start, clamp(point))
        case .textArea(let origin):
            let end = clamp(point)
            textArea = CGRect(x: min(origin.x, end.x), y: min(origin.y, end.y), width: abs(end.x - origin.x), height: abs(end.y - origin.y))
        case nil:
            return
        }
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        endDrag(at: clamp(documentPoint(event)))
    }

    func endDrag(at point: CGPoint? = nil) {
        defer {
            drag = nil
            before = nil
            changed = false
            needsDisplay = true
        }
        switch drag {
        case .create(let id, _):
            guard let shape = shape(id) else { return }
            if isDegenerate(shape) {
                remove(id)
                select(nil)
            } else if let before {
                commit(from: before, name: shape.kind == .smartEraser ? Tool.smartEraser.title : Tool(rawValue: shape.kind.rawValue)?.title ?? "Forma")
            }
        case .move:
            if changed, let before { commit(from: before, name: "Mover") }
        case .handle:
            if changed, let before { commit(from: before, name: "Redimensionar") }
        case .crop, .cropHandle:
            if let crop = pendingCrop, crop.width < 3 || crop.height < 3 { pendingCrop = nil }
        case .cut(let start):
            cutPreview = nil
            if let point { applyCut(from: start, to: point) }
        case .textArea:
            let area = textArea
            selectingText = false
            if point != nil, let area, area.width >= 3, area.height >= 3 {
                delegate?.annotationView(self, recognizeIn: area)
            }
        case nil:
            break
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard let shape = topShape(at: documentPoint(event)) else { return nil }
        select(shape.id)
        let menu = NSMenu()
        for (title, action) in [
            ("Duplicar", #selector(duplicateSelection)),
            ("Traer al frente", #selector(bringSelectionToFront)),
            ("Enviar al fondo", #selector(sendSelectionToBack)),
            ("Eliminar", #selector(deleteSelection)),
        ] {
            menu.addItem(withTitle: title, action: action, keyEquivalent: "").target = self
        }
        return menu
    }


    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection([.command, .option, .control, .shift])
        switch Int(event.keyCode) {
        case 53:
            if picking != nil {
                picking = nil
            } else if selectingText {
                selectingText = false
            } else if pendingCrop != nil {
                cancelCrop()
            } else {
                select(nil)
            }
            return
        case 36, 76:
            if pendingCrop != nil {
                applyCrop()
            } else if let shape = selectedShape, shape.kind.isText {
                beginEditing(shape.id, before: document.state)
            }
            return
        case 51, 117:
            deleteSelection()
            return
        case 123, 124, 125, 126:
            let step: CGFloat = flags.contains(.shift) ? 10 : 1
            let delta: CGVector = switch Int(event.keyCode) {
            case 123: CGVector(dx: -step, dy: 0)
            case 124: CGVector(dx: step, dy: 0)
            case 125: CGVector(dx: 0, dy: step)
            default: CGVector(dx: 0, dy: -step)
            }
            nudge(by: delta)
            return
        default:
            break
        }
        if flags.isEmpty, let character = event.charactersIgnoringModifiers?.lowercased().first, let tool = Tool.matching(character) {
            delegate?.annotationView(self, didPick: tool)
            return
        }
        super.keyDown(with: event)
    }

    @objc func deleteSelection() {
        guard let selectedID else { return }
        let before = document.state
        remove(selectedID)
        select(nil)
        commit(from: before, name: "Eliminar")
    }

    @objc private func duplicateSelection() {
        guard let shape = selectedShape else { return }
        let before = document.state
        let copy = shape.duplicated(offset: 12)
        document.state.shapes.append(copy)
        select(copy.id)
        commit(from: before, name: "Duplicar")
    }

    @objc private func bringSelectionToFront() {
        reorder { shapes, index in shapes.append(shapes.remove(at: index)) }
    }

    @objc private func sendSelectionToBack() {
        reorder { shapes, index in shapes.insert(shapes.remove(at: index), at: 0) }
    }

    private func reorder(_ body: (inout [Shape], Int) -> Void) {
        guard let selectedID, let index = document.state.shapes.firstIndex(where: { $0.id == selectedID }) else { return }
        let before = document.state
        body(&document.state.shapes, index)
        commit(from: before, name: "Ordenar")
    }

    private func nudge(by delta: CGVector) {
        guard let selectedID else { return }
        let before = document.state
        update(selectedID) { $0.map { CGPoint(x: $0.x + delta.dx, y: $0.y + delta.dy) } }
        commit(from: before, name: "Mover")
    }

    func applyCrop() {
        guard let crop = pendingCrop else { return }
        let pixelRect = CGRect(x: crop.minX * scale, y: crop.minY * scale, width: crop.width * scale, height: crop.height * scale).integral
        guard let image = document.state.source.current?.cropped(to: pixelRect) else { return }
        let before = document.state
        let dx = pixelRect.minX / scale
        let dy = pixelRect.minY / scale
        var shapes = document.state.shapes
        for index in shapes.indices { shapes[index].map { CGPoint(x: $0.x - dx, y: $0.y - dy) } }
        document.state = EditorState(base: image, shapes: shapes)
        pendingCrop = nil
        commit(from: before, name: Tool.crop.title)
    }

    func cancelCrop() {
        guard pendingCrop != nil else { return }
        pendingCrop = nil
        needsDisplay = true
    }

    private func applyCut(from start: CGPoint, to end: CGPoint) {
        let columns = abs(end.x - start.x) >= abs(end.y - start.y)
        let low = columns ? min(start.x, end.x) : min(start.y, end.y)
        let high = columns ? max(start.x, end.x) : max(start.y, end.y)
        let lowerPixel = Int((low * scale).rounded())
        let upperPixel = Int((high * scale).rounded())
        guard upperPixel - lowerPixel >= 2,
              let base = document.state.source.current,
              let image = ImageOps.cutOut(base, removing: lowerPixel..<upperPixel, columns: columns)
        else { return }
        let before = document.state
        let lower = CGFloat(lowerPixel) / scale
        let upper = CGFloat(upperPixel) / scale
        let removed = upper - lower
        let collapse: (CGFloat) -> CGFloat = { value in
            value >= upper ? value - removed : (value > lower ? lower : value)
        }
        var shapes = document.state.shapes
        for index in shapes.indices {
            shapes[index].map { columns ? CGPoint(x: collapse($0.x), y: $0.y) : CGPoint(x: $0.x, y: collapse($0.y)) }
        }
        document.state = EditorState(base: image, shapes: shapes)
        commit(from: before, name: Tool.cutOut.title)
    }

    private func beginEditing(_ id: UUID, before: EditorState) {
        guard let shape = shape(id), shape.kind.isText else { return }
        editingID = id
        editingBefore = before

        textUndo = UndoManager()
        typed = false
        let textView = EditingTextView(frame: shape.rect)
        textView.onHistory = { [weak self] redo in self?.textHistory(redo: redo) ?? false }
        textView.delegate = self
        textView.isRichText = false
        textView.allowsUndo = true
        textView.importsGraphics = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isHorizontallyResizable = false
        textView.isVerticallyResizable = false
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = false
        textView.textContainer?.containerSize = CGSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
        textView.wantsLayer = true
        textView.string = shape.text
        self.textView = textView
        refreshTextEditor()
        canvas.addSubview(textView)
        window?.makeFirstResponder(textView)
        textView.selectAll(nil)
        needsDisplay = true
    }

    private func refreshTextEditor() {
        guard let textView, let editingID, let shape = shape(editingID) else { return }
        var attributes = TextMetrics.attributes(shape)
        attributes[.foregroundColor] = NSColor.clear
        textView.font = TextMetrics.font(shape)
        textView.textColor = .clear
        textView.insertionPointColor = .clear
        textView.selectedTextAttributes = [.backgroundColor: NSColor.clear, .foregroundColor: NSColor.clear]
        textView.typingAttributes = attributes
        textView.textStorage?.setAttributes(attributes, range: NSRange(location: 0, length: textView.string.utf16.count))
        textView.drawsBackground = false
        textView.textContainerInset = TextMetrics.padding(shape)
        textView.frame = shape.rect
        needsDisplay = true
    }

    func undoManager(for view: NSTextView) -> UndoManager? {
        textUndo
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        needsDisplay = true
    }

    private func textHistory(redo: Bool) -> Bool {
        guard let editingID else { return false }
        let original = editingBefore?.shapes.first { $0.id == editingID }?.text ?? ""
        let belongsToDocument = redo ? !typed : shape(editingID)?.text == original
        guard belongsToDocument else { return false }
        delegate?.annotationView(self, documentHistory: redo)
        return true
    }

    func restoreFocus() {
        window?.makeFirstResponder(textView ?? self)
    }

    func releaseScene() {
        releaseBacking()
        Memory.releaseSoon()
    }

    func releaseBacking() {
        suspended = true
        scene = nil
        baseView = nil
        frames.release()
        layer?.contents = nil
    }

    func resumeBacking() {
        suspended = false
        needsDisplay = true
    }

    var frameSize: (width: Int, height: Int) {
        (frames.shown?.width ?? 0, frames.shown?.height ?? 0)
    }

    var frameKind: String { frames.kind }

    func writeFrame(to url: URL) -> Bool {
        guard let pixels = frames.shown?.copyPixels() else { return false }
        return (try? ImageWriter.write(pixels, scale: 1, to: url)) != nil
    }

    func textDidChange(_ notification: Notification) {
        guard let editingID, let textView else { return }
        typed = true
        let text = textView.string
        update(editingID) {
            $0.text = text
            TextMetrics.fit(&$0)
        }
        if let shape = shape(editingID) { textView.frame = shape.rect }
        needsDisplay = true
    }

    func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard commandSelector == #selector(NSResponder.cancelOperation(_:)) else { return false }
        endEditing()
        return true
    }

    func endEditing(commit shouldCommit: Bool = true) {
        guard let id = editingID else { return }
        let before = editingBefore
        textView?.removeFromSuperview()
        textView = nil
        editingID = nil
        editingBefore = nil
        if let shape = shape(id), shape.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            remove(id)
            select(nil)
        }
        if shouldCommit, let before {
            let previous = before.shapes.first { $0.id == id }
            let current = shape(id)
            if previous?.text != current?.text || (previous == nil) != (current == nil) {
                commit(from: before, name: "Texto")
            }
        }
        window?.makeFirstResponder(self)
        needsDisplay = true
    }

    override var wantsUpdateLayer: Bool { true }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        needsDisplay = true
    }

    @discardableResult
    func tagWindow() -> CGColorSpace {
        let source = document.state.source
        if frameSpace?.source != ObjectIdentifier(source) {
            frameSpace = (ObjectIdentifier(source), source.current?.colorSpace ?? Pixels.sRGB)
        }
        let space = frameSpace?.space ?? Pixels.sRGB
        if !Self.probeKeepsWindowSpace, let window, window.colorSpace?.cgColorSpace.map({ CFEqual($0, space) }) != true, let tagged = NSColorSpace(cgColorSpace: space) {
            window.colorSpace = tagged
        }
        return space
    }

    override func updateLayer() {
        guard let layer, let window else { return }
        guard !suspended else {
            EditorManager.shared.refreshVisibility()
            return
        }
        let backing = Self.probeBacking ?? window.backingScaleFactor
        let width = Int((bounds.width * backing).rounded(.up))
        let height = Int((bounds.height * backing).rounded(.up))
        guard width > 1, height > 1 else { return }
        layer.contentsGravity = .resize
        layer.isOpaque = false
        frames.present(width: width, height: height, colorSpace: tagWindow(), on: layer) { context in
            context.translateBy(x: 0, y: CGFloat(height))
            context.scaleBy(x: backing, y: -backing)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
            render(in: context, backing: backing)
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private func render(in context: CGContext, backing: CGFloat) {
        let transform = documentTransform
        let interacting = drag != nil || editingID != nil
        let live = interacting ? liveFrom : 0
        if interacting {
            drawSettled(upTo: live, transform: transform, backing: backing, in: context)
        } else {
            if scene != nil {
                scene = nil
                Memory.releaseSoon()
            }
            drawBase(in: context, transform: transform, backing: backing)
        }
        context.saveGState()
        context.concatenate(transform)
        context.clip(to: canvas.bounds)
        document.renderer.draw(document.state, scale: scale, in: context, range: live..<document.state.shapes.count)
        drawCropAndCut(in: context)
        context.restoreGState()
        drawTextCursor(in: context, transform: transform)
        drawSelection(in: context, transform: transform)
    }

    private var liveFrom: Int {
        var ids: [UUID] = []
        switch drag {
        case .create(let id, _), .move(let id, _), .handle(let id, _): ids.append(id)
        default: break
        }
        ids += [editingID, selectedID].compactMap { $0 }
        let shapes = document.state.shapes
        return ids.compactMap { id in shapes.firstIndex { $0.id == id } }.min() ?? shapes.count
    }

    private func drawBase(in context: CGContext, transform: CGAffineTransform, backing: CGFloat) {
        let source = document.state.source
        let visible = bounds.applying(transform.inverted()).intersection(canvas.bounds)
        guard !visible.isNull, !visible.isEmpty else { return }
        let area = CGRect(x: visible.minX * scale, y: visible.minY * scale, width: visible.width * scale, height: visible.height * scale)
            .integral.intersection(CGRect(x: 0, y: 0, width: source.width, height: source.height))
        guard !area.isNull, !area.isEmpty else { return }
        let placed = CGRect(x: area.minX / scale, y: area.minY / scale, width: area.width / scale, height: area.height / scale).applying(transform)
        let target = CGRect(x: placed.minX * backing, y: placed.minY * backing, width: placed.width * backing, height: placed.height * backing)
        let ratio = min(magnification * backing / scale, 1)
        guard ratio < 1 else {
            baseView = nil
            source.current?.blit(from: area, into: context, at: target)
            return
        }
        let key = BaseViewKey(
            source: ObjectIdentifier(source), area: area,
            width: max(Int((area.width * ratio).rounded()), 1), height: max(Int((area.height * ratio).rounded()), 1)
        )
        if baseView?.key != key {
            baseView = nil
            guard let view = source.current?.scaled(from: area, width: key.width, height: key.height) else { return }
            baseView = (key, view)
            Memory.releaseSoon()
        }
        guard let pixels = baseView?.pixels else { return }
        pixels.blit(from: pixels.bounds, into: context, at: target)
    }

    private func drawSettled(upTo live: Int, transform: CGAffineTransform, backing: CGFloat, in target: CGContext) {
        let key = SceneKey(transform: transform, size: bounds.size, backing: backing, base: ObjectIdentifier(document.state.source))
        let settled = ShapeRenderer.fingerprints(document.state.shapes[..<live])
        var current: Scene
        if let scene, scene.key == key, settled.starts(with: scene.drawn) {
            current = scene
        } else {
            scene = nil
            guard let surface = Pixels.drawable(width: target.width, height: target.height, colorSpace: target.colorSpace) else {
                drawBase(in: target, transform: transform, backing: backing)
                target.saveGState()
                target.concatenate(transform)
                target.clip(to: canvas.bounds)
                document.renderer.draw(document.state, scale: scale, in: target, range: 0..<live)
                target.restoreGState()
                return
            }
            let bitmap = surface.context
            bitmap.translateBy(x: 0, y: CGFloat(target.height))
            bitmap.scaleBy(x: backing, y: -backing)
            drawBase(in: bitmap, transform: transform, backing: backing)
            current = Scene(key: key, pixels: surface.pixels, context: bitmap, drawn: [])
        }
        if current.drawn.count < live {
            let bitmap = current.context
            bitmap.saveGState()
            bitmap.concatenate(transform)
            bitmap.clip(to: canvas.bounds)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: bitmap, flipped: true)
            document.renderer.draw(document.state, scale: scale, in: bitmap, range: current.drawn.count..<live)
            NSGraphicsContext.restoreGraphicsState()
            bitmap.restoreGState()
        }
        current.drawn = settled
        scene = current
        current.pixels.blit(from: current.pixels.bounds, into: target, at: CGRect(x: 0, y: 0, width: target.width, height: target.height))
    }

    private func drawTextCursor(in context: CGContext, transform: CGAffineTransform) {
        guard let textView, let editingID, let shape = shape(editingID), let window else { return }
        outline(ShapeGeometry.bounds(shape).applying(transform).insetBy(dx: -3, dy: -3), in: context)
        let local = { (rect: CGRect) in self.convert(window.convertFromScreen(rect), from: nil) }
        let range = textView.selectedRange()
        if range.length == 0 {
            let caret = local(textView.firstRect(forCharacterRange: range, actualRange: nil))
            let bar = CGRect(x: caret.minX - 0.75, y: caret.minY, width: 1.5, height: caret.height)
            context.setFillColor(.white)
            context.fill(bar.insetBy(dx: -1, dy: -1))
            context.setFillColor(.black)
            context.fill(bar)
            return
        }
        context.setFillColor(NSColor(hex: 0x0A84FF, alpha: 0.35).cgColor)
        var remaining = range
        while remaining.length > 0 {
            var actual = NSRange()
            context.fill(local(textView.firstRect(forCharacterRange: remaining, actualRange: &actual)))
            let end = actual.location + actual.length
            guard actual.length > 0, end > remaining.location else { break }
            remaining = NSRange(location: end, length: remaining.location + remaining.length - end)
        }
    }

    private func drawCropAndCut(in context: CGContext) {
        if let crop = pendingCrop ?? textArea {
            let path = CGMutablePath()
            path.addRect(canvas.bounds)
            path.addRect(crop)
            context.addPath(path)
            context.setFillColor(CGColor(gray: 0, alpha: 0.5))
            context.fillPath(using: .evenOdd)
        }
        if let cut = cutPreview {
            let columns = abs(cut.end.x - cut.start.x) >= abs(cut.end.y - cut.start.y)
            let band = columns
                ? CGRect(x: min(cut.start.x, cut.end.x), y: 0, width: abs(cut.end.x - cut.start.x), height: canvas.bounds.height)
                : CGRect(x: 0, y: min(cut.start.y, cut.end.y), width: canvas.bounds.width, height: abs(cut.end.y - cut.start.y))
            context.setFillColor(NSColor.systemRed.withAlphaComponent(0.35).cgColor)
            context.fill(band)
        }
    }

    private func drawSelection(in context: CGContext, transform: CGAffineTransform) {
        if let area = textArea {
            outline(area.applying(transform), in: context)
        }
        if let crop = pendingCrop {
            outline(crop.applying(transform), in: context)
            drawHandles(ShapeGeometry.handles(for: crop).map { $0.1.applying(transform) }, in: context)
        }
        guard let shape = selectedShape, editingID == nil else { return }
        let handles = ShapeGeometry.handles(shape)
        if shape.kind != .line, shape.kind != .arrow {
            let cornered = handles.contains {
                if case .corner = $0.0 { return true }
                return false
            }
            let frame = cornered ? shape.rect : ShapeGeometry.bounds(shape)
            let margin = cornered ? 0 : shape.style.lineWidth / 2 * magnification + 3
            outline(frame.applying(transform).insetBy(dx: -margin, dy: -margin), in: context)
        }
        drawHandles(handles.map { $0.1.applying(transform) }, in: context)
    }

    private func outline(_ rect: CGRect, in context: CGContext) {
        let edge = rect.integral.insetBy(dx: 0.5, dy: 0.5)
        context.saveGState()
        context.setLineWidth(1)
        context.setStrokeColor(.black)
        context.stroke(edge)
        context.setStrokeColor(.white)
        context.setLineDash(phase: 0, lengths: [4, 4])
        context.stroke(edge)
        context.restoreGState()
    }

    private func drawHandles(_ points: [CGPoint], in context: CGContext) {
        for point in points {
            let rect = CGRect(x: point.x.rounded() - 4.5, y: point.y.rounded() - 4.5, width: 9, height: 9)
            context.setFillColor(.white)
            context.fill(rect)
            context.setStrokeColor(.black)
            context.setLineWidth(1)
            context.stroke(rect.insetBy(dx: 0.5, dy: 0.5))
        }
    }


    private var documentTransform: CGAffineTransform {
        let origin = convert(CGPoint.zero, from: canvas)
        let unitX = convert(CGPoint(x: 1, y: 0), from: canvas)
        let unitY = convert(CGPoint(x: 0, y: 1), from: canvas)
        return CGAffineTransform(
            a: unitX.x - origin.x, b: unitX.y - origin.y,
            c: unitY.x - origin.x, d: unitY.y - origin.y,
            tx: origin.x, ty: origin.y
        )
    }

    private func documentPoint(_ event: NSEvent) -> CGPoint {
        canvas.convert(event.locationInWindow, from: nil)
    }

    private func clamp(_ point: CGPoint) -> CGPoint {
        CGPoint(x: min(max(point.x, 0), canvas.bounds.width), y: min(max(point.y, 0), canvas.bounds.height))
    }

    private func shape(_ id: UUID) -> Shape? {
        document.state.shapes.first { $0.id == id }
    }

    private func update(_ id: UUID, _ body: (inout Shape) -> Void) {
        guard let index = document.state.shapes.firstIndex(where: { $0.id == id }) else { return }
        body(&document.state.shapes[index])
    }

    private func remove(_ id: UUID) {
        document.state.shapes.removeAll { $0.id == id }
    }

    private func topShape(at point: CGPoint) -> Shape? {
        document.state.shapes.last { ShapeGeometry.contains($0, point, tolerance: tolerance) }
    }

    private func handle(at point: CGPoint, in handles: [(Handle, CGPoint)]) -> Handle? {
        let reach = 7 / magnification
        return handles.first { abs($0.1.x - point.x) <= reach && abs($0.1.y - point.y) <= reach }?.0
    }

    private func isDegenerate(_ shape: Shape) -> Bool {
        switch shape.kind {
        case .freehand: shape.points.count < 2
        case .line, .arrow: hypot(shape.end.x - shape.start.x, shape.end.y - shape.start.y) < 3
        case .step, .textOutline, .textBackground, .speechBalloon: false
        default: shape.rect.width < 3 || shape.rect.height < 3
        }
    }

    private func create(_ kind: ShapeKind, at point: CGPoint) {
        var shape = Shape(kind: kind, style: StyleStore.style(for: kind), at: point)
        if kind == .smartEraser {
            shape.style.fill = document.state.source.current?.color(at: CGPoint(x: point.x * scale, y: point.y * scale)) ?? .white
        }
        if kind.isText {
            TextMetrics.fit(&shape)
            shape.tail = CGPoint(x: shape.start.x + 24, y: shape.end.y + 36)
        }
        document.state.shapes.append(shape)
        select(shape.id)
        changed = true
        needsDisplay = true

        if kind.isText {
            let snapshot = before ?? document.state
            before = nil
            drag = nil
            beginEditing(shape.id, before: snapshot)
        } else if kind == .step {
            drag = .move(shape.id, last: point)
        } else {
            drag = .create(shape.id, origin: point)
        }
    }

    private func commit(from before: EditorState, name: String) {
        needsDisplay = true
        delegate?.annotationView(self, commitFrom: before, actionName: name)
    }
}

@MainActor
final class EditingTextView: NSTextView {
    var onHistory: ((Bool) -> Bool)?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard window?.firstResponder === self,
              event.charactersIgnoringModifiers?.lowercased() == "z",
              flags == .command || flags == [.command, .shift]
        else { return super.performKeyEquivalent(with: event) }
        let redo = flags.contains(.shift)
        if onHistory?(redo) == true { return true }
        if redo {
            if undoManager?.canRedo == true { undoManager?.redo() }
        } else if undoManager?.canUndo == true {
            undoManager?.undo()
        }
        return true
    }
}
