import Darwin
import Foundation
import ImageIO
import UniformTypeIdentifiers

enum ImageWriterError: LocalizedError {
    case encodingFailed(URL)

    var errorDescription: String? {
        switch self {
        case .encodingFailed(let url): "No se pudo escribir \(url.lastPathComponent)."
        }
    }
}

enum ImageWriter {
    static func data(_ image: CGImage, type: UTType, scale: CGFloat? = nil) -> Data? {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, type.identifier as CFString, 1, nil) else { return nil }
        let options = scale.map { [kCGImagePropertyDPIWidth: 72 * $0, kCGImagePropertyDPIHeight: 72 * $0] as CFDictionary }
        CGImageDestinationAddImage(destination, image, options)
        return CGImageDestinationFinalize(destination) ? data as Data : nil
    }

    static func write(_ pixels: Pixels, scale: CGFloat, to url: URL) throws {
        let type = UTType(filenameExtension: url.pathExtension) ?? .png
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        guard type.conforms(to: .jpeg) else {
            guard let png = PNGEncoder.encode(pixels, scale: scale, fast: false) else { throw ImageWriterError.encodingFailed(url) }
            try png.write(to: url, options: .atomic)
            return
        }
        guard let image = pixels.image,
              let destination = CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil)
        else { throw ImageWriterError.encodingFailed(url) }
        let options: [CFString: Any] = [
            kCGImagePropertyDPIWidth: 72 * scale,
            kCGImagePropertyDPIHeight: 72 * scale,
            kCGImageDestinationLossyCompressionQuality: 0.9,
        ]
        CGImageDestinationAddImage(destination, image, options as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw ImageWriterError.encodingFailed(url) }
    }

    static func writeNew(_ data: Data, to url: URL) throws -> Bool {
        let directory = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString)")
        try data.write(to: temporary)
        defer { try? FileManager.default.removeItem(at: temporary) }
        if renamex_np(temporary.path, url.path, UInt32(RENAME_EXCL)) == 0 { return true }
        let code = errno
        if code == EEXIST { return false }
        guard code == ENOTSUP || code == EINVAL else { throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO) }
        if FileManager.default.fileExists(atPath: url.path) { return false }
        try FileManager.default.moveItem(at: temporary, to: url)
        return true
    }

    static func read(_ url: URL) -> (pixels: Pixels, scale: CGFloat)? {
        let options = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url as CFURL, options),
              let image = CGImageSourceCreateImageAtIndex(source, 0, options),
              let pixels = Pixels(image)
        else { return nil }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let dpi = (properties?[kCGImagePropertyDPIWidth] as? NSNumber)?.doubleValue ?? 72
        return (pixels, max(1, (dpi / 72 * 100).rounded() / 100))
    }

    static func uniqueURL(in directory: URL, date: Date, fileExtension: String = "png", taken: (URL) -> Bool = { _ in false }) -> URL {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd_HH-mm-ss"
        let stem = "ShareK_\(formatter.string(from: date))"
        var url = directory.appendingPathComponent("\(stem).\(fileExtension)")
        var index = 2
        while FileManager.default.fileExists(atPath: url.path) || taken(url) {
            url = directory.appendingPathComponent("\(stem)_\(index).\(fileExtension)")
            index += 1
        }
        return url
    }
}
