import AppKit

@MainActor
enum EditorClipboard {
    private static var pending: [UInt64: Task<Void, Never>] = [:]

    static func copy(_ document: CaptureDocument) {
        pending.removeValue(forKey: document.id)?.cancel()
        MenuBarLink.shared.copied(document.id)
        send(document, copy: true)
    }

    static func documentDidChange(_ document: CaptureDocument) {
        guard MenuBarLink.shared.clipboardOwner == document.id else { return }
        let id = document.id
        pending[id]?.cancel()
        pending[id] = Task { @MainActor [weak document] in
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled, let document else { return }
            pending[id] = nil
            send(document, copy: false)
        }
    }

    static func release(_ document: CaptureDocument) {
        if let task = pending.removeValue(forKey: document.id) {
            task.cancel()
            send(document, copy: false)
        }
        MenuBarLink.shared.send(.release(id: document.id))
    }

    static func discard(_ document: CaptureDocument) {
        pending.removeValue(forKey: document.id)?.cancel()
        MenuBarLink.shared.send(.discard(id: document.id))
    }

    static func release(_ media: MediaCapture) {
        MenuBarLink.shared.send(.release(id: media.id))
    }

    static func discard(_ media: MediaCapture) {
        MenuBarLink.shared.send(.discard(id: media.id))
    }

    static func copy(_ media: MediaCapture) {
        MenuBarLink.shared.copied(media.id)
        MenuBarLink.shared.send(.copyFile(id: media.id, path: media.url.path, gif: media.format == .gif))
    }

    static func mediaDidMove(_ media: MediaCapture) {
        guard MenuBarLink.shared.clipboardOwner == media.id else { return }
        MenuBarLink.shared.send(.copyFile(id: media.id, path: media.url.path, gif: media.format == .gif))
    }

    static func pin(_ pixels: Pixels, scale: CGFloat) {
        MenuBarLink.shared.send(.pin(image: pixels.format, scale: Double(scale)), payload: pixels.data)
    }

    private static func send(_ document: CaptureDocument, copy: Bool) {
        guard let pixels = document.rendered() else { return }
        let scale = document.scale
        let id = document.id
        document.settle()
        MenuBarLink.shared.sendLater {
            guard let png = PNGEncoder.encode(pixels, scale: scale, fast: true) else { return nil }
            return (copy ? ToMenuBar.copy(id: id) : ToMenuBar.changed(id: id), png)
        }
    }
}
