import AppKit
import SwiftUI
import TerminalCore
import TerminalTheme

/// A sample terminal drawn in a theme's colours, after the same contrast adjustment the real terminal applies.
/// `compact` is the gallery card; the full size is the editor's live preview.
struct ThemePreview: View {
    let appearance: TerminalAppearance
    var compact = false

    init(resolved: ResolvedAppearance, compact: Bool = false) {
        self.appearance = Theme.appearance(for: resolved)
        self.compact = compact
    }

    private var font: Font {
        .custom(appearance.fontName, size: compact ? 9 : appearance.fontSize).monospaced()
    }

    /// ANSI colour `index` (0–15) as adjusted for the background.
    private func ansi(_ index: Int) -> Color { Color(nsColor: appearance.ansi[index]) }
    private var fg: Color { Color(nsColor: appearance.foreground) }
    private var dim: Color { ansi(8) }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 1 : 3) {
            prompt(Text("ls"))
            line(Text("README.md  ").foregroundStyle(fg) + Text("Sources/  Tests/  ").foregroundStyle(ansi(12))
                 + Text("build.sh").foregroundStyle(ansi(10)))
            prompt(Text("git status -s"))
            line(Text(" M ").foregroundStyle(ansi(1)) + Text("Theme.swift  ").foregroundStyle(fg)
                 + Text("?? ").foregroundStyle(ansi(1)) + Text("notes.md").foregroundStyle(fg))
            if !compact {
                prompt(Text("swift test"))
                line(Text("✔ ").foregroundStyle(ansi(2)) + Text("47 tests passed  ").foregroundStyle(fg)
                     + Text("⚠ 1 warning").foregroundStyle(ansi(3)))
                line(Text("error: ").foregroundStyle(ansi(9)).bold() + Text("cannot find 'x' in scope").foregroundStyle(fg))
                HStack(spacing: 0) {
                    Text("selected").foregroundStyle(fg).font(font)
                        .background(Color(nsColor: appearance.selection ?? .selectedTextBackgroundColor))
                    Text(" text, ").foregroundStyle(fg).font(font)
                    Text("bold").foregroundStyle(appearance.boldIsBright ? ansi(15) : fg).bold().font(font)
                    Text(", ").foregroundStyle(fg).font(font)
                    Text("dim").foregroundStyle(dim).font(font)
                }
            }
            HStack(spacing: 0) {
                Text("~/glasshouse ").foregroundStyle(ansi(6)).font(font)
                Text("% ").foregroundStyle(fg).font(font)
                cursor
            }
            swatches.padding(.top, compact ? 3 : 6)
        }
        .padding(.horizontal, compact ? 8 : appearance.paddingHorizontal)
        .padding(.vertical, compact ? 6 : appearance.paddingVertical)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: appearance.background.withAlphaComponent(appearance.backgroundOpacity)))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Preview of the theme")
    }

    private func prompt(_ command: Text) -> some View {
        line(Text("~/glasshouse ").foregroundStyle(ansi(6)) + Text("% ").foregroundStyle(fg)
             + command.foregroundStyle(fg))
    }

    private func line(_ text: Text) -> some View {
        text.font(font).lineLimit(1)
    }

    @ViewBuilder private var cursor: some View {
        let size = compact ? 9.0 : appearance.fontSize
        let color = Color(nsColor: appearance.cursor)
        switch appearance.cursorShape {
        case .block:
            Text("x").font(font).foregroundStyle(appearance.cursorText.map { Color(nsColor: $0) } ?? Color(nsColor: appearance.background))
                .background(color)
        case .underline:
            Rectangle().fill(color).frame(width: size * 0.6, height: 2).offset(y: size * 0.45)
        case .bar:
            Rectangle().fill(color).frame(width: 2, height: size * 1.2)
        }
    }

    private var swatches: some View {
        VStack(spacing: 2) {
            ForEach(0..<2, id: \.self) { row in
                HStack(spacing: 2) {
                    ForEach(0..<8, id: \.self) { column in
                        RoundedRectangle(cornerRadius: 2)
                            .fill(ansi(row * 8 + column))
                            .frame(height: compact ? 6 : 14)
                    }
                }
            }
        }
    }
}
