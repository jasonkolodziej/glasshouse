import Foundation

/// An sRGB colour, written in theme files as `"#rrggbb"` (or `"#rrggbbaa"` when not opaque).
public struct ThemeColor: Hashable, Sendable, Codable, CustomStringConvertible {
    /// sRGB components, 0…1.
    public var red: Double
    public var green: Double
    public var blue: Double
    public var alpha: Double

    public init(red: Double, green: Double, blue: Double, alpha: Double = 1) {
        func clamp(_ v: Double) -> Double { min(max(v.isFinite ? v : 0, 0), 1) }
        self.red = clamp(red); self.green = clamp(green); self.blue = clamp(blue); self.alpha = clamp(alpha)
    }

    /// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the `#`.
    public init(hex: String) throws {
        var digits = hex.trimmingCharacters(in: .whitespaces)
        if digits.hasPrefix("#") { digits.removeFirst() }
        guard digits.allSatisfy(\.isHexDigit), [3, 6, 8].contains(digits.count) else {
            throw ThemeError.invalidColor(hex)
        }
        if digits.count == 3 { digits = digits.map { "\($0)\($0)" }.joined() }
        let value = UInt64(digits, radix: 16)!
        let byte = { (shift: UInt64) in Double((value >> shift) & 0xFF) / 255 }
        if digits.count == 8 {
            self.init(red: byte(24), green: byte(16), blue: byte(8), alpha: byte(0))
        } else {
            self.init(red: byte(16), green: byte(8), blue: byte(0))
        }
    }

    public var hex: String {
        let parts = [red, green, blue] + (alpha < 1 ? [alpha] : [])
        return "#" + parts.map { String(format: "%02x", Int(($0 * 255).rounded())) }.joined()
    }

    public var description: String { hex }

    public init(from decoder: Decoder) throws {
        try self.init(hex: decoder.singleValueContainer().decode(String.self))
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(hex)
    }

    /// WCAG 2 relative luminance.
    public var relativeLuminance: Double {
        func linear(_ c: Double) -> Double { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
        return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    /// WCAG 2 contrast ratio, 1…21.
    public func contrastRatio(with other: ThemeColor) -> Double {
        let (a, b) = (relativeLuminance, other.relativeLuminance)
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    public var isDark: Bool { relativeLuminance < 0.18 }

    /// `amount` of the way towards `other` (0 = self, 1 = other), in sRGB.
    public func mixed(with other: ThemeColor, amount: Double) -> ThemeColor {
        let t = min(max(amount, 0), 1)
        return ThemeColor(red: red + (other.red - red) * t, green: green + (other.green - green) * t,
                          blue: blue + (other.blue - blue) * t, alpha: alpha + (other.alpha - alpha) * t)
    }

    public static let black = ThemeColor(red: 0, green: 0, blue: 0)
    public static let white = ThemeColor(red: 1, green: 1, blue: 1)
}
