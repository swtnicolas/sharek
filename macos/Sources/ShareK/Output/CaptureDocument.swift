import AppKit

@MainActor
final class CaptureDocument {
    let id: UInt64
    let scale: CGFloat
    let createdAt: Date
    let title: String
    let renderer = ShapeRenderer()

    var state: EditorState {
        didSet {
            version &+= 1
            if state.source !== oldValue.source { track(state.source) }
        }
    }

    var isHidden = false {
        didSet {
            if isHidden { releaseMemory() }
        }
    }

    private(set) var version = 0
    private var savedVersion = 0
    private(set) var savedURL: URL?
    private(set) var savedAutomatically = false
    private var sources: [WeakSource] = []

    private struct WeakSource {
        weak var value: BaseImage?
    }

    init(id: UInt64, source: BaseImage, scale: CGFloat, createdAt: Date = Date()) {
        self.id = id
        self.scale = scale
        self.createdAt = createdAt
        state = EditorState(source: source, shapes: [])
        title = "\(createdAt.formatted(date: .omitted, time: .standard)) · \(source.width)×\(source.height)"
        track(state.source)
    }

    convenience init(id: UInt64, pixels: Pixels, scale: CGFloat, createdAt: Date = Date()) {
        self.init(id: id, source: BaseImage(pixels), scale: scale, createdAt: createdAt)
    }

    var pointSize: CGSize {
        CGSize(width: CGFloat(state.source.width) / scale, height: CGFloat(state.source.height) / scale)
    }

    var hasUnsavedChanges: Bool { version != savedVersion }

    func rendered() -> Pixels? {
        renderer.export(state, scale: scale)
    }

    func releaseMemory() {
        renderer.clearCaches()
        for source in sources { source.value?.release() }
    }

    func settle() {
        if isHidden { releaseMemory() }
    }

    func discardContents() {
        guard let blank = Pixels.blank() else { return }
        sources.removeAll()
        state = EditorState(base: blank, shapes: [])
        renderer.clearCaches()
    }

    func markAutoSaved(at url: URL) {
        savedURL = url
        savedAutomatically = true
        savedVersion = version
    }

    func resaveIfAutoSaved() {
        guard savedAutomatically, hasUnsavedChanges, let savedURL else { return }
        write(to: savedURL)
    }

    func save() {
        savedAutomatically = false
        write(to: savedURL ?? ImageWriter.uniqueURL(in: Settings.saveDirectory, date: createdAt))
    }

    func save(to url: URL) {
        savedAutomatically = false
        write(to: url)
    }

    func trashAutoSavedFile() {
        guard savedAutomatically, savedURL != nil else { return }
        MenuBarLink.shared.send(.trash(id: id))
        savedURL = nil
    }

    private func track(_ current: BaseImage) {
        sources.removeAll { $0.value == nil }
        for source in sources where source.value !== current { source.value?.release() }
        if !sources.contains(where: { $0.value === current }) { sources.append(WeakSource(value: current)) }
    }

    private static let writer = DispatchQueue(label: "ShareK.save", qos: .utility)

    private func write(to url: URL) {
        guard let pixels = rendered() else {
            Toast.show("No se pudo guardar", detail: ImageWriterError.encodingFailed(url).localizedDescription)
            return
        }
        savedURL = url
        savedVersion = version
        let scale = scale
        EditorManager.shared.beginWrite()
        Self.writer.async {
            var failure: String?
            do {
                try ImageWriter.write(pixels, scale: scale, to: url)
            } catch {
                failure = error.localizedDescription
            }
            DispatchQueue.main.async { [failure] in
                MainActor.assumeIsolated {
                    if let failure {
                        Toast.show("No se pudo guardar", detail: failure)
                    } else {
                        History.shared.record(url)
                    }
                    EditorManager.shared.endWrite()
                }
            }
        }
    }
}
