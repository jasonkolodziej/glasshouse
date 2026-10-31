import AppKit
import DesignSystem
import SwiftUI
import TerminalTheme

/// The theme editor window. It edits your own themes only: Settings duplicates a built-in theme before opening it.
struct ThemeEditorView: View {
    static let windowID = "theme-editor"
    let themeID: String
    private let library = ThemeLibrary.shared

    var body: some View {
        if let theme = library.theme(id: themeID), !library.isBuiltIn(theme) {
            ThemeEditorForm(original: theme).id(theme.id)
        } else {
            ContentUnavailableView("No Theme to Edit", systemImage: "paintpalette",
                                   description: Text("Choose Edit… or Customize… on a theme in Settings › Appearance."))
                .frame(minWidth: 480, minHeight: 320)
        }
    }
}

private struct ThemeEditorForm: View {
    @State private var saved: TerminalTheme
    @State private var draft: TerminalTheme
    @State private var error: String?
    @State private var exporting: ThemeYAMLDocument?
    @State private var fontFamilies: [String] = []
    private let library = ThemeLibrary.shared

    init(original: TerminalTheme) {
        _saved = State(initialValue: original)
        _draft = State(initialValue: original)
    }

    private var resolved: ResolvedAppearance { ResolvedAppearance(theme: draft, config: library.config) }
    private var policy: ContrastPolicy {
        Theme.contrastPolicy(resolved.contrast,
                             increaseContrast: NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Form {
                    details
                    colors
                    ansi
                    overrides
                }
                .formStyle(.grouped)
                .frame(minWidth: 420, idealWidth: 440)
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        ThemePreview(resolved: resolved)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .strokeBorder(Color.secondary.opacity(0.3)))
                        ContrastReport(theme: draft, policy: policy)
                    }
                    .padding(20)
                }
                .frame(minWidth: 420)
            }
            Divider()
            HStack {
                if let error { Text(error).font(.caption).foregroundStyle(.red).lineLimit(2) }
                Spacer()
                Button("Revert") { draft = saved; error = nil }
                    .disabled(draft == saved)
                Button("Export…") {
                    do { exporting = ThemeYAMLDocument(name: draft.id, text: try ThemeStore.yaml(for: draft)) }
                    catch { self.error = error.localizedDescription }
                }
                Button("Save") { save() }
                    .keyboardShortcut("s")
                    .disabled(draft == saved)
            }
            .padding(12)
        }
        .frame(minWidth: 860, minHeight: 600)
        .navigationTitle(draft.name + (draft == saved ? "" : " — Edited"))
        .onAppear { fontFamilies = AppearanceSettingsView.monospacedFamilies() }
        .fileExporter(isPresented: Binding(get: { exporting != nil }, set: { if !$0 { exporting = nil } }),
                      document: exporting, contentType: ThemeYAMLDocument.contentType,
                      defaultFilename: exporting?.name) { result in
            if case .failure(let failure) = result { error = failure.localizedDescription }
        }
    }

    private func save() {
        do {
            draft = try library.save(draft)
            saved = draft
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    // MARK: Sections

    private var details: some View {
        Section("Theme") {
            TextField("Name", text: $draft.name)
            TextField("Author", text: Binding(get: { draft.author ?? "" },
                                              set: { draft.author = $0.isEmpty ? nil : $0 }), prompt: Text("Optional"))
            Picker("Made for", selection: $draft.appearance) {
                Text("Dark Mode").tag(TerminalTheme.Appearance.dark)
                Text("Light Mode").tag(TerminalTheme.Appearance.light)
            }
            .pickerStyle(.segmented)
        }
    }

    private var colors: some View {
        Section("Colours") {
            colorRow("Background", $draft.colors.background)
            colorRow("Text", $draft.colors.foreground)
            optionalColorRow("Bold text", $draft.colors.bold, unset: "Same as text", start: draft.colors.foreground)
            optionalColorRow("Cursor", $draft.colors.cursor, unset: "System accent colour", start: draft.colors.foreground)
            optionalColorRow("Text under the cursor", $draft.colors.cursorText, unset: "Automatic",
                             start: draft.colors.background)
            optionalColorRow("Selection", $draft.colors.selection, unset: "System selection colour",
                             start: draft.colors.foreground.mixed(with: draft.colors.background, amount: 0.7))
        }
    }

    private var ansi: some View {
        Section("ANSI colours") {
            Grid(horizontalSpacing: 6, verticalSpacing: 6) {
                GridRow {
                    Text("")
                    ForEach(TerminalTheme.ANSIColors.names, id: \.self) {
                        Text($0.prefix(3).capitalized).font(.caption2).foregroundStyle(.secondary)
                    }
                }
                ansiRow("Normal", $draft.colors.normal, offset: 0)
                ansiRow("Bright", $draft.colors.bright, offset: 8)
            }
            HStack {
                Button("Derive Bright Row") {
                    draft.colors.bright = draft.colors.normal.mixed(
                        with: draft.colors.background.isDark ? .white : .black, amount: 0.25)
                }
                .help("Makes each bright colour a lighter (dark themes) or darker (light themes) version of the normal one")
                Button("Reset to Glasshouse Palette") {
                    draft.colors.normal = TerminalTheme.glasshouseNormal
                    draft.colors.bright = TerminalTheme.glasshouseBright
                }
            }
            Text("A dot marks a colour that Glasshouse will adjust to stay readable on this background.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private func ansiRow(_ title: String, _ row: Binding<TerminalTheme.ANSIColors>, offset: Int) -> some View {
        GridRow {
            Text(title).font(.caption)
            ForEach(0..<8, id: \.self) { index in
                let color = Binding(get: { row.wrappedValue.all[index] },
                                    set: { var all = row.wrappedValue.all; all[index] = $0; row.wrappedValue = .init(all) })
                VStack(spacing: 2) {
                    ColorPicker("", selection: cgColor(color), supportsOpacity: false)
                        .labelsHidden()
                        .accessibilityLabel("\(title) \(TerminalTheme.ANSIColors.names[index])")
                    Circle()
                        .fill(ContrastReport.isAdjusted(slot: offset + index, color: color.wrappedValue,
                                                        background: draft.colors.background, policy: policy)
                              ? Color.orange : Color.clear)
                        .frame(width: 5, height: 5)
                }
            }
        }
    }

    private var overrides: some View {
        Section {
            Toggle("Use its own font", isOn: Binding(
                get: { draft.font != nil },
                set: { draft.font = $0 ? FontSettings(family: resolved.fontFamily, size: resolved.fontSize,
                                                      boldIsBright: resolved.boldIsBright) : nil }))
            if draft.font != nil {
                Picker("Font", selection: Binding(get: { draft.font?.family ?? resolved.fontFamily },
                                                  set: { draft.font?.family = $0 })) {
                    let families = fontFamilies.contains(resolved.fontFamily) ? fontFamilies : [resolved.fontFamily] + fontFamilies
                    ForEach(families, id: \.self) { Text($0).tag($0) }
                }
                Stepper("Size: \(Int(resolved.fontSize)) pt",
                        value: Binding(get: { draft.font?.size ?? resolved.fontSize }, set: { draft.font?.size = $0 }),
                        in: 9...32)
                Toggle("Bold text in bright colours", isOn: Binding(get: { draft.font?.boldIsBright ?? true },
                                                                    set: { draft.font?.boldIsBright = $0 }))
            }

            Toggle("Use its own cursor", isOn: Binding(
                get: { draft.cursor != nil },
                set: { draft.cursor = $0 ? CursorSettings(style: resolved.cursorShape, blink: resolved.cursorBlink) : nil }))
            if draft.cursor != nil {
                Picker("Shape", selection: Binding(get: { draft.cursor?.style ?? .block }, set: { draft.cursor?.style = $0 })) {
                    Text("Block").tag(CursorShape.block)
                    Text("Underline").tag(CursorShape.underline)
                    Text("Bar").tag(CursorShape.bar)
                }
                .pickerStyle(.segmented)
                Toggle("Blink", isOn: Binding(get: { draft.cursor?.blink ?? false }, set: { draft.cursor?.blink = $0 }))
            }

            Toggle("Use its own padding and opacity", isOn: Binding(
                get: { draft.window != nil },
                set: { draft.window = $0 ? WindowSettings(paddingHorizontal: resolved.paddingHorizontal,
                                                          paddingVertical: resolved.paddingVertical,
                                                          opacity: resolved.opacity) : nil }))
            if draft.window != nil {
                Stepper("Side padding: \(Int(resolved.paddingHorizontal)) pt",
                        value: Binding(get: { draft.window?.paddingHorizontal ?? 10 },
                                       set: { draft.window?.paddingHorizontal = $0 }), in: 0...32)
                Stepper("Top and bottom padding: \(Int(resolved.paddingVertical)) pt",
                        value: Binding(get: { draft.window?.paddingVertical ?? 6 },
                                       set: { draft.window?.paddingVertical = $0 }), in: 0...32)
                LabeledContent("Opacity") {
                    Slider(value: Binding(get: { draft.window?.opacity ?? 1 },
                                          set: { draft.window?.opacity = ($0 * 100).rounded() / 100 }), in: 0.5...1)
                }
            }

            Toggle("Use its own contrast level", isOn: Binding(
                get: { draft.contrast != nil }, set: { draft.contrast = $0 ? resolved.contrast : nil }))
            if draft.contrast != nil {
                Picker("Readability floor", selection: Binding(get: { draft.contrast ?? .standard },
                                                               set: { draft.contrast = $0 })) {
                    Text("Standard").tag(ContrastLevel.standard)
                    Text("Faithful to the theme").tag(ContrastLevel.faithful)
                    Text("High").tag(ContrastLevel.high)
                }
            }
        } header: {
            Text("This theme's own settings")
        } footer: {
            Text("Off: the theme uses the font, cursor, window and contrast settings from Settings › Appearance.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    // MARK: Colour rows

    private func colorRow(_ title: String, _ color: Binding<ThemeColor>) -> some View {
        ColorPicker(title, selection: cgColor(color), supportsOpacity: false)
    }

    private func optionalColorRow(_ title: String, _ color: Binding<ThemeColor?>, unset: String,
                                  start: ThemeColor) -> some View {
        HStack {
            Toggle(title, isOn: Binding(get: { color.wrappedValue != nil },
                                        set: { color.wrappedValue = $0 ? start : nil }))
            Spacer()
            if color.wrappedValue != nil {
                ColorPicker(title, selection: cgColor(Binding(get: { color.wrappedValue ?? start },
                                                             set: { color.wrappedValue = $0 })),
                            supportsOpacity: false)
                    .labelsHidden()
            } else {
                Text(unset).foregroundStyle(.secondary)
            }
        }
    }

    private func cgColor(_ color: Binding<ThemeColor>) -> Binding<CGColor> {
        Binding(get: { color.wrappedValue.cgColor }, set: { color.wrappedValue = ThemeColor(cgColor: $0) })
    }
}

/// APCA contrast of the text and each ANSI colour against the background, before Glasshouse adjusts them.
private struct ContrastReport: View {
    let theme: TerminalTheme
    let policy: ContrastPolicy

    /// Slots 0, 7, 8 and 15 (blacks and whites) are meant to match or invert the background and are never adjusted.
    static let exempt: Set<Int> = [0, 7, 8, 15]

    static func isAdjusted(slot: Int, color: ThemeColor, background: ThemeColor, policy: ContrastPolicy) -> Bool {
        !exempt.contains(slot) && abs(APCA.contrast(text: color.apca, background: background.apca)) < policy.paletteMinimum
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Contrast").font(.headline)
            Text("APCA Lc against the background. Below the floor, Glasshouse moves the colour just far enough to reach it.")
                .font(.caption).foregroundStyle(.secondary)
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
                row("Text", theme.colors.foreground, minimum: policy.foregroundMinimum)
                ForEach(Array(theme.colors.ansi.enumerated()), id: \.offset) { slot, color in
                    if !Self.exempt.contains(slot) {
                        row((slot < 8 ? "" : "Bright ") + TerminalTheme.ANSIColors.names[slot % 8],
                            color, minimum: policy.paletteMinimum)
                    }
                }
            }
            .font(.caption)
        }
    }

    private func row(_ name: String, _ color: ThemeColor, minimum: Double) -> some View {
        let lc = abs(APCA.contrast(text: color.apca, background: theme.colors.background.apca))
        let passes = lc >= minimum
        return GridRow {
            RoundedRectangle(cornerRadius: 2).fill(color.color).frame(width: 14, height: 10)
            Text(name.capitalized)
            Text("Lc \(Int(lc.rounded()))").monospacedDigit()
            Label(passes ? "Readable" : "Adjusted to Lc \(Int(minimum))",
                  systemImage: passes ? "checkmark.circle" : "exclamationmark.triangle")
                .foregroundStyle(passes ? Color.secondary : Color.orange)
        }
    }
}
