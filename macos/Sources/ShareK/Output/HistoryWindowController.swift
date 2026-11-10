import AppKit
import AVFoundation
import QuickLookThumbnailing

@MainActor
final class HistoryWindowController: NSWindowController, NSWindowDelegate, NSCollectionViewDataSource, NSMenuItemValidation {
    var onClose: ((NSWindowController) -> Void)?

    private let collectionView = HistoryCollectionView()
    private let emptyLabel = NSTextField(labelWithString: "Aún no hay capturas guardadas")
    private let thumbnails = NSCache<NSString, NSImage>()
    private var entries: [HistoryEntry] = []
    private var loadedAt: Date?
    private var poll: Timer?

    init() {
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 820, height: 560),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.title = "Historial"
        window.minSize = CGSize(width: 420, height: 300)
        window.center()
        window.setFrameAutosaveName("ShareK.history")
        super.init(window: window)
        window.delegate = self

        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = CGSize(width: 176, height: 152)
        layout.sectionInset = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        layout.minimumInteritemSpacing = 12
        layout.minimumLineSpacing = 12
        collectionView.collectionViewLayout = layout
        collectionView.isSelectable = true
        collectionView.allowsMultipleSelection = true
        collectionView.backgroundColors = [.underPageBackgroundColor]
        collectionView.dataSource = self
        collectionView.register(HistoryItem.self, forItemWithIdentifier: HistoryItem.identifier)
        collectionView.onOpen = { [weak self] in self?.open(nil) }
        collectionView.onRemove = { [weak self] in self?.removeFromHistory(nil) }
        collectionView.onTrash = { [weak self] in self?.moveToTrash(nil) }
        collectionView.menuProvider = { [weak self] in self?.contextMenu() }

        let scrollView = NSScrollView()
        scrollView.documentView = collectionView
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true

        emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false
        scrollView.addSubview(emptyLabel)
        NSLayoutConstraint.activate([
            emptyLabel.centerXAnchor.constraint(equalTo: scrollView.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: scrollView.centerYAnchor),
        ])
        window.contentView = scrollView

