import Foundation
import Testing
@testable import TerminalTheme

private func fixture(_ name: String) throws -> URL {
    try #require(Bundle.module.url(forResource: name, withExtension: nil, subdirectory: "Fixtures"))
}

private func hex(_ value: String) -> ThemeColor { try! ThemeColor(hex: value) }

/// A throwaway Application Support folder.
private struct Sandbox: ~Copyable {
    let root: URL
    let store: ThemeStore
    init() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("glasshouse-themes-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        store = ThemeStore(userDirectory: root.appendingPathComponent("Themes"),
                           configURL: root.appendingPathComponent("appearance.yaml"))
    }
    deinit { try? FileManager.default.removeItem(at: root) }

    func write(_ text: String, _ path: String) throws {
        let url = root.appendingPathComponent(path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try text.write(to: url, atomically: true, encoding: .utf8)
    }
}

@Suite("Colours")
struct ColorTests {
    @Test(arguments: [("#1e1f22", "#1e1f22"), ("1E1F22", "#1e1f22"), ("#abc", "#aabbcc"), ("#11223380", "#11223380")])
    func parsesAndWritesHex(input: String, output: String) throws {
        #expect(try ThemeColor(hex: input).hex == output)
    }

    @Test(arguments: ["", "#12", "#12345", "#gggggg", "red", "#1234567"])
    func refusesWhatIsNotAColour(input: String) {
        #expect(throws: ThemeError.invalidColor(input)) { try ThemeColor(hex: input) }
    }

    @Test func measuresContrastAndMixes() {
        #expect(abs(ThemeColor.black.contrastRatio(with: .white) - 21) < 0.01)
        #expect(ThemeColor.black.isDark && !ThemeColor.white.isDark)
        #expect(ThemeColor.black.mixed(with: .white, amount: 0.5).hex == "#808080")
    }
}

@Suite("Bundled templates")
struct TemplateTests {
    let themes = ThemeStore.builtInThemes

    @Test func allFifteenLoadWithUniqueIDsAndNames() {
        #expect(themes.count == 15)
        #expect(Set(themes.map(\.id)).count == themes.count)
        #expect(Set(themes.map(\.name)).count == themes.count)
        #expect(themes.contains { $0.id == "glasshouse-dark" } && themes.contains { $0.id == "glasshouse-light" })
    }

    @Test func everyTemplateIsReadableAndLabelledCorrectly() throws {
        for theme in themes {
            _ = try theme.validated()
            #expect(theme.colors.ansi.count == 16, "\(theme.id)")
            // Catches typos. Some themes sit just under WCAG's 4.5 by design (Solarized Light is 4.1); the app's
            // APCA floor raises the text contrast when a theme loads.
            #expect(theme.colors.foreground.contrastRatio(with: theme.colors.background) >= 4.0, "\(theme.id)")
            #expect(theme.appearance == (theme.colors.background.isDark ? .dark : .light), "\(theme.id)")
        }
    }

    @Test func survivesAWriteAndReadBack() throws {
        for theme in themes {
            let text = try ThemeStore.yaml(for: theme)
            #expect(try ThemeStore.theme(fromYAML: text, id: theme.id) == theme, "\(theme.id)")
        }
    }

    @Test func matchesTheColoursGlasshouseUsedBefore() throws {
        let dark = try #require(themes.first { $0.id == "glasshouse-dark" })
        #expect(dark.colors.background.hex == "#16181d")
        #expect(dark.colors.cursor == nil)
        #expect(dark.colors.normal == TerminalTheme.glasshouseNormal)
        #expect(dark.colors.bright == TerminalTheme.glasshouseBright)
    }
}

@Suite("YAML format")
struct YAMLTests {
    @Test func aMinimalThemeGetsDefaults() throws {
        let text = try String(contentsOf: fixture("minimal.yaml"), encoding: .utf8)
        let theme = try ThemeStore.theme(fromYAML: text, id: "minimal")
        #expect(theme.name == "Minimal")
        #expect(theme.appearance == .dark)
        #expect(theme.schema == 1)
        #expect(theme.colors.normal == TerminalTheme.glasshouseNormal)
        #expect(theme.colors.bright == TerminalTheme.glasshouseNormal.mixed(with: .white, amount: 0.25))
        #expect(theme.font == nil && theme.cursor == nil && theme.window == nil && theme.contrast == nil)
    }

