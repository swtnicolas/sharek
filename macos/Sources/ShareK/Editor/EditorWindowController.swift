import AppKit

@MainActor
final class EditorWindow: NSWindow {
    var onLeavingScreen: (() -> Void)?
    var onEnteringScreen: (() -> Void)?
    var onCloseTab: (() -> Void)?

    override func performClose(_ sender: Any?) {
        guard sender is NSMenuItem, let onCloseTab else {
            super.performClose(sender)
            return
        }
        onCloseTab()
    }

    override func close() {
        onLeavingScreen?()
        super.close()
    }

    override func order(_ place: NSWindow.OrderingMode, relativeTo otherWin: Int) {
        if place == .out {
            onLeavingScreen?()
        } else {
            onEnteringScreen?()
        }
        super.order(place, relativeTo: otherWin)
    }
}

@MainActor
final class EditorWindowController: NSWindowController, NSWindowDelegate {
    private(set) var page: EditorPage?
    private let tabBar = TabBar()
    private let root = NSView()
    private var barHeight: NSLayoutConstraint?
    private var hidden = false

    init(contentSize: CGSize) {
        let window = EditorWindow(
            contentRect: CGRect(origin: .zero, size: contentSize),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = Theme.window
        window.minSize = CGSize(width: 720, height: 480)
        window.center()
        super.init(window: window)

        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.window.cgColor
        tabBar.translatesAutoresizingMaskIntoConstraints = false
        tabBar.isHidden = true
        tabBar.onSelect = { EditorManager.shared.select($0) }
        tabBar.onClose = { EditorManager.shared.close($0) }
        root.addSubview(tabBar)
        let height = tabBar.heightAnchor.constraint(equalToConstant: 0)
        barHeight = height
        NSLayoutConstraint.activate([
            tabBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tabBar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            tabBar.topAnchor.constraint(equalTo: root.topAnchor),
            height,
        ])
        window.contentView = root
        window.delegate = self
        window.onLeavingScreen = { [weak self] in self?.suspend() }
        window.onEnteringScreen = { [weak self] in
            if self?.isOnScreen == true { self?.resume() }
        }
        window.onCloseTab = { EditorManager.shared.closeCurrent() }
    }

    required init?(coder: NSCoder) { nil }

    func show(_ next: EditorPage?) {
        if let page {
            page.close()
            page.view.removeFromSuperview()
        }
        page = next
        guard let next else { return }
        let view = next.view
        view.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(view, positioned: .below, relativeTo: tabBar)
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            view.topAnchor.constraint(equalTo: tabBar.bottomAnchor),
            view.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        next.attached()
        if hidden { next.suspend() }
    }

    func showTabs(_ items: [TabBar.Item], current: Int) {
        let height: CGFloat = items.count > 1 ? TabBar.height : 0
        tabBar.show(items, current: current)
        guard barHeight?.constant != height else { return }
        barHeight?.constant = height
        tabBar.isHidden = height == 0
        root.layoutSubtreeIfNeeded()
        page?.windowDidResize()
    }

    private var isOnScreen: Bool {
        guard let window else { return false }
        return window.isVisible && !window.isMiniaturized
    }

    func updateVisibility() {
        guard isOnScreen else {
            suspend()
            return
        }
        if window?.occlusionState.contains(.visible) == true { resume() }
    }

    private func resume() {
        guard hidden else { return }
        hidden = false
        page?.resume()
    }

    private func suspend() {
        guard !hidden else { return }
        hidden = true
        page?.suspend()
    }

    override func supplementalTarget(forAction action: Selector, sender: Any?) -> Any? {
        if let page, page.responds(to: action) { return page }
        return super.supplementalTarget(forAction: action, sender: sender)
    }

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? {
        let manager = EditorManager.shared
        guard manager.tabs.indices.contains(manager.current), case .capture(let tab) = manager.tabs[manager.current] else { return nil }
        return tab.undo
    }

    func windowDidResize(_ notification: Notification) {
        page?.windowDidResize()
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        if window?.occlusionState.contains(.visible) == true {
            updateVisibility()
        } else {
            suspend()
        }
    }

    func windowDidBecomeKey(_ notification: Notification) {
        updateVisibility()
    }

    func windowWillMiniaturize(_ notification: Notification) {
        suspend()
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        updateVisibility()
    }

    func windowWillClose(_ notification: Notification) {
        EditorManager.shared.windowWillClose()
    }

    var probeState: String {
        let size = window?.frame.size ?? .zero
        return "window=\(Int(size.width))x\(Int(size.height)) backing=\(window?.backingScaleFactor ?? 0) bar=\(!tabBar.isHidden) \(page?.probeState ?? "page=none")"
    }

    func probeResize(_ size: CGSize) {
        guard let window else { return }
        window.setFrame(CGRect(origin: window.frame.origin, size: size), display: true)
    }

    func probeTabPoint(_ index: Int) -> CGPoint {
        tabBar.convert(tabBar.center(of: index), to: nil)
    }
}
