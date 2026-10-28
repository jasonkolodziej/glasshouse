import Foundation

/// APCA (Accessible Perceptual Contrast Algorithm), SAPC-APCA 0.0.98G-4g constants.
/// Returns Lc in roughly [-108, 106]; sign encodes polarity (positive = dark text on light).
/// Glasshouse thresholds (design critique): default foreground ≥ Lc 75, ANSI palette ≥ Lc 60,
/// both raised under Increase Contrast.
public enum APCA {
    public struct RGB: Equatable, Sendable {
        public var r: Double, g: Double, b: Double   // sRGB, 0…1
        public init(_ r: Double, _ g: Double, _ b: Double) { self.r = r; self.g = g; self.b = b }
    }

    private static let mainTRC = 2.4
    private static let sRco = 0.2126729
    private static let sGco = 0.7151522
    private static let sBco = 0.0721750
    private static let normBG = 0.56
    private static let normTXT = 0.57
    private static let revTXT = 0.62
    private static let revBG = 0.65
    private static let blkThrs = 0.022
    private static let blkClmp = 1.414
    private static let scaleBoW = 1.14
    private static let scaleWoB = 1.14
    private static let loBoWoffset = 0.027
    private static let loWoBoffset = 0.027
    private static let deltaYmin = 0.0005
    private static let loClip = 0.1

    static func luminance(_ c: RGB) -> Double {
        sRco * pow(c.r, mainTRC) + sGco * pow(c.g, mainTRC) + sBco * pow(c.b, mainTRC)
    }

    private static func softClamp(_ y: Double) -> Double {
        y > blkThrs ? y : y + pow(blkThrs - y, blkClmp)
    }

    public static func contrast(text: RGB, background: RGB) -> Double {
        let yt = softClamp(luminance(text))
        let yb = softClamp(luminance(background))
        guard abs(yb - yt) >= deltaYmin else { return 0 }

        let sapc: Double
        if yb > yt { // dark text on light background
            sapc = (pow(yb, normBG) - pow(yt, normTXT)) * scaleBoW
            return sapc < loClip ? 0 : (sapc - loBoWoffset) * 100
        } else {     // light text on dark background
            sapc = (pow(yb, revBG) - pow(yt, revTXT)) * scaleWoB
            return sapc > -loClip ? 0 : (sapc + loWoBoffset) * 100
        }
    }

    /// Nudges `text` toward white (on dark bg) or black (on light bg) until |Lc| ≥ minimum.
    /// Hue is preserved as long as possible by mixing rather than replacing — the same order Zed uses
    /// (lightness first, black/white as last resort).
    public static func enforce(minimum: Double, text: RGB, background: RGB) -> RGB {
        if abs(contrast(text: text, background: background)) >= minimum { return text }
        let darkBackground = luminance(background) < 0.18
        let target = darkBackground ? RGB(1, 1, 1) : RGB(0, 0, 0)
        var t = 0.0
        var candidate = text
        while t < 1 {
            t += 0.05
            candidate = RGB(text.r + (target.r - text.r) * t,
                            text.g + (target.g - text.g) * t,
                            text.b + (target.b - text.b) * t)
            if abs(contrast(text: candidate, background: background)) >= minimum { return candidate }
        }
        return target
    }
}

public struct ContrastPolicy: Sendable, Equatable {
    public var foregroundMinimum: Double
    public var paletteMinimum: Double

    public static let standard = ContrastPolicy(foregroundMinimum: 75, paletteMinimum: 60)
    /// Opt-out for users who want exact theme colors.
    public static let themeFaithful = ContrastPolicy(foregroundMinimum: 75, paletteMinimum: 45)
    public static let increased = ContrastPolicy(foregroundMinimum: 90, paletteMinimum: 75)
}
