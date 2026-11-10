import AppKit
import UniformTypeIdentifiers

enum ClipboardItem {
    case image(ImageProvider)
    case file(URL, gif: Bool)
    case files([URL])
}

@MainActor
final class ImageProvider: NSObject {
    private var png: Data?
    private var pixels: Pixels?
    private let scale: CGFloat

    init(png: Data) {
        self.png = png
        scale = 1
    }

    init(pixels: Pixels, scale: CGFloat) {
        self.pixels = pixels
        self.scale = scale
    }

    func encode() {
        guard png == nil, let pixels else { return }
        let scale = scale
        Task.detached(priority: .userInitiated) { [weak self] in
            let png = PNGEncoder.encode(pixels, scale: scale, fast: true)
            await self?.finish(png)
        }
    }

    func finish(_ data: Data?) {
        if png == nil { png = data }
        guard pixels != nil else { return }
        pixels = nil
        Memory.releaseSoon()
    }

    func pngData() -> Data? {
        if let png { return png }
        guard let pixels else { return nil }
        let data = PNGEncoder.encode(pixels, scale: scale, fast: true)
        finish(data)
        return data
    }

}

extension ImageProvider: NSPasteboardItemDataProvider {
    nonisolated func pasteboard(_ pasteboard: NSPasteboard?, item: NSPasteboardItem, provideDataForType type: NSPasteboard.PasteboardType) {
        let data = MainActor.assumeIsolated { self.pngData() }
        if let data { item.setData(data, forType: type) }
    }
}

@MainActor
final class ClipboardManager {
    static let shared = ClipboardManager()

    private static let gifType = NSPasteboard.PasteboardType(UTType.gif.identifier)

    private let pasteboard = NSPasteboard.general
    private let historyLimit = 5
    private var history: [(id: UInt64, item: ClipboardItem)] = []
    private var baseline: [NSPasteboardItem] = []
    private var ownedChangeCount = -1
    private var provider: AnyObject?

    private var ownsPasteboard: Bool { pasteboard.changeCount == ownedChangeCount }

    var owner: UInt64? { ownsPasteboard ? history.last?.id : nil }

    func forgetIfReplaced() {
        guard !ownsPasteboard, !history.isEmpty || provider != nil || !baseline.isEmpty else { return }
        history.removeAll()
        baseline = []
        provider = nil
        ownedChangeCount = -1
    }

    func copy(_ id: UInt64, _ item: ClipboardItem) {
        if !ownsPasteboard {
            baseline = snapshot()
            history.removeAll()
        }
        history.removeAll { $0.id == id }
        history.append((id, item))
        if history.count > historyLimit { history.removeFirst(history.count - historyLimit) }
        write(item)
    }

    @discardableResult
    func copy(_ id: UInt64, pixels: Pixels, scale: CGFloat) -> ImageProvider {
        let provider = ImageProvider(pixels: pixels, scale: scale)
        copy(id, .image(provider))
        return provider
    }

    func update(_ id: UInt64, _ item: ClipboardItem) {
        guard let index = history.firstIndex(where: { $0.id == id }) else { return }
        history[index].item = item
        if ownsPasteboard, index == history.count - 1 { write(item) }
    }

    func discard(_ id: UInt64) {
        let wasCurrent = history.last?.id == id
        history.removeAll { $0.id == id }
        guard ownsPasteboard, wasCurrent else { return }
        if let previous = history.last {
            write(previous.item)
        } else {
            restoreBaseline()
        }
    }

    func release(_ id: UInt64) {
        guard let index = history.firstIndex(where: { $0.id == id }) else { return }
        let entry = history.remove(at: index)
        if index == history.count {
            if ownsPasteboard { materialize(entry.item) }
            provider = nil
            ownedChangeCount = -1
        }
        if history.isEmpty { baseline = [] }
    }

    func flush() {
        guard ownsPasteboard, let entry = history.last else { return }
        materialize(entry.item)
    }

    private func write(_ item: ClipboardItem) {
        pasteboard.clearContents()
        provider = nil
        switch item {
        case .image(let source):
            let entry = NSPasteboardItem()
            entry.setDataProvider(source, forTypes: [.png])
            pasteboard.writeObjects([entry])
            provider = source
        case .file(let url, let gif):
            let entry = NSPasteboardItem()
            entry.setData(Data(url.absoluteString.utf8), forType: .fileURL)
            if gif, let data = try? Data(contentsOf: url) { entry.setData(data, forType: Self.gifType) }
            pasteboard.writeObjects([entry])
        case .files(let urls):
            pasteboard.writeObjects(urls.map { $0 as NSURL })
        }
        ownedChangeCount = pasteboard.changeCount
    }

    private func materialize(_ item: ClipboardItem) {
        guard case .image(let source) = item, let png = source.pngData() else { return }
        let entry = NSPasteboardItem()
        entry.setData(png, forType: .png)
        pasteboard.clearContents()
        pasteboard.writeObjects([entry])
        provider = nil
    }

    private func restoreBaseline() {
        pasteboard.clearContents()
        if !baseline.isEmpty { pasteboard.writeObjects(baseline) }
        baseline = []
        provider = nil
        ownedChangeCount = -1
    }

    private func snapshot() -> [NSPasteboardItem] {
        (pasteboard.pasteboardItems ?? []).map { item in
            let copy = NSPasteboardItem()
            for type in item.types {
                if let data = item.data(forType: type) { copy.setData(data, forType: type) }
            }
            return copy
        }
    }
}
