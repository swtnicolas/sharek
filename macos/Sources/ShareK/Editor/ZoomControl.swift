import AppKit

@MainActor
final class ZoomControl: NSView {
    var onZoom: ((String) -> Void)?
    private let label = NSTextField(labelWithString: "100 %")

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = Theme.tooltip.cgColor
        layer?.borderColor = Theme.border.cgColor
        layer?.borderWidth = 1
        layer?.cornerRadius = 16
        label.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        label.textColor = Theme.secondary
        label.alignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        label.widthAnchor.constraint(equalToConstant: 46).isActive = true
        let out = button(.zoomOut, "out")
        let into = button(.zoomIn, "in")
        let fit = button(Icon.fit.sized(16, lineWidth: 1.8), "fit")
        let divider = NSBox()
        divider.boxType = .separator
        let stack = NSStackView(views: [out, label, into, divider, fit])
        stack.spacing = 2
        stack.alignment = .centerY
        stack.edgeInsets = NSEdgeInsets(top: 0, left: 6, bottom: 0, right: 6)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: 32),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            divider.heightAnchor.constraint(equalToConstant: 16),
        ])
    }

    required init?(coder: NSCoder) { nil }

    func show(magnification: CGFloat) {
        label.stringValue = "\(Int((magnification * 100).rounded())) %"
    }

    private func button(_ icon: Icon, _ kind: String) -> IconButton {
        let button = IconButton(icon: icon, color: Theme.text, size: CGSize(width: 26, height: 26))
        button.translatesAutoresizingMaskIntoConstraints = false
        button.widthAnchor.constraint(equalToConstant: 26).isActive = true
        button.heightAnchor.constraint(equalToConstant: 26).isActive = true
        button.onClick = { [weak self] _ in self?.onZoom?(kind) }
        return button
    }
}
