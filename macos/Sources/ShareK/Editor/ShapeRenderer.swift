import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins

@MainActor
final class ShapeRenderer {
    private let effects = EffectCache()
    private var scratch: MappedScratch?

    func clearCaches() {
        effects.clear()
        scratch = nil
    }

    func draw(_ state: EditorState, scale: CGFloat, in context: CGContext, range: Range<Int>? = nil) {
        let range = range ?? state.shapes.indices
        let source = state.source
        let canvas = CGRect(x: 0, y: 0, width: CGFloat(source.width) / scale, height: CGFloat(source.height) / scale)
        var step = state.shapes[..<range.lowerBound].filter { $0.kind == .step }.count
        for shape in state.shapes[range] {
            if shape.kind == .step { step += 1 }
            draw(shape, number: step, source: source, scale: scale, canvas: canvas, in: context)
        }
        effects.prune(keeping: state.shapes)
    }

    static func fingerprints(_ shapes: ArraySlice<Shape>) -> [Int] {
        var step = 0
        return shapes.map { shape in
            var hasher = Hasher()
            hasher.combine(shape.id)
            hasher.combine(shape.kind)
            if shape.kind == .step {
                step += 1
                hasher.combine(step)
            }
            let style = shape.style
            for color in [style.stroke, style.fill, style.text] {
                for value in [color.r, color.g, color.b, color.a] { hasher.combine(value) }
            }
            for value in [style.lineWidth, style.cornerRadius, style.fontSize, style.strength] { hasher.combine(value) }
            hasher.combine(style.shadow)
            for point in [shape.start, shape.end, shape.tail] + shape.points {
                hasher.combine(point.x)
                hasher.combine(point.y)
            }
            hasher.combine(shape.text)
            hasher.combine(shape.image.map { ObjectIdentifier($0) })
            return hasher.finalize()
        }
    }

