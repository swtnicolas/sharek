import AppKit

@MainActor
final class CanvasView: NSView {
    init(size: CGSize) {
        super.init(frame: CGRect(origin: .zero, size: size))
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    func resize(to size: CGSize) {
        guard frame.size != size else { return }
        setFrameSize(size)
        needsDisplay = true
    }

    override func updateLayer() {
        guard let layer else { return }
        layer.shadowOpacity = 0.25
        layer.shadowRadius = 6
        layer.shadowOffset = CGSize(width: 0, height: -2)
        layer.shadowPath = CGPath(rect: bounds, transform: nil)
    }
}

@MainActor
final class EditorScrollView: NSScrollView {
    let overlay: AnnotationView

    init(overlay: AnnotationView) {
        self.overlay = overlay
        super.init(frame: .zero)
        contentView = CenteringClipView()
        contentView.postsBoundsChangedNotifications = true
        addSubview(overlay, positioned: .above, relativeTo: contentView)
    }

    required init?(coder: NSCoder) { nil }

    override func tile() {
        super.tile()
        guard overlay.frame != contentView.frame else { return }
        overlay.frame = contentView.frame
        overlay.needsDisplay = true
    }
}

private final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var rect = super.constrainBoundsRect(proposedBounds)
        guard let documentView else { return rect }
        if rect.width > documentView.frame.width { rect.origin.x = (documentView.frame.width - rect.width) / 2 }
        if rect.height > documentView.frame.height { rect.origin.y = (documentView.frame.height - rect.height) / 2 }
        return rect
    }
}