        History.shared.onChange = { [weak self] in self?.reload() }
        History.shared.pruneMissingFiles()
        reload()
        poll = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.reloadIfChanged() }
        }
    }

    required init?(coder: NSCoder) { nil }

    func windowWillClose(_ notification: Notification) {
        poll?.invalidate()
        poll = nil
        History.shared.onChange = nil
        onClose?(self)
    }

    private func reloadIfChanged() {
        if History.shared.modificationDate != loadedAt { reload() }
    }

    private func reload() {
        loadedAt = History.shared.modificationDate
        entries = History.shared.load()
        collectionView.reloadData()
        emptyLabel.isHidden = !entries.isEmpty
        window?.subtitle = entries.isEmpty ? "" : "\(entries.count) \(entries.count == 1 ? "archivo" : "archivos")"
    }

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        entries.count
    }

    func collectionView(_ collectionView: NSCollectionView, itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let item = collectionView.makeItem(withIdentifier: HistoryItem.identifier, for: indexPath)
        guard let historyItem = item as? HistoryItem else { return item }
        let entry = entries[indexPath.item]
        historyItem.configure(entry)
        if let cached = thumbnails.object(forKey: entry.path as NSString) {
            historyItem.imageView?.image = cached
        } else {
            requestThumbnail(for: entry)
        }
        return historyItem
    }

    private func requestThumbnail(for entry: HistoryEntry) {
        let scale = window?.backingScaleFactor ?? 2
        let request = QLThumbnailGenerator.Request(fileAt: entry.url, size: HistoryItem.thumbnailSize, scale: scale, representationTypes: .thumbnail)
        let path = entry.path
        QLThumbnailGenerator.shared.generateBestRepresentation(for: request) { [weak self] representation, _ in
            guard let cgImage = representation?.cgImage else { return }
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    let image = NSImage(cgImage: cgImage, size: CGSize(width: CGFloat(cgImage.width) / scale, height: CGFloat(cgImage.height) / scale))
                    self.thumbnails.setObject(image, forKey: path as NSString)
                    for case let item as HistoryItem in self.collectionView.visibleItems() where item.entry?.path == path {
                        item.imageView?.image = image
                    }
                }
            }
        }
    }

    private var selectedEntries: [HistoryEntry] {
        collectionView.selectionIndexPaths.map(\.item).sorted().compactMap { entries.indices.contains($0) ? entries[$0] : nil }
    }

    private func contextMenu() -> NSMenu? {
        let selection = selectedEntries
        guard !selection.isEmpty else { return nil }
        let menu = NSMenu()
        menu.addItem(withTitle: selection.count == 1 && selection[0].format == nil ? "Abrir en el editor" : "Abrir", action: #selector(open(_:)), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Copiar", action: #selector(copy(_:)), keyEquivalent: "").target = self
        if selection.contains(where: { $0.format == nil }) {
            menu.addItem(withTitle: "Fijar en pantalla", action: #selector(pin(_:)), keyEquivalent: "").target = self
        }
        menu.addItem(withTitle: "Mostrar en Finder", action: #selector(revealInFinder(_:)), keyEquivalent: "").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quitar del historial", action: #selector(removeFromHistory(_:)), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Mover a la papelera", action: #selector(moveToTrash(_:)), keyEquivalent: "").target = self
        return menu
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        !selectedEntries.isEmpty
    }

    @objc func open(_ sender: Any?) {
        for entry in selectedEntries {
            if let format = entry.format {
                openMedia(entry.url, format: format)
            } else if let (pixels, scale) = ImageWriter.read(entry.url) {
                EditorManager.shared.open(CaptureDocument(id: EditorManager.shared.nextLocalID(), pixels: pixels, scale: scale))
            }
        }
    }

    private func openMedia(_ url: URL, format: RecordingFormat) {
        Task { @MainActor in
            let pixels = await Self.pixelSize(of: url, format: format)
            let scale = format == .gif ? 1 : (NSScreen.main?.backingScaleFactor ?? 2)
            let size = CGSize(width: pixels.width / scale, height: pixels.height / scale)
            EditorManager.shared.open(MediaCapture(id: EditorManager.shared.nextLocalID(), url: url, format: format, pointSize: size, savedAutomatically: false, isSaved: true))
        }
    }

    private static func pixelSize(of url: URL, format: RecordingFormat) async -> CGSize {
        switch format {
        case .gif:
            guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
                  let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
                  let width = properties[kCGImagePropertyPixelWidth] as? CGFloat,
                  let height = properties[kCGImagePropertyPixelHeight] as? CGFloat
            else { return .zero }
            return CGSize(width: width, height: height)
        case .video:
            let track = try? await AVURLAsset(url: url).loadTracks(withMediaType: .video).first
            return (try? await track?.load(.naturalSize)) ?? .zero
        }
    }

    @objc func copy(_ sender: Any?) {
        let selection = selectedEntries
        guard let first = selection.first else { return }
        let link = MenuBarLink.shared
        guard selection.count == 1 else {
            link.send(.copyFiles(paths: selection.map(\.path)))
            return
        }
        let id = EditorManager.shared.nextLocalID()
        link.copied(id)
        if let format = first.format {
            link.send(.copyFile(id: id, path: first.path, gif: format == .gif))
            return
        }
        let url = first.url
        link.sendLater {
            guard let bytes = try? Data(contentsOf: url) else { return nil }
            if bytes.starts(with: [0x89, 0x50, 0x4E, 0x47]) { return (ToMenuBar.copy(id: id), bytes) }
            guard let decoded = ImageWriter.read(url),
                  let png = PNGEncoder.encode(decoded.pixels, scale: decoded.scale, fast: true)
            else { return nil }
            return (ToMenuBar.copy(id: id), png)
        }
    }

    @objc func pin(_ sender: Any?) {
        for entry in selectedEntries where entry.format == nil {
            let url = entry.url
            MenuBarLink.shared.sendLater {
                guard let decoded = ImageWriter.read(url) else { return nil }
                return (ToMenuBar.pin(image: decoded.pixels.format, scale: Double(decoded.scale)), decoded.pixels.data)
            }
        }
    }

    @objc func revealInFinder(_ sender: Any?) {
        NSWorkspace.shared.activateFileViewerSelecting(selectedEntries.map(\.url))
    }

    @objc func removeFromHistory(_ sender: Any?) {
        History.shared.remove(selectedEntries.map(\.url))
    }

    @objc func moveToTrash(_ sender: Any?) {
        let urls = selectedEntries.map(\.url)
        guard !urls.isEmpty, let window else { return }
        let alert = NSAlert()
        alert.messageText = urls.count == 1 ? "¿Mover \(urls[0].lastPathComponent) a la papelera?" : "¿Mover \(urls.count) archivos a la papelera?"
        alert.informativeText = "Puedes recuperarlos desde la papelera."
        alert.addButton(withTitle: "Mover a la papelera")
        alert.addButton(withTitle: "Cancelar")
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            NSWorkspace.shared.recycle(urls) { _, _ in
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { History.shared.remove(urls) }
                }
            }
        }
    }
}

