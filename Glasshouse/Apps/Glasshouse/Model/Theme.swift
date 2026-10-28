import AppKit
import DesignSystem
import TerminalCore

/// Default theme, with the APCA floor applied at load time. Because the grid is opaque,
/// the effective background is known exactly and the contrast check is deterministic.
enum Theme {
    @MainActor
    static func current() -> TerminalAppearance {
        let d = UserDefaults.standard
        let increased = NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        let reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        let policy: ContrastPolicy = increased ? .increased
            : (d.bool(forKey: "themeFaithfulColors") ? .themeFaithful : .standard)
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua

        let bg = dark ? rgb(0x16, 0x18, 0x1D) : rgb(0xFA, 0xFA, 0xF9)
        let fg = dark ? rgb(0xE6, 0xE6, 0xE3) : rgb(0x1E, 0x1F, 0x22)
        // Palette in the spirit of Terminal.app's "Basic", tuned for both appearances.
        let base: [APCA.RGB] = [
            rgb(0x1E, 0x1F, 0x22), rgb(0xD1, 0x3B, 0x3B), rgb(0x2E, 0x9E, 0x5B), rgb(0xC9, 0x8A, 0x10),
            rgb(0x2F, 0x6F, 0xD6), rgb(0xA2, 0x4B, 0xC9), rgb(0x1E, 0x96, 0xA8), rgb(0xC8, 0xC8, 0xC4),
            rgb(0x5C, 0x5F, 0x66), rgb(0xF0, 0x5A, 0x5A), rgb(0x4C, 0xC3, 0x7A), rgb(0xE8, 0xB3, 0x3B),
            rgb(0x5B, 0x92, 0xF0), rgb(0xC3, 0x77, 0xE8), rgb(0x3C, 0xC2, 0xD6), rgb(0xFF, 0xFF, 0xFF),
        ]
        // Black/white slots (0, 7, 8, 15) are exempt: they are *meant* to match or invert the background.
        let exempt: Set<Int> = [0, 7, 8, 15]
        let ansi = base.enumerated().map { i, c in
            exempt.contains(i) ? c : APCA.enforce(minimum: policy.paletteMinimum, text: c, background: bg)
        }
        let foreground = APCA.enforce(minimum: policy.foregroundMinimum, text: fg, background: bg)

        // Translucency is opt-in and forced off by the accessibility settings.
        let translucent = d.bool(forKey: "translucentGrid") && !reduceTransparency && !increased
        let size = d.double(forKey: "fontSize")

        return TerminalAppearance(
            fontName: "SF Mono", fontSize: size > 0 ? size : 13,
            foreground: ns(foreground), background: ns(bg), cursor: .controlAccentColor,
            backgroundOpacity: translucent ? 0.82 : 1,
            ansi: ansi.map(ns))
    }

    private static func rgb(_ r: Int, _ g: Int, _ b: Int) -> APCA.RGB {
        APCA.RGB(Double(r) / 255, Double(g) / 255, Double(b) / 255)
    }
    private static func ns(_ c: APCA.RGB) -> NSColor {
        NSColor(srgbRed: c.r, green: c.g, blue: c.b, alpha: 1)
    }
}
