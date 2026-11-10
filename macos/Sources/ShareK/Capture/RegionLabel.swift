import Foundation

enum RegionLabel {
    static let common: [(Int, Int)] = [(1, 1), (5, 4), (4, 3), (3, 2), (16, 10), (16, 9), (21, 9)]
        .flatMap { $0 == $1 ? [($0, $1)] : [($0, $1), ($1, $0)] }

    static func text(width: Int, height: Int, screenWidth: Int, screenHeight: Int, locale: Locale = .current) -> String {
        let parts = ["\(width) × \(height) px", ratio(width: width, height: height, locale: locale), share(width * height, of: screenWidth * screenHeight)]
        return " \(parts.compactMap { $0 }.joined(separator: " · ")) "
    }

    static func ratio(width: Int, height: Int, locale: Locale = .current) -> String? {
        guard width > 0, height > 0 else { return nil }
        if let exact = common.first(where: { width * $0.1 == height * $0.0 }) {
            return "\(exact.0):\(exact.1)"
        }
        let divisor = gcd(width, height)
        let terms = (width / divisor, height / divisor)
        if max(terms.0, terms.1) <= 10 {
            return "\(terms.0):\(terms.1)"
        }
        let value = Double(width) / Double(height)
        let nearest = common.min { abs(value / ratio(of: $0) - 1) < abs(value / ratio(of: $1) - 1) }!
        if abs(value / ratio(of: nearest) - 1) <= 0.01 {
            return "≈ \(nearest.0):\(nearest.1)"
        }
        let format = FloatingPointFormatStyle<Double>.number.precision(.fractionLength(2)).locale(locale)
        return width >= height ? "\(value.formatted(format)):1" : "1:\((1 / value).formatted(format))"
    }

    static func share(_ area: Int, of total: Int) -> String? {
        guard area > 0, total > 0 else { return nil }
        guard area < total else { return "100 %" }
        let percent = Int((Double(area) * 100 / Double(total)).rounded())
        return "\(min(max(percent, 1), 99)) %"
    }

    private static func ratio(of terms: (Int, Int)) -> Double {
        Double(terms.0) / Double(terms.1)
    }

    private static func gcd(_ a: Int, _ b: Int) -> Int {
        b == 0 ? a : gcd(b, a % b)
    }
}
