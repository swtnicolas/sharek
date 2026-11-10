import AppKit

@MainActor
final class CaptureTab {
    struct Viewport {
        let magnification: CGFloat
        let origin: CGPoint
    }

    let capture: CaptureDocument
    let undo = UndoManager()
    var autoFit = true
    var viewport: Viewport?
    var selection: UUID?
    var onRestore: (() -> Void)?

    init(_ capture: CaptureDocument) {
        self.capture = capture
        undo.levelsOfUndo = 100
    }

    func register(_ state: EditorState, name: String) {
        undo.registerUndo(withTarget: self) { target in
            target.restore(state, name: name)
        }
        undo.setActionName(name)
    }

    private func restore(_ state: EditorState, name: String) {
        let current = capture.state
        capture.state = state
        register(current, name: name)
        onRestore?()
    }
}

@MainActor
enum EditorTab {
    case capture(CaptureTab)
    case media(MediaCapture)

    var title: String {
        switch self {
        case .capture(let tab): tab.capture.title
        case .media(let media): media.title
        }
    }

    var edited: Bool {
        switch self {
        case .capture(let tab): tab.capture.hasUnsavedChanges
        case .media(let media): !media.isSaved
        }
    }

    var subtitle: String {
        switch self {
        case .capture(let tab):
            let capture = tab.capture
            guard let url = capture.savedURL else { return "Sin guardar" }
            let prefix = capture.savedAutomatically ? "Guardado automático" : "Guardado"
            return "\(prefix): \(url.lastPathComponent)\(capture.hasUnsavedChanges ? " · con cambios" : "")"
        case .media(let media):
            guard media.isSaved else { return "Sin guardar · \(media.info)" }
            let prefix = media.savedAutomatically ? "Guardado automático" : "Guardado"
            return "\(prefix): \(media.url.lastPathComponent) · \(media.info)"
        }
    }
}

@MainActor
protocol EditorPage: NSViewController {
    var probeState: String { get }
    func attached()
    func windowDidResize()
    func resume()
    func suspend()
    func close()
}
