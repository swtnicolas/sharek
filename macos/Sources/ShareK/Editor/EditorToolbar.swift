import AppKit

@MainActor
final class EditorToolbar: NSView {
    struct Action {
        let name: String
        let title: String
        let help: String
        let icon: Icon
    }

    static let height: CGFloat = 52
    static let button: CGFloat = 34
    static let groupButton: CGFloat = 48
    static let groupGap: CGFloat = 10
    static let separator: CGFloat = 13
    static let padding: CGFloat = 10
    static let foldOrder = [3, 2, 0, 1]

    static let captureActions = [
        Action(name: "save", title: "Guardar · ⌘S", help: "Guarda la imagen en la carpeta de capturas.", icon: .save),
        Action(name: "save-as", title: "Guardar como… · ⇧⌘S", help: "Elige el nombre, la carpeta y el formato.", icon: .saveAs),
        Action(name: "ocr", title: "Extraer texto (OCR)", help: "Reconoce el texto de toda la imagen y lo muestra para copiarlo.", icon: .recognize),
        Action(name: "ocr-area", title: "Extraer texto de una zona", help: "Arrastra sobre el texto que quieres reconocer.", icon: .recognizeArea),
        Action(name: "pin", title: "Fijar en pantalla", help: "Deja la imagen flotando sobre las demás ventanas.", icon: .pin),
    ]
    static let historyActions = [
        Action(name: "undo", title: "Deshacer · ⌘Z", help: "", icon: .undo),
        Action(name: "redo", title: "Rehacer · ⇧⌘Z", help: "", icon: .redo),
    ]
    static let imageActions = [
        Action(name: "rotate", title: "Girar 90°", help: "Gira la imagen en el sentido del reloj. Con ⇧, al revés.", icon: .rotate),
        Action(name: "flip", title: "Voltear", help: "Refleja la imagen de izquierda a derecha. Con ⇧, de arriba abajo.", icon: .flip),
        Action(name: "frame", title: "Marco", help: "Agrega margen, fondo, esquinas redondeadas, borde o sombra a toda la imagen.", icon: .frame),
    ]
    static let discard = Action(name: "discard", title: "Descartar · ⌘⌫", help: "Cierra la pestaña sin guardar.", icon: .discard)

    var onAction: ((String, Bool, NSView) -> Void)?
    var onSelect: ((Tool) -> Void)?
    var onHint: ((String, String, NSView?) -> Void)?
    let copyButton = CopyButton()

    var selected: Tool = .select {
        didSet {
            if let index = Tool.sections.firstIndex(where: { $0.tools.contains(selected) }) { shown[index] = selected }
            refresh()
        }
    }

    private var actionButtons: [IconButton] = []
    private var historyButtons: [IconButton] = []
    private var imageButtons: [IconButton] = []
    private let discardButton: IconButton
    private var toolButtons: [[IconButton]] = []
    private var foldButtons: [IconButton] = []
    private var chevrons: [IconButton] = []
    private var shown: [Tool] = Tool.sections.map { $0.tools[0] }
    private let lines: [NSView] = (0..<5).map { _ in
        let line = NSView()
        line.wantsLayer = true
        line.layer?.backgroundColor = Theme.border.cgColor
        return line
    }

    static var fullWidth: CGFloat {
        let tools = Tool.sections.reduce(CGFloat(0)) { $0 + CGFloat($1.tools.count) * button }
        return width(tools: tools, groups: Tool.sections.count, compactCopy: false)
    }

    private static func width(tools: CGFloat, groups: Int, compactCopy: Bool) -> CGFloat {
        padding * 2 + (compactCopy ? CopyButton.compactWidth : CopyButton.fullWidth) + 6
            + CGFloat(captureActions.count + historyActions.count + imageActions.count + 1) * button
            + separator * 4 + groupGap * CGFloat(groups - 1) + tools
    }

    init() {
        discardButton = IconButton(icon: Self.discard.icon, color: Theme.danger)
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = Theme.panel.cgColor
        for line in lines { addSubview(line) }
        addSubview(copyButton)
        copyButton.onHover = { [weak self] inside in self?.onHint?("Copiar · ⌘C", "Arrástralo para soltar la captura en otra aplicación.", inside ? self?.copyButton : nil) }
        actionButtons = Self.captureActions.map(actionButton)
        historyButtons = Self.historyActions.map(actionButton)
        imageButtons = Self.imageActions.map(actionButton)
        wire(discardButton, Self.discard)
        addSubview(discardButton)
        for (index, section) in Tool.sections.enumerated() {
            toolButtons.append(section.tools.map { tool in
                let button = IconButton(icon: tool.icon, color: Theme.icon)
                button.onClick = { [weak self] _ in self?.onSelect?(tool) }
                button.onHover = { [weak self, weak button] inside in
                    self?.onHint?("\(tool.title) · \(String(tool.shortcut).uppercased())", tool.help, inside ? button : nil)
                }
                addSubview(button)
                return button
            })
            let fold = IconButton(icon: section.tools[0].icon, color: Theme.icon, size: CGSize(width: 30, height: 34))
            fold.offset = 2
            fold.onClick = { [weak self] _ in
                guard let self else { return }
                self.onSelect?(self.shown[index])
            }
            fold.onHover = { [weak self, weak fold] inside in
                guard let self else { return }
                let tool = self.shown[index]
                self.onHint?("\(tool.title) · \(String(tool.shortcut).uppercased())", tool.help, inside ? fold : nil)
            }
            let chevron = IconButton(icon: .chevron, color: Theme.tertiary, size: CGSize(width: 18, height: 34))
            chevron.offset = -2
            chevron.onHover = { [weak chevron] inside in chevron?.color = inside ? Theme.text : Theme.tertiary }
            chevron.onClick = { [weak self] _ in
                guard let self else { return }
                self.showMenu(for: index, from: self.foldButtons[index])
            }
            addSubview(fold)
            addSubview(chevron)
            foldButtons.append(fold)
            chevrons.append(chevron)
        }
        refresh()
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }

