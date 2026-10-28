import DesignSystem
import ShellIntegration
import SwiftUI

/// Scrollbar-gutter command marks, in a reserved inset outside the cell grid.
/// Shape *and* color encode status (never color alone): ● success, ■ failure, ○ running.
/// Each tick has a 16 pt hit area and a hover popover with command, exit code and duration.
struct CommandGutter: View {
    let session: TerminalSession
    @State private var hovered: CommandRecord.ID?

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .top) {
                Color.clear
                ForEach(session.timeline.commands) { record in
                    tick(record)
                        .frame(width: GH.Size.gutterWidth, height: GH.Size.gutterHitHeight)
                        .contentShape(Rectangle())
                        .offset(y: y(for: record, height: geo.size.height))
                        .onHover { hovered = $0 ? record.id : (hovered == record.id ? nil : hovered) }
                        .onTapGesture { session.jump(to: record) }
                        .popover(isPresented: .constant(hovered == record.id), arrowEdge: .leading) {
                            GutterPopover(record: record, output: { session.output(of: record) })
                        }
                        .accessibilityElement()
                        .accessibilityLabel(axLabel(record))
                        .accessibilityAddTraits(.isButton)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Commands")
    }

    private func y(for record: CommandRecord, height: CGFloat) -> CGFloat {
        let frac = CGFloat(record.promptRow) / CGFloat(max(session.totalRows, 1))
        return min(max(frac * height - GH.Size.gutterHitHeight / 2, 0), height - GH.Size.gutterHitHeight)
    }

    @ViewBuilder private func tick(_ r: CommandRecord) -> some View {
        let s = GH.Size.gutterTick
        switch r.status {
        case .succeeded: Circle().fill(Color.secondary).frame(width: s, height: s)
        case .failed: Rectangle().fill(Color.red).frame(width: s, height: s)
        case .running: Circle().strokeBorder(Color.accentColor, lineWidth: 1.5).frame(width: s, height: s)
        case .unknown: Capsule().fill(Color.secondary.opacity(0.5)).frame(width: s - 2, height: 2)
        }
    }

    private func axLabel(_ r: CommandRecord) -> String {
        let cmd = r.commandLine ?? "command"
        switch r.status {
        case .succeeded: return "\(cmd), succeeded"
        case .failed: return "\(cmd), failed with exit \(r.exitCode ?? -1)"
        case .running: return "\(cmd), running"
        case .unknown: return cmd
        }
    }
}

struct GutterPopover: View {
    let record: CommandRecord
    let output: () -> String

    var body: some View {
        VStack(alignment: .leading, spacing: GH.Space.s) {
            Text(record.commandLine ?? "command").font(.system(.body, design: .monospaced)).lineLimit(2)
            HStack(spacing: GH.Space.m) {
                if let code = record.exitCode { Label("exit \(code)", systemImage: code == 0 ? "checkmark" : "xmark") }
                if let d = record.duration { Label(String(format: "%.1fs", d), systemImage: "clock") }
                if let dir = record.workingDirectory?.lastPathComponent { Label(dir, systemImage: "folder") }
            }
            .font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("Copy Output") { copy(output()) }
                Button("Copy Command") { copy(record.commandLine ?? "") }
            }
            .controlSize(.small)
        }
        .padding(GH.Space.m)
        .frame(maxWidth: 360, alignment: .leading)
    }

    private func copy(_ s: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
    }
}

/// Solid (not glass) pinned row showing the command whose output fills the top of the viewport.
struct StickyCommandHeader: View {
    let record: CommandRecord
    let jump: () -> Void

    var body: some View {
        Button(action: jump) {
            HStack(spacing: GH.Space.s) {
                Image(systemName: "chevron.up.circle")
                Text(record.commandLine ?? "command")
                    .font(.system(.body, design: .monospaced)).lineLimit(1)
                Spacer()
                if let code = record.exitCode, code != 0 {
                    Text("exit \(code)").font(.caption.monospaced()).foregroundStyle(.red)
                }
            }
            .padding(.horizontal, GH.Space.s).padding(.vertical, GH.Space.xs)
            .frame(maxWidth: .infinity)
            .background(Color(nsColor: .controlBackgroundColor))    // solid: contrast is deterministic
            .overlay(alignment: .bottom) { Divider() }
        }
        .buttonStyle(.plain)
        .help("Jump to this command (⌘↑)")
        .accessibilityLabel("Scrolled into output of \(record.commandLine ?? "command"). Jump to command.")
    }
}
