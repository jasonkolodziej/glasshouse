import AppKit
import SwiftUI
import TerminalTheme
import UniformTypeIdentifiers

/// Settings › Appearance: pick themes for Light and Dark Mode from a gallery, manage your own, and set the font,
/// cursor, window and contrast every theme starts from. Everything is saved to `appearance.yaml` as you change it.
struct AppearanceSettingsView: View {
    private let library = ThemeLibrary.shared
    @Environment(\.openWindow) private var openWindow
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @State private var importing = false
    @State private var exporting: ThemeYAMLDocument?
    @State private var deleting: TerminalTheme?
    @State private var opacity: Double = 1
    @State private var fontFamilies: [String] = []
    @State private var error: String?

    private var config: AppearanceConfig { library.config }
    private var isDark: Bool { colorScheme == .dark }
    /// The theme in use right now, with its overrides.
    private var current: ResolvedAppearance { library.resolved(dark: isDark) }

    var body: some View {
        Form {
            Section("Theme") {
                Toggle("Match the system's Light and Dark Mode", isOn: binding(\.theme.followSystem))
                Picker(config.theme.followSystem ? "Dark Mode" : "Theme", selection: binding(\.theme.dark)) {
                    themeOptions
                }
                if config.theme.followSystem {
                    Picker("Light Mode", selection: binding(\.theme.light)) { themeOptions }
                }
            }

            Section("Gallery") {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: 12)], spacing: 14) {
                    ForEach(library.themes) { theme in card(theme) }
                }
                .padding(.vertical, 4)
                HStack {
                    Menu("New Theme") {
                        Button("From Scratch (Dark)") { startNew(from: "glasshouse-dark") }
                        Button("From Scratch (Light)") { startNew(from: "glasshouse-light") }
                        Button("From “\(current.theme.name)”") { edit(current.theme, copy: true) }
                    }
                    .fixedSize()
                    Button("Import…") { importing = true }
                        .help("Glasshouse YAML, iTerm2 .itermcolors or Ghostty theme files")
                    Spacer()
                    Button("Show Themes Folder") { showThemesFolder() }
                }
                ForEach(library.problems, id: \.file) { problem in
                    Label("\(problem.file): \(problem.problem)", systemImage: "exclamationmark.triangle")
                        .font(.caption).foregroundStyle(.orange)
                }
            }

            Section("Text") {
                Picker("Font", selection: binding(\.font.family, default: ResolvedAppearance.defaultFontFamily)) {
                    ForEach(fontChoices, id: \.self) { Text($0).font(.custom($0, size: 13)).tag($0) }
                }
                Stepper("Size: \(Int(config.font.size ?? ResolvedAppearance.defaultFontSize)) pt",
                        value: binding(\.font.size, default: ResolvedAppearance.defaultFontSize),
                        in: 9...32)
                Toggle("Show bold text in bright colours", isOn: binding(\.font.boldIsBright, default: true))
                overrideNote(current.theme.font != nil, "font")
            }

            Section("Cursor") {
                Picker("Shape", selection: binding(\.cursor.style, default: .block)) {
                    Text("Block").tag(CursorShape.block)
                    Text("Underline").tag(CursorShape.underline)
                    Text("Bar").tag(CursorShape.bar)
                }
                .pickerStyle(.segmented)
                Toggle("Blink", isOn: binding(\.cursor.blink, default: false))
                overrideNote(current.theme.cursor != nil, "cursor")
            }

            Section("Window") {
                Stepper("Side padding: \(Int(config.window.paddingHorizontal ?? 10)) pt",
                        value: binding(\.window.paddingHorizontal, default: 10), in: 0...32)
                Stepper("Top and bottom padding: \(Int(config.window.paddingVertical ?? 6)) pt",
                        value: binding(\.window.paddingVertical, default: 6), in: 0...32)
                LabeledContent("Background opacity") {
                    Slider(value: $opacity, in: 0.5...1, onEditingChanged: { editing in
                        // Saved when the drag ends, not on every step.
                        if !editing { update { $0.window.opacity = (opacity * 100).rounded() / 100 } }
                    })
                    .frame(width: 200)
                    Text("\(Int(opacity * 100))%").monospacedDigit().frame(width: 44, alignment: .trailing)
                }
                .disabled(reduceTransparency || systemContrast == .increased)
                Text(reduceTransparency || systemContrast == .increased
                     ? "The terminal stays opaque while Reduce Transparency or Increase Contrast is on."
                     : "Below 100%, Liquid Glass shows through the terminal. Glass is for controls and the terminal is content, so opaque is the default.")
                    .font(.caption).foregroundStyle(.secondary)
                overrideNote(current.theme.window != nil, "padding and opacity")
            }

            Section("Contrast") {
                Picker("Readability floor", selection: binding(\.contrast)) {
                    Text("Standard").tag(ContrastLevel.standard)
                    Text("Faithful to the theme").tag(ContrastLevel.faithful)
                    Text("High").tag(ContrastLevel.high)
                }
                Text("Glasshouse nudges colours that are too close to the background (APCA): text to at least Lc 75 and ANSI colours to at least Lc 60. Faithful lowers the ANSI floor to Lc 45; High raises both. Increase Contrast in System Settings always uses High.")
                    .font(.caption).foregroundStyle(.secondary)
                overrideNote(current.theme.contrast != nil, "contrast level")
            }

            if let error {
                Section { Text(error).foregroundStyle(.red).font(.caption) }
            }
        }
        .formStyle(.grouped)
        .onAppear {
            library.reload()   // picks up theme files added to the folder by hand
            opacity = config.window.opacity ?? 1
            fontFamilies = Self.monospacedFamilies()
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            importThemes(result)
        }
        .fileExporter(isPresented: Binding(get: { exporting != nil }, set: { if !$0 { exporting = nil } }),
                      document: exporting, contentType: ThemeYAMLDocument.contentType,
                      defaultFilename: exporting?.name) { result in
            if case .failure(let failure) = result { error = failure.localizedDescription }
        }
        .confirmationDialog("Delete “\(deleting?.name ?? "")”?",
                            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
            Button("Delete Theme", role: .destructive) {
                if let theme = deleting { perform { try library.delete(theme) } }
            }
        } message: {
            Text("Its file is removed from the themes folder. A mode that used it switches back to Glasshouse's theme.")
        }
    }

    // MARK: Gallery

    private var themeOptions: some View {
        ForEach(library.themes) { Text($0.name).tag($0.id) }
    }

    private func card(_ theme: TerminalTheme) -> some View {
        let darkSlot = config.theme.dark == theme.id
        let lightSlot = config.theme.followSystem && config.theme.light == theme.id
        let inUse = current.theme.id == theme.id
        let builtIn = library.isBuiltIn(theme)
        return VStack(alignment: .leading, spacing: 6) {
            ThemePreview(resolved: ResolvedAppearance(theme: theme, config: config), compact: true)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .strokeBorder(inUse ? Color.accentColor : Color.secondary.opacity(0.3), lineWidth: inUse ? 2 : 1))
            HStack(spacing: 4) {
                Text(theme.name).font(.callout).lineLimit(1)
                Spacer(minLength: 4)
                if darkSlot { slotBadge(config.theme.followSystem ? "Dark" : "In use") }
                if lightSlot { slotBadge("Light") }
            }
            Text(builtIn ? (theme.author ?? "Built-in") : "Yours")
                .font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
        .contentShape(Rectangle())
        .onTapGesture { use(theme, dark: isDark || !config.theme.followSystem) }
        .contextMenu {
            Button("Use for Dark Mode") { use(theme, dark: true) }
            Button("Use for Light Mode") { use(theme, dark: false) }.disabled(!config.theme.followSystem)
            Divider()
            if builtIn {
                Button("Customize…") { edit(theme, copy: true) }
            } else {
                Button("Edit…") { edit(theme, copy: false) }
                Button("Duplicate…") { edit(theme, copy: true) }
            }
            Button("Export…") { export(theme) }
            if !builtIn {
                Divider()
                Button("Delete…", role: .destructive) { deleting = theme }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(inUse ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(named: builtIn ? "Customize" : "Edit") { edit(theme, copy: builtIn) }
    }

    private func slotBadge(_ text: String) -> some View {
        Text(text).font(.caption2.weight(.semibold))
            .padding(.horizontal, 5).padding(.vertical, 1)
            .background(.quaternary, in: Capsule())
    }

    // MARK: Actions

    private func use(_ theme: TerminalTheme, dark: Bool) {
        update { if dark { $0.theme.dark = theme.id } else { $0.theme.light = theme.id } }
    }

    /// Opens the editor. A built-in theme, or `copy`, is duplicated first: only your own themes are edited.
    private func edit(_ theme: TerminalTheme, copy: Bool) {
        perform {
            let target = (copy || library.isBuiltIn(theme)) ? try library.customize(theme) : theme
            openWindow(id: ThemeEditorView.windowID, value: target.id)
        }
    }

    private func startNew(from baseID: String) {
        guard let base = library.theme(id: baseID) else { return }
        perform {
            let fresh = try library.store.duplicate(base, named: "Untitled Theme")
            library.reload()
            openWindow(id: ThemeEditorView.windowID, value: fresh.id)
        }
    }

    private func export(_ theme: TerminalTheme) {
        perform { exporting = ThemeYAMLDocument(name: theme.id, text: try ThemeStore.yaml(for: theme)) }
    }

    private func importThemes(_ result: Result<[URL], Error>) {
        guard case .success(let urls) = result else { return }
        perform {
            for url in urls {
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                try library.importTheme(from: url)
            }
        }
    }

    private func showThemesFolder() {
        perform {
            try FileManager.default.createDirectory(at: library.store.userDirectory, withIntermediateDirectories: true)
            NSWorkspace.shared.open(library.store.userDirectory)
        }
    }

    private func update(_ change: (inout AppearanceConfig) -> Void) {
        perform { try library.updateConfig(change) }
    }

    private func perform(_ action: () throws -> Void) {
        do { try action(); error = nil } catch { self.error = error.localizedDescription }
    }

    // MARK: Bindings

    private func binding<T>(_ keyPath: WritableKeyPath<AppearanceConfig, T>) -> Binding<T> {
        Binding(get: { library.config[keyPath: keyPath] },
                set: { value in update { $0[keyPath: keyPath] = value } })
    }

    /// For a setting that may be unset in the file: shows `fallback` until it is set.
    private func binding<T>(_ keyPath: WritableKeyPath<AppearanceConfig, T?>, default fallback: T) -> Binding<T> {
        Binding(get: { library.config[keyPath: keyPath] ?? fallback },
                set: { value in update { $0[keyPath: keyPath] = value } })
    }

    @ViewBuilder private func overrideNote(_ overridden: Bool, _ what: String) -> some View {
        if overridden {
            Label("“\(current.theme.name)” sets its own \(what), so this doesn't apply while it's in use.",
                  systemImage: "info.circle")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    // MARK: Fonts

    /// Monospaced families, always including the one in use.
    private var fontChoices: [String] {
        let currentFamily = config.font.family ?? ResolvedAppearance.defaultFontFamily
        return fontFamilies.contains(currentFamily) ? fontFamilies : [currentFamily] + fontFamilies
    }

    static func monospacedFamilies() -> [String] {
        let manager = NSFontManager.shared
        return manager.availableFontFamilies.filter { family in
            guard let member = manager.availableMembers(ofFontFamily: family)?.first,
                  let postScriptName = member.first as? String,
                  let font = NSFont(name: postScriptName, size: 12) else { return false }
            return font.isFixedPitch
        }
    }
}

/// A theme as a YAML file, for Export….
struct ThemeYAMLDocument: FileDocument {
    static let contentType = UTType(filenameExtension: "yaml", conformingTo: .text) ?? .plainText
    static var readableContentTypes: [UTType] { [contentType] }

    var name: String
    var text: String

    init(name: String, text: String) {
        self.name = name
        self.text = text
    }

    init(configuration: ReadConfiguration) throws {
        name = "theme"
        text = String(decoding: configuration.file.regularFileContents ?? Data(), as: UTF8.self)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}
