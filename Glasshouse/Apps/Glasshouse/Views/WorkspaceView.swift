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
        .sharedBackgroundVisibility(.hidden) // TabStrip draws its own capsule with a uniform inset
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
///
/// The group draws its own capsule (the toolbar item's shared glass is hidden) so every tab bubble
/// sits concentric inside it: the same `tabGroupInset` on all four sides, like a segmented control.
struct TabStrip: View {
    @Bindable var workspace: Workspace
    @Namespace private var selection

    var body: some View {
        // With one tab there is nothing to select between: a filled pill inside the group would
        // read as a pressed button, so a lone tab is just its title.
        let multiple = workspace.sessions.count > 1
        HStack(spacing: GH.Size.tabSpacing) {
            ForEach(workspace.sessions) { session in
                TabItem(session: session,
                        active: multiple && session.id == workspace.selectedID,
                        closable: multiple,
                        selection: selection,
                        select: { workspace.selectedID = session.id },
                        close: { workspace.close(session) })
            }
        }
        .padding(GH.Size.tabGroupInset)
        .glassChrome(.controls, cornerRadius: GH.Radius.capsule)
        .animation(GH.motion, value: workspace.selectedID)
        .animation(GH.motion, value: workspace.sessions.map(\.id))
    }
}

/// One tab. The title is centred in its bubble; hovering grows the bubble to the leading side to
/// reveal a close button (Safari / Terminal.app placement), and leaving shrinks it back.
private struct TabItem: View {
    let session: TerminalSession
    let active: Bool
    let closable: Bool
    let selection: Namespace.ID
    let select: () -> Void
    let close: () -> Void
    @State private var hovering = false

    private var showsClose: Bool { closable && hovering }

    var body: some View {
        HStack(spacing: GH.Space.xs) {
            if showsClose {
                Button(action: close) {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                        .frame(width: 16, height: 16)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help("Close Tab (⌘W)")
                .accessibilityHidden(true) // exposed as a named action on the tab instead
                .transition(.opacity.combined(with: .scale(scale: 0.6)))
            }
            statusDot
            title
        }
        // Symmetric padding while the close button is hidden keeps the title centred; with it shown
        // the leading side tightens so the button sits as far from the edge as the title does.
        .padding(.leading, showsClose ? GH.Space.xs + 2 : GH.Space.m)
        .padding(.trailing, GH.Space.m)
        .frame(height: GH.Size.tabHeight)
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
        .onHover { inside in withAnimation(GH.motion) { hovering = inside } }
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
