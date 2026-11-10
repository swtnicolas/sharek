import AppKit

@MainActor
final class OptionsPill: NSView {
    var onChange: ((String, (inout ShapeStyle) -> Void) -> Void)?
    var onApplyCrop: (() -> Void)?
    var onCancelCrop: (() -> Void)?
    var onPickColor: ((String) -> Void)?

    private let stack = NSStackView()
    private var heading: NSTextField?
    private var colorLabels: [NSTextField] = []
    private var signature: String?
    private var refreshers: [(ShapeStyle) -> Void] = []
    private var targets: [PillTarget] = []

    private static let lineWidths: [CGFloat] = [0, 1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20]
    private static let radii: [CGFloat] = [0, 3, 6, 10, 15, 20, 30]
    private static let fontSizes: [CGFloat] = [10, 12, 14, 16, 18, 20, 24, 28, 32, 40, 48, 64, 80, 96]

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = Theme.tooltip.cgColor
        layer?.borderColor = Theme.border.cgColor
        layer?.borderWidth = 1
        layer?.cornerRadius = 22
        layer?.shadowColor = NSColor.black.cgColor
        layer?.shadowOpacity = 0.4
        layer?.shadowRadius = 12
        stack.orientation = .horizontal
        stack.spacing = 14
        stack.alignment = .centerY
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: 44),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 18),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -18),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { nil }

    var compact = false {
        didSet {
            guard compact != oldValue else { return }
            heading?.isHidden = compact
            colorLabels.forEach { $0.isHidden = compact }
        }
    }

    var widths: (full: CGFloat, compact: CGFloat) {
        let shown = fittingSize.width
        let optional = (heading.map { $0.fittingSize.width + stack.spacing } ?? 0)
            + colorLabels.reduce(0) { $0 + $1.fittingSize.width + 6 }
        return compact ? (shown + optional, shown) : (shown, shown - optional)
    }

    func show(tool: Tool, kind: ShapeKind?, style: ShapeStyle?) {
        let signature = tool == .crop || tool == .cutOut ? tool.rawValue : kind?.rawValue ?? tool.rawValue
        guard signature != self.signature else {
            if let style { refreshers.forEach { $0(style) } }
            return
        }
        self.signature = signature
        rebuild(tool: tool, kind: kind, style: style)
    }

    private func rebuild(tool: Tool, kind: ShapeKind?, style: ShapeStyle?) {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        refreshers.removeAll()
        targets.removeAll()
        colorLabels.removeAll()

        let title: String = if tool == .crop || tool == .cutOut {
            tool.title
        } else if kind == .image {
            "Imagen"
        } else {
            (kind.flatMap { Tool(rawValue: $0.rawValue) } ?? tool).title
        }
        let heading = NSTextField(labelWithString: title)
        heading.font = .systemFont(ofSize: 12, weight: .bold)
        heading.textColor = Theme.text
        heading.isHidden = compact
        stack.addArrangedSubview(heading)
        self.heading = heading

        switch tool {
        case .crop:
            addHint("Arrastra para elegir el área. Enter o doble clic aplica; Esc cancela.")
            addButton("Aplicar") { [weak self] in self?.onApplyCrop?() }
            addButton("Cancelar") { [weak self] in self?.onCancelCrop?() }
            return
        case .cutOut:
            addHint("Arrastra en horizontal para quitar columnas o en vertical para quitar filas.")
            return
        default:
            break
        }

        guard let kind, let style else {
            addHint("Haz clic en una forma para seleccionarla y arrástrala para moverla.")
            return
        }
        if kind == .smartEraser {
            addHint("Rellena el área con el color del punto donde empiezas a arrastrar.")
            return
        }

        let options = kind.options
        if options.contains(.text) { addColor("Texto", key: "text", \.text, style) }
        if options.contains(.stroke) { addColor(kind.strokeLabel, key: "stroke", \.stroke, style) }
        if options.contains(.fill) { addColor(kind.fillLabel, key: "fill", \.fill, style) }
        if options.contains(.lineWidth) {
            addChoice("Grosor", key: "lineWidth", \.lineWidth, values: Self.lineWidths, title: { "\($0.formatted()) px" }, style)
        }
        if options.contains(.cornerRadius) {
            addChoice("Esquinas", key: "cornerRadius", \.cornerRadius, values: Self.radii, title: { "\($0.formatted()) px" }, style)
        }
        if options.contains(.fontSize) {
            addChoice("Tamaño", key: "fontSize", \.fontSize, values: Self.fontSizes, title: { "\($0.formatted()) pt" }, style)
        }
        if options.contains(.strength) {
            addChoice(kind.strengthLabel, key: "strength", \.strength, values: kind.strengthChoices, title: kind.strengthTitle, style)
        }
        if options.contains(.shadow) { addShadow(style) }
    }

    private func addHint(_ text: String) {
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: 12)
        label.textColor = Theme.secondary
        stack.addArrangedSubview(label)
    }

    private func addButton(_ title: String, _ handler: @escaping () -> Void) {
        let target = PillTarget { _ in handler() }
        targets.append(target)
        let button = NSButton(title: title, target: target, action: #selector(PillTarget.fire(_:)))
        button.controlSize = .small
        button.bezelStyle = .push
        stack.addArrangedSubview(button)
    }

    private func label(_ title: String) -> NSTextField {
        let label = NSTextField(labelWithString: title)
        label.font = .systemFont(ofSize: 12)
        label.textColor = Theme.secondary
        return label
    }

    private func addColor(_ title: String, key: String, _ keyPath: WritableKeyPath<ShapeStyle, RGBA>, _ style: ShapeStyle) {
        let chip = ColorChip(value: style[keyPath: keyPath])
        chip.onClick = { [weak self, weak chip] in
            guard let self, let chip else { return }
            SwatchPopover.show(from: chip, title: title, current: chip.value, onPick: { [weak self] value in
                chip.value = value
                self?.onChange?(key) { $0[keyPath: keyPath] = value }
            }, onEyedropper: { [weak self] in
                self?.onPickColor?(key)
            })
        }
        refreshers.append { style in chip.value = style[keyPath: keyPath] }
        let caption = label(title)
        caption.isHidden = compact
        colorLabels.append(caption)
        let group = NSStackView(views: [chip, caption])
        group.spacing = 6
        group.alignment = .centerY
        stack.addArrangedSubview(group)
    }

    private func addChoice(_ title: String, key: String, _ keyPath: WritableKeyPath<ShapeStyle, CGFloat>, values: [CGFloat], title format: @escaping (CGFloat) -> String, _ style: ShapeStyle) {
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.controlSize = .small
        popup.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        var choices = values
        let fill = { (current: CGFloat) in
            if !choices.contains(current) { choices = (choices + [current]).sorted() }
            popup.removeAllItems()
            popup.addItems(withTitles: choices.map(format))
            popup.selectItem(at: choices.firstIndex(of: current) ?? 0)
        }
        fill(style[keyPath: keyPath])
        let target = PillTarget { [weak self] sender in
            guard let popup = sender as? NSPopUpButton, choices.indices.contains(popup.indexOfSelectedItem) else { return }
            let value = choices[popup.indexOfSelectedItem]
            self?.onChange?(key) { $0[keyPath: keyPath] = value }
        }
        targets.append(target)
        popup.target = target
        popup.action = #selector(PillTarget.fire(_:))
        refreshers.append { style in fill(style[keyPath: keyPath]) }
        let group = NSStackView(views: [label(title), popup])
        group.spacing = 6
        group.alignment = .centerY
        stack.addArrangedSubview(group)
    }

    private func addShadow(_ style: ShapeStyle) {
        let target = PillTarget { [weak self] sender in
            guard let box = sender as? NSButton else { return }
            let value = box.state == .on
            self?.onChange?("shadow") { $0.shadow = value }
        }
        targets.append(target)
        let box = NSButton(checkboxWithTitle: "Sombra", target: target, action: #selector(PillTarget.fire(_:)))
        box.controlSize = .small
        box.font = .systemFont(ofSize: 12)
        box.state = style.shadow ? .on : .off
        refreshers.append { box.state = $0.shadow ? .on : .off }
        stack.addArrangedSubview(box)
    }
}

@MainActor
final class ColorChip: NSView {
    var value: RGBA { didSet { needsDisplay = true } }
    var onClick: (() -> Void)?

    init(value: RGBA, diameter: CGFloat = 20) {
        self.value = value
        super.init(frame: CGRect(x: 0, y: 0, width: diameter, height: diameter))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: diameter).isActive = true
        heightAnchor.constraint(equalToConstant: diameter).isActive = true
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        let circle = NSBezierPath(ovalIn: bounds.insetBy(dx: 1, dy: 1))
        if value.isVisible {
            value.nsColor.setFill()
            circle.fill()
        } else {
            NSColor.white.setFill()
            circle.fill()
            NSColor(hex: 0xf23c3c).setStroke()
            let slash = NSBezierPath()
            slash.move(to: CGPoint(x: bounds.minX + 4, y: bounds.minY + 4))
            slash.line(to: CGPoint(x: bounds.maxX - 4, y: bounds.maxY - 4))
            slash.lineWidth = 2
            slash.stroke()
        }
        Theme.border.setStroke()
        circle.lineWidth = 1
        circle.stroke()
    }

    override func mouseDown(with event: NSEvent) {}

    override func mouseUp(with event: NSEvent) {
        if bounds.contains(convert(event.locationInWindow, from: nil)) { onClick?() }
    }
}

