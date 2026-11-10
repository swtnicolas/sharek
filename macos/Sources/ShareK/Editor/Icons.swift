import AppKit

struct Icon: Equatable {
    let commands: String
    var box: CGFloat = 24
    var side: CGFloat = 22
    var lineWidth: CGFloat = 1.9

    static let copy = Icon(commands: "M 8 8 L 19 8 L 19 20 L 8 20 Z M 5 16 L 5 4 L 16 4")
    static let save = Icon(commands: "M 5 4 L 16 4 L 20 8 L 20 19 A 1 1 0 0 1 19 20 L 5 20 A 1 1 0 0 1 4 19 L 4 5 A 1 1 0 0 1 5 4 Z M 8 4 L 8 9 L 15 9 L 15 4 M 7 20 L 7 14 L 17 14 L 17 20")
    static let saveAs = Icon(commands: "M 11 20 L 5 20 A 1 1 0 0 1 4 19 L 4 5 A 1 1 0 0 1 5 4 L 15 4 L 19 8 L 19 10 M 8 4 L 8 8.5 L 14 8.5 L 14 4 M 7 20 L 7 14 L 12 14 M 13.5 21 L 14 18 L 19.5 12.5 L 22 15 L 16.5 20.5 Z")
    static let recognize = Icon(commands: "M 4 8 L 4 4 L 8 4 M 16 4 L 20 4 L 20 8 M 20 16 L 20 20 L 16 20 M 8 20 L 4 20 L 4 16 M 8 9 L 16 9 M 12 9 L 12 16")
    static let recognizeArea = Icon(commands: "M 4 7 L 4 4 L 7 4 M 10 4 L 14 4 M 17 4 L 20 4 L 20 7 M 20 10 L 20 14 M 20 17 L 20 20 L 17 20 M 14 20 L 10 20 M 7 20 L 4 20 L 4 17 M 4 14 L 4 10 M 9 9 L 15 9 M 12 9 L 12 16")
    static let pin = Icon(commands: "M 9 4 L 15 4 L 14 10 L 17 13 L 7 13 L 10 10 Z M 12 13 L 12 20")
    static let discard = Icon(commands: "M 5 7 L 19 7 M 9 7 L 9 4 L 15 4 L 15 7 M 7 7 L 8 20 L 16 20 L 17 7")
    static let play = Icon(commands: "M 8 5 L 19 12 L 8 19 Z")
    static let reveal = Icon(commands: "M 3 6 L 9 6 L 11 8 L 21 8 L 21 19 L 3 19 Z")
    static let undo = Icon(commands: "M 9 14 L 4 9 L 9 4 M 4 9 L 14 9 A 5 5 0 0 1 14 19 L 10 19")
    static let redo = Icon(commands: "M 15 14 L 20 9 L 15 4 M 20 9 L 10 9 A 5 5 0 0 0 10 19 L 14 19")
    static let rotate = Icon(commands: "M 20 12 A 8 8 0 1 1 16.5 5.4 M 17 2 L 17 6 L 13 6")
    static let flip = Icon(commands: "M 12 3 L 12 21 M 9 6 L 3 18 L 9 18 Z M 15 6 L 21 18 L 15 18 Z")
    static let frame = Icon(commands: "M 3 6 A 3 3 0 0 1 6 3 L 18 3 A 3 3 0 0 1 21 6 L 21 18 A 3 3 0 0 1 18 21 L 6 21 A 3 3 0 0 1 3 18 Z M 7 7 L 17 7 L 17 17 L 7 17 Z")
    static let eyedropper = Icon(commands: "M 13 6 L 18 11 M 16 4 A 2.5 2.5 0 0 1 20 8 L 17.5 10.5 L 13.5 6.5 Z M 15 9 L 7 17 L 5 19.5 L 4.5 19.5 L 4.5 19 L 7 17")
    static let fit = Icon(commands: "M 4 9 L 4 4 L 9 4 M 15 4 L 20 4 L 20 9 M 20 15 L 20 20 L 15 20 M 9 20 L 4 20 L 4 15")
    static let chevron = Icon(commands: "M 1.5 3.5 L 5 7 L 8.5 3.5", box: 10, side: 10, lineWidth: 1.6)
    static let playBadge = Icon(commands: "M 2 1 L 9 5 L 2 9 Z", box: 10, side: 10, lineWidth: 0)
    static let zoomOut = Icon(commands: "M 6 12 L 18 12", side: 16, lineWidth: 1.5)
    static let zoomIn = Icon(commands: "M 6 12 L 18 12 M 12 6 L 12 18", side: 16, lineWidth: 1.5)

