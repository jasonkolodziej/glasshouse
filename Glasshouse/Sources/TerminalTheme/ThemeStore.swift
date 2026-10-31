import Foundation
import Yams

/// Built-in templates (read-only, bundled) and your own themes (YAML files in `userDirectory`), plus `appearance.yaml`.
public struct ThemeStore: Sendable {
    public let userDirectory: URL
    public let configURL: URL

    public init(userDirectory: URL, configURL: URL) {
        self.userDirectory = userDirectory
        self.configURL = configURL
    }

    /// `~/Library/Application Support/Glasshouse/Themes` and `…/Glasshouse/appearance.yaml`.
    public static func standard(fileManager fm: FileManager = .default) -> ThemeStore {
        let support = (fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? fm.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support"))
            .appendingPathComponent("Glasshouse")
        return ThemeStore(userDirectory: support.appendingPathComponent("Themes"),
                          configURL: support.appendingPathComponent("appearance.yaml"))
    }

    // MARK: Themes

    /// The bundled templates, sorted by name.
    public static let builtInThemes: [TerminalTheme] = {
        let directory = Bundle.module.url(forResource: "Themes", withExtension: nil)
        let urls = directory.flatMap { try? FileManager.default.contentsOfDirectory(at: $0, includingPropertiesForKeys: nil) } ?? []
        return urls.filter { $0.pathExtension == "yaml" }.compactMap { url in
            guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
            return try? theme(fromYAML: text, id: url.deletingPathExtension().lastPathComponent)
        }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }()

    public static func isBuiltIn(_ id: String) -> Bool { builtInThemes.contains { $0.id == id } }

    /// Your themes, sorted by name. Files that don't parse are left out; `userThemeProblems()` lists them.
    public func userThemes() -> [TerminalTheme] {
        userThemeFiles().compactMap { try? load($0) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    /// File name and reason for every theme file that couldn't be read.
    public func userThemeProblems() -> [(file: String, problem: String)] {
        userThemeFiles().compactMap { url in
            do { _ = try load(url); return nil }
            catch { return (url.lastPathComponent, error.localizedDescription) }
        }
    }

    /// Built-in templates first, then your themes.
    public func allThemes() -> [TerminalTheme] { Self.builtInThemes + userThemes() }

    public func theme(id: String) -> TerminalTheme? {
        Self.builtInThemes.first { $0.id == id } ?? userThemes().first { $0.id == id }
    }

    /// Validates and writes the theme. A theme without an id, or with a built-in one, gets a new id from its name.
    @discardableResult
    public func save(_ theme: TerminalTheme) throws -> TerminalTheme {
        var theme = try theme.validated()
        if theme.id.isEmpty || Self.isBuiltIn(theme.id) || !Self.isValidID(theme.id) {
            theme.id = uniqueID(for: theme.name)
        }
        try write(Data(Self.yaml(for: theme).utf8), to: fileURL(theme.id))
        return theme
    }

    /// Saves a copy of `theme` under a new name and id, as a theme of yours.
    public func duplicate(_ theme: TerminalTheme, named name: String) throws -> TerminalTheme {
        try save(theme.copy(named: name))
    }

    public func delete(id: String) throws {
        guard !Self.isBuiltIn(id) else { throw ThemeError.readOnly(id) }
        guard Self.isValidID(id), FileManager.default.fileExists(atPath: fileURL(id).path) else {
            throw ThemeError.notFound(id)
        }
        try FileManager.default.removeItem(at: fileURL(id))
    }

    /// Reads a Glasshouse YAML theme, an iTerm2 `.itermcolors` file or a Ghostty theme, and saves it as yours.
    @discardableResult
    public func importTheme(from url: URL) throws -> TerminalTheme {
        guard let data = try? Data(contentsOf: url) else { throw ThemeError.unreadable(url.lastPathComponent) }
        let name = url.deletingPathExtension().lastPathComponent
        let theme: TerminalTheme
        switch url.pathExtension.lowercased() {
        case "itermcolors":
            theme = try ThemeImporter.iTerm(data, name: name)
        case "yaml", "yml":
            theme = try Self.theme(fromYAML: String(decoding: data, as: UTF8.self), id: "")
        default:
            theme = try ThemeImporter.ghostty(String(decoding: data, as: UTF8.self), name: name)
        }
        var mine = theme
        mine.id = ""
        if Self.builtInThemes.contains(where: { $0.name == mine.name }) || userThemes().contains(where: { $0.name == mine.name }) {
            mine.name = uniqueName(for: mine.name)
        }
        return try save(mine)
    }

    // MARK: appearance.yaml

    /// The saved settings, or the defaults when there are none or they don't parse.
    public func loadConfig() -> AppearanceConfig {
        guard let text = try? String(contentsOf: configURL, encoding: .utf8),
              let config = try? YAMLDecoder().decode(AppearanceConfig.self, from: text).validated() else {
            return AppearanceConfig()
        }
        return config
    }

    public func saveConfig(_ config: AppearanceConfig) throws {
        let text = "# Glasshouse appearance settings. Settings › Appearance writes this file.\n"
            + Self.plainNumbers(try YAMLEncoder().encode(config.validated()))
        try write(Data(text.utf8), to: configURL)
    }

    /// The theme for the current system appearance, with every setting resolved. A missing theme falls back to
    /// Glasshouse's own.
    public func resolved(dark: Bool, config: AppearanceConfig? = nil) -> ResolvedAppearance {
        let config = config ?? loadConfig()
        let fallback = dark ? "glasshouse-dark" : "glasshouse-light"
        let theme = theme(id: config.theme.themeID(dark: dark)) ?? theme(id: fallback) ?? Self.emergencyTheme
        return ResolvedAppearance(theme: theme, config: config)
    }

    // MARK: YAML

    public static func yaml(for theme: TerminalTheme) throws -> String {
        "# Glasshouse terminal theme. See the README's Themes section for every key.\n"
            + plainNumbers(try YAMLEncoder().encode(theme.validated()))
    }

    /// Yams writes numbers like `1.4e+1`; files people edit read better as `14` and `0.9`. Only unquoted values that
    /// parse as numbers change, so quoted strings (colours, names) are left alone.
    static func plainNumbers(_ yaml: String) -> String {
        yaml.split(separator: "\n", omittingEmptySubsequences: false).map { line -> String in
            guard let colon = line.range(of: ": ", options: .backwards) else { return String(line) }
            let value = line[colon.upperBound...]
            guard value.contains("e"), let number = Double(value), number.isFinite else { return String(line) }
            let plain = number == number.rounded() && abs(number) < 1e15 ? String(Int(number)) : String(number)
            return line[..<colon.upperBound] + plain
        }.joined(separator: "\n")
    }

    public static func theme(fromYAML text: String, id: String) throws -> TerminalTheme {
        do {
            var theme = try YAMLDecoder().decode(TerminalTheme.self, from: text)
            theme.id = id
            return try theme.validated()
        } catch let error as ThemeError {
            throw error
        } catch DecodingError.keyNotFound(let key, let context) {
            throw ThemeError.missingKey((context.codingPath + [key]).map(\.stringValue).joined(separator: "."))
        } catch let error as DecodingError {
            // Yams wraps an error thrown while decoding a value (an invalid colour, say) in a DecodingError.
            switch error {
            case .dataCorrupted(let context), .typeMismatch(_, let context), .valueNotFound(_, let context):
                if let underlying = context.underlyingError as? ThemeError { throw underlying }
            default: break
            }
            throw ThemeError.unreadable(id.isEmpty ? "The file" : id)
        } catch {
            throw ThemeError.unreadable(id.isEmpty ? "The file" : id)
        }
    }

    // MARK: Files

    static func isValidID(_ id: String) -> Bool {
        !id.isEmpty && id.count <= 64 && id.first != "-"
            && id.allSatisfy { $0.isASCII && ($0.isLowercase || $0.isNumber || $0 == "-") }
    }

    /// `"My Theme!"` → `my-theme`, then `my-theme-2`, … until no theme has it.
    func uniqueID(for name: String) -> String {
        let slug = name.lowercased().map { $0.isASCII && ($0.isLetter || $0.isNumber) ? String($0) : "-" }.joined()
            .split(separator: "-").joined(separator: "-")
        let base = String((slug.isEmpty ? "theme" : slug).prefix(56))
        let taken = Set(allThemes().map(\.id))
        if !taken.contains(base) { return base }
        return (2...).lazy.map { "\(base)-\($0)" }.first { !taken.contains($0) }!
    }

    func uniqueName(for name: String) -> String {
        let taken = Set(allThemes().map(\.name))
        return (2...).lazy.map { "\(name) \($0)" }.first { !taken.contains($0) }!
    }

    private func fileURL(_ id: String) -> URL { userDirectory.appendingPathComponent(id + ".yaml") }

    private func userThemeFiles() -> [URL] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: userDirectory.path)) ?? []
        return names.filter { $0.hasSuffix(".yaml") }.sorted().map { userDirectory.appendingPathComponent($0) }
    }

    private func load(_ url: URL) throws -> TerminalTheme {
        let id = url.deletingPathExtension().lastPathComponent
        guard Self.isValidID(id), !Self.isBuiltIn(id) else { throw ThemeError.unreadable(url.lastPathComponent) }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            throw ThemeError.unreadable(url.lastPathComponent)
        }
        return try Self.theme(fromYAML: text, id: id)
    }

    private func write(_ data: Data, to url: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: url, options: .atomic)
    }

    /// Used only if the bundle's themes are missing.
    static let emergencyTheme = TerminalTheme(
        id: "glasshouse-dark", name: "Glasshouse Dark",
        colors: .init(background: try! ThemeColor(hex: "#16181d"), foreground: try! ThemeColor(hex: "#e6e6e3"),
                      normal: TerminalTheme.glasshouseNormal, bright: TerminalTheme.glasshouseBright))
}
