import AppKit

@MainActor
final class EditorManager {
    static let shared = EditorManager()

    var onQuiet: (() -> Void)?

    private(set) var tabs: [EditorTab] = []
    private(set) var current = 0
    private(set) var window: EditorWindowController?
    private var history: HistoryWindowController?
    private var writes = 0
    private var lastLocalID = UInt64(1) << 48 | UInt64(getpid()) << 24

    var isBusy: Bool { writes > 0 || MenuBarLink.shared.isSending }
    var isIdle: Bool { tabs.isEmpty && history == nil && writes == 0 }

    func nextLocalID() -> UInt64 {
        lastLocalID += 1
        return lastLocalID
    }

    func open(_ document: CaptureDocument) {
        add(.capture(CaptureTab(document)), size: Self.initialSize(for: document.pointSize, extra: CapturePage.chrome, minimumWidth: EditorToolbar.fullWidth))
    }

    func open(_ media: MediaCapture) {
        add(.media(media), size: Self.initialSize(for: media.pointSize, extra: CGSize(width: 32, height: 80), minimumWidth: 720))
    }

    func showHistory() {
        let controller = history ?? HistoryWindowController()
        if history == nil {
            controller.onClose = { [weak self] _ in
                self?.history = nil
                self?.settle()
            }
            history = controller
        }
        NSApp.activate()
        controller.showWindow(nil)
        MenuBarLink.shared.send(.activate)
    }

    func select(_ index: Int) {
        guard tabs.indices.contains(index), index != current, !hasSheet else { return }
        show(index)
    }

    func closeCurrent(discarded: Bool = false) {
        close(current, discarded: discarded)
    }

    func close(_ index: Int, discarded: Bool = false) {
        guard tabs.indices.contains(index) else { return }
        guard index != current || !hasSheet else {
            NSSound.beep()
            return
        }
        if index == current { window?.show(nil) }
        dispose(tabs.remove(at: index), discarded: discarded)
        guard !tabs.isEmpty else {
            let closing = window
            window = nil
            current = 0
            closing?.window?.close()
            settle()
            return
        }
        let next = Self.selection(afterClosing: index, from: current, remaining: tabs.count)
        if index == current {
            show(next)
        } else {
            current = next
            refreshTabs()
        }
        Memory.releaseSoon()
    }

    nonisolated static func selection(afterClosing index: Int, from current: Int, remaining: Int) -> Int {
        index < current ? current - 1 : min(current, remaining - 1)
    }

    func windowWillClose() {
        guard let closing = window else { return }
        closing.show(nil)
        window = nil
        current = 0
        let closed = tabs
        tabs = []
        for tab in closed { dispose(tab, discarded: false) }
        settle()
    }

    func closeAll() {
        window?.window?.close()
        history?.window?.close()
    }

    func refreshTabs() {
        guard let window, tabs.indices.contains(current) else { return }
        let tab = tabs[current]
        window.showTabs(tabs.map { TabBar.Item(title: $0.title, edited: $0.edited) }, current: current)
        window.window?.title = tab.title
        window.window?.subtitle = tab.subtitle
        window.window?.isDocumentEdited = tab.edited
    }

    func refreshVisibility() {
        Task { @MainActor [weak self] in
            self?.window?.updateVisibility()
        }
    }

    func beginWrite() {
        writes += 1
    }

    func endWrite() {
        writes = max(0, writes - 1)
        finishIfQuiet()
        MenuBarLink.shared.maybeExit()
    }

    func finishIfQuiet() {
        guard !isBusy, let onQuiet else { return }
        self.onQuiet = nil
        onQuiet()
    }

    private func add(_ tab: EditorTab, size: CGSize) {
        let controller = window ?? EditorWindowController(contentSize: size)
        window = controller
        tabs.append(tab)
        if hasSheet {
            if case .capture(let added) = tab { added.capture.isHidden = true }
            refreshTabs()
        } else {
            show(tabs.count - 1)
        }
        NSApp.activate()
        if controller.window?.isMiniaturized == true { controller.window?.deminiaturize(nil) }
        controller.window?.makeKeyAndOrderFront(nil)
        MenuBarLink.shared.send(.activate)
    }

    private var hasSheet: Bool {
        window?.window?.attachedSheet != nil
    }

    private func show(_ index: Int) {
        guard let window else { return }
        window.show(nil)
        current = index
        refreshTabs()
        switch tabs[index] {
        case .capture(let tab): window.show(CapturePage(tab: tab))
        case .media(let media): window.show(MediaPage(media: media))
        }
        Memory.releaseSoon()
    }

    private func dispose(_ tab: EditorTab, discarded: Bool) {
        switch tab {
        case .capture(let tab):
            let capture = tab.capture
            if discarded {
                EditorClipboard.discard(capture)
                capture.trashAutoSavedFile()
            }
            capture.resaveIfAutoSaved()
            EditorClipboard.release(capture)
            tab.undo.removeAllActions()
            capture.discardContents()
        case .media(let media):
            if discarded {
                EditorClipboard.discard(media)
                media.discard()
            }
            EditorClipboard.release(media)
        }
    }

    private func settle() {
        Memory.releaseSoon()
        Task { @MainActor in MenuBarLink.shared.maybeExit() }
    }

    private static func initialSize(for size: CGSize, extra: CGSize, minimumWidth: CGFloat) -> CGSize {
        let visible = (NSScreen.underMouse ?? NSScreen.main)?.visibleFrame.size ?? CGSize(width: 1280, height: 800)
        return CGSize(
            width: max(min(max(size.width + extra.width, minimumWidth), visible.width * 0.85), min(720, minimumWidth)),
            height: max(min(size.height + extra.height, visible.height * 0.85), 480)
        )
    }
}
