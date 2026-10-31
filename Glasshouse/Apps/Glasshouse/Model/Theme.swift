import AppKit
import DesignSystem
import Observation
import SwiftUI
import TerminalCore
import TerminalTheme

/// The themes and `appearance.yaml`, shared by Settings, the theme editor and every terminal session.
/// Changes are saved straight away and announced with `didChange`, which open sessions listen for.
@MainActor
@Observable
final class ThemeLibrary {
    static let shared = ThemeLibrary()
    static let didChange = Notification.Name("GlasshouseAppearanceDidChange")

    let store: ThemeStore
    private(set) var themes: [TerminalTheme] = []
    /// Theme files in the themes folder that couldn't be read, with the reason.
    private(set) var problems: [(file: String, problem: String)] = []
    private(set) var config = AppearanceConfig()

    init(store: ThemeStore = .standard()) {
        self.store = store
        migrateOldSettings()
        reload()
    }

    func reload() {
        themes = store.allThemes()
        problems = store.userThemeProblems()
        config = store.loadConfig()
    }

    func theme(id: String) -> TerminalTheme? { themes.first { $0.id == id } }

    func isBuiltIn(_ theme: TerminalTheme) -> Bool { ThemeStore.isBuiltIn(theme.id) }

    func resolved(dark: Bool) -> ResolvedAppearance {
        let chosen = theme(id: config.theme.themeID(dark: dark))
            ?? theme(id: dark ? "glasshouse-dark" : "glasshouse-light") ?? themes.first
        return chosen.map { ResolvedAppearance(theme: $0, config: config) } ?? store.resolved(dark: dark, config: config)
    }

    /// Changes `appearance.yaml`. An invalid change is not saved.
    func updateConfig(_ change: (inout AppearanceConfig) -> Void) throws {
        var updated = config
        change(&updated)
        try store.saveConfig(updated)
        config = updated
        notify()
    }

    @discardableResult
    func save(_ theme: TerminalTheme) throws -> TerminalTheme {
        let saved = try store.save(theme)
        reload()
        notify()
        return saved
    }

    /// A copy of `theme` that is yours to edit.
    func customize(_ theme: TerminalTheme) throws -> TerminalTheme {
        let base = theme.name.hasSuffix(" (Custom)") ? theme.name : "\(theme.name) (Custom)"
        let taken = Set(themes.map(\.name))
        let name = taken.contains(base) ? (2...).lazy.map { "\(base) \($0)" }.first { !taken.contains($0) }! : base
        let copy = try store.duplicate(theme, named: name)
        reload()
        return copy
    }

    func delete(_ theme: TerminalTheme) throws {
        try store.delete(id: theme.id)
        reload()
        notify()
    }

    @discardableResult
    func importTheme(from url: URL) throws -> TerminalTheme {
        let theme = try store.importTheme(from: url)
        reload()
        return theme
    }

    private func notify() {
        NotificationCenter.default.post(name: Self.didChange, object: nil)
    }

    /// Before themes, font size, translucency and theme-faithful colours lived in UserDefaults. Carry them over
    /// once, the first time there is no `appearance.yaml`.
    private func migrateOldSettings() {
        guard !FileManager.default.fileExists(atPath: store.configURL.path) else { return }
        let d = UserDefaults.standard
        var config = AppearanceConfig()
        if d.double(forKey: "fontSize") > 0 { config.font.size = d.double(forKey: "fontSize") }
        if d.bool(forKey: "translucentGrid") { config.window.opacity = 0.82 }
        if d.bool(forKey: "themeFaithfulColors") { config.contrast = .faithful }
        try? store.saveConfig(config)
    }
}

/// Builds the engine's `TerminalAppearance` from the chosen theme, with the APCA floor applied at load time.
/// Because the grid is opaque, the effective background is known exactly and the contrast check is deterministic.
enum Theme {
    @MainActor
    static func current() -> TerminalAppearance {
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return appearance(for: ThemeLibrary.shared.resolved(dark: dark))
    }

    @MainActor
    static func appearance(for resolved: ResolvedAppearance) -> TerminalAppearance {
        let increased = NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        let reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        let policy = contrastPolicy(resolved.contrast, increaseContrast: increased)
        let colors = resolved.theme.colors
        let bg = colors.background.apca

        // Black/white slots (0, 7, 8, 15) are exempt: they are *meant* to match or invert the background.
        let exempt: Set<Int> = [0, 7, 8, 15]
        let ansi = colors.ansi.enumerated().map { i, c in
            exempt.contains(i) ? c.apca : APCA.enforce(minimum: policy.paletteMinimum, text: c.apca, background: bg)
        }
        let foreground = APCA.enforce(minimum: policy.foregroundMinimum, text: colors.foreground.apca, background: bg)

        // Translucency is opt-in and forced off by the accessibility settings.
        let opacity = (reduceTransparency || increased) ? 1 : resolved.opacity

        return TerminalAppearance(
            fontName: resolved.fontFamily, fontSize: resolved.fontSize,
            foreground: ns(foreground), background: ns(bg),
            cursor: colors.cursor?.nsColor ?? .controlAccentColor,
            cursorText: colors.cursorText?.nsColor, selection: colors.selection?.nsColor,
            cursorShape: shape(resolved.cursorShape), cursorBlink: resolved.cursorBlink,
            boldIsBright: resolved.boldIsBright,
            backgroundOpacity: opacity,
            ansi: ansi.map(ns),
            paddingHorizontal: resolved.paddingHorizontal, paddingVertical: resolved.paddingVertical)
    }

    static func contrastPolicy(_ level: ContrastLevel, increaseContrast: Bool) -> ContrastPolicy {
        if increaseContrast { return .increased }
        switch level {
        case .standard: return .standard
        case .faithful: return .themeFaithful
        case .high: return .increased
        }
    }

    private static func shape(_ shape: CursorShape) -> TerminalCursorShape {
        switch shape {
        case .block: .block
        case .underline: .underline
        case .bar: .bar
        }
    }

    private static func ns(_ c: APCA.RGB) -> NSColor {
        NSColor(srgbRed: c.r, green: c.g, blue: c.b, alpha: 1)
    }
}

extension ThemeColor {
    var apca: APCA.RGB { APCA.RGB(red, green, blue) }
    var nsColor: NSColor { NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha) }
    var color: Color { Color(.sRGB, red: red, green: green, blue: blue, opacity: alpha) }
    var cgColor: CGColor { CGColor(srgbRed: red, green: green, blue: blue, alpha: alpha) }

    /// From a colour picker: converted to sRGB first, so a P3 pick is stored as its nearest sRGB colour.
    init(cgColor: CGColor) {
        let srgb = CGColorSpace(name: CGColorSpace.sRGB).flatMap {
            cgColor.converted(to: $0, intent: .defaultIntent, options: nil)
        } ?? cgColor
        let c = srgb.components ?? [0, 0, 0, 1]
        if c.count >= 3 {
            self.init(red: c[0], green: c[1], blue: c[2], alpha: c.count > 3 ? c[3] : 1)
        } else {
            self.init(red: c[0], green: c[0], blue: c[0], alpha: c.count > 1 ? c[1] : 1)
        }
    }
}
