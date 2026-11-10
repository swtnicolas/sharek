import Accelerate
import Compression
import CoreGraphics
import Foundation

struct PixelFormat: Codable {
    let width: Int
    let height: Int
    let bytesPerRow: Int
    let colorSpaceName: String?
    let iccProfile: Data?

    init(width: Int, height: Int, bytesPerRow: Int, colorSpace: CGColorSpace) {
        self.width = width
        self.height = height
        self.bytesPerRow = bytesPerRow
        let name = colorSpace.name as String?
        colorSpaceName = name
        iccProfile = name == nil ? colorSpace.copyICCData() as Data? : nil
    }

    private init(width: Int, height: Int, bytesPerRow: Int, colorSpaceName: String?, iccProfile: Data?) {
        self.width = width
        self.height = height
        self.bytesPerRow = bytesPerRow
        self.colorSpaceName = colorSpaceName
        self.iccProfile = iccProfile
    }

    func with(width: Int, height: Int, bytesPerRow: Int) -> PixelFormat {
        PixelFormat(width: width, height: height, bytesPerRow: bytesPerRow, colorSpaceName: colorSpaceName, iccProfile: iccProfile)
    }

    var colorSpace: CGColorSpace {
        colorSpaceName.flatMap { CGColorSpace(name: $0 as CFString) }
            ?? iccProfile.flatMap { CGColorSpace(iccData: $0 as CFData) }
            ?? Pixels.sRGB
    }
}

struct Pixels {
    static let bitmapInfo = CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue

    static var sRGB: CGColorSpace { CGColorSpace(name: CGColorSpace.sRGB)! }

    let format: PixelFormat
    let data: Data

    var width: Int { format.width }
    var height: Int { format.height }
    var colorSpace: CGColorSpace { format.colorSpace }

    init?(format: PixelFormat, data: Data) {
        guard format.width > 0, format.height > 0, format.bytesPerRow >= format.width * 4,
              data.count >= format.bytesPerRow * format.height
        else { return nil }
        self.format = format
        self.data = data
    }

    init?(_ image: CGImage) {
        let rect = CGRect(x: 0, y: 0, width: image.width, height: image.height)
        guard let pixels = Pixels.render(width: image.width, height: image.height, colorSpace: image.colorSpace, { $0.draw(image, in: rect) }) else {
            return nil
        }
        self = pixels
    }

    static func render(width: Int, height: Int, colorSpace: CGColorSpace?, _ draw: (CGContext) -> Void) -> Pixels? {
        guard width > 0, height > 0 else { return nil }
        let rgb = colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
        for space in [rgb, sRGB].compactMap({ $0 }) {
            let bytesPerRow = width * 4
            let data = MappedData.make(count: bytesPerRow * height) { pointer in
                guard let context = CGContext(
                    data: pointer, width: width, height: height, bitsPerComponent: 8, bytesPerRow: bytesPerRow,
                    space: space, bitmapInfo: bitmapInfo
                ) else { return false }
                draw(context)
                return true
            }
            if let data {
                return Pixels(format: PixelFormat(width: width, height: height, bytesPerRow: bytesPerRow, colorSpace: space), data: data)
            }
        }
        return nil
    }

    static func drawable(width: Int, height: Int, colorSpace: CGColorSpace?) -> (pixels: Pixels, context: CGContext)? {
        guard width > 1, height > 1 else { return nil }
        var kept: CGContext?
        guard let pixels = render(width: width, height: height, colorSpace: colorSpace, { kept = $0 }), let kept else { return nil }
        return (pixels, kept)
    }

    static func blank() -> Pixels? {
        render(width: 1, height: 1, colorSpace: nil) { _ in }
    }

