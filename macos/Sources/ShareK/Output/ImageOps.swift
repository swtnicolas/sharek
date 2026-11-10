import CoreGraphics
import Foundation

enum ImageOps {
    static func crop(_ image: CGImage, to pixelRect: CGRect) -> Pixels? {
        let rect = pixelRect.integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
        guard !rect.isNull, !rect.isEmpty, let cropped = image.cropping(to: rect) else { return nil }
        return Pixels.render(width: cropped.width, height: cropped.height, colorSpace: image.colorSpace) { context in
            context.draw(cropped, in: CGRect(x: 0, y: 0, width: cropped.width, height: cropped.height))
        }
    }

    static func cutOut(_ pixels: Pixels, removing range: Range<Int>, columns: Bool) -> Pixels? {
        let width = pixels.width
        let height = pixels.height
        let limit = columns ? width : height
        let lower = max(0, range.lowerBound)
        let upper = min(limit, range.upperBound)
        guard upper > lower, upper - lower < limit else { return nil }
        let removed = upper - lower
        let newWidth = columns ? width - removed : width
        let newHeight = columns ? height : height - removed
        let rowBytes = newWidth * 4
        let stride = pixels.format.bytesPerRow
        let source = pixels.data
        guard let output = MappedData.make(count: rowBytes * newHeight, { to in
            source.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> Bool in
                guard let from = raw.baseAddress else { return false }
                if columns {
                    for row in 0..<height {
                        let line = from + row * stride
                        let out = to + row * rowBytes
                        if lower > 0 { out.copyMemory(from: line, byteCount: lower * 4) }
                        if upper < width { (out + lower * 4).copyMemory(from: line + upper * 4, byteCount: (width - upper) * 4) }
                    }
                } else {
                    var next = 0
                    for row in 0..<height where row < lower || row >= upper {
                        (to + next * rowBytes).copyMemory(from: from + row * stride, byteCount: rowBytes)
                        next += 1
                    }
                }
                return true
            }
        }) else { return nil }
        return Pixels(format: pixels.format.with(width: newWidth, height: newHeight, bytesPerRow: rowBytes), data: output)
    }

    static func rotated(_ pixels: Pixels, clockwise: Bool) -> Pixels? {
        guard let image = pixels.image else { return nil }
        let width = pixels.width
        let height = pixels.height
        return Pixels.render(width: height, height: width, colorSpace: pixels.colorSpace) { context in
            if clockwise {
                context.translateBy(x: 0, y: CGFloat(width))
                context.rotate(by: -.pi / 2)
            } else {
                context.translateBy(x: CGFloat(height), y: 0)
                context.rotate(by: .pi / 2)
            }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
    }

    static func flipped(_ pixels: Pixels, horizontal: Bool) -> Pixels? {
        guard let image = pixels.image else { return nil }
        let width = pixels.width
        let height = pixels.height
        return Pixels.render(width: width, height: height, colorSpace: pixels.colorSpace) { context in
            if horizontal {
                context.translateBy(x: CGFloat(width), y: 0)
                context.scaleBy(x: -1, y: 1)
            } else {
                context.translateBy(x: 0, y: CGFloat(height))
                context.scaleBy(x: 1, y: -1)
            }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
    }

    static func framed(_ pixels: Pixels, scale: CGFloat, frame: ImageFrame) -> (pixels: Pixels, offset: CGFloat)? {
        guard let image = pixels.image else { return nil }
        let margin = (frame.effectiveMargin * scale).rounded()
        let width = Int(CGFloat(image.width) + 2 * margin)
        let height = Int(CGFloat(image.height) + 2 * margin)
        let result = Pixels.render(width: width, height: height, colorSpace: pixels.colorSpace) { context in
            context.setFillColor(frame.background.cgColor)
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            let rect = CGRect(x: margin, y: margin, width: CGFloat(image.width), height: CGFloat(image.height))
            let radius = min(frame.radius * scale, rect.width / 2, rect.height / 2)
            let outline = CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
            if frame.shadow {
                context.saveGState()
                context.setShadow(offset: CGSize(width: 0, height: -3 * scale), blur: 12 * scale, color: CGColor(gray: 0, alpha: 0.55))
                context.addPath(outline)
                context.setFillColor(CGColor(gray: 0, alpha: 1))
                context.fillPath()
                context.restoreGState()
            }
            context.saveGState()
            context.addPath(outline)
            context.clip()
            context.draw(image, in: rect)
            context.restoreGState()
            if frame.border > 0, frame.borderColor.isVisible {
                let line = frame.border * scale
                let inner = rect.insetBy(dx: line / 2, dy: line / 2)
                let innerRadius = max(0, radius - line / 2)
                context.addPath(CGPath(roundedRect: inner, cornerWidth: innerRadius, cornerHeight: innerRadius, transform: nil))
                context.setStrokeColor(frame.borderColor.cgColor)
                context.setLineWidth(line)
                context.strokePath()
            }
        }
        guard let result else { return nil }
        return (result, margin / scale)
    }
}
