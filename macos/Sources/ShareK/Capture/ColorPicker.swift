import AppKit

@MainActor
enum ColorPicker {
    private static var sampler: NSColorSampler?

    static func pick() {
        guard sampler == nil else { return }
        let sampler = NSColorSampler()
        self.sampler = sampler
        sampler.show { color in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    Self.sampler = nil
                    if let color { copy(color) }
                }
            }
        }
    }

    static func copy(_ color: NSColor) {
        guard let srgb = color.usingColorSpace(.sRGB) else { return }
        let description = Self.describe(srgb)
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(description.hex, forType: .string)
        Toast.show("Color copiado: \(description.hex)", detail: description.detail)
    }

    static func describe(_ color: NSColor) -> (hex: String, detail: String) {
        let red = Int((color.redComponent * 255).rounded())
        let green = Int((color.greenComponent * 255).rounded())
        let blue = Int((color.blueComponent * 255).rounded())
        let hex = String(format: "#%02X%02X%02X", red, green, blue)
        let hue = Int((color.hueComponent * 360).rounded()) % 360
        let saturation = Int((color.saturationComponent * 100).rounded())
        let brightness = Int((color.brightnessComponent * 100).rounded())
        return (hex, "RGB \(red), \(green), \(blue) · HSB \(hue)°, \(saturation) %, \(brightness) %")
    }
}
