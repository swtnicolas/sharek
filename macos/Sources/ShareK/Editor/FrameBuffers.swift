import AppKit

protocol FrameStore: AnyObject {
    var width: Int { get }
    var height: Int { get }
    var colorSpace: CGColorSpace { get }
    var layerContents: Any? { get }
    func draw(_ body: (CGContext) -> Void) -> Bool
    func copyPixels() -> Pixels?
}

extension SurfaceImage: FrameStore {
    var layerContents: Any? { surface }

    func copyPixels() -> Pixels? {
        pixels()
    }
}

final class MappedFrame: FrameStore {
    let width: Int
    let height: Int
    let colorSpace: CGColorSpace
    private let bytesPerRow: Int
    private let memory: MappedScratch

    init?(width: Int, height: Int, colorSpace: CGColorSpace) {
        let bytesPerRow = (width * 4 + 63) & ~63
        guard width > 0, height > 0, let memory = MappedScratch(capacity: bytesPerRow * height) else { return nil }
        self.width = width
        self.height = height
        self.colorSpace = colorSpace
        self.bytesPerRow = bytesPerRow
        self.memory = memory
    }

    func draw(_ body: (CGContext) -> Void) -> Bool {
        guard let context = CGContext(
            data: memory.pointer, width: width, height: height, bitsPerComponent: 8, bytesPerRow: bytesPerRow,
            space: colorSpace, bitmapInfo: Pixels.bitmapInfo
        ) else { return false }
        _ = memset(memory.pointer, 0, bytesPerRow * height)
        body(context)
        return true
    }

    var layerContents: Any? {
        let kept = Unmanaged.passRetained(memory)
        let provider = CGDataProvider(dataInfo: kept.toOpaque(), data: memory.pointer, size: bytesPerRow * height) { info, _, _ in
            guard let info else { return }
            Unmanaged<MappedScratch>.fromOpaque(info).release()
        }
        guard let provider else {
            kept.release()
            return nil
        }
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: bytesPerRow,
            space: colorSpace, bitmapInfo: CGBitmapInfo(rawValue: Pixels.bitmapInfo),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent
        )
    }

    func copyPixels() -> Pixels? {
        let kept = memory
        let data = Data(bytesNoCopy: memory.pointer, count: bytesPerRow * height, deallocator: .custom { _, _ in _ = kept })
        return Pixels(format: PixelFormat(width: width, height: height, bytesPerRow: bytesPerRow, colorSpace: colorSpace), data: data)
    }
}

@MainActor
final class FrameBuffers {
    private enum Mode {
        case unknown
        case measuring
        case mapped
        case surface
    }

    private static var mode = Mode.unknown
    private static let probeSide = 2048
    private static let live = NSHashTable<FrameBuffers>.weakObjects()

    var onFallback: (() -> Void)?

    private var front: FrameStore?
    private var spares: [FrameStore] = []
    private var trim: Task<Void, Never>?

    init() {
        Self.live.add(self)
    }

    var shown: FrameStore? { front }
    var kind: String { front == nil ? "none" : front is MappedFrame ? "mapped" : "surface" }

    static func probeMeasureAgain() {
        mode = .unknown
    }

    func present(width: Int, height: Int, colorSpace: CGColorSpace, on layer: CALayer, _ body: (CGContext) -> Void) {
        let mapped = Self.mode != .surface
        func fits(_ frame: FrameStore) -> Bool {
            frame.width == width && frame.height == height && (frame is MappedFrame) == mapped && CFEqual(frame.colorSpace, colorSpace)
        }
        func made() -> FrameStore? {
            if mapped { return MappedFrame(width: width, height: height, colorSpace: colorSpace) }
            return SurfaceImage(width: width, height: height, colorSpace: colorSpace)
        }
        spares.removeAll { !fits($0) }
        let oldest = spares.count >= 2 ? spares.removeFirst() : nil
        guard let target = oldest ?? made(), target.draw(body), let contents = target.layerContents else { return }
        if let front, fits(front) { spares.append(front) }
        front = target
        layer.contents = contents
        trim?.cancel()
        trim = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(500))
            guard !Task.isCancelled else { return }
            self?.spares = []
            Memory.releaseSoon()
        }
        if Self.mode == .unknown { Self.measure(beside: layer, colorSpace: colorSpace) }
    }

    func release() {
        trim?.cancel()
        trim = nil
        front = nil
        spares = []
    }

    private static func measure(beside layer: CALayer, colorSpace: CGColorSpace) {
        Self.mode = .measuring
        Task { @MainActor [weak layer] in
            let side = Self.probeSide
            let fill = Theme.canvas.cgColor
            guard let layer,
                  let probe = MappedFrame(width: side, height: side, colorSpace: colorSpace),
                  probe.draw({ context in
                      context.setFillColor(fill)
                      context.fill(CGRect(x: 0, y: 0, width: side, height: side))
                  }),
                  let image = probe.layerContents
            else {
                Self.mode = .unknown
                return
            }
            CATransaction.flush()
            let before = Memory.footprint
            let sample = CALayer()
            sample.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
            sample.contents = image
            layer.addSublayer(sample)
            CATransaction.flush()
            let copied = before > 0 && Memory.footprint > before + UInt64(side * side * 4) * 6 / 10
            sample.removeFromSuperlayer()
            CATransaction.flush()
            Self.mode = copied ? .surface : .mapped
            guard copied else { return }
            for buffers in Self.live.allObjects {
                buffers.release()
                buffers.onFallback?()
            }
            Memory.releaseSoon()
        }
    }
}
