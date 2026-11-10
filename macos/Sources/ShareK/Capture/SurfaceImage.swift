import CoreGraphics
import CoreVideo
import Foundation
import IOSurface

final class SurfaceImage: @unchecked Sendable {
    let width: Int
    let height: Int
    let colorSpace: CGColorSpace
    private let buffer: CVPixelBuffer

    private static var colorSpaceKey: CFString { "IOSurfaceColorSpace" as CFString }

    init?(_ buffer: CVPixelBuffer, colorSpace: CGColorSpace? = nil) {
        guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_32BGRA,
              CVPixelBufferGetIOSurface(buffer) != nil
        else { return nil }
        self.buffer = buffer
        width = CVPixelBufferGetWidth(buffer)
        height = CVPixelBufferGetHeight(buffer)
        self.colorSpace = colorSpace ?? Pixels.sRGB
        if let surface, let list = self.colorSpace.copyPropertyList() {
            IOSurfaceSetValue(surface, Self.colorSpaceKey, list)
        }
    }

    convenience init?(width: Int, height: Int, colorSpace: CGColorSpace) {
        var created: CVPixelBuffer?
        let attributes = [kCVPixelBufferIOSurfacePropertiesKey as String: [String: Any]()] as CFDictionary
        guard width > 0, height > 0,
              CVPixelBufferCreate(kCFAllocatorDefault, width, height, kCVPixelFormatType_32BGRA, attributes, &created) == kCVReturnSuccess,
              let buffer = created
        else { return nil }
        self.init(buffer, colorSpace: colorSpace)
    }

    var surface: IOSurfaceRef? {
        CVPixelBufferGetIOSurface(buffer)?.takeUnretainedValue()
    }

    func draw(_ body: (CGContext) -> Void) -> Bool {
        guard CVPixelBufferLockBaseAddress(buffer, []) == kCVReturnSuccess else { return false }
        defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        guard let base = CVPixelBufferGetBaseAddress(buffer),
              let context = CGContext(
                  data: base, width: width, height: height, bitsPerComponent: 8, bytesPerRow: stride,
                  space: colorSpace, bitmapInfo: Pixels.bitmapInfo
              )
        else { return false }
        _ = memset(base, 0, stride * height)
        body(context)
        return true
    }

    func pixels(in rect: CGRect? = nil) -> Pixels? {
        let full = CGRect(x: 0, y: 0, width: width, height: height)
        let area = (rect ?? full).integral.intersection(full)
        guard !area.isNull, !area.isEmpty else { return nil }
        let lock = SurfaceLock(buffer)
        guard let base = lock.base else { return nil }
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        if area == full {
            let data = Data(bytesNoCopy: base, count: stride * height, deallocator: .custom { _, _ in lock.unlock() })
            return Pixels(format: PixelFormat(width: width, height: height, bytesPerRow: stride, colorSpace: colorSpace), data: data)
        }
        defer { lock.unlock() }
        let x = Int(area.minX)
        let y = Int(area.minY)
        let cropWidth = Int(area.width)
        let cropHeight = Int(area.height)
        let rowBytes = cropWidth * 4
        guard let data = MappedData.make(count: rowBytes * cropHeight, { to in
            for row in 0..<cropHeight {
                (to + row * rowBytes).copyMemory(from: base + (y + row) * stride + x * 4, byteCount: rowBytes)
            }
            return true
        }) else { return nil }
        return Pixels(format: PixelFormat(width: cropWidth, height: cropHeight, bytesPerRow: rowBytes, colorSpace: colorSpace), data: data)
    }
}

private final class SurfaceLock: @unchecked Sendable {
    private let buffer: CVPixelBuffer
    private let lock = NSLock()
    private var locked: Bool

    init(_ buffer: CVPixelBuffer) {
        self.buffer = buffer
        locked = CVPixelBufferLockBaseAddress(buffer, .readOnly) == kCVReturnSuccess
    }

    deinit {
        unlock()
    }

    var base: UnsafeMutableRawPointer? {
        locked ? CVPixelBufferGetBaseAddress(buffer) : nil
    }

    func unlock() {
        lock.lock()
        defer { lock.unlock() }
        guard locked else { return }
        locked = false
        CVPixelBufferUnlockBaseAddress(buffer, .readOnly)
    }
}