    @Test func writesKebabCaseKeysAndOptionalSections() throws {
        var theme = try #require(ThemeStore.builtInThemes.first { $0.id == "dracula" })
        theme.font = FontSettings(family: "JetBrains Mono", size: 14, boldIsBright: false)
        theme.cursor = CursorSettings(style: .bar, blink: true)
        theme.window = WindowSettings(paddingHorizontal: 16, paddingVertical: 8, opacity: 0.9)
        theme.contrast = .faithful
        let text = try ThemeStore.yaml(for: theme)
        for key in ["cursor-text:", "bold-is-bright: false", "size: 14\n", "padding-horizontal: 16\n", "opacity: 0.9\n",
                    "style: bar", "contrast: faithful"] {
            #expect(text.contains(key), "\(key) in:\n\(text)")
        }
        #expect(!text.contains("id:"))
        #expect(try ThemeStore.theme(fromYAML: text, id: "dracula") == theme)
    }

    @Test func explainsWhatIsWrong() {
        #expect(throws: ThemeError.missingKey("colors")) { try ThemeStore.theme(fromYAML: "name: X\n", id: "x") }
        #expect(throws: ThemeError.invalidColor("blue")) {
            try ThemeStore.theme(fromYAML: "name: X\ncolors: {background: blue, foreground: '#fff'}\n", id: "x")
        }
        #expect(throws: ThemeError.outOfRange("font size", 100)) {
            try ThemeStore.theme(fromYAML: "name: X\ncolors: {background: '#000', foreground: '#fff'}\nfont: {size: 100}\n", id: "x")
        }
        #expect(throws: ThemeError.unsupportedSchema(2)) {
            try ThemeStore.theme(fromYAML: "schema: 2\nname: X\ncolors: {background: '#000', foreground: '#fff'}\n", id: "x")
        }
        #expect(throws: ThemeError.invalidName("")) {
            try ThemeStore.theme(fromYAML: "name: ''\ncolors: {background: '#000', foreground: '#fff'}\n", id: "x")
        }
    }
}

@Suite("Appearance settings")
struct AppearanceConfigTests {
    @Test func themeSettingsWinOverGlobalOnesWhichWinOverDefaults() throws {
        var theme = try #require(ThemeStore.builtInThemes.first { $0.id == "nord" })
        let config = AppearanceConfig(font: FontSettings(family: "Menlo", size: 12),
                                      window: WindowSettings(opacity: 0.8), contrast: .faithful)
        var resolved = ResolvedAppearance(theme: theme, config: config)
        #expect(resolved.fontFamily == "Menlo" && resolved.fontSize == 12 && resolved.boldIsBright)
        #expect(resolved.opacity == 0.8 && resolved.paddingHorizontal == 10 && resolved.contrast == .faithful)
        #expect(resolved.cursorShape == .block && !resolved.cursorBlink)

        theme.font = FontSettings(size: 15)
        theme.contrast = .high
        resolved = ResolvedAppearance(theme: theme, config: config)
        #expect(resolved.fontFamily == "Menlo" && resolved.fontSize == 15 && resolved.contrast == .high)
    }

    @Test func picksTheThemeForTheSystemAppearance() {
        let choice = AppearanceConfig.ThemeChoice(dark: "dracula", light: "solarized-light", followSystem: true)
        #expect(choice.themeID(dark: true) == "dracula")
        #expect(choice.themeID(dark: false) == "solarized-light")
        #expect(AppearanceConfig.ThemeChoice(dark: "dracula", light: "x", followSystem: false).themeID(dark: false) == "dracula")
    }

    @Test func savesLoadsAndFallsBack() throws {
        let box = try Sandbox()
        #expect(box.store.loadConfig() == AppearanceConfig())

        var config = AppearanceConfig()
        config.theme.dark = "dracula"
        config.font.size = 15
        try box.store.saveConfig(config)
        #expect(box.store.loadConfig() == config)
        #expect(try String(contentsOf: box.store.configURL, encoding: .utf8).contains("follow-system: true"))

        try box.write("theme: { dark: nord }\n", "appearance.yaml")
        #expect(box.store.loadConfig().theme.dark == "nord")
        #expect(box.store.loadConfig().theme.light == "glasshouse-light")

        try box.write("font: { size: 999 }\n", "appearance.yaml")
        #expect(box.store.loadConfig() == AppearanceConfig())

        config.font.size = 999
        #expect(throws: ThemeError.self) { try box.store.saveConfig(config) }
    }

