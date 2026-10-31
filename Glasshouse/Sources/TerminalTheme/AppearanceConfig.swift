import Foundation

/// `appearance.yaml`: which themes to use, and the settings every theme starts from.
///
/// ```yaml
/// schema: 1
/// theme:
///   dark: dracula
///   light: solarized-light
///   follow-system: true      # false: always use the dark theme
/// font: { family: SF Mono, size: 13, bold-is-bright: true }
/// cursor: { style: block, blink: false }
/// window: { padding-horizontal: 10, padding-vertical: 6, opacity: 1 }
/// contrast: standard
/// ```
public struct AppearanceConfig: Codable, Equatable, Sendable {
    public struct ThemeChoice: Codable, Equatable, Sendable {
        public var dark: String
        public var light: String
        /// Use `light` in Light Mode and `dark` in Dark Mode. When false, `dark` is used all the time.
        public var followSystem: Bool

        public init(dark: String = "glasshouse-dark", light: String = "glasshouse-light", followSystem: Bool = true) {
            self.dark = dark; self.light = light; self.followSystem = followSystem
        }

        private enum CodingKeys: String, CodingKey { case dark, light, followSystem = "follow-system" }

        public init(from decoder: Decoder) throws {
            let defaults = ThemeChoice()
            let c = try decoder.container(keyedBy: CodingKeys.self)
            dark = try c.decodeIfPresent(String.self, forKey: .dark) ?? defaults.dark
            light = try c.decodeIfPresent(String.self, forKey: .light) ?? defaults.light
            followSystem = try c.decodeIfPresent(Bool.self, forKey: .followSystem) ?? defaults.followSystem
        }

        public func themeID(dark isDark: Bool) -> String { followSystem && !isDark ? light : dark }
    }

    public var schema: Int
    public var theme: ThemeChoice
    public var font: FontSettings
    public var cursor: CursorSettings
    public var window: WindowSettings
    public var contrast: ContrastLevel

    public init(theme: ThemeChoice = ThemeChoice(), font: FontSettings = FontSettings(),
                cursor: CursorSettings = CursorSettings(), window: WindowSettings = WindowSettings(),
                contrast: ContrastLevel = .standard) {
        schema = TerminalTheme.currentSchema
        self.theme = theme; self.font = font; self.cursor = cursor; self.window = window; self.contrast = contrast
    }

    private enum CodingKeys: String, CodingKey { case schema, theme, font, cursor, window, contrast }

    /// Every key may be missing; a missing one keeps its default.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(theme: try c.decodeIfPresent(ThemeChoice.self, forKey: .theme) ?? ThemeChoice(),
                  font: try c.decodeIfPresent(FontSettings.self, forKey: .font) ?? FontSettings(),
                  cursor: try c.decodeIfPresent(CursorSettings.self, forKey: .cursor) ?? CursorSettings(),
                  window: try c.decodeIfPresent(WindowSettings.self, forKey: .window) ?? WindowSettings(),
                  contrast: try c.decodeIfPresent(ContrastLevel.self, forKey: .contrast) ?? .standard)
        schema = try c.decodeIfPresent(Int.self, forKey: .schema) ?? TerminalTheme.currentSchema
    }

    public func validated() throws -> AppearanceConfig {
        guard (1...TerminalTheme.currentSchema).contains(schema) else { throw ThemeError.unsupportedSchema(schema) }
        try font.validate()
        try window.validate()
        return self
    }
}

/// Everything the terminal needs, after a theme's overrides are laid over `AppearanceConfig` and both over the
/// built-in defaults.
public struct ResolvedAppearance: Equatable, Sendable {
    public var theme: TerminalTheme
    public var fontFamily: String
    public var fontSize: Double
    public var boldIsBright: Bool
    public var cursorShape: CursorShape
    public var cursorBlink: Bool
    public var paddingHorizontal: Double
    public var paddingVertical: Double
    public var opacity: Double
    public var contrast: ContrastLevel

    public static let defaultFontFamily = "SF Mono"
    public static let defaultFontSize = 13.0

    public init(theme: TerminalTheme, config: AppearanceConfig) {
        self.theme = theme
        fontFamily = theme.font?.family ?? config.font.family ?? Self.defaultFontFamily
        fontSize = theme.font?.size ?? config.font.size ?? Self.defaultFontSize
        boldIsBright = theme.font?.boldIsBright ?? config.font.boldIsBright ?? true
        cursorShape = theme.cursor?.style ?? config.cursor.style ?? .block
        cursorBlink = theme.cursor?.blink ?? config.cursor.blink ?? false
        paddingHorizontal = theme.window?.paddingHorizontal ?? config.window.paddingHorizontal ?? 10
        paddingVertical = theme.window?.paddingVertical ?? config.window.paddingVertical ?? 6
        opacity = theme.window?.opacity ?? config.window.opacity ?? 1
        contrast = theme.contrast ?? config.contrast
    }
}
