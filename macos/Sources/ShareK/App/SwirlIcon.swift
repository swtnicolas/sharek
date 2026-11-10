import AppKit

enum SwirlIcon {
    private static let outer = CGPoint(x: -26.13, y: -27.54)
    private static let radius: CGFloat = 92
    private static let innerOffset = CGVector(dx: 14, dy: 56)
    private static let innerRadius: CGFloat = 72
    private static let extent: CGFloat = 240
    private static let colors: [CGColor] = [0xFF3D7F, 0xFF9F1C, 0x2EC4B6, 0x5B5BFF].map { (hex: Int) -> CGColor in
        let red = CGFloat((hex >> 16) & 255) / 255
        let green = CGFloat((hex >> 8) & 255) / 255
        let blue = CGFloat(hex & 255) / 255
        return CGColor(srgbRed: red, green: green, blue: blue, alpha: 1)
    }

    static func blades(in rect: CGRect, gap: CGFloat, tipRounding: CGFloat) -> [CGPath] {
        let scale = min(rect.width, rect.height) / extent
        let center = CGPoint(x: rect.midX, y: rect.midY)
        func place(_ p: CGPoint, _ k: Int) -> CGPoint {
            var d = p
            for _ in 0..<k { d = CGPoint(x: -d.y, y: d.x) }
            return CGPoint(x: center.x + d.x * scale, y: center.y + d.y * scale)
        }
        func circle(_ c: CGPoint, _ r: CGFloat) -> CGPath {
            CGPath(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r), transform: nil)
        }
        func dilate(_ path: CGPath, _ amount: CGFloat) -> CGPath {
            path.union(path.copy(strokingWithWidth: 2 * amount, lineCap: .round, lineJoin: .round, miterLimit: 1))
        }
        let crescents = (0..<4).map { k -> CGPath in
            let inner = CGPoint(x: outer.x + innerOffset.dx, y: outer.y + innerOffset.dy)
            let raw = circle(place(outer, k), radius * scale).subtracting(circle(place(inner, k), innerRadius * scale))
            let e = tipRounding * scale
            return dilate(raw.subtracting(raw.copy(strokingWithWidth: 2 * e, lineCap: .round, lineJoin: .round, miterLimit: 1)), e)
        }
        return (0..<4).map { k in crescents[k].subtracting(dilate(crescents[(k + 3) % 4], gap * scale)) }
    }

    static func menuBarImage(size: CGFloat = 18) -> NSImage {
        let image = NSImage(size: CGSize(width: size, height: size), flipped: true) { rect in
            guard let context = NSGraphicsContext.current?.cgContext else { return false }
            for (blade, color) in zip(blades(in: rect.insetBy(dx: 0.5, dy: 0.5), gap: 6, tipRounding: 4), colors) {
                context.addPath(blade)
                context.setFillColor(color)
                context.fillPath()
            }
            return true
        }
        image.accessibilityDescription = "ShareK"
        return image
    }

    static func stopImage(size: CGFloat = 18) -> NSImage {
        let image = NSImage(size: CGSize(width: size, height: size), flipped: true) { rect in
            guard let context = NSGraphicsContext.current?.cgContext else { return false }
            let radius = size * 0.47
            let half = size * 0.16
            context.setFillColor(CGColor(srgbRed: 1, green: 59 / 255, blue: 48 / 255, alpha: 1))
            context.fillEllipse(in: CGRect(x: rect.midX - radius, y: rect.midY - radius, width: radius * 2, height: radius * 2))
            context.setFillColor(CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1))
            context.fill(CGRect(x: rect.midX - half, y: rect.midY - half, width: half * 2, height: half * 2))
            return true
        }
        image.accessibilityDescription = "Detener grabación"
        return image
    }
}
