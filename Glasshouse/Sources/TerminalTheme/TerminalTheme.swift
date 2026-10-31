import Foundation

/// A terminal theme, stored as YAML. Colours are required; `font`, `cursor`, `window` and `contrast` are optional and,
/// when present, override the global settings in `appearance.yaml` while this theme is in use.
///
/// ```yaml
/// schema: 1
/// name: Dracula
/// appearance: dark
/// colors:
///   background: "#282a36"
///   foreground: "#f8f8f2"
///   normal: { black: "#21222c", red: "#ff5555", … }
///   bright: { black: "#6272a4", red: "#ff6e6e", … }
/// font: { family: JetBrains Mono, size: 14 }   # optional
/// ```
public struct TerminalTheme: Codable, Hashable, Sendable, Identifiable {
    public static let currentSchema = 1

    public enum Appearance: String, Codable, Sendable, CaseIterable {
        case dark, light
    }

    /// The eight colours of one ANSI row (normal or bright).
    public struct ANSIColors: Codable, Hashable, Sendable {
        public var black, red, green, yellow, blue, magenta, cyan, white: ThemeColor

        public static let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]

        public init(black: ThemeColor, red: ThemeColor, green: ThemeColor, yellow: ThemeColor,
                    blue: ThemeColor, magenta: ThemeColor, cyan: ThemeColor, white: ThemeColor) {
            self.black = black; self.red = red; self.green = green; self.yellow = yellow
            self.blue = blue; self.magenta = magenta; self.cyan = cyan; self.white = white
        }

        /// Exactly eight colours, in ANSI order.
        public init(_ colors: [ThemeColor]) {
            precondition(colors.count == 8, "an ANSI row has eight colours")
            self.init(black: colors[0], red: colors[1], green: colors[2], yellow: colors[3],
                      blue: colors[4], magenta: colors[5], cyan: colors[6], white: colors[7])
        }

        public var all: [ThemeColor] {
            get { [black, red, green, yellow, blue, magenta, cyan, white] }
            set { self = ANSIColors(newValue) }
        }

