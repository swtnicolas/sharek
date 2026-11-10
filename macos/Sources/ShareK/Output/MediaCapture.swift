import AppKit

@MainActor
final class MediaCapture {
    let id: UInt64
    let format: RecordingFormat
    let pointSize: CGSize
    let createdAt: Date
    let title: String
    private(set) var url: URL
    private(set) var savedAutomatically: Bool
    private(set) var isSaved: Bool

    init(id: UInt64, url: URL, format: RecordingFormat, pointSize: CGSize, createdAt: Date = Date(), savedAutomatically: Bool, isSaved: Bool = false) {
        self.id = id
        self.url = url
        self.format = format
        self.pointSize = pointSize
        self.createdAt = createdAt
        self.savedAutomatically = savedAutomatically
        self.isSaved = savedAutomatically || isSaved
        title = "\(createdAt.formatted(date: .omitted, time: .standard)) · \(format.title)"
    }

    var info: String {
        let kind = format == .gif ? "GIF" : "Video MP4"
        let bytes = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).flatMap { $0 } ?? 0
        let size = bytes < 1_048_576 ? "\((bytes + 1023) / 1024) KB" : "\((Double(bytes) / 1_048_576).formatted(.number.precision(.fractionLength(1)).grouping(.never))) MB"
        return "\(kind) · \(Int(pointSize.width.rounded())) × \(Int(pointSize.height.rounded())) · \(size)"
    }

    static var temporaryDirectory: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("ShareK", isDirectory: true)
    }

    static func temporaryURL(for format: RecordingFormat) -> URL {
        temporaryDirectory.appendingPathComponent("\(UUID().uuidString).\(format.fileExtension)")
    }

    static func clearTemporaryFiles() {
        try? FileManager.default.removeItem(at: temporaryDirectory)
    }

    func save() throws {
        savedAutomatically = false
        guard !isSaved else { return }
        let destination = ImageWriter.uniqueURL(in: Settings.saveDirectory, date: createdAt, fileExtension: format.fileExtension)
        try FileManager.default.createDirectory(at: Settings.saveDirectory, withIntermediateDirectories: true)
        try FileManager.default.moveItem(at: url, to: destination)
        url = destination
        isSaved = true
        History.shared.record(destination)
    }

    func save(to destination: URL) throws {
        let manager = FileManager.default
        if manager.fileExists(atPath: destination.path) { try manager.removeItem(at: destination) }
        if isSaved {
            try manager.copyItem(at: url, to: destination)
        } else {
            try manager.moveItem(at: url, to: destination)
        }
        url = destination
        isSaved = true
        savedAutomatically = false
        History.shared.record(destination)
    }

    func discard() {
        if !isSaved {
            try? FileManager.default.removeItem(at: url)
        } else if savedAutomatically {
            try? FileManager.default.trashItem(at: url, resultingItemURL: nil)
            History.shared.remove([url])
        }
    }
}
