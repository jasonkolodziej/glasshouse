import Foundation

/// Turns other terminals' theme files into `TerminalTheme`s.
public enum ThemeImporter {
    /// iTerm2's `.itermcolors`: a property list of colour dictionaries ("Ansi 0 Color" … "Ansi 15 Color",
    /// "Background Color", "Foreground Color", …) with 0–1 components. Components are taken as sRGB; iTerm2's
    /// "Calibrated" and P3 colours come out slightly different.
    public static func iTerm(_ data: Data, name: String) throws -> TerminalTheme {
        guard let plist = try? PropertyListSerialization.propertyList(from: data, format: nil),
              let root = plist as? [String: Any] else { throw ThemeError.unknownFormat(name) }
        func color(_ key: String) -> ThemeColor? {
            guard let dict = root[key] as? [String: Any] else { return nil }
            func component(_ name: String) -> Double? { (dict[name] as? NSNumber)?.doubleValue }
            guard let r = component("Red Component"), let g = component("Green Component"),
                  let b = component("Blue Component") else { return nil }
            return ThemeColor(red: r, green: g, blue: b, alpha: component("Alpha Component") ?? 1)
        }
        guard let background = color("Background Color"), let foreground = color("Foreground Color") else {
            throw ThemeError.unknownFormat(name)
        }
        let ansi = (0..<16).map { color("Ansi \($0) Color") }
        return TerminalTheme(name: name, colors: palette(background: background, foreground: foreground,
                                                         bold: color("Bold Color"), cursor: color("Cursor Color"),
                                                         cursorText: color("Cursor Text Color"),
                                                         selection: color("Selection Color"), ansi: ansi))
    }

    /// Ghostty's theme format: `key = value` lines, such as `background = #282a36` and `palette = 1=#ff5555`.
    public static func ghostty(_ text: String, name: String) throws -> TerminalTheme {
        var values: [String: ThemeColor] = [:]
        var ansi = [ThemeColor?](repeating: nil, count: 16)
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.hasPrefix("#"), let eq = line.firstIndex(of: "=") else { continue }
            let key = line[..<eq].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            if key == "palette", let inner = value.firstIndex(of: "=") {
                if let index = Int(value[..<inner].trimmingCharacters(in: .whitespaces)), (0..<16).contains(index) {
                    ansi[index] = try? ThemeColor(hex: String(value[value.index(after: inner)...]))
                }
            } else if let color = try? ThemeColor(hex: value) {
                values[key] = color
            }
        }
        guard let background = values["background"], let foreground = values["foreground"] else {
            throw ThemeError.unknownFormat(name)
        }
        return TerminalTheme(name: name, colors: palette(
            background: background, foreground: foreground, bold: values["bold-color"],
            cursor: values["cursor-color"], cursorText: values["cursor-text"],
            selection: values["selection-background"], ansi: ansi))
    }

    /// Missing ANSI colours come from Glasshouse's palette (normal row) or are derived from the normal row (bright).
    static func palette(background: ThemeColor, foreground: ThemeColor, bold: ThemeColor?, cursor: ThemeColor?,
                        cursorText: ThemeColor?, selection: ThemeColor?, ansi: [ThemeColor?]) -> TerminalTheme.Palette {
        let normal = (0..<8).map { ansi[$0] ?? TerminalTheme.glasshouseNormal.all[$0] }
        let toward = background.isDark ? ThemeColor.white : .black
        let bright = (0..<8).map { ansi[$0 + 8] ?? normal[$0].mixed(with: toward, amount: 0.25) }
        return TerminalTheme.Palette(background: background, foreground: foreground, bold: bold, cursor: cursor,
                                     cursorText: cursorText, selection: selection,
                                     normal: .init(normal), bright: .init(bright))
    }
}
