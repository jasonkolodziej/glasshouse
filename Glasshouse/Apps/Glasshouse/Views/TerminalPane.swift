import DesignSystem
import ShellIntegration
import SwiftUI
import TerminalCore

/// L1 content: the opaque grid with Glasshouse's non-occluding overlays.
/// - Find bar docks *above* the grid and pushes it down (never floats over text).
/// - Gutter sits in a reserved trailing inset outside the cell grid (critique U5).
/// - Sticky header appears only when scrolled away from the bottom (critique U4).
/// - Approval card anchors at the bottom, by the cursor where `sudo` was typed (critique U2).
struct TerminalPane: View {
    @Bindable var session: TerminalSession
    @Bindable var workspace: Workspace
    @Environment(\.controlActiveState) private var activeState

    var body: some View {
        VStack(spacing: 0) {
            if workspace.showsFind {
                FindBar(query: $workspace.findQuery) { workspace.showsFind = false }
                    .padding(GH.Space.s)
                    .ghTransition(from: .top)
            }
            HStack(spacing: 0) {
                TerminalHostView(engine: session.engine)
                    .overlay(alignment: .top) { stickyHeader }
                    .accessibilityLabel("Terminal, \(session.title)")
                CommandGutter(session: session)
                    .frame(width: GH.Size.gutterWidth)
            }
            .background(Color(nsColor: .textBackgroundColor)) // opaque content layer
            .overlay(alignment: .bottom) {
                if let ctx = session.approval {
                    ApprovalCard(context: ctx)
                        .padding(GH.Space.l)
                        .ghTransition(from: .bottom)
                }
            }
            .animation(GH.motion, value: session.approval)
        }
        .animation(GH.motion, value: workspace.showsFind)
        .onChange(of: activeState, initial: true) { _, state in
            session.isVisibleAndKey = (state == .key)
            if state == .key { Workspace.frontmost = workspace }
        }
        .onDisappear { session.isVisibleAndKey = false }
    }

    @ViewBuilder private var stickyHeader: some View {
        if let record = stickyRecord {
            StickyCommandHeader(record: record) { session.jump(to: record) }
                .ghTransition(from: .top)
        }
    }

    /// The command whose prompt is above the viewport while its output fills the top of it —
    /// only when the user has scrolled away from the live bottom.
    private var stickyRecord: CommandRecord? {
        let top = session.topVisibleRow
        // Scrolled away = the live cursor row is below the viewport. In the alternate screen
        // (vim, htop) there is no scrollback, so this is never true and the header stays hidden.
        guard top + session.visibleRows <= session.cursorRow else { return nil }
        return session.timeline.commands.last { $0.promptRow < top && ($0.endRow ?? .max) > top }
    }
}

/// Bridges the engine's AppKit view. `updateNSView` is intentionally empty: the engine renders itself.
struct TerminalHostView: NSViewRepresentable {
    let engine: any TerminalEngine

    func makeNSView(context: Context) -> NSView { engine.view }
    func updateNSView(_ nsView: NSView, context: Context) {}
}
