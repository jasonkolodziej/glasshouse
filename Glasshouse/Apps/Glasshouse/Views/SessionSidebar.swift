import DesignSystem
import SwiftUI

/// Sidebar (system floating Liquid Glass via NavigationSplitView — no custom material behind it).
struct SessionSidebar: View {
    @Bindable var workspace: Workspace

    var body: some View {
        List(selection: $workspace.selectedID) {
            Section("Open Sessions") {
                ForEach(workspace.sessions) { s in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(s.title).lineLimit(1)
                        if let running = s.timeline.current, running.isRunning, let cmd = running.commandLine {
                            Text(cmd).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1)
                        } else if let cwd = s.cwd {
                            Text(cwd.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                                .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .tag(s.id)
                    .contextMenu { Button("Close Tab") { workspace.close(s) } }
                }
            }
            Section("Hosts") {
                ForEach(workspace.hosts.hosts) { host in
                    Button { workspace.openHost(host) } label: {
                        HStack {
                            Label(host.alias, systemImage: "server.rack")
                            Spacer()
                            Text(host.credential.rawValue)
                                .font(.caption2.weight(.semibold))
                                .padding(.horizontal, 5).padding(.vertical, 1)
                                .background(.quaternary, in: Capsule())
                                .help(help(for: host.credential))
                        }
                    }
                    .buttonStyle(.plain)
                }
                if workspace.hosts.hosts.isEmpty {
                    Text("No hosts in ~/.ssh/config").foregroundStyle(.secondary)
                }
            }
        }
        .listStyle(.sidebar)
    }

    private func help(for c: SSHHost.Credential) -> String {
        switch c {
        case .secureEnclave: "Secure Enclave key via /usr/lib/ssh-keychain.dylib — Touch ID per use"
        case .agent: "Glasshouse Agent — Touch ID prompt names host and process"
        case .other: "Key file or another agent"
        }
    }
}

/// Docked find bar (L2). Pushes the grid down rather than floating over it.
struct FindBar: View {
    @Binding var query: String
    let close: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: GH.Space.s) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Find in scrollback", text: $query)
                .textFieldStyle(.plain)
                .focused($focused)
                .onKeyPress(.escape) { close(); return .handled }
            // TODO: wire to engine search (regex / case / whole word / all tabs, ⌘G ⇧⌘G).
            Button(action: close) { Image(systemName: "xmark.circle.fill") }
                .buttonStyle(.plain).foregroundStyle(.secondary)
                .accessibilityLabel("Close find bar")
        }
        .padding(.horizontal, GH.Space.m).padding(.vertical, GH.Space.s)
        .glassChrome(.controls, cornerRadius: GH.Radius.bar)
        .onAppear { focused = true }
    }
}
