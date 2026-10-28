import DesignSystem
import SwiftUI

/// Window layout (layer model from the design spec):
///   L2 controls  — toolbar + tab strip, sidebar, find bar      → system Liquid Glass
///   L1 content   — terminal grid, gutter, sticky header        → opaque, never glass
///   L3 transient — command palette, approval card              → glass + dimming
///   L4 trusted   — TrustCapsule, card header                   → kernel state only
struct WorkspaceView: View {
    @State private var workspace = Workspace()

    var body: some View {
        NavigationSplitView(columnVisibility: sidebarVisibility) {
            SessionSidebar(workspace: workspace)
                .navigationSplitViewColumnWidth(min: 200, ideal: GH.Size.sidebarWidth, max: 320)
        } detail: {
            ZStack {
                if let session = workspace.selected {
                    TerminalPane(session: session, workspace: workspace)
                        .id(session.id)
                } else {
                    ContentUnavailableView("No Tabs", systemImage: "terminal",
                                           description: Text("Press ⌘T for a new tab."))
                }
            }
            .dimmed(workspace.showsPalette)
            .overlay(alignment: .top) {
                if workspace.showsPalette {
                    CommandPalette(workspace: workspace)
                        .padding(.top, 56)
                        .ghTransition(from: .top)
                }
            }
            .animation(GH.motion, value: workspace.showsPalette)
        }
        .toolbar { toolbar }
        .navigationTitle(workspace.selected?.title ?? "Glasshouse")
        .focusedSceneValue(workspace)
        .onAppear { workspace.hosts.reload() }
    }

    private var sidebarVisibility: Binding<NavigationSplitViewVisibility> {
        Binding(get: { workspace.showsSidebar ? .all : .detailOnly },
                set: { workspace.showsSidebar = ($0 != .detailOnly) })
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            TabStrip(workspace: workspace)
        }
        ToolbarSpacer(.flexible)
        ToolbarItem(placement: .primaryAction) {
            Button { workspace.newTab() } label: { Label("New Tab", systemImage: "plus") }
                .help("New Tab (⌘T)")
        }
        ToolbarSpacer(.fixed)
        // L4: trailing edge, filled + tinted, never overflows (TODO: AppKit NSToolbarItem
        // visibilityPriority = .high — not exposed in SwiftUI; bridge if overflow becomes an issue).
        ToolbarItem(placement: .status) {
            TrustCapsule(state: workspace.selected?.trust ?? .idle)
        }
        .sharedBackgroundVisibility(.hidden) // the capsule is its own filled surface, not glass
    }
}

/// Tabs live in one glass group. Active tab = fill + weight (tint is reserved for trust signals).
/// Titles are plain text: program-controlled strings must never look like the trust capsule.
struct TabStrip: View {
    @Bindable var workspace: Workspace

    var body: some View {
        HStack(spacing: 2) {
            ForEach(workspace.sessions) { session in
                let active = session.id == workspace.selectedID
                Button { workspace.selectedID = session.id } label: {
                    HStack(spacing: 6) {
                        statusDot(session)
                        Text(session.title)
                            .lineLimit(1)
                            .fontWeight(active ? .semibold : .regular)
                            .frame(maxWidth: 160)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 4)
                    .background(active ? Color.primary.opacity(0.10) : .clear, in: Capsule())
                }
                .buttonStyle(.plain)
                .contextMenu { Button("Close Tab") { workspace.close(session) } }
                .accessibilityLabel("\(session.title)\(active ? ", selected" : "")")
            }
        }
    }

    @ViewBuilder private func statusDot(_ s: TerminalSession) -> some View {
        switch s.timeline.current?.status {
        case .running?: ProgressView().controlSize(.mini)
        case .failed?: Image(systemName: "xmark.circle.fill").foregroundStyle(.red).imageScale(.small)
        default: EmptyView()
        }
    }
}
