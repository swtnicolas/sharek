import Darwin
import Foundation

struct HistoryEntry: Codable, Hashable {
    let path: String
    let date: Date

    var url: URL { URL(fileURLWithPath: path) }

    var format: RecordingFormat? {
        switch url.pathExtension.lowercased() {
        case "gif": .gif
        case "mp4", "mov", "m4v", "mkv", "webm": .video
        default: nil
        }
    }
}

@MainActor
final class History {
    static let shared = History()
    static let limit = 1000

    var onChange: (() -> Void)?

    private static var fileURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("ShareK", isDirectory: true)
            .appendingPathComponent("history.json")
    }

    var modificationDate: Date? {
        (try? FileManager.default.attributesOfItem(atPath: Self.fileURL.path))?[.modificationDate] as? Date
    }

    func load() -> [HistoryEntry] {
        guard let data = try? Data(contentsOf: Self.fileURL) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return (try? decoder.decode([HistoryEntry].self, from: data)) ?? []
    }

    func record(_ url: URL) {
        let path = url.standardizedFileURL.path
        update { entries in
            entries.removeAll { $0.path == path }
            entries.insert(HistoryEntry(path: path, date: Date()), at: 0)
            if entries.count > Self.limit { entries.removeLast(entries.count - Self.limit) }
        }
    }

    func remove(_ urls: [URL]) {
        let paths = Set(urls.map(\.standardizedFileURL.path))
        update { entries in entries.removeAll { paths.contains($0.path) } }
    }

    func pruneMissingFiles() {
        let manager = FileManager.default
        update { entries in entries.removeAll { !manager.fileExists(atPath: $0.path) } }
    }

    private func update(_ change: (inout [HistoryEntry]) -> Void) {
        let url = Self.fileURL
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let lock = Darwin.open(url.appendingPathExtension("lock").path, O_CREAT | O_RDWR | O_CLOEXEC, 0o644)
        if lock >= 0 { _ = flock(lock, LOCK_EX) }
        defer {
            if lock >= 0 { _ = Darwin.close(lock) }
        }
        var entries = load()
        let before = entries
        change(&entries)
        guard entries != before else { return }
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        guard let data = try? encoder.encode(entries) else { return }
        try? data.write(to: url, options: .atomic)
        onChange?()
    }
}
