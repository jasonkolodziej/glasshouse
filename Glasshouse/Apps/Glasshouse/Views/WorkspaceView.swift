import DesignSystem
import SwiftUI

/// Window layout (layer model from the design spec):
///   L2 controls  — toolbar + tab strip, sidebar, find bar      → system Liquid Glass
///   L1 content   — terminal grid, gutter, sticky header        → opaque, never glass
///   L3 transient — command palette, approval card              → glass + dimming
///   L4 trusted   — TrustCapsule, card header                   → kernel state only
struct WorkspaceView: View {
    @State private var workspace = Workspace()
    @Environment(\.dismiss) private var dismiss

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
        // Terminal.app: closing the last tab closes the window.
        .onChange(of: workspace.sessions.isEmpty) { _, empty in if empty { dismiss() } }
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
    @Namespace private var selection

    var body: some View {
        // With one tab there is nothing to select between: a filled pill inside the toolbar's own
        // glass bubble reads as a pressed button, so a lone tab is just its title.
        let multiple = workspace.sessions.count > 1
        HStack(spacing: 2) {
            ForEach(workspace.sessions) { session in
                TabItem(session: session,
                        active: multiple && session.id == workspace.selectedID,
                        closable: multiple,
                        selection: selection,
                        select: { workspace.selectedID = session.id },
                        close: { workspace.close(session) })
            }
        }
        .animation(GH.motion, value: workspace.selectedID)
        .animation(GH.motion, value: workspace.sessions.map(\.id))
    }
}

/// One tab: hover reveals a leading close button (Safari / Terminal.app placement).
private struct TabItem: View {
    let session: TerminalSession
    let active: Bool
    let closable: Bool
    let selection: Namespace.ID
    let select: () -> Void
    let close: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 4) {
            if closable {
                Button(action: close) {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                        .frame(width: 16, height: 16)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .opacity(hovering ? 1 : 0)
                .allowsHitTesting(hovering)
                .help("Close Tab (⌘W)")
                .accessibilityHidden(true) // exposed as a named action on the tab instead
            }
            statusDot
            title
        }
        .padding(.leading, closable ? 4 : 10).padding(.trailing, 10).padding(.vertical, 4)
        .background {
            if active {
                Capsule().fill(Color.primary.opacity(0.10))
                    .matchedGeometryEffect(id: "selection", in: selection)
            } else if hovering && closable {
                Capsule().fill(Color.primary.opacity(0.05))
            }
        }
        .contentShape(Capsule())
        .onTapGesture(perform: select)
        .onHover { hovering = $0 }
        .contextMenu { Button("Close Tab", action: close) }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(session.title)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(.default, select)
        .accessibilityAction(named: "Close Tab", close)
    }

    /// Width is reserved for the semibold title so selecting a tab never truncates or shifts it.
    private var title: some View {
        ZStack {
            Text(session.title).fontWeight(.semibold).hidden()
            Text(session.title).fontWeight(active ? .semibold : .regular)
        }
        .lineLimit(1)
        .frame(maxWidth: 160)
    }

    @ViewBuilder private var statusDot: some View {
        switch session.timeline.current?.status {
        case .running?: ProgressView().controlSize(.mini)
        case .failed?: Image(systemName: "xmark.circle.fill").foregroundStyle(.red).imageScale(.small)
        default: EmptyView()
        }
    }
}
