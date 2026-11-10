import AppKit
import QuartzCore

@MainActor
final class RecordingChrome {
    private let border: NSWindow
    private let panel: NSPanel
    private let timeLabel = NSTextField(labelWithString: "00:00")
    private let formatLabel: NSTextField
    private let manualButton: NSButton
    private let stopButton: NSButton
    private let cancelButton: NSButton
    private let actions: ChromeActions
    private var timer: Timer?
    private let startedAt = Date()

    var windowNumbers: [Int] { [border.windowNumber, panel.windowNumber] }

    init(rect: CGRect, screen: NSScreen, title: String, onManual: (() -> Void)? = nil, onStop: @escaping () -> Void, onCancel: @escaping () -> Void) {
        actions = ChromeActions(manual: onManual ?? {}, stop: onStop, cancel: onCancel)

        border = NSWindow(contentRect: rect.insetBy(dx: -3, dy: -3), styleMask: .borderless, backing: .buffered, defer: false)
        border.isOpaque = false
        border.backgroundColor = .clear
        border.hasShadow = false
        border.ignoresMouseEvents = true
        border.level = .statusBar
        border.isReleasedWhenClosed = false
        border.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        let outline = NSView()
        outline.wantsLayer = true
        let shape = CAShapeLayer()
        shape.path = CGPath(rect: CGRect(origin: .zero, size: border.frame.size).insetBy(dx: 1.5, dy: 1.5), transform: nil)
        shape.fillColor = nil
        shape.strokeColor = NSColor.systemRed.cgColor
        shape.lineWidth = 2
        shape.lineDashPattern = [6, 4]
        outline.layer?.addSublayer(shape)
        border.contentView = outline

        let size = CGSize(width: onManual == nil ? 250 : 340, height: 40)
        panel = NSPanel(contentRect: CGRect(origin: .zero, size: size), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.level = .statusBar
        panel.hasShadow = true
        panel.isReleasedWhenClosed = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.isMovableByWindowBackground = true
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]

        let background = NSVisualEffectView(frame: CGRect(origin: .zero, size: size))
        background.material = .hudWindow
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = 10
        background.layer?.masksToBounds = true

        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.backgroundColor = NSColor.systemRed.cgColor
        dot.layer?.cornerRadius = 5
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: 10).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 10).isActive = true

        formatLabel = NSTextField(labelWithString: title)
        formatLabel.font = .systemFont(ofSize: 11, weight: .semibold)
        formatLabel.textColor = .secondaryLabelColor
        timeLabel.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)

        manualButton = NSButton(title: "Manual", target: actions, action: #selector(ChromeActions.manual(_:)))
        manualButton.controlSize = .small
        manualButton.bezelStyle = .push
        manualButton.isHidden = onManual == nil
        stopButton = NSButton(title: "Detener", target: actions, action: #selector(ChromeActions.stop(_:)))
        stopButton.controlSize = .small
        stopButton.bezelStyle = .push
        cancelButton = NSButton(title: "Cancelar", target: actions, action: #selector(ChromeActions.cancel(_:)))
        cancelButton.controlSize = .small
        cancelButton.bezelStyle = .push

        let stack = NSStackView(views: [dot, formatLabel, timeLabel, manualButton, stopButton, cancelButton])
        stack.spacing = 8
        stack.alignment = .centerY
        stack.translatesAutoresizingMaskIntoConstraints = false
        background.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerYAnchor.constraint(equalTo: background.centerYAnchor),
            stack.centerXAnchor.constraint(equalTo: background.centerXAnchor),
        ])
        panel.contentView = background
        panel.setFrameOrigin(Self.panelOrigin(for: rect, size: size, screen: screen))
    }

    func show() {
        border.orderFrontRegardless()
        panel.orderFrontRegardless()
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    func setTitle(_ title: String, showsManual: Bool) {
        formatLabel.stringValue = title
        manualButton.isHidden = !showsManual
    }

    func showProcessing() {
        timer?.invalidate()
        timer = nil
        timeLabel.stringValue = "Procesando…"
        manualButton.isEnabled = false
        stopButton.isEnabled = false
        cancelButton.isEnabled = false
    }

    func close() {
        timer?.invalidate()
        timer = nil
        border.orderOut(nil)
        panel.orderOut(nil)
    }

    private func tick() {
        let elapsed = Int(Date().timeIntervalSince(startedAt))
        timeLabel.stringValue = String(format: "%02d:%02d", elapsed / 60, elapsed % 60)
    }

    private static func panelOrigin(for rect: CGRect, size: CGSize, screen: NSScreen) -> CGPoint {
        let visible = screen.visibleFrame
        let x = min(max(rect.midX - size.width / 2, visible.minX + 8), visible.maxX - size.width - 8)
        if rect.minY - size.height - 12 >= visible.minY {
            return CGPoint(x: x, y: rect.minY - size.height - 12)
        }
        if rect.maxY + size.height + 12 <= visible.maxY {
            return CGPoint(x: x, y: rect.maxY + 12)
        }
        return CGPoint(x: x, y: max(rect.minY, visible.minY) + 12)
    }
}

@MainActor
private final class ChromeActions: NSObject {
    private let onManual: () -> Void
    private let onStop: () -> Void
    private let onCancel: () -> Void

    init(manual: @escaping () -> Void, stop: @escaping () -> Void, cancel: @escaping () -> Void) {
        onManual = manual
        onStop = stop
        onCancel = cancel
    }

    @objc func manual(_ sender: Any?) { onManual() }
    @objc func stop(_ sender: Any?) { onStop() }
    @objc func cancel(_ sender: Any?) { onCancel() }
}
