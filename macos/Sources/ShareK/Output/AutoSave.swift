import Foundation

@MainActor
final class AutoSave {
    static let shared = AutoSave()

    private struct Entry {
        let url: URL
        var tracked = true
        var landed = false
        var trashed = false
        var released = false
    }

    private static let writer = DispatchQueue(label: "ShareK.autosave", qos: .utility)

    private var entries: [UInt64: Entry] = [:]

    func reserve(_ id: UInt64, date: Date) -> URL {
        let pending = Set(entries.values.filter { !$0.landed }.map(\.url.path))
        let url = ImageWriter.uniqueURL(in: Settings.saveDirectory, date: date) { pending.contains($0.path) }
        entries[id] = Entry(url: url)
        return url
    }

    func write(_ png: Data?, to url: URL, id: UInt64, tracked: Bool) {
        entries[id]?.tracked = tracked
        guard entries[id]?.trashed != true else {
            entries[id] = nil
            return
        }
        guard let png else {
            entries[id] = nil
            Toast.show("No se pudo guardar", detail: ImageWriterError.encodingFailed(url).localizedDescription)
            return
        }
        Self.writer.async {
            let result = Result { try ImageWriter.writeNew(png, to: url) }
            DispatchQueue.main.async {
                MainActor.assumeIsolated { AutoSave.shared.landed(id, result) }
            }
        }
    }

    func trash(_ id: UInt64) {
        guard let entry = entries[id] else { return }
        guard entry.landed else {
            entries[id]?.trashed = true
            return
        }
        entries[id] = nil
        History.shared.remove([entry.url])
        try? FileManager.default.trashItem(at: entry.url, resultingItemURL: nil)
    }

    func release(_ id: UInt64) {
        guard let entry = entries[id] else { return }
        if entry.landed {
            entries[id] = nil
        } else {
            entries[id]?.released = true
        }
    }

    func forgetLanded() {
        entries = entries.filter { !$0.value.landed }
    }

    private func landed(_ id: UInt64, _ result: Result<Bool, Error>) {
        defer { Memory.releaseSoon() }
        guard let entry = entries[id] else { return }
        switch result {
        case .success(let written):
            guard written else {
                entries[id] = nil
                if !entry.tracked, !entry.trashed {
                    Toast.show("No se pudo guardar", detail: "Ya existe \(entry.url.lastPathComponent).")
                }
                return
            }
            if entry.trashed {
                entries[id] = nil
                try? FileManager.default.trashItem(at: entry.url, resultingItemURL: nil)
                return
            }
            History.shared.record(entry.url)
            if entry.tracked, !entry.released {
                entries[id]?.landed = true
            } else {
                entries[id] = nil
            }
        case .failure(let error):
            entries[id] = nil
            Toast.show("No se pudo guardar", detail: error.localizedDescription)
        }
    }
}
