import DesignSystem
import GlasshouseSecurity
import SwiftUI

/// ⇧⌘P / ⇧⌘O. One field, mixed results (actions, hosts, sessions, recent commands), a "top result"
/// that wins regardless of category, and the shortcut shown on every row so users learn it.
struct CommandPalette: View {
    @Bindable var workspace: Workspace
    @Environment(\.openSettings) private var openSettings
    @AppStorage(SettingsPane.storageKey) private var settingsPane: SettingsPane = .security
    @State private var query = ""
    @State private var selection = 0
    @FocusState private var focused: Bool

    struct Item: Identifiable {
        let id = UUID()
        let title: String
        let subtitle: String?
        let symbol: String
        let shortcut: String?
        let kind: String
        let action: @MainActor () -> Void
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: GH.Space.s) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("Run a command, open a host, switch tab…", text: $query)
                    .textFieldStyle(.plain)
                    .font(.title3)
                    .focused($focused)
                    .onSubmit(runSelected)
                    .onKeyPress(.downArrow) { move(1); return .handled }
                    .onKeyPress(.upArrow) { move(-1); return .handled }
                    .onKeyPress(.escape) { workspace.showsPalette = false; return .handled }
            }
            .padding(GH.Space.l)

            Divider()

            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(Array(results.enumerated()), id: \.element.id) { index, item in
                        PaletteRow(item: item, isSelected: index == selection, isTop: index == 0 && !query.isEmpty)
                            .onTapGesture { selection = index; runSelected() }
                    }
                }
                .padding(GH.Space.s)
            }
            .frame(maxHeight: 360)
        }
        .frame(width: GH.Size.paletteWidth)
        .glassChrome(.transient, cornerRadius: GH.Radius.palette)
        .onAppear { focused = true; workspace.hosts.reload() }
        .onChange(of: query) { selection = 0 }
    }

    private var allItems: [Item] {
        var items: [Item] = [
            Item(title: "New Tab", subtitle: nil, symbol: "plus.rectangle", shortcut: "⌘T", kind: "Action") {
                workspace.newTab() },
            Item(title: "Copy Last Output", subtitle: nil, symbol: "doc.on.doc", shortcut: "⇧⌘A", kind: "Action") {
                workspace.selected?.copyLastOutput() },
            Item(title: "Toggle Sessions Sidebar", subtitle: nil, symbol: "sidebar.left", shortcut: "⌃⌘S", kind: "Action") {
                workspace.showsSidebar.toggle() },
            Item(title: "Set Up Touch ID for sudo…", subtitle: "Runs a visible, reviewable script",
                 symbol: "touchid", shortcut: nil, kind: "Security") {
                workspace.runVisibleScript(title: "Touch ID setup",
                                           script: TouchIDSudo.setupScript(includeReattach: false)) },
            Item(title: "Manage SSH Keys and Hosts…", subtitle: "Secure Enclave keys that ask for Touch ID",
                 symbol: "key.horizontal", shortcut: nil, kind: "Security") {
                settingsPane = .ssh
                openSettings() },
        ]
        items += workspace.hosts.hosts.map { host in
            Item(title: host.alias,
                 subtitle: [host.user, host.hostName].compactMap { $0 }.joined(separator: "@"),
                 symbol: "server.rack", shortcut: nil, kind: "Host · \(host.credential.rawValue)") {
                workspace.openHost(host) }
        }
        items += workspace.sessions.map { s in
            Item(title: s.title, subtitle: s.cwd?.path, symbol: "terminal", shortcut: nil, kind: "Tab") {
                workspace.selectedID = s.id }
        }
        let recent = workspace.selected?.timeline.commands.compactMap(\.commandLine) ?? []
        items += Array(Set(recent)).prefix(20).map { cmd in
            Item(title: cmd, subtitle: "Insert without running", symbol: "clock.arrow.circlepath",
                 shortcut: nil, kind: "Recent") {
                workspace.selected?.engine.send(text: cmd) }   // insert only; user presses Return
        }
        return items
    }

    private var results: [Item] {
        guard !query.isEmpty else { return allItems }
        let q = query.lowercased()
        return allItems
            .compactMap { item -> (Item, Int)? in
                let t = item.title.lowercased()
                if t.hasPrefix(q) { return (item, 0) }
                if t.contains(q) { return (item, 1) }
                if (item.subtitle?.lowercased().contains(q) ?? false) { return (item, 2) }
                return nil
            }
            .sorted { $0.1 < $1.1 }
            .map(\.0)
    }

    private func move(_ d: Int) { selection = max(0, min(results.count - 1, selection + d)) }

    private func runSelected() {
        guard results.indices.contains(selection) else { return }
        let item = results[selection]
        workspace.showsPalette = false
        item.action()
    }
}

private struct PaletteRow: View {
    let item: CommandPalette.Item
    let isSelected: Bool
    let isTop: Bool

    var body: some View {
        HStack(spacing: GH.Space.m) {
            Image(systemName: item.symbol).frame(width: 20)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.title).lineLimit(1)
                if let sub = item.subtitle, !sub.isEmpty {
                    Text(sub).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer()
            Text(isTop ? "Top result" : item.kind).font(.caption).foregroundStyle(.secondary)
            if let sc = item.shortcut {
                Text(sc).font(.caption.monospaced()).padding(.horizontal, 6).padding(.vertical, 2)
                    .background(.quaternary, in: RoundedRectangle(cornerRadius: 4))
            }
        }
        .padding(.horizontal, GH.Space.m).padding(.vertical, GH.Space.s)
        // Rows inside glass use fills, not nested glass (no glass-on-glass).
        .background(isSelected ? Color.accentColor.opacity(0.18) : .clear,
                    in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .contentShape(Rectangle())
    }
}
