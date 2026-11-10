import AppKit

struct ImageFrame: Codable, Equatable {
    var margin: CGFloat = 32
    var background: RGBA = .white
    var radius: CGFloat = 12
    var shadow = true
    var border: CGFloat = 0
    var borderColor: RGBA = .black

    static let margins: [CGFloat] = [0, 16, 32, 48, 64]
    static let radii: [CGFloat] = [0, 6, 12, 18, 24]
    static let borders: [CGFloat] = [0, 1, 2, 4, 8]
    static let swatches: [UInt32] = [0xFFFFFF, 0xF2F3F5, 0x8B939C, 0x2A2F38, 0x000000, 0xFF3D7F, 0xFF9F1C, 0x2EC4B6, 0x5B5BFF]
    private static let shadowRoom: CGFloat = 16

    var effectiveMargin: CGFloat { shadow ? max(margin, Self.shadowRoom) : margin }

    static func load() -> ImageFrame {
        guard let data = UserDefaults.standard.data(forKey: "frame"), let frame = try? JSONDecoder().decode(ImageFrame.self, from: data) else { return ImageFrame() }
        return frame
    }

    func save() {
        guard let data = try? JSONEncoder().encode(self) else { return }
        UserDefaults.standard.set(data, forKey: "frame")
    }
}

@MainActor
enum FramePopover {
    private static var current: NSPopover?
    private static var targets: [PillTarget] = []
    private static let closer = PopoverCloser {
        FramePopover.current = nil
        FramePopover.targets = []
    }

    static func dismiss() {
        current?.close()
        current = nil
        targets = []
    }

    static func show(from anchor: NSView, onApply: @escaping (ImageFrame) -> Void) {
        current?.close()
        var frame = ImageFrame.load()
        targets = []
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = closer

        func segmented(_ values: [CGFloat], selected: CGFloat, format: (CGFloat) -> String, _ set: @escaping (CGFloat) -> Void) -> NSSegmentedControl {
            let control = NSSegmentedControl(labels: values.map(format), trackingMode: .selectOne, target: nil, action: nil)
            control.controlSize = .small
            control.selectedSegment = values.firstIndex(of: selected) ?? 0
            let target = PillTarget { sender in
                guard let control = sender as? NSSegmentedControl, values.indices.contains(control.selectedSegment) else { return }
                set(values[control.selectedSegment])
            }
            targets.append(target)
            control.target = target
            control.action = #selector(PillTarget.fire(_:))
            return control
        }
        func row(_ title: String, _ control: NSView) -> NSStackView {
            let label = NSTextField(labelWithString: title)
            label.font = .systemFont(ofSize: 12)
            label.textColor = Theme.secondary
            label.translatesAutoresizingMaskIntoConstraints = false
            label.widthAnchor.constraint(equalToConstant: 64).isActive = true
            let stack = NSStackView(views: [label, control])
            stack.spacing = 8
            stack.alignment = .centerY
            return stack
        }

        let number = { (value: CGFloat) in value.formatted() }
        let margin = segmented(ImageFrame.margins, selected: frame.margin, format: number) { frame.margin = $0; frame.save() }
        let radius = segmented(ImageFrame.radii, selected: frame.radius, format: number) { frame.radius = $0; frame.save() }
        let border = segmented(ImageFrame.borders, selected: frame.border, format: number) { frame.border = $0; frame.save() }
        let shadow = NSSegmentedControl(labels: ["Sí", "No"], trackingMode: .selectOne, target: nil, action: nil)
        shadow.controlSize = .small
        shadow.selectedSegment = frame.shadow ? 0 : 1
        let shadowTarget = PillTarget { sender in
            guard let control = sender as? NSSegmentedControl else { return }
            frame.shadow = control.selectedSegment == 0
            frame.save()
        }
        targets.append(shadowTarget)
        shadow.target = shadowTarget
        shadow.action = #selector(PillTarget.fire(_:))

        var chips: [ColorChip] = []
        for hex in ImageFrame.swatches {
            let color = RGBA(NSColor(hex: hex))
            let chip = ColorChip(value: color, diameter: 20)
            chips.append(chip)
        }
        for chip in chips {
            chip.onClick = { [weak chip] in
                guard let chip else { return }
                frame.background = chip.value
                frame.save()
            }
        }
        let backgrounds = NSStackView(views: chips)
        backgrounds.spacing = 6

        let heading = NSTextField(labelWithString: "Marco de la imagen")
        heading.font = .systemFont(ofSize: 12, weight: .bold)
        heading.textColor = Theme.text
        let apply = NSButton(title: "Aplicar marco", target: nil, action: nil)
        let applyTarget = PillTarget { [weak popover] _ in
            popover?.close()
            onApply(frame)
        }
        targets.append(applyTarget)
        apply.target = applyTarget
        apply.action = #selector(PillTarget.fire(_:))
        apply.bezelColor = Theme.accent
        apply.keyEquivalent = "\r"

        let content = NSStackView(views: [heading, row("Margen", margin), row("Fondo", backgrounds), row("Esquinas", radius), row("Borde", border), row("Sombra", shadow), apply])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        content.edgeInsets = NSEdgeInsets(top: 12, left: 14, bottom: 12, right: 14)
        content.setCustomSpacing(14, after: content.arrangedSubviews[5])
        let controller = NSViewController()
        controller.view = content
        popover.contentViewController = controller
        popover.appearance = NSAppearance(named: .darkAqua)
        popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .maxY)
        current = popover
    }
}