        /// Each colour moved `amount` of the way towards `target`: a quick way to make a bright row.
        public func mixed(with target: ThemeColor, amount: Double) -> ANSIColors {
            ANSIColors(all.map { $0.mixed(with: target, amount: amount) })
        }
    }

    public struct Palette: Codable, Hashable, Sendable {
        public var background: ThemeColor
        public var foreground: ThemeColor
        /// Bold text; `nil` uses the foreground.
        public var bold: ThemeColor?
        /// `nil` uses the system accent colour.
        public var cursor: ThemeColor?
        /// Text under a block cursor; `nil` lets the terminal choose.
        public var cursorText: ThemeColor?
        /// `nil` uses the system selection colour.
        public var selection: ThemeColor?
        public var normal: ANSIColors
        public var bright: ANSIColors

        public init(background: ThemeColor, foreground: ThemeColor, bold: ThemeColor? = nil,
                    cursor: ThemeColor? = nil, cursorText: ThemeColor? = nil, selection: ThemeColor? = nil,
                    normal: ANSIColors, bright: ANSIColors) {
            self.background = background; self.foreground = foreground; self.bold = bold
            self.cursor = cursor; self.cursorText = cursorText; self.selection = selection
            self.normal = normal; self.bright = bright
        }

        /// All 16 ANSI colours, normal then bright.
        public var ansi: [ThemeColor] { normal.all + bright.all }

        private enum CodingKeys: String, CodingKey {
            case background, foreground, bold, cursor, cursorText = "cursor-text", selection, normal, bright
        }

        /// A hand-written theme may leave out the ANSI rows: a missing normal row comes from Glasshouse's own
        /// palette, and a missing bright row is derived from the normal one.
        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            background = try c.decode(ThemeColor.self, forKey: .background)
            foreground = try c.decode(ThemeColor.self, forKey: .foreground)
            bold = try c.decodeIfPresent(ThemeColor.self, forKey: .bold)
            cursor = try c.decodeIfPresent(ThemeColor.self, forKey: .cursor)
            cursorText = try c.decodeIfPresent(ThemeColor.self, forKey: .cursorText)
            selection = try c.decodeIfPresent(ThemeColor.self, forKey: .selection)
            normal = try c.decodeIfPresent(ANSIColors.self, forKey: .normal) ?? TerminalTheme.glasshouseNormal
            bright = try c.decodeIfPresent(ANSIColors.self, forKey: .bright)
                ?? normal.mixed(with: background.isDark ? .white : .black, amount: 0.25)
        }
    }

    /// The file name without `.yaml`. Not stored in the file.
    public var id: String
    public var schema: Int
    public var name: String
    public var author: String?
    /// Where the colours come from: a URL or a credit line.
    public var source: String?
    /// Which system appearance the theme is made for. Only used to suggest it; any theme fits either slot.
    public var appearance: Appearance
    public var colors: Palette
    public var font: FontSettings?
    public var cursor: CursorSettings?
    public var window: WindowSettings?
    public var contrast: ContrastLevel?

    public init(id: String = "", name: String, author: String? = nil, source: String? = nil,
                appearance: Appearance? = nil, colors: Palette, font: FontSettings? = nil,
                cursor: CursorSettings? = nil, window: WindowSettings? = nil, contrast: ContrastLevel? = nil) {
        self.id = id
        schema = Self.currentSchema
        self.name = name
        self.author = author
        self.source = source
        self.appearance = appearance ?? (colors.background.isDark ? .dark : .light)
        self.colors = colors
        self.font = font
        self.cursor = cursor
        self.window = window
        self.contrast = contrast
    }

    private enum CodingKeys: String, CodingKey {
        case schema, name, author, source, appearance, colors, font, cursor, window, contrast
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let colors = try c.decode(Palette.self, forKey: .colors)
        self.init(name: try c.decode(String.self, forKey: .name),
                  author: try c.decodeIfPresent(String.self, forKey: .author),
                  source: try c.decodeIfPresent(String.self, forKey: .source),
                  appearance: try c.decodeIfPresent(Appearance.self, forKey: .appearance),
                  colors: colors,
                  font: try c.decodeIfPresent(FontSettings.self, forKey: .font),
                  cursor: try c.decodeIfPresent(CursorSettings.self, forKey: .cursor),
                  window: try c.decodeIfPresent(WindowSettings.self, forKey: .window),
                  contrast: try c.decodeIfPresent(ContrastLevel.self, forKey: .contrast))
        schema = try c.decodeIfPresent(Int.self, forKey: .schema) ?? Self.currentSchema
    }

    /// Checks everything a hand-edited file could get wrong.
    public func validated() throws -> TerminalTheme {
        guard (1...Self.currentSchema).contains(schema) else { throw ThemeError.unsupportedSchema(schema) }
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty, trimmedName.count <= 80, !trimmedName.contains(where: \.isNewline) else {
            throw ThemeError.invalidName(name)
        }
        for text in [author, source].compactMap({ $0 }) where text.count > 200 || text.contains(where: \.isNewline) {
            throw ThemeError.invalidText(String(text.prefix(40)))
        }
        try font?.validate()
        try window?.validate()
        var theme = self
        theme.name = trimmedName
        return theme
    }

    /// A theme with the same colours that is yours to edit: new id and name, no author or source.
    public func copy(named newName: String) -> TerminalTheme {
        var theme = self
        theme.id = ""
        theme.name = newName
        theme.author = nil
        theme.source = nil
        return theme
    }

    /// Glasshouse's own palette, tuned to read on both light and dark backgrounds.
    public static let glasshouseNormal = ANSIColors(
        ["#1e1f22", "#d13b3b", "#2e9e5b", "#c98a10", "#2f6fd6", "#a24bc9", "#1e96a8", "#c8c8c4"].map { try! ThemeColor(hex: $0) })
    public static let glasshouseBright = ANSIColors(
        ["#5c5f66", "#f05a5a", "#4cc37a", "#e8b33b", "#5b92f0", "#c377e8", "#3cc2d6", "#ffffff"].map { try! ThemeColor(hex: $0) })
}

/// Font settings. Every field is optional so a theme can override just one; `AppearanceConfig` fills the rest.
public struct FontSettings: Codable, Hashable, Sendable {
    public var family: String?
    public var size: Double?
    /// Draw bold text in the bright ANSI colours, as most terminals do.
    public var boldIsBright: Bool?