    func sized(_ side: CGFloat, lineWidth: CGFloat) -> Icon {
        Icon(commands: commands, box: box, side: side, lineWidth: lineWidth)
    }
}

extension Tool {
    var icon: Icon {
        switch self {
        case .select: Icon(commands: "M 6 3 L 6 19 L 10 15 L 13 21 L 15.5 20 L 12.5 14 L 18 14 Z")
        case .rectangle: Icon(commands: "M 4 6 L 20 6 L 20 18 L 4 18 Z")
        case .ellipse: Icon(commands: "M 3 12 A 9 7 0 1 0 21 12 A 9 7 0 1 0 3 12 Z")
        case .freehand: Icon(commands: "M 3 17 C 6 8 9 8 10 13 C 11 18 14 18 16 10 C 17 7 19 6 21 7")
        case .line: Icon(commands: "M 5 19 L 19 5")
        case .arrow: Icon(commands: "M 5 19 L 19 5 M 11 5 L 19 5 L 19 13")
        case .textOutline: Icon(commands: "M 6 20 L 12 4 L 18 20 M 8.5 14 L 15.5 14")
        case .textBackground: Icon(commands: "M 3 5 L 21 5 L 21 19 L 3 19 Z M 8 9 L 16 9 M 12 9 L 12 16")
        case .speechBalloon: Icon(commands: "M 4 5 L 20 5 L 20 15 L 11 15 L 6 20 L 7 15 L 4 15 Z")
        case .step: Icon(commands: "M 3 12 A 9 9 0 1 0 21 12 A 9 9 0 1 0 3 12 Z M 10 9 L 12.5 7.5 L 12.5 16.5")
        case .magnify: Icon(commands: "M 4 10 A 6 6 0 1 0 16 10 A 6 6 0 1 0 4 10 Z M 14.5 14.5 L 20 20 M 10 7 L 10 13 M 7 10 L 13 10")
        case .smartEraser: Icon(commands: "M 3 16 L 12 7 L 18 13 L 11 20 L 6 20 Z M 7.5 11.5 L 13.5 17.5 M 11 20 L 21 20")
        case .blur: Icon(commands: "M 12 3 C 12 3 5 11 5 15 A 7 7 0 0 0 19 15 C 19 11 12 3 12 3 Z")
        case .pixelate: Icon(commands: "M 4 4 L 10 4 L 10 10 L 4 10 Z M 14 4 L 20 4 L 20 10 L 14 10 Z M 4 14 L 10 14 L 10 20 L 4 20 Z M 14 14 L 20 14 L 20 20 L 14 20 Z")
        case .highlight: Icon(commands: "M 14 4 L 20 10 L 11 19 L 5 19 L 5 13 Z M 9 9 L 15 15")
        case .spotlight: Icon(commands: "M 3 12 A 9 6 0 1 0 21 12 A 9 6 0 1 0 3 12 Z M 9 12 A 3 3 0 1 0 15 12 A 3 3 0 1 0 9 12 Z")
        case .crop: Icon(commands: "M 7 3 L 7 17 L 21 17 M 3 7 L 17 7 L 17 21")
        case .cutOut: Icon(commands: "M 3 12 L 21 12 M 12 3 L 12 8 M 9 5.5 L 12 8 L 15 5.5 M 12 21 L 12 16 M 9 18.5 L 12 16 L 15 18.5")
        }
    }
}

extension Icon {
    func draw(centeredIn bounds: CGRect, flipped: Bool, stroke: CGColor, fill: CGColor? = nil) {
        draw(at: CGPoint(x: bounds.midX - side / 2, y: bounds.midY - side / 2), flipped: flipped, stroke: stroke, fill: fill)
    }

    func outline(at origin: CGPoint, flipped: Bool) -> CGPath? {
        let scale = (side - lineWidth) / box
        let inset = lineWidth / 2
        var transform = flipped
            ? CGAffineTransform(a: scale, b: 0, c: 0, d: scale, tx: origin.x + inset, ty: origin.y + inset)
            : CGAffineTransform(a: scale, b: 0, c: 0, d: -scale, tx: origin.x + inset, ty: origin.y + side - inset)
        return IconPath.parse(commands).copy(using: &transform)
    }

    func draw(at origin: CGPoint, flipped: Bool, stroke: CGColor, fill: CGColor? = nil) {
        guard let context = NSGraphicsContext.current?.cgContext, let shape = outline(at: origin, flipped: flipped) else { return }
        context.saveGState()
        if let fill, fill.alpha > 0 {
            context.addPath(shape)
            context.setFillColor(fill)
            context.fillPath()
        }
        if lineWidth > 0, stroke.alpha > 0 {
            context.addPath(shape)
            context.setStrokeColor(stroke)
            context.setLineWidth(lineWidth)
            context.setLineCap(.butt)
            context.setLineJoin(.miter)
            context.setMiterLimit(4)
            context.strokePath()
        }
        context.restoreGState()
    }

