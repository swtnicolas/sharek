import AppKit
import AVKit
import UniformTypeIdentifiers

@MainActor
final class MediaPage: NSViewController, EditorPage {
    private struct Action {
        let name: String
        let title: String
        let help: String
        let icon: Icon
    }

    private static let actions = [
        Action(name: "save", title: "Guardar · ⌘S", help: "Guarda la grabación en la carpeta de capturas.", icon: .save),
        Action(name: "save-as", title: "Guardar como… · ⇧⌘S", help: "Elige el nombre y la carpeta.", icon: .saveAs),
        Action(name: "play", title: "Abrir en el reproductor", help: "", icon: .play),
        Action(name: "reveal", title: "Mostrar en Finder", help: "", icon: .reveal),
    ]
    private static let discard = Action(name: "discard", title: "Descartar · ⌘⌫", help: "Cierra la pestaña y borra la grabación.", icon: .discard)

    private let media: MediaCapture
    private var player: AVPlayer?
    private let bar = NSView()
    private let hint = HintView()

    init(media: MediaCapture) {
        self.media = media
        super.init(nibName: nil, bundle: nil)
        let root = MediaRootView()
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.canvas.cgColor
        view = root

        bar.wantsLayer = true
        bar.layer?.backgroundColor = Theme.panel.cgColor
        bar.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(bar)

        let content = contentView()
        content.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(content)
        root.addSubview(hint)
        NSLayoutConstraint.activate([
            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            bar.topAnchor.constraint(equalTo: root.topAnchor),
            bar.heightAnchor.constraint(equalToConstant: EditorToolbar.height),
            content.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 16),
            content.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -16),
            content.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 16),
            content.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -16),
        ])
        fillBar()
    }

    required init?(coder: NSCoder) { nil }

    private func contentView() -> NSView {
        switch media.format {
        case .video:
            let player = AVPlayer(url: media.url)
            let view = AVPlayerView()
            view.player = player
            view.controlsStyle = .inline
            view.showsSharingServiceButton = false
            self.player = player
            return view
        case .gif:
            let view = NSImageView()
            view.image = NSImage(contentsOf: media.url)
            view.animates = true
            view.imageScaling = .scaleProportionallyDown
            return view
        }
    }

    private func fillBar() {
        let top = (EditorToolbar.height - EditorToolbar.button) / 2
        let copy = CopyButton()
        copy.frame = CGRect(x: EditorToolbar.padding, y: top + 1, width: CopyButton.fullWidth, height: 32)
        copy.onClick = { [weak self] in self?.copy(nil) }
        copy.onHover = { [weak self, weak copy] inside in self?.showHint("Copiar · ⌘C", "", inside ? copy : nil) }
        bar.addSubview(copy)
        var x = copy.frame.maxX + 6
        for action in Self.actions {
            let item = iconButton(for: action)
            item.frame = CGRect(x: x, y: top, width: EditorToolbar.button, height: EditorToolbar.button)
            bar.addSubview(item)
            x += EditorToolbar.button
        }
        let discard = iconButton(for: Self.discard)
        discard.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(discard)
        NSLayoutConstraint.activate([
            discard.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -EditorToolbar.padding),
            discard.topAnchor.constraint(equalTo: bar.topAnchor, constant: top),
            discard.widthAnchor.constraint(equalToConstant: EditorToolbar.button),
            discard.heightAnchor.constraint(equalToConstant: EditorToolbar.button),
        ])
    }

    private func iconButton(for action: Action) -> IconButton {
        let button = IconButton(icon: action.icon, color: Theme.actionColor(action.name))
        button.onClick = { [weak self] _ in self?.perform(action.name) }
        button.onHover = { [weak self, weak button] inside in self?.showHint(action.title, action.help, inside ? button : nil) }
        return button
    }

    private func showHint(_ title: String, _ body: String, _ anchor: NSView?) {
        guard let anchor else {
            hint.isHidden = true
            return
        }
        let rect = view.convert(anchor.bounds, from: anchor)
        hint.show(title: title, body: body, at: CGPoint(x: rect.minX, y: rect.maxY + 4), within: view.bounds)
    }

    private func perform(_ name: String) {
        hint.isHidden = true
        switch name {
        case "save": saveCapture(nil)
        case "save-as": saveCaptureAs(nil)
        case "play": _ = NSWorkspace.shared.open(media.url)
        case "reveal": NSWorkspace.shared.activateFileViewerSelecting([media.url])
        case "discard": discardCapture(nil)
        default: break
        }
    }

    var probeState: String { "media=\(media.format.fileExtension)" }

    func attached() {
        view.window?.makeFirstResponder(view)
    }

    func windowDidResize() {}

    func resume() {}

    func suspend() {
        player?.pause()
    }

    func close() {
        player?.pause()
        player = nil
    }

    @objc func copy(_ sender: Any?) {
        EditorClipboard.copy(media)
    }

    @objc func saveCapture(_ sender: Any?) {
        do {
            try media.save()
            EditorClipboard.mediaDidMove(media)
        } catch {
            Toast.show("No se pudo guardar", detail: error.localizedDescription)
        }
        EditorManager.shared.refreshTabs()
    }

    @objc func saveCaptureAs(_ sender: Any?) {
        guard let window = view.window else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [media.format == .video ? .mpeg4Movie : .gif]
        panel.directoryURL = Settings.saveDirectory
        panel.nameFieldStringValue = ImageWriter.uniqueURL(in: Settings.saveDirectory, date: media.createdAt, fileExtension: media.format.fileExtension).lastPathComponent
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            do {
                try self.media.save(to: url)
                EditorClipboard.mediaDidMove(self.media)
            } catch {
                Toast.show("No se pudo guardar", detail: error.localizedDescription)
            }
            EditorManager.shared.refreshTabs()
        }
    }

    @objc func discardCapture(_ sender: Any?) {
        EditorManager.shared.closeCurrent(discarded: true)
    }
}

private final class MediaRootView: NSView {
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
}