private final class HistoryCollectionView: NSCollectionView {
    var onOpen: (() -> Void)?
    var onRemove: (() -> Void)?
    var onTrash: (() -> Void)?
    var menuProvider: (() -> NSMenu?)?

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        if event.clickCount == 2, indexPathForItem(at: convert(event.locationInWindow, from: nil)) != nil {
            onOpen?()
        }
    }

    override func keyDown(with event: NSEvent) {
        switch Int(event.keyCode) {
        case 36, 76: onOpen?()
        case 51, 117: event.modifierFlags.contains(.command) ? onTrash?() : onRemove?()
        default: super.keyDown(with: event)
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard let indexPath = indexPathForItem(at: convert(event.locationInWindow, from: nil)) else { return nil }
        if !selectionIndexPaths.contains(indexPath) {
            deselectAll(nil)
            selectItems(at: [indexPath], scrollPosition: [])
        }
        return menuProvider?()
    }
}

private final class HistoryItem: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("HistoryItem")
    static let thumbnailSize = CGSize(width: 160, height: 108)

    private(set) var entry: HistoryEntry?
    private let dateLabel = NSTextField(labelWithString: "")
    private let badge = NSView()
    private let badgeIcon = NSImageView()
    private let badgeLabel = NSTextField(labelWithString: "GIF")

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.cornerRadius = 8

        let thumbnail = NSImageView()
        thumbnail.imageScaling = .scaleProportionallyDown
        let name = NSTextField(labelWithString: "")
        name.font = .systemFont(ofSize: 11)
        name.alignment = .center
        name.lineBreakMode = .byTruncatingMiddle
        dateLabel.font = .systemFont(ofSize: 10)
        dateLabel.textColor = .secondaryLabelColor
        dateLabel.alignment = .center
        badge.wantsLayer = true
        badge.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.55).cgColor
        badge.layer?.cornerRadius = 4
        let play = Icon.playBadge.image(stroke: CGColor.clear, fill: CGColor.white)
        play.accessibilityDescription = "Video"
        badgeIcon.image = play
        badgeLabel.font = .systemFont(ofSize: 9, weight: .bold)
        badgeLabel.textColor = .white
        for view in [badgeIcon, badgeLabel] {
            view.translatesAutoresizingMaskIntoConstraints = false
            badge.addSubview(view)
        }

        for view in [thumbnail, name, dateLabel, badge] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        NSLayoutConstraint.activate([
            thumbnail.topAnchor.constraint(equalTo: root.topAnchor, constant: 8),
            thumbnail.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            thumbnail.widthAnchor.constraint(equalToConstant: Self.thumbnailSize.width),
            thumbnail.heightAnchor.constraint(equalToConstant: Self.thumbnailSize.height),
            name.topAnchor.constraint(equalTo: thumbnail.bottomAnchor, constant: 4),
            name.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 6),
            name.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -6),
            dateLabel.topAnchor.constraint(equalTo: name.bottomAnchor, constant: 1),
            dateLabel.leadingAnchor.constraint(equalTo: name.leadingAnchor),
            dateLabel.trailingAnchor.constraint(equalTo: name.trailingAnchor),
            badge.trailingAnchor.constraint(equalTo: thumbnail.trailingAnchor, constant: -4),
            badge.bottomAnchor.constraint(equalTo: thumbnail.bottomAnchor, constant: -4),
            badge.widthAnchor.constraint(equalToConstant: 26),
            badge.heightAnchor.constraint(equalToConstant: 18),
            badgeIcon.centerXAnchor.constraint(equalTo: badge.centerXAnchor),
            badgeIcon.centerYAnchor.constraint(equalTo: badge.centerYAnchor),
            badgeLabel.centerXAnchor.constraint(equalTo: badge.centerXAnchor),
            badgeLabel.centerYAnchor.constraint(equalTo: badge.centerYAnchor),
        ])
        imageView = thumbnail
        textField = name
        view = root
    }

    func configure(_ entry: HistoryEntry) {
        self.entry = entry
        imageView?.image = nil
        textField?.stringValue = entry.url.lastPathComponent
        dateLabel.stringValue = entry.date.formatted(date: .abbreviated, time: .shortened)
        badgeIcon.isHidden = entry.format != .video
        badgeLabel.isHidden = entry.format != .gif
        badge.isHidden = entry.format == nil
        updateHighlight()
    }

    override var isSelected: Bool {
        didSet { updateHighlight() }
    }

    private func updateHighlight() {
        view.layer?.backgroundColor = isSelected ? NSColor.selectedContentBackgroundColor.withAlphaComponent(0.35).cgColor : nil
    }
}
