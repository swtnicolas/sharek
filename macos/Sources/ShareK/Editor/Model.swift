import AppKit

struct RGBA: Codable, Equatable, Sendable {
    var r: CGFloat
    var g: CGFloat
    var b: CGFloat
    var a: CGFloat

    static let clear = RGBA(r: 0, g: 0, b: 0, a: 0)
    static let white = RGBA(r: 1, g: 1, b: 1, a: 1)
    static let black = RGBA(r: 0, g: 0, b: 0, a: 1)
    static let red = RGBA(r: 242 / 255, g: 60 / 255, b: 60 / 255, a: 1)
    static let yellow = RGBA(r: 1, g: 1, b: 0, a: 1)
    static let gray = RGBA(r: 142 / 255, g: 142 / 255, b: 147 / 255, a: 1)

    var cgColor: CGColor { CGColor(srgbRed: r, green: g, blue: b, alpha: a) }
    var nsColor: NSColor { NSColor(srgbRed: r, green: g, blue: b, alpha: a) }
    var isVisible: Bool { a > 0.001 }
}

extension RGBA {
    init(_ color: NSColor) {
        let srgb = color.usingColorSpace(.sRGB) ?? .black
        self.init(r: srgb.redComponent, g: srgb.greenComponent, b: srgb.blueComponent, a: srgb.alphaComponent)
    }
}

struct ShapeStyle: Codable, Equatable, Sendable {
    var stroke: RGBA
    var fill: RGBA
    var text: RGBA
    var lineWidth: CGFloat
    var cornerRadius: CGFloat
    var fontSize: CGFloat
    var strength: CGFloat
    var shadow: Bool
}

struct StyleOptions: OptionSet {
    let rawValue: Int
    static let stroke = StyleOptions(rawValue: 1 << 0)
    static let fill = StyleOptions(rawValue: 1 << 1)
    static let text = StyleOptions(rawValue: 1 << 2)
    static let lineWidth = StyleOptions(rawValue: 1 << 3)
    static let cornerRadius = StyleOptions(rawValue: 1 << 4)
    static let fontSize = StyleOptions(rawValue: 1 << 5)
    static let strength = StyleOptions(rawValue: 1 << 6)
    static let shadow = StyleOptions(rawValue: 1 << 7)
}

enum ShapeKind: String, Codable, CaseIterable, Sendable {
    case rectangle, ellipse, freehand, line, arrow
    case textOutline, textBackground, speechBalloon, step
    case magnify, smartEraser, blur, pixelate, highlight, spotlight
    case image

    var isText: Bool { self == .textOutline || self == .textBackground || self == .speechBalloon }
    var isEffect: Bool { self == .blur || self == .pixelate || self == .highlight }

    var options: StyleOptions {
        switch self {
        case .rectangle: [.stroke, .fill, .lineWidth, .cornerRadius, .shadow]
        case .ellipse: [.stroke, .fill, .lineWidth, .shadow]
        case .freehand, .line, .arrow: [.stroke, .lineWidth, .shadow]
        case .textOutline: [.text, .stroke, .lineWidth, .fontSize, .shadow]
        case .textBackground, .speechBalloon: [.text, .fill, .stroke, .lineWidth, .cornerRadius, .fontSize, .shadow]
        case .step: [.text, .fill, .stroke, .lineWidth, .fontSize, .shadow]
        case .magnify: [.stroke, .lineWidth, .strength, .shadow]
        case .smartEraser: []
        case .blur, .pixelate, .spotlight: [.strength]
        case .highlight: [.fill]
        case .image: [.stroke, .lineWidth, .shadow]
        }
    }

    var strokeLabel: String {
        switch self {
        case .freehand, .line, .arrow: "Color"
        case .textOutline: "Contorno"
        default: "Borde"
        }
    }

    var fillLabel: String { self == .highlight ? "Color" : "Relleno" }