    private func actionButton(_ action: Action) -> IconButton {
        let button = IconButton(icon: action.icon, color: Theme.actionColor(action.name))
        wire(button, action)
        addSubview(button)
        return button
    }

    private func wire(_ button: IconButton, _ action: Action) {
        button.onClick = { [weak self, weak button] shift in
            guard let self, let button else { return }
            self.onAction?(action.name, shift, button)
        }
        button.onHover = { [weak self, weak button] inside in self?.onHint?(action.title, action.help, inside ? button : nil) }
    }

    private func showMenu(for index: Int, from anchor: NSView) {
        let menu = NSMenu()
        for tool in Tool.sections[index].tools {
            let item = NSMenuItem(title: tool.title, action: #selector(menuPicked(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = tool.rawValue
            let colors = Theme.iconColors(tool, tool.kind.map(StyleStore.style(for:)))
            item.image = tool.icon.sized(18, lineWidth: 1.6).image(stroke: colors.stroke.cgColor, fill: colors.fill.cgColor)
            item.state = tool == selected ? .on : .off
            menu.addItem(item)
        }
        menu.popUp(positioning: nil, at: CGPoint(x: 0, y: anchor.isFlipped ? anchor.bounds.maxY + 4 : -4), in: anchor)
    }

    @objc private func menuPicked(_ item: NSMenuItem) {
        guard let raw = item.representedObject as? String, let tool = Tool(rawValue: raw) else { return }
        onSelect?(tool)
    }

    func refresh() {
        for (index, section) in Tool.sections.enumerated() {
            for (tool, button) in zip(section.tools, toolButtons[index]) {
                paint(button, tool)
                button.isSelected = tool == selected
            }
            foldButtons[index].icon = shown[index].icon
            paint(foldButtons[index], shown[index])
            foldButtons[index].isSelected = section.tools.contains(selected)
        }
        needsLayout = true
    }

    private func paint(_ button: IconButton, _ tool: Tool) {
        let colors = Theme.iconColors(tool, tool.kind.map(StyleStore.style(for:)))
        button.color = colors.stroke
        button.fill = colors.fill
    }

    override func layout() {
        super.layout()
        var folded = Set<Int>()
        var compactCopy = false
        func required() -> CGFloat {
            let tools = Tool.sections.enumerated().reduce(CGFloat(0)) { total, entry in
                total + (folded.contains(entry.offset) ? Self.groupButton : CGFloat(entry.element.tools.count) * Self.button)
            }
            return Self.width(tools: tools, groups: Tool.sections.count, compactCopy: compactCopy)
        }
        for index in Self.foldOrder where required() > bounds.width {
            folded.insert(index)
        }
        if required() > bounds.width { compactCopy = true }

        let top = (Self.height - Self.button) / 2
        var x = Self.padding
        copyButton.compact = compactCopy
        let copyWidth = compactCopy ? CopyButton.compactWidth : CopyButton.fullWidth
        copyButton.frame = CGRect(x: x, y: top + 1, width: copyWidth, height: 32)
        x += copyWidth + 6
        func place(_ buttons: [IconButton]) {
            for button in buttons {
                button.frame = CGRect(x: x, y: top, width: Self.button, height: Self.button)
                x += Self.button
            }
        }
        place(actionButtons)
        x += Self.separator
        place(historyButtons)
        x += Self.separator
        for index in Tool.sections.indices {
            if index > 0 { x += Self.groupGap }
            let isFolded = folded.contains(index)
            toolButtons[index].forEach { $0.isHidden = isFolded }
            foldButtons[index].isHidden = !isFolded
            chevrons[index].isHidden = !isFolded
            if isFolded {
                foldButtons[index].frame = CGRect(x: x, y: top, width: 30, height: Self.button)
                chevrons[index].frame = CGRect(x: x + 30, y: top, width: 18, height: Self.button)
                x += Self.groupButton
            } else {
                place(toolButtons[index])
            }
        }
        x += Self.separator
        place(imageButtons)
        discardButton.frame = CGRect(x: bounds.width - Self.padding - Self.button, y: top, width: Self.button, height: Self.button)
        lines[0].frame = CGRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1)
        var marks: [CGFloat] = []
        if let last = actionButtons.last { marks.append(last.frame.maxX + Self.separator / 2) }
        if let last = historyButtons.last { marks.append(last.frame.maxX + Self.separator / 2) }
        if let first = imageButtons.first { marks.append(first.frame.minX - Self.separator / 2) }
        marks.append(discardButton.frame.minX - Self.separator / 2)
        for (line, x) in zip(lines.dropFirst(), marks) {
            line.frame = CGRect(x: x.rounded(), y: 14, width: 1, height: Self.height - 28)
        }
    }
}