    var image: CGImage? {
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: format.bytesPerRow,
            space: colorSpace, bitmapInfo: CGBitmapInfo(rawValue: Self.bitmapInfo),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
        )
    }

    var bounds: CGRect { CGRect(x: 0, y: 0, width: width, height: height) }

    func blit(from area: CGRect, into context: CGContext, at target: CGRect) {
        let source = area.integral.intersection(bounds)
        guard let raw = context.data, !source.isNull, !source.isEmpty, target.width > 0, target.height > 0 else { return }
        let left = max(Int(target.minX.rounded()), 0)
        let right = min(Int(target.maxX.rounded()), context.width)
        let top = max(Int(target.minY.rounded()), 0)
        let bottom = min(Int(target.maxY.rounded()), context.height)
        guard left < right, top < bottom else { return }
        let columns = Self.nearest(from: left, to: right, origin: target.minX, length: target.width, first: Int(source.minX), count: Int(source.width))
        let rows = Self.nearest(from: top, to: bottom, origin: target.minY, length: target.height, first: Int(source.minY), count: Int(source.height))
        var run = 1
        while run < columns.count, columns[run] == columns[0] + run { run += 1 }
        let targetStride = context.bytesPerRow
        let stride = format.bytesPerRow
        data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            guard let from = bytes.baseAddress else { return }
            for (offset, row) in rows.enumerated() {
                let line = raw + (top + offset) * targetStride + left * 4
                if offset > 0, rows[offset - 1] == row {
                    line.copyMemory(from: line - targetStride, byteCount: columns.count * 4)
                    continue
                }
                line.copyMemory(from: from + row * stride + columns[0] * 4, byteCount: run * 4)
                guard run < columns.count else { continue }
                let input = (from + row * stride).assumingMemoryBound(to: UInt32.self)
                let output = line.assumingMemoryBound(to: UInt32.self)
                for index in run..<columns.count { output[index] = input[columns[index]] }
            }
        }
    }

    static func nearest(from start: Int, to end: Int, origin: CGFloat, length: CGFloat, first: Int, count: Int) -> [Int] {
        let last = first + count - 1
        if abs(length - CGFloat(count)) < 1 {
            let shift = first - Int(origin.rounded())
            return (start..<end).map { min(max($0 + shift, first), last) }
        }
        let step = CGFloat(count) / length
        return (start..<end).map { min(max(first + Int(((CGFloat($0) + 0.5 - origin) * step).rounded(.down)), first), last) }
    }

    func cropped(to rect: CGRect) -> Pixels? {
        let area = rect.integral.intersection(bounds)
        guard !area.isNull, !area.isEmpty else { return nil }
        if area == bounds { return self }
        let x = Int(area.minX)
        let y = Int(area.minY)
        let width = Int(area.width)
        let height = Int(area.height)
        let rowBytes = width * 4
        let stride = format.bytesPerRow
        let source = data
        guard let output = MappedData.make(count: rowBytes * height, { to in
            source.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
                guard let from = raw.baseAddress else { return false }
                for row in 0..<height {
                    (to + row * rowBytes).copyMemory(from: from + (y + row) * stride + x * 4, byteCount: rowBytes)
                }
                return true
            }
        }) else { return nil }
        return Pixels(format: format.with(width: width, height: height, bytesPerRow: rowBytes), data: output)
    }

    func scaled(from rect: CGRect? = nil, width targetWidth: Int, height targetHeight: Int) -> Pixels? {
        let area = (rect ?? bounds).integral.intersection(bounds)
        guard !area.isNull, !area.isEmpty, targetWidth > 0, targetHeight > 0 else { return nil }
        let rowBytes = targetWidth * 4
        let stride = format.bytesPerRow
        let source = data
        guard let output = MappedData.make(count: rowBytes * targetHeight, { to in
            source.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
                guard let from = raw.baseAddress else { return false }
                var input = vImage_Buffer(
                    data: UnsafeMutableRawPointer(mutating: from + Int(area.minY) * stride + Int(area.minX) * 4),
                    height: vImagePixelCount(area.height), width: vImagePixelCount(area.width), rowBytes: stride
                )
                var result = vImage_Buffer(data: to, height: vImagePixelCount(targetHeight), width: vImagePixelCount(targetWidth), rowBytes: rowBytes)
                let needed = vImageScale_ARGB8888(&input, &result, nil, vImage_Flags(kvImageGetTempBufferSize))
                let spare = needed > 0 ? MappedScratch(capacity: needed) : nil
                return withExtendedLifetime(spare) {
                    vImageScale_ARGB8888(&input, &result, spare?.pointer, vImage_Flags(kvImageNoFlags)) == kvImageNoError
                }
            }
        }) else { return nil }
        return Pixels(format: format.with(width: targetWidth, height: targetHeight, bytesPerRow: rowBytes), data: output)
    }

    var isOpaque: Bool {
        let stride = format.bytesPerRow
        let rowBytes = format.width * 4
        let rows = format.height
        return data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) -> Bool in
            for row in 0..<rows {
                var index = row * stride + 3
                let end = row * stride + rowBytes
                while index < end {
                    if bytes[index] != 255 { return false }
                    index += 4
                }
            }
            return true
        }
    }

    func color(at pixel: CGPoint) -> RGBA? {
        let x = Int(pixel.x)
        let y = Int(pixel.y)
        guard x >= 0, y >= 0, x < width, y < height else { return nil }
        let offset = y * format.bytesPerRow + x * 4
        let bytes = data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> [UInt8] in [raw[offset], raw[offset + 1], raw[offset + 2], raw[offset + 3]] }
        guard bytes[3] > 0 else { return .clear }
        let alpha = CGFloat(bytes[3]) / 255
        let components = [CGFloat(bytes[2]) / 255 / alpha, CGFloat(bytes[1]) / 255 / alpha, CGFloat(bytes[0]) / 255 / alpha, 1]
        guard let color = CGColor(colorSpace: colorSpace, components: components)?
                .converted(to: Pixels.sRGB, intent: .defaultIntent, options: nil),
              let converted = color.components, converted.count >= 3
        else { return nil }
        return RGBA(r: converted[0], g: converted[1], b: converted[2], a: alpha)
    }
}