    func export(_ state: EditorState, scale: CGFloat) -> Pixels? {
        guard let base = state.source.current else { return nil }
        guard !state.shapes.isEmpty, let image = base.image else { return base }
        let width = base.width
        let height = base.height
        return Pixels.render(width: width, height: height, colorSpace: base.colorSpace) { context in
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            context.translateBy(x: 0, y: CGFloat(height))
            context.scaleBy(x: scale, y: -scale)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
            self.draw(state, scale: scale, in: context)
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private func draw(_ shape: Shape, number: Int, source: BaseImage, scale: CGFloat, canvas: CGRect, in context: CGContext) {
        let style = shape.style
        context.saveGState()
        defer { context.restoreGState() }
        context.setLineCap(.round)
        context.setLineJoin(.round)

        switch shape.kind {
        case .rectangle:
            let radius = min(style.cornerRadius, shape.rect.width / 2, shape.rect.height / 2)
            let path = CGPath(roundedRect: shape.rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
            shadowed(shape, canvas: canvas, context) { context in paint(path, style: style, in: context) }

        case .ellipse:
            let path = CGPath(ellipseIn: shape.rect, transform: nil)
            shadowed(shape, canvas: canvas, context) { context in paint(path, style: style, in: context) }

        case .freehand:
            let path = CGMutablePath()
            path.addLines(between: shape.points.count > 1 ? shape.points : [shape.start, shape.start])
            shadowed(shape, canvas: canvas, context) { context in stroke(path, color: style.stroke, width: style.lineWidth, in: context) }

        case .line:
            let path = CGMutablePath()
            path.addLines(between: [shape.start, shape.end])
            shadowed(shape, canvas: canvas, context) { context in stroke(path, color: style.stroke, width: style.lineWidth, in: context) }

        case .arrow:
            guard let paths = ShapeGeometry.arrowPaths(shape) else { return }
            shadowed(shape, canvas: canvas, context) { context in
                stroke(paths.shaft, color: style.stroke, width: style.lineWidth, in: context)
                context.addPath(paths.head)
                context.setFillColor(style.stroke.cgColor)
                context.setStrokeColor(style.stroke.cgColor)
                context.setLineWidth(max(1, style.lineWidth / 3))
                context.drawPath(using: .fillStroke)
            }

        case .textOutline:
            shadowed(shape, canvas: canvas, context) { _ in drawText(shape) }

        case .textBackground:
            let radius = min(style.cornerRadius, shape.rect.width / 2, shape.rect.height / 2)
            let path = CGPath(roundedRect: shape.rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
            shadowed(shape, canvas: canvas, context) { context in
                paint(path, style: style, in: context)
                drawText(shape)
            }

        case .speechBalloon:
            let path = ShapeGeometry.balloonPath(shape)
            shadowed(shape, canvas: canvas, context) { context in
                paint(path, style: style, in: context)
                drawText(shape)
            }

        case .step:
            let bounds = ShapeGeometry.bounds(shape)
            let path = CGPath(ellipseIn: bounds, transform: nil)
            shadowed(shape, canvas: canvas, context) { context in
                paint(path, style: style, in: context)
                let label = "\(number)" as NSString
                let attributes = TextMetrics.attributes(shape)
                let size = label.size(withAttributes: attributes)
                label.draw(at: CGPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2), withAttributes: attributes)
            }

        case .magnify:
            let rect = shape.rect
            let zoom = max(style.strength, 1)
            let area = CGRect(
                x: (rect.midX - rect.width / zoom / 2) * scale,
                y: (rect.midY - rect.height / zoom / 2) * scale,
                width: rect.width / zoom * scale,
                height: rect.height / zoom * scale
            )
            guard rect.width >= 1, rect.height >= 1, let image = source.current?.cropped(to: area)?.image else { return }
            shadowed(shape, canvas: canvas, context) { context in
                context.interpolationQuality = .high
                drawImage(image, in: rect, context: context)
                if style.lineWidth > 0 { stroke(CGPath(rect: rect, transform: nil), color: style.stroke, width: style.lineWidth, in: context) }
            }

        case .smartEraser:
            context.setFillColor(style.fill.cgColor)
            context.fill(shape.rect)

        case .highlight:
            context.clip(to: shape.rect)
            if let base = source.current, let pixels = ShapeRenderer.pixels(of: shape.rect, in: base, scale: scale), let image = pixels.pixels.image {
                context.setBlendMode(.destinationOver)
                drawImage(image, in: pixels.rect, context: context)
            }
            context.setBlendMode(.multiply)
            context.setFillColor(style.fill.cgColor)
            context.fill(shape.rect)

        case .blur, .pixelate:
            guard let effect = effects.image(for: shape, source: source, scale: scale) else { return }
            context.interpolationQuality = shape.kind == .pixelate ? .none : .default
            drawImage(effect.image, in: effect.rect, context: context)

        case .spotlight:
            let path = CGMutablePath()
            path.addRect(canvas)
            path.addEllipse(in: shape.rect)
            context.addPath(path)
            context.setFillColor(CGColor(gray: 0, alpha: style.strength))
            context.fillPath(using: .evenOdd)

        case .image:
            guard let image = shape.image else { return }
            shadowed(shape, canvas: canvas, context) { context in
                context.interpolationQuality = .high
                drawImage(image, in: shape.rect, context: context)
                if style.lineWidth > 0 { stroke(CGPath(rect: shape.rect, transform: nil), color: style.stroke, width: style.lineWidth, in: context) }
            }
        }
    }

    private func shadowed(_ shape: Shape, canvas: CGRect, _ context: CGContext, _ body: (CGContext) -> Void) {
        defer { body(context) }
        guard shape.style.shadow, let target = context.data else { return }
        let ctm = context.ctm
        let zoom = max(abs(ctm.a), 0.01)
        let height = CGFloat(context.height)
        let frame = CGRect(x: 0, y: 0, width: CGFloat(context.width), height: height)
        func placed(_ rect: CGRect) -> CGRect {
            let device = rect.applying(ctm)
            return CGRect(x: device.minX, y: height - device.maxY, width: device.width, height: device.height).integral.intersection(frame)
        }
        let margin = shape.style.lineWidth * 2 + 16
        let area = placed(ShapeGeometry.bounds(shape).insetBy(dx: -margin, dy: -margin))
        let clip = placed(canvas)
        guard !area.isNull, !clip.isNull, area.width >= 1, area.height >= 1 else { return }
        let sigma = 2.5 * zoom
        let reduction = sigma >= 1.5 ? 2 : 1
        let width = (Int(area.width) + reduction - 1) / reduction
        let rows = (Int(area.height) + reduction - 1) / reduction
        let count = width * rows
        let offset = (width * 4 + 15) & ~15
        guard let memory = buffer(offset + count * 6) else { return }
        let pixels = memory + offset
        _ = memset(pixels, 0, count * 4)
        guard let layer = CGContext(
            data: pixels, width: width, height: rows, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: context.colorSpace ?? Pixels.sRGB, bitmapInfo: Pixels.bitmapInfo
        ) else { return }
        let shrink = 1 / CGFloat(reduction)
        layer.scaleBy(x: shrink, y: shrink)
        layer.translateBy(x: -area.minX, y: -(height - area.minY - CGFloat(rows * reduction)))
        layer.concatenate(ctm)
        layer.setLineCap(.round)
        layer.setLineJoin(.round)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: layer, flipped: true)
        body(layer)
        NSGraphicsContext.restoreGraphicsState()
        let alpha = pixels.bindMemory(to: UInt8.self, capacity: count * 6)
        let coverage = alpha + count * 4
        for index in 0..<count { coverage[index] = alpha[index * 4 + 3] }
        SoftShadow.blur(
            coverage, scratch: coverage + count, sums: memory.bindMemory(to: UInt32.self, capacity: width),
            width: width, height: rows, sigma: sigma * shrink
        )
        SoftShadow.composite(
            coverage: coverage, width: width, height: rows, reduction: reduction,
            into: target, stride: context.bytesPerRow, clip: clip,
            originX: Int((area.minX + zoom).rounded()), originY: Int((area.minY + 2 * zoom).rounded())
        )
    }

    private func buffer(_ bytes: Int) -> UnsafeMutableRawPointer? {
        if (scratch?.capacity ?? 0) < bytes {
            var capacity = 1 << 20
            while capacity < bytes { capacity <<= 1 }
            scratch = MappedScratch(capacity: capacity)
        }
        return scratch?.pointer
    }

    private func paint(_ path: CGPath, style: ShapeStyle, in context: CGContext) {
        if style.fill.isVisible {
            context.addPath(path)
            context.setFillColor(style.fill.cgColor)
            context.fillPath()
        }
        if style.lineWidth > 0, style.stroke.isVisible {
            stroke(path, color: style.stroke, width: style.lineWidth, in: context)
        }
    }

    private func stroke(_ path: CGPath, color: RGBA, width: CGFloat, in context: CGContext) {
        context.addPath(path)
        context.setStrokeColor(color.cgColor)
        context.setLineWidth(width)
        context.strokePath()
    }

    private func drawText(_ shape: Shape) {
        let rect = TextMetrics.textRect(shape)
        let text = shape.text as NSString
        let attributes = TextMetrics.attributes(shape)
        if shape.kind == .textOutline, shape.style.lineWidth > 0 {
            var outline = attributes
            outline[.strokeColor] = shape.style.stroke.nsColor
            outline[.strokeWidth] = shape.style.lineWidth * 2 / shape.style.fontSize * 100
            text.draw(with: rect, options: [.usesLineFragmentOrigin], attributes: outline)
        }
        text.draw(with: rect, options: [.usesLineFragmentOrigin], attributes: attributes)
    }

    static func placement(of rect: CGRect, width: Int, height: Int, scale: CGFloat) -> (pixels: CGRect, rect: CGRect)? {
        let pixelRect = CGRect(x: rect.minX * scale, y: rect.minY * scale, width: rect.width * scale, height: rect.height * scale)
            .integral.intersection(CGRect(x: 0, y: 0, width: width, height: height))
        guard !pixelRect.isNull, !pixelRect.isEmpty else { return nil }
        return (pixelRect, CGRect(x: pixelRect.minX / scale, y: pixelRect.minY / scale, width: pixelRect.width / scale, height: pixelRect.height / scale))
    }

    static func pixels(of rect: CGRect, in base: Pixels, scale: CGFloat) -> (pixels: Pixels, rect: CGRect)? {
        guard let placement = placement(of: rect, width: base.width, height: base.height, scale: scale),
              let crop = base.cropped(to: placement.pixels)
        else { return nil }
        return (crop, placement.rect)
    }

    private func drawImage(_ image: CGImage, in rect: CGRect, context: CGContext) {
        context.saveGState()
        context.translateBy(x: rect.minX, y: rect.maxY)
        context.scaleBy(x: 1, y: -1)
        context.draw(image, in: CGRect(origin: .zero, size: rect.size))
        context.restoreGState()
    }
}

@MainActor
private final class EffectCache {
    private struct Key: Equatable {
        let kind: ShapeKind
        let placement: CGRect
        let strength: CGFloat
        let base: ObjectIdentifier
    }

    private static let context = CIContext(options: [.cacheIntermediates: false])
    private var entries: [UUID: (key: Key, image: CGImage)] = [:]

    func image(for shape: Shape, source: BaseImage, scale: CGFloat) -> (image: CGImage, rect: CGRect)? {
        guard let area = ShapeRenderer.placement(of: shape.rect, width: source.width, height: source.height, scale: scale) else { return nil }
        let placement = area.rect
        let key = Key(kind: shape.kind, placement: placement, strength: shape.style.strength, base: ObjectIdentifier(source))
        if let entry = entries[shape.id], entry.key == key { return (entry.image, placement) }

        guard let base = source.current, let crop = base.cropped(to: area.pixels)?.image else { return nil }
        let input = CIImage(cgImage: crop)
        let output: CIImage?
        switch shape.kind {
        case .blur:
            output = input.clampedToExtent().applyingGaussianBlur(sigma: Double(shape.style.strength * scale / 2)).cropped(to: input.extent)
        case .pixelate:
            let filter = CIFilter.pixellate()
            filter.inputImage = input.clampedToExtent()
            filter.center = .zero
            filter.scale = Float(max(1, shape.style.strength * scale))
            output = filter.outputImage?.cropped(to: input.extent)
        default:
            output = nil
        }
        guard let output,
              let image = Self.context.createCGImage(
                  output, from: input.extent, format: .BGRA8,
                  colorSpace: base.colorSpace
              )
        else { return nil }
        entries[shape.id] = (key, image)
        return (image, placement)
    }

    func clear() {
        entries.removeAll()
    }

    func prune(keeping shapes: [Shape]) {
        guard entries.count > 0 else { return }
        let alive = Set(shapes.lazy.filter { $0.kind == .blur || $0.kind == .pixelate }.map(\.id))
        if entries.keys.contains(where: { !alive.contains($0) }) {
            entries = entries.filter { alive.contains($0.key) }
        }
    }
}