    var strengthLabel: String {
        switch self {
        case .blur: "Radio"
        case .pixelate: "Píxel"
        case .magnify: "Zoom"
        case .spotlight: "Oscurecer"
        default: ""
        }
    }

    var strengthChoices: [CGFloat] {
        switch self {
        case .blur: [3, 5, 8, 10, 15, 20, 30, 40]
        case .pixelate: [3, 5, 7, 10, 14, 20, 30]
        case .magnify: [1.5, 2, 2.5, 3, 4, 5]
        case .spotlight: [0.3, 0.45, 0.6, 0.75, 0.9]
        default: []
        }
    }

    func strengthTitle(_ value: CGFloat) -> String {
        switch self {
        case .magnify: "\(value.formatted())×"
        case .spotlight: "\(Int((value * 100).rounded())) %"
        default: "\(value.formatted()) px"
        }
    }

    var defaultStyle: ShapeStyle {
        var style = ShapeStyle(
            stroke: .red, fill: .clear, text: .white, lineWidth: 2,
            cornerRadius: 0, fontSize: 14, strength: 0, shadow: true
        )
        switch self {
        case .arrow:
            style.lineWidth = 3
        case .textOutline:
            style.text = .black
            style.stroke = .white
            style.fontSize = 16
            style.shadow = false
        case .textBackground:
            style.text = .black
            style.fill = .white
            style.stroke = .gray
            style.lineWidth = 0
            style.cornerRadius = 4
        case .speechBalloon:
            style.text = .black
            style.fill = .white
            style.stroke = .gray
            style.cornerRadius = 6
        case .step:
            style.fill = .red
            style.stroke = .white
            style.fontSize = 12
        case .magnify:
            style.strength = 2
        case .smartEraser:
            style.shadow = false
        case .blur:
            style.strength = 15
            style.shadow = false
        case .pixelate:
            style.strength = 10
            style.shadow = false
        case .highlight:
            style.fill = .yellow
            style.shadow = false
        case .spotlight:
            style.strength = 0.6
            style.shadow = false
        case .image:
            style.lineWidth = 0
        default:
            break
        }
        return style
    }
}

struct Shape: Identifiable {
    let id: UUID
    var kind: ShapeKind
    var style: ShapeStyle
    var start: CGPoint
    var end: CGPoint
    var points: [CGPoint] = []
    var text = ""
    var tail: CGPoint = .zero
    var image: CGImage?

    init(kind: ShapeKind, style: ShapeStyle, at point: CGPoint) {
        id = UUID()
        self.kind = kind
        self.style = style
        start = point
        end = point
        if kind == .freehand { points = [point] }
    }

    var rect: CGRect {
        CGRect(x: min(start.x, end.x), y: min(start.y, end.y), width: abs(end.x - start.x), height: abs(end.y - start.y))
    }

    func duplicated(offset: CGFloat) -> Shape {
        var copy = Shape(kind: kind, style: style, at: start)
        copy.end = end
        copy.points = points
        copy.text = text
        copy.tail = tail
        copy.image = image
        copy.map { CGPoint(x: $0.x + offset, y: $0.y + offset) }
        return copy
    }

    mutating func map(_ transform: (CGPoint) -> CGPoint) {
        start = transform(start)
        end = transform(end)
        tail = transform(tail)
        points = points.map(transform)
    }
}

struct EditorState {
    let source: BaseImage
    var shapes: [Shape]

    @MainActor
    init(base: Pixels, shapes: [Shape]) {
        source = BaseImage(base)
        self.shapes = shapes
    }

    init(source: BaseImage, shapes: [Shape]) {
        self.source = source
        self.shapes = shapes
    }
}

enum Tool: String, CaseIterable {
    case select, rectangle, ellipse, freehand, line, arrow
    case textOutline, textBackground, speechBalloon, step
    case magnify, smartEraser, blur, pixelate, highlight, spotlight
    case crop, cutOut

    var kind: ShapeKind? { ShapeKind(rawValue: rawValue) }

