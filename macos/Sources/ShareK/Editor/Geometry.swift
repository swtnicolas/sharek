import AppKit

enum Handle: Equatable {
    case corner(Int)
    case edge(Int)
    case start
    case end
    case tail
}

@MainActor
enum TextMetrics {
    static func font(_ shape: Shape) -> NSFont {
        .systemFont(ofSize: shape.style.fontSize, weight: shape.kind == .step ? .bold : .semibold)
    }

    static func attributes(_ shape: Shape) -> [NSAttributedString.Key: Any] {
        [.font: font(shape), .foregroundColor: shape.style.text.nsColor]
    }

    static func padding(_ shape: Shape) -> CGSize {
        if shape.kind == .textOutline {
            let inset = shape.style.lineWidth + 2
            return CGSize(width: inset, height: inset)
        }
        return CGSize(width: (shape.style.fontSize * 0.5).rounded(), height: (shape.style.fontSize * 0.3).rounded())
    }

    static func textSize(_ shape: Shape) -> CGSize {
        var string = shape.text.isEmpty ? " " : shape.text
        if string.hasSuffix("\n") { string += " " }
        let bounds = (string as NSString).boundingRect(
            with: CGSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin],
            attributes: attributes(shape)
        )
        return CGSize(width: ceil(bounds.width) + 2, height: ceil(bounds.height))
    }

    static func fit(_ shape: inout Shape) {
        let padding = padding(shape)
        let size = textSize(shape)
        shape.end = CGPoint(x: shape.start.x + size.width + padding.width * 2, y: shape.start.y + size.height + padding.height * 2)
    }

    static func textRect(_ shape: Shape) -> CGRect {
        let padding = padding(shape)
        return CGRect(origin: CGPoint(x: shape.start.x + padding.width, y: shape.start.y + padding.height), size: textSize(shape))
    }
}

@MainActor
enum ShapeGeometry {
    static func stepRadius(_ style: ShapeStyle) -> CGFloat {
        (style.fontSize * 0.8 + 4).rounded()
    }

    static func bounds(_ shape: Shape) -> CGRect {
        switch shape.kind {
        case .freehand:
            guard let first = shape.points.first else { return .zero }
            return shape.points.reduce(CGRect(origin: first, size: .zero)) { $0.union(CGRect(origin: $1, size: .zero)) }
        case .step:
            let radius = stepRadius(shape.style)
            return CGRect(x: shape.start.x - radius, y: shape.start.y - radius, width: radius * 2, height: radius * 2)
        case .speechBalloon:
            return shape.rect.union(CGRect(origin: shape.tail, size: .zero))
        default:
            return shape.rect
        }
    }

    static func contains(_ shape: Shape, _ point: CGPoint, tolerance: CGFloat) -> Bool {
        let reach = shape.style.lineWidth / 2 + tolerance
        switch shape.kind {
        case .line, .arrow:
            return distance(point, shape.start, shape.end) <= reach
        case .freehand:
            guard shape.points.count > 1 else { return hypot(point.x - shape.start.x, point.y - shape.start.y) <= reach }
            return zip(shape.points, shape.points.dropFirst()).contains { distance(point, $0, $1) <= reach }
        case .ellipse:
            return CGPath(ellipseIn: shape.rect.insetBy(dx: -tolerance, dy: -tolerance), transform: nil).contains(point)
        case .step:
            return hypot(point.x - shape.start.x, point.y - shape.start.y) <= stepRadius(shape.style) + tolerance
        case .speechBalloon:
            return shape.rect.insetBy(dx: -tolerance, dy: -tolerance).contains(point)
                || hypot(point.x - shape.tail.x, point.y - shape.tail.y) <= tolerance * 2
        default:
            return shape.rect.insetBy(dx: -tolerance, dy: -tolerance).contains(point)
        }
    }

    static func handles(_ shape: Shape) -> [(Handle, CGPoint)] {
        switch shape.kind {
        case .line, .arrow:
            return [(.start, shape.start), (.end, shape.end)]
        case .speechBalloon:
            return [(.tail, shape.tail)]
        case .freehand, .step, .textOutline, .textBackground:
            return []
        default:
            return handles(for: shape.rect)
        }
    }

    static func handles(for rect: CGRect) -> [(Handle, CGPoint)] {
        [
            (.corner(0), CGPoint(x: rect.minX, y: rect.minY)),
            (.corner(1), CGPoint(x: rect.maxX, y: rect.minY)),
            (.corner(2), CGPoint(x: rect.maxX, y: rect.maxY)),
            (.corner(3), CGPoint(x: rect.minX, y: rect.maxY)),
            (.edge(0), CGPoint(x: rect.midX, y: rect.minY)),
            (.edge(1), CGPoint(x: rect.maxX, y: rect.midY)),
            (.edge(2), CGPoint(x: rect.midX, y: rect.maxY)),
            (.edge(3), CGPoint(x: rect.minX, y: rect.midY)),
        ]
    }