    @Test func resolvesAMissingThemeToGlasshouses() throws {
        let box = try Sandbox()
        var config = AppearanceConfig()
        config.theme.dark = "deleted-theme"
        #expect(box.store.resolved(dark: true, config: config).theme.id == "glasshouse-dark")
        #expect(box.store.resolved(dark: false, config: config).theme.id == "glasshouse-light")
    }
}

@Suite("Your themes")
struct ThemeStoreTests {
    @Test func savesWithAnIDFromTheNameAndNeverOverwritesATemplate() throws {
        let box = try Sandbox()
        let dracula = try #require(ThemeStore.builtInThemes.first { $0.id == "dracula" })

        let copy = try box.store.duplicate(dracula, named: "My Dracula!")
        #expect(copy.id == "my-dracula")
        #expect(copy.author == nil && copy.source == nil)
        #expect(box.store.theme(id: "my-dracula") == copy)

        var edited = dracula
        edited.colors.background = hex("#000000")
        let saved = try box.store.save(edited)
        #expect(saved.id == "dracula-2")
        #expect(box.store.theme(id: "dracula")?.colors.background.hex == "#282a36")

        #expect(box.store.allThemes().count == 17)
        #expect(throws: ThemeError.readOnly("dracula")) { try box.store.delete(id: "dracula") }
        try box.store.delete(id: "dracula-2")
        #expect(box.store.theme(id: "dracula-2") == nil)
        #expect(throws: ThemeError.notFound("../x")) { try box.store.delete(id: "../x") }
    }

    @Test func listsFilesThatDoNotLoadInsteadOfFailing() throws {
        let box = try Sandbox()
        try box.write("name: Broken\n", "Themes/broken.yaml")
        try box.write("name: Spoof\ncolors: {background: '#000', foreground: '#fff'}\n", "Themes/dracula.yaml")
        try box.write(try String(contentsOf: fixture("minimal.yaml"), encoding: .utf8), "Themes/minimal.yaml")

        #expect(box.store.userThemes().map(\.id) == ["minimal"])
        #expect(box.store.userThemeProblems().map(\.file) == ["broken.yaml", "dracula.yaml"])
        #expect(box.store.theme(id: "dracula")?.name == "Dracula")
    }
}

@Suite("Importing")
struct ImportTests {
    @Test func readsITerm2Colors() throws {
        let theme = try ThemeImporter.iTerm(Data(contentsOf: fixture("Sample.itermcolors")), name: "Sample")
        #expect(theme.colors.background.hex == "#1e1e1e")
        #expect(theme.colors.foreground.hex == "#cccccc")
        #expect(theme.colors.selection?.hex == "#264f78")
        #expect(theme.colors.normal.red.hex == "#cd3131")
        #expect(theme.colors.bright.white.hex == "#ffffff")
        #expect(theme.appearance == .dark)
    }

    @Test func readsGhosttyThemesAndFillsGaps() throws {
        let text = try String(contentsOf: fixture("sample-ghostty"), encoding: .utf8)
        let theme = try ThemeImporter.ghostty(text, name: "Tomorrow")
        #expect(theme.colors.background.hex == "#1d1f21")
        #expect(theme.colors.cursor?.hex == "#c5c8c6")
        #expect(theme.colors.normal.blue.hex == "#81a2be")
        #expect(theme.colors.bright.red.hex == "#d54e53")
        #expect(theme.colors.bright.black == theme.colors.normal.black.mixed(with: .white, amount: 0.25))
    }

    @Test func refusesFilesItDoesNotUnderstand() {
        #expect(throws: ThemeError.unknownFormat("x")) { try ThemeImporter.ghostty("hello = world\n", name: "x") }
        #expect(throws: ThemeError.unknownFormat("x")) { try ThemeImporter.iTerm(Data("not a plist".utf8), name: "x") }
    }

    @Test func importsIntoYourThemesWithoutClashingNames() throws {
        let box = try Sandbox()
        let first = try box.store.importTheme(from: fixture("Sample.itermcolors"))
        let second = try box.store.importTheme(from: fixture("Sample.itermcolors"))
        #expect(first.name == "Sample" && first.id == "sample")
        #expect(second.name == "Sample 2" && second.id == "sample-2")
        #expect(try box.store.importTheme(from: fixture("minimal.yaml")).id == "minimal")
    }
}