    static let sections: [(title: String, tools: [Tool])] = [
        ("Editar", [.select, .crop, .cutOut, .smartEraser]),
        ("Dibujar", [.rectangle, .ellipse, .line, .arrow, .freehand, .highlight]),
        ("Texto", [.textOutline, .textBackground, .speechBalloon, .step]),
        ("Efectos", [.blur, .pixelate, .spotlight, .magnify]),
    ]

    var title: String {
        switch self {
        case .select: "Seleccionar y mover"
        case .rectangle: "Rectángulo"
        case .ellipse: "Elipse"
        case .freehand: "Mano alzada"
        case .line: "Línea"
        case .arrow: "Flecha"
        case .textOutline: "Texto con contorno"
        case .textBackground: "Texto con fondo"
        case .speechBalloon: "Globo de diálogo"
        case .step: "Numeración por pasos"
        case .magnify: "Lupa"
        case .smartEraser: "Borrador inteligente"
        case .blur: "Desenfoque"
        case .pixelate: "Pixelado"
        case .highlight: "Resaltador"
        case .spotlight: "Foco"
        case .crop: "Recortar"
        case .cutOut: "Quitar franja"
        }
    }

    var help: String {
        switch self {
        case .select: "Haz clic en una forma para seleccionarla y arrástrala para moverla."
        case .rectangle: "Arrastra para dibujar un rectángulo. Con ⇧, un cuadrado."
        case .ellipse: "Arrastra para dibujar una elipse. Con ⇧, un círculo."
        case .freehand: "Dibuja un trazo libre siguiendo el puntero."
        case .line: "Arrastra para trazar una línea. Con ⇧, en ángulos fijos."
        case .arrow: "Arrastra para señalar algo con una flecha. Con ⇧, en ángulos fijos."
        case .textOutline: "Texto con contorno, legible sobre cualquier fondo."
        case .textBackground: "Texto sobre un recuadro de color."
        case .speechBalloon: "Texto dentro de un globo de diálogo."
        case .step: "Círculos numerados 1, 2, 3… para marcar pasos."
        case .magnify: "Muestra ampliada la zona que encierras."
        case .smartEraser: "Tapa un área con el color del punto donde empiezas a arrastrar."
        case .blur: "Desenfoca un área para ocultar información."
        case .pixelate: "Pixela un área para ocultar información."
        case .highlight: "Resalta un área como un marcador fluorescente."
        case .spotlight: "Oscurece todo menos el óvalo que dibujas."
        case .crop: "Recorta la imagen al área que elijas."
        case .cutOut: "Quita una franja horizontal o vertical y junta los bordes."
        }
    }

    var shortcut: Character {
        switch self {
        case .select: "m"
        case .rectangle: "r"
        case .ellipse: "e"
        case .freehand: "f"
        case .line: "l"
        case .arrow: "a"
        case .textOutline: "o"
        case .textBackground: "t"
        case .speechBalloon: "s"
        case .step: "i"
        case .magnify: "g"
        case .smartEraser: "y"
        case .blur: "b"
        case .pixelate: "p"
        case .highlight: "h"
        case .spotlight: "k"
        case .crop: "c"
        case .cutOut: "x"
        }
    }

    static func matching(_ character: Character) -> Tool? {
        allCases.first { $0.shortcut == character }
    }
}

enum StyleStore {
    static func style(for kind: ShapeKind) -> ShapeStyle {
        guard let data = UserDefaults.standard.data(forKey: "style.\(kind.rawValue)"),
              let style = try? JSONDecoder().decode(ShapeStyle.self, from: data)
        else { return kind.defaultStyle }
        return style
    }

    static func save(_ style: ShapeStyle, for kind: ShapeKind) {
        guard let data = try? JSONEncoder().encode(style) else { return }
        UserDefaults.standard.set(data, forKey: "style.\(kind.rawValue)")
        NotificationCenter.default.post(name: didChange, object: nil)
    }

    static let didChange = Notification.Name("ShareK.styleStoreDidChange")
}