    static func resize(_ rect: CGRect, handle: Handle, to point: CGPoint) -> CGRect {
        var minX = rect.minX, maxX = rect.maxX, minY = rect.minY, maxY = rect.maxY
        switch handle {
        case .corner(0): minX = point.x; minY = point.y
        case .corner(1): maxX = point.x; minY = point.y
        case .corner(2): maxX = point.x; maxY = point.y
        case .corner(3): minX = point.x; maxY = point.y
        case .edge(0): minY = point.y
        case .edge(1): maxX = point.x
        case .edge(2): maxY = point.y
        case .edge(3): minX = point.x
        default: break
        }
        return CGRect(x: min(minX, maxX), y: min(minY, maxY), width: abs(maxX - minX), height: abs(maxY - minY))
    }

    static func apply(_ handle: Handle, point: CGPoint, to shape: inout Shape, constrained: Bool) {
        switch handle {
        case .start:
            shape.start = constrained ? snapAngle(point, from: shape.end) : point
        case .end:
            shape.end = constrained ? snapAngle(point, from: shape.start) : point
        case .tail:
            shape.tail = point
        case .corner, .edge:
            let rect = resize(shape.rect, handle: handle, to: point)
            shape.start = rect.origin
            shape.end = CGPoint(x: rect.maxX, y: rect.maxY)
        }
    }

    static func snapAngle(_ point: CGPoint, from origin: CGPoint) -> CGPoint {
        let dx = point.x - origin.x
        let dy = point.y - origin.y
        let length = hypot(dx, dy)
        let step = CGFloat.pi / 4
        let angle = (atan2(dy, dx) / step).rounded() * step
        return CGPoint(x: origin.x + cos(angle) * length, y: origin.y + sin(angle) * length)
    }

    static func square(_ point: CGPoint, from origin: CGPoint) -> CGPoint {
        let dx = point.x - origin.x
        let dy = point.y - origin.y
        let side = max(abs(dx), abs(dy))
        return CGPoint(x: origin.x + (dx < 0 ? -side : side), y: origin.y + (dy < 0 ? -side : side))
    }

    static func distance(_ point: CGPoint, _ a: CGPoint, _ b: CGPoint) -> CGFloat {
        let dx = b.x - a.x
        let dy = b.y - a.y
        let lengthSquared = dx * dx + dy * dy
        guard lengthSquared > 0 else { return hypot(point.x - a.x, point.y - a.y) }
        let t = max(0, min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / lengthSquared))
        return hypot(point.x - (a.x + t * dx), point.y - (a.y + t * dy))
    }

    static func balloonPath(_ shape: Shape) -> CGPath {
        let rect = shape.rect
        let radius = min(shape.style.cornerRadius, rect.width / 2, rect.height / 2)
        let body = CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)
        guard !rect.contains(shape.tail) else { return body }
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let dx = shape.tail.x - center.x
        let dy = shape.tail.y - center.y
        let length = hypot(dx, dy)
        guard length > 0 else { return body }
        let halfBase = max(6, min(rect.width, rect.height) * 0.22)
        let perpendicular = CGPoint(x: -dy / length * halfBase, y: dx / length * halfBase)
        let tail = CGMutablePath()
        tail.move(to: CGPoint(x: center.x + perpendicular.x, y: center.y + perpendicular.y))
        tail.addLine(to: shape.tail)
        tail.addLine(to: CGPoint(x: center.x - perpendicular.x, y: center.y - perpendicular.y))
        tail.closeSubpath()
        return body.union(tail)
    }

    static func arrowPaths(_ shape: Shape) -> (shaft: CGPath, head: CGPath)? {
        let dx = shape.end.x - shape.start.x
        let dy = shape.end.y - shape.start.y
        let length = hypot(dx, dy)
        guard length > 1 else { return nil }
        let ux = dx / length
        let uy = dy / length
        let width = shape.style.lineWidth
        let headLength = min(max(width * 3.5, 10), length)
        let halfHead = max(width * 1.6, 5)
        let base = CGPoint(x: shape.end.x - ux * headLength, y: shape.end.y - uy * headLength)

        let shaft = CGMutablePath()
        shaft.move(to: shape.start)
        shaft.addLine(to: CGPoint(x: shape.end.x - ux * headLength * 0.6, y: shape.end.y - uy * headLength * 0.6))

        let head = CGMutablePath()
        head.move(to: shape.end)
        head.addLine(to: CGPoint(x: base.x - uy * halfHead, y: base.y + ux * halfHead))
        head.addLine(to: CGPoint(x: base.x + uy * halfHead, y: base.y - ux * halfHead))
        head.closeSubpath()
        return (shaft, head)
    }
}