    public init(family: String? = nil, size: Double? = nil, boldIsBright: Bool? = nil) {
        self.family = family; self.size = size; self.boldIsBright = boldIsBright
    }

    private enum CodingKeys: String, CodingKey { case family, size, boldIsBright = "bold-is-bright" }

    public static let sizeRange = 6.0...72.0

    func validate() throws {
        if let family, family.trimmingCharacters(in: .whitespaces).isEmpty || family.count > 100
            || family.contains(where: { $0.isNewline }) { throw ThemeError.invalidFont(family) }
        if let size, !Self.sizeRange.contains(size) { throw ThemeError.outOfRange("font size", size) }
    }
}

public enum CursorShape: String, Codable, Sendable, CaseIterable {
    case block, underline, bar
}

public struct CursorSettings: Codable, Hashable, Sendable {
    public var style: CursorShape?
    public var blink: Bool?

    public init(style: CursorShape? = nil, blink: Bool? = nil) { self.style = style; self.blink = blink }
}

public struct WindowSettings: Codable, Hashable, Sendable {
    /// Points between the window edge and the text, left side.
    public var paddingHorizontal: Double?
    /// Points above and below the text.
    public var paddingVertical: Double?
    /// 1 is opaque. Below 1 the terminal background lets Liquid Glass show through, unless Reduce Transparency
    /// or Increase Contrast is on.
    public var opacity: Double?

    public init(paddingHorizontal: Double? = nil, paddingVertical: Double? = nil, opacity: Double? = nil) {
        self.paddingHorizontal = paddingHorizontal; self.paddingVertical = paddingVertical; self.opacity = opacity
    }

    private enum CodingKeys: String, CodingKey {
        case paddingHorizontal = "padding-horizontal", paddingVertical = "padding-vertical", opacity
    }

    public static let paddingRange = 0.0...64.0
    public static let opacityRange = 0.3...1.0

    func validate() throws {
        for (label, value) in [("horizontal padding", paddingHorizontal), ("vertical padding", paddingVertical)] {
            if let value, !Self.paddingRange.contains(value) { throw ThemeError.outOfRange(label, value) }
        }
        if let opacity, !Self.opacityRange.contains(opacity) { throw ThemeError.outOfRange("opacity", opacity) }
    }
}

/// How far Glasshouse nudges colours to keep text readable (APCA, see DesignSystem). macOS's Increase Contrast
/// setting always raises it to `high`.
public enum ContrastLevel: String, Codable, Sendable, CaseIterable {
    /// Default text at least Lc 75, ANSI colours at least Lc 60.
    case standard
    /// Keeps a theme's colours closer to how they were designed: ANSI colours at least Lc 45.
    case faithful
    /// Default text at least Lc 90, ANSI colours at least Lc 75.
    case high
}

public enum ThemeError: Error, Equatable, LocalizedError {
    case invalidColor(String)
    case invalidName(String)
    case invalidText(String)
    case invalidFont(String)
    case outOfRange(String, Double)
    case unsupportedSchema(Int)
    case unreadable(String)
    case missingKey(String)
    case unknownFormat(String)
    case readOnly(String)
    case notFound(String)

    public var errorDescription: String? {
        switch self {
        case .invalidColor(let v): "“\(v)” isn't a colour. Use #rrggbb, for example #1e1f22."
        case .invalidName(let v): "“\(v)” can't be used as a theme name. Use 1–80 characters on one line."
        case .invalidText(let v): "“\(v)…” is too long or spans several lines."
        case .invalidFont(let v): "“\(v)” isn't a font family name."
        case .outOfRange(let what, let v): "\(v) is out of range for the \(what)."
        case .unsupportedSchema(let v): "This theme uses format \(v), which this version of Glasshouse can't read."
        case .unreadable(let v): "\(v) couldn't be read as a theme."
        case .missingKey(let v): "The theme has no “\(v)”, which every theme needs."
        case .unknownFormat(let v): "\(v) isn't a Glasshouse, iTerm2 or Ghostty theme."
        case .readOnly(let v): "“\(v)” is a built-in theme. Customize a copy instead."
        case .notFound(let v): "There's no theme “\(v)”."
        }
    }
}