    func image(stroke: CGColor, fill: CGColor? = nil) -> NSImage {
        let icon = self
        return NSImage(size: CGSize(width: side, height: side), flipped: true) { _ in
            icon.draw(at: .zero, flipped: true, stroke: stroke, fill: fill)
            return true
        }
    }
}

enum IconPath {
    static func parse(_ commands: String) -> CGPath {
        let path = CGMutablePath()
        var tokens = commands.split(whereSeparator: { $0 == " " || $0 == "," })[...]
        var command: Character = "M"
        var current = CGPoint.zero
        var start = CGPoint.zero

        func number() -> CGFloat? {
            guard let token = tokens.first, let value = Double(token) else { return nil }
            tokens = tokens.dropFirst()
            return CGFloat(value)
        }
        func point() -> CGPoint? {
            guard let x = number(), let y = number() else { return nil }
            return CGPoint(x: x, y: y)
        }

        while let token = tokens.first {
            if token.count == 1, let letter = token.first, letter.isLetter {
                command = letter
                tokens = tokens.dropFirst()
                if letter == "Z" {
                    path.closeSubpath()
                    current = start
                    continue
                }
            }
            switch command {
            case "M":
                guard let to = point() else { return path }
                path.move(to: to)
                current = to
                start = to
                command = "L"
            case "L":
                guard let to = point() else { return path }
                path.addLine(to: to)
                current = to
            case "C":
                guard let first = point(), let second = point(), let to = point() else { return path }
                path.addCurve(to: to, control1: first, control2: second)
                current = to
            case "A":
                guard let radii = point(), let rotation = number(), let large = number(), let sweep = number(), let to = point() else { return path }
                arc(path, from: current, radii: radii, rotation: rotation, large: large != 0, sweep: sweep != 0, to: to)
                current = to
            default:
                return path
            }
        }
        return path
    }

    private static func arc(_ path: CGMutablePath, from start: CGPoint, radii: CGPoint, rotation: CGFloat, large: Bool, sweep: Bool, to end: CGPoint) {
        guard start != end else { return }
        var rx = abs(radii.x)
        var ry = abs(radii.y)
        guard rx > 0, ry > 0 else {
            path.addLine(to: end)
            return
        }
        let phi = rotation * CGFloat.pi / 180
        let cosPhi = cos(phi)
        let sinPhi = sin(phi)
        let dx = (start.x - end.x) / 2
        let dy = (start.y - end.y) / 2
        let x1 = cosPhi * dx + sinPhi * dy
        let y1 = cosPhi * dy - sinPhi * dx
        let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry)
        if lambda > 1 {
            rx *= lambda.squareRoot()
            ry *= lambda.squareRoot()
        }
        let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1
        let sign: CGFloat = large == sweep ? -1 : 1
        let factor = sign * max(numerator / denominator, 0).squareRoot()
        let cx1 = factor * rx * y1 / ry
        let cy1 = -factor * ry * x1 / rx
        let centerX = cosPhi * cx1 - sinPhi * cy1 + (start.x + end.x) / 2
        let centerY = sinPhi * cx1 + cosPhi * cy1 + (start.y + end.y) / 2
        let first = atan2((y1 - cy1) / ry, (x1 - cx1) / rx)
        var delta = atan2((-y1 - cy1) / ry, (-x1 - cx1) / rx) - first
        if sweep, delta < 0 { delta += 2 * CGFloat.pi }
        if !sweep, delta > 0 { delta -= 2 * CGFloat.pi }
        let pieces = max(Int((abs(delta) / (CGFloat.pi / 2)).rounded(.up)), 1)
        let step = delta / CGFloat(pieces)
        let handle = 4 / 3 * tan(step / 4)

        func place(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: centerX + rx * x * cosPhi - ry * y * sinPhi, y: centerY + rx * x * sinPhi + ry * y * cosPhi)
        }

        for index in 0..<pieces {
            let from = first + step * CGFloat(index)
            let to = from + step
            let control1 = place(cos(from) - handle * sin(from), sin(from) + handle * cos(from))
            let control2 = place(cos(to) + handle * sin(to), sin(to) - handle * cos(to))
            path.addCurve(to: index == pieces - 1 ? end : place(cos(to), sin(to)), control1: control1, control2: control2)
        }
    }
}