struct PackedPixels {
    private let format: PixelFormat
    private let count: Int
    private let data: Data

    init?(_ pixels: Pixels) {
        let source = pixels.data
        let size = source.count
        guard let packed = MappedData.make(capacity: size, { target in
            source.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Int? in
                guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return nil }
                let written = compression_encode_buffer(target.assumingMemoryBound(to: UInt8.self), size, base, size, nil, COMPRESSION_LZFSE)
                return written > 0 ? written : nil
            }
        }) else { return nil }
        format = pixels.format
        count = size
        data = packed
    }

    func unpacked() -> Pixels? {
        let size = count
        let packed = data
        guard let raw = MappedData.make(count: size, { target in
            packed.withUnsafeBytes { (stored: UnsafeRawBufferPointer) -> Bool in
                guard let base = stored.bindMemory(to: UInt8.self).baseAddress else { return false }
                return compression_decode_buffer(target.assumingMemoryBound(to: UInt8.self), size, base, stored.count, nil, COMPRESSION_LZFSE) == size
            }
        }) else { return nil }
        return Pixels(format: format, data: raw)
    }
}

@MainActor
final class BaseImage {
    private static let packer = DispatchQueue(label: "ShareK.pack", qos: .utility)

    let width: Int
    let height: Int
    private var pixels: Pixels?
    private var packed: PackedPixels?
    private var packing = false
    private var packable = true
    private var released = false

    init(_ pixels: Pixels) {
        width = pixels.width
        height = pixels.height
        self.pixels = pixels
    }

    var current: Pixels? {
        released = false
        if pixels == nil { pixels = packed?.unpacked() }
        return pixels
    }

    func release() {
        released = true
        guard let pixels else { return }
        if packed != nil {
            self.pixels = nil
            return
        }
        guard !packing, packable else { return }
        packing = true
        Self.packer.async { [weak self] in
            let result = PackedPixels(pixels)
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.finishPacking(result) }
            }
        }
    }

    private func finishPacking(_ result: PackedPixels?) {
        packing = false
        guard let result else {
            packable = false
            return
        }
        packed = result
        guard released else { return }
        pixels = nil
        Memory.releaseSoon()
    }
}