@MainActor
enum SwatchPopover {
    static let swatches: [(UInt32, String)] = [
        (0xF23C3C, "Rojo"), (0xFF9500, "Naranja"), (0xFFCC00, "Amarillo"), (0x34C759, "Verde"),
        (0x30B0C7, "Turquesa"), (0x0A84FF, "Azul"), (0xAF52DE, "Morado"), (0xFF2D55, "Rosa"),
        (0xFFFFFF, "Blanco"), (0xC7C7CC, "Gris claro"), (0x8E8E93, "Gris"), (0x3A3A3C, "Gris oscuro"),
        (0x2A2F38, "Pizarra"), (0x000000, "Negro"), (0xFFFF00, "Amarillo flúor"),
    ]

    private static var current: NSPopover?
    private static var panelTarget: PillTarget?
    private static let closer = PopoverCloser {
        SwatchPopover.current = nil
        SwatchPopover.retained = []
    }

    static func dismiss() {
        current?.close()
        current = nil
        retained = []
        guard panelTarget != nil else { return }
        NSColorPanel.shared.setTarget(nil)
        NSColorPanel.shared.setAction(nil)
        panelTarget = nil
    }

    static func show(from anchor: NSView, title: String, current value: RGBA, onPick: @escaping (RGBA) -> Void, onEyedropper: @escaping () -> Void) {
        self.current?.close()
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = closer
        let grid = NSGridView()
        grid.rowSpacing = 6
        grid.columnSpacing = 6
        var entries: [RGBA] = swatches.map { RGBA(NSColor(hex: $0.0)) }
        entries.append(.clear)
        for row in 0..<2 {
            let cells: [NSView] = entries[(row * 8)..<min((row + 1) * 8, entries.count)].map { color in
                let chip = ColorChip(value: color, diameter: 22)
                chip.onClick = { [weak popover] in
                    onPick(color)
                    popover?.close()
                }
                return chip
            }
            grid.addRow(with: cells)
        }
        let eyedropper = NSButton(title: "Tomar color de la imagen", target: nil, action: nil)
        let picker = PillTarget { [weak popover] _ in
            popover?.close()
            onEyedropper()
        }
        eyedropper.target = picker
        eyedropper.action = #selector(PillTarget.fire(_:))
        eyedropper.controlSize = .small
        eyedropper.image = Icon.eyedropper.sized(16, lineWidth: 1.8).image(stroke: Theme.text.cgColor)
        eyedropper.imagePosition = .imageLeading
        let other = NSButton(title: "Otro color…", target: nil, action: nil)
        let panel = PillTarget { [weak popover] _ in
            popover?.close()
            let colors = NSColorPanel.shared
            colors.setTarget(nil)
            colors.setAction(nil)
            colors.showsAlpha = true
            colors.color = value.nsColor
            let relay = PillTarget { sender in
                guard let colors = sender as? NSColorPanel else { return }
                onPick(RGBA(colors.color))
            }
            panelTarget = relay
            colors.setTarget(relay)
            colors.setAction(#selector(PillTarget.fire(_:)))
            colors.orderFront(nil)
        }
        other.target = panel
        other.action = #selector(PillTarget.fire(_:))
        other.controlSize = .small
        let buttons = NSStackView(views: [eyedropper, other])
        buttons.spacing = 8
        let heading = NSTextField(labelWithString: title)
        heading.font = .systemFont(ofSize: 12, weight: .bold)
        heading.textColor = Theme.text
        let content = NSStackView(views: [heading, grid, buttons])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        content.edgeInsets = NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12)
        let controller = NSViewController()
        controller.view = content
        retained = [picker, panel]
        popover.contentViewController = controller
        popover.appearance = NSAppearance(named: .darkAqua)
        popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .maxY)
        self.current = popover
    }

    private static var retained: [PillTarget] = []
}

@MainActor
final class PopoverCloser: NSObject, NSPopoverDelegate {
    private let onClose: @MainActor () -> Void

    init(_ onClose: @escaping @MainActor () -> Void) {
        self.onClose = onClose
    }

    func popoverDidClose(_ notification: Notification) {
        onClose()
    }
}

@MainActor
final class PillTarget: NSObject {
    private let handler: (Any?) -> Void

    init(_ handler: @escaping (Any?) -> Void) {
        self.handler = handler
    }

    @objc func fire(_ sender: Any?) {
        handler(sender)
    }
}
