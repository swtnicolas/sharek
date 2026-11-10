import AppKit

@MainActor
final class EditorProcess {
    static let shared = EditorProcess()

    var onAction: ((String) -> Void)?

    @MainActor
    private final class Link {
        let channel: Channel
        let pid: pid_t
        var sent: UInt64 = 0

        init(channel: Channel, pid: pid_t) {
            self.channel = channel
            self.pid = pid
        }
    }

    private var link: Link?
    private var processes: [Process] = []
    private var announcedOwner: UInt64??

    @discardableResult
    func open(_ pixels: Pixels, id: UInt64, scale: CGFloat, created: Date, saved url: URL?) -> Bool {
        present(.open(id: id, image: pixels.format, scale: Double(scale), created: created, saved: url?.path), payload: pixels.data)
    }

    func openMedia(id: UInt64, url: URL, format: RecordingFormat, pointSize: CGSize, savedAutomatically: Bool) {
        present(.openMedia(
            id: id, path: url.path, gif: format == .gif, width: Double(pointSize.width), height: Double(pointSize.height),
            created: Date(), savedAutomatically: savedAutomatically
        ))
    }

    func showHistory() {
        present(.showHistory)
    }

    func syncClipboardOwner() {
        ClipboardManager.shared.forgetIfReplaced()
        guard let link else { return }
        let owner = ClipboardManager.shared.owner
        if let announced = announcedOwner, announced == owner { return }
        if send(.clipboardOwner(id: owner), on: link) { announcedOwner = .some(owner) }
    }

    @discardableResult
    private func present(_ message: ToEditor, payload: Data = Data()) -> Bool {
        guard let link = link ?? spawn() else { return false }
        NSApp.activate()
        guard send(message, payload: payload, on: link) else {
            Toast.show("No se pudo abrir el editor", detail: "La imagen es demasiado grande para enviarla al editor.")
            return false
        }
        syncClipboardOwner()
        return true
    }

    @discardableResult
    private func send(_ message: ToEditor, payload: Data = Data(), on link: Link) -> Bool {
        guard link.channel.send(message, payload: payload) else { return false }
        link.sent += 1
        return true
    }

    private func spawn() -> Link? {
        guard let executable = Bundle.main.executableURL else { return nil }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["--editor"]
        let toEditor = Pipe()
        let fromEditor = Pipe()
        process.standardInput = toEditor
        process.standardOutput = fromEditor
        process.terminationHandler = { finished in
            let pid = finished.processIdentifier
            DispatchQueue.main.async {
                MainActor.assumeIsolated { EditorProcess.shared.reap(pid) }
            }
        }
        do {
            try process.run()
        } catch {
            Toast.show("No se pudo abrir el editor", detail: error.localizedDescription)
            return nil
        }
        processes.append(process)
        let channel = Channel(read: fromEditor.fileHandleForReading.fileDescriptor, write: toEditor.fileHandleForWriting.fileDescriptor)
        let link = Link(channel: channel, pid: process.processIdentifier)
        self.link = link
        announcedOwner = nil
        channel.receive(ToMenuBar.self) { [weak self] message, payload in
            self?.handle(message, payload, from: link)
        } closed: { [weak self] in
            link.channel.close()
            self?.forget(link)
            if self?.link == nil { AutoSave.shared.forgetLanded() }
        }
        return link
    }

    private func forget(_ closed: Link) {
        guard link === closed else { return }
        link = nil
        announcedOwner = nil
    }

    private func reap(_ pid: pid_t) {
        processes.removeAll { $0.processIdentifier == pid }
        if let link, link.pid == pid { forget(link) }
    }

    private func handle(_ message: ToMenuBar, _ payload: Data, from sender: Link) {
        let clipboard = ClipboardManager.shared
        switch message {
        case .copy(let id):
            clipboard.copy(id, .image(ImageProvider(png: payload)))
        case .changed(let id):
            clipboard.update(id, .image(ImageProvider(png: payload)))
        case .discard(let id):
            clipboard.discard(id)
        case .release(let id):
            clipboard.release(id)
            AutoSave.shared.release(id)
        case .trash(let id):
            AutoSave.shared.trash(id)
        case .copyFile(let id, let path, let gif):
            clipboard.copy(id, .file(URL(fileURLWithPath: path), gif: gif))
        case .copyFiles(let paths):
            clipboard.copy(CaptureID.next(), .files(paths.map { URL(fileURLWithPath: $0) }))
        case .pin(let format, let scale):
            if let pixels = Pixels(format: format, data: payload) { PinManager.pin(pixels, scale: CGFloat(scale)) }
        case .toast(let title, let detail):
            Toast.show(title, detail: detail)
        case .action(let name):
            onAction?(name)
        case .activate:
            guard let app = NSRunningApplication(processIdentifier: sender.pid) else { return }
            NSApp.yieldActivation(to: app)
            _ = app.activate(from: .current, options: [])
        case .exiting(let received):
            if sender === link {
                guard received == sender.sent else { return }
                forget(sender)
            }
            send(.exitAccepted, on: sender)
            Memory.releaseSoon()
            return
        case .probed:
            return
        }
        syncClipboardOwner()
    }
}
