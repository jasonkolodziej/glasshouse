import DesignSystem
import GlasshouseSecurity
import ServiceManagement
import SSHAgentCore
import SSHConfig
import SwiftUI
import UniformTypeIdentifiers

/// Which Settings pane is showing. Stored so other views (the command palette) can open a specific one.
enum SettingsPane: String {
    case security, ssh, appearance
    static let storageKey = "settingsPane"
}

/// Settings split into three short panes (critique of Terminal.app's six-sub-pane profile editor).
struct SettingsView: View {
    @AppStorage(SettingsPane.storageKey) private var pane: SettingsPane = .security

    var body: some View {
        TabView(selection: $pane) {
            Tab("Security", systemImage: "touchid", value: .security) { SecuritySettings() }
            Tab("SSH Keys", systemImage: "key.horizontal", value: .ssh) { SSHSettings() }
            Tab("Appearance", systemImage: "paintbrush", value: .appearance) { AppearanceSettingsView() }
        }
        .frame(width: 680, height: 640)
    }
}

/// Settings is its own scene; scripts open as a new tab in the most recently focused window.
@MainActor
func requestVisibleScript(title: String, script: String) {
    Workspace.frontmost?.runVisibleScript(title: title, script: script)
    NSApp.activate()
}

// MARK: - Security

private struct SecuritySettings: View {
    @State private var status = TouchIDSudo.status()
    @State private var includeReattach = false
    @AppStorage("secureKeyboardAlways") private var secureAlways = false

    var body: some View {
        Form {
            Section("Touch ID for sudo") {
                LabeledContent("pam_tid in sudo_local") { statusBadge(status.pamTidEnabled) }
                if status.pamReattachConfigured {
                    LabeledContent("pam_reattach (tmux)") { statusBadge(true) }
                }
                if status.pamReattachUserWritable {
                    Label("pam_reattach is loaded from a user-writable path. Any process running as you could replace it and gain root on your next sudo. Re-run setup to install a root-owned copy.",
                          systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
                Toggle("I use tmux or screen (install pam_reattach, root-owned)", isOn: $includeReattach)
                HStack {
                    Button(status.pamTidEnabled ? "Re-run Setup in New Tab…" : "Set Up in New Tab…") {
                        requestVisibleScript(title: "Touch ID setup",
                                             script: TouchIDSudo.setupScript(includeReattach: includeReattach))
                    }
                    Button("Roll Back…") {
                        requestVisibleScript(title: "Touch ID rollback", script: TouchIDSudo.rollbackScript)
                    }
                    Spacer()
                    Button("Refresh") { status = TouchIDSudo.status() }
                }
                Text("Glasshouse never edits /etc itself. Setup opens a tab showing every command before it runs. Password stays the fallback; each Touch ID tap authorizes exactly one command.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Secure Keyboard Entry") {
                LabeledContent("Automatic") { Text("On while a password prompt has focus").foregroundStyle(.secondary) }
                Toggle("Always on (CIS benchmark)", isOn: $secureAlways)
                    .onChange(of: secureAlways) { _, on in SecureKeyboardEntry.shared.alwaysOn = on }
                Text("Always-on can disable global hotkeys and interfere with accessibility tools in other apps.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func statusBadge(_ ok: Bool) -> some View {
        Label(ok ? "Enabled" : "Not set up", systemImage: ok ? "checkmark.circle.fill" : "circle.dashed")
            .foregroundStyle(ok ? .green : .secondary)
    }
}

// MARK: - SSH

private struct SSHSettings: View {
    @State private var agentStatus = SMAppService.loginItem(identifier: AgentConfiguration.loginItemIdentifier).status
    @State private var keys: [AgentKey] = []
    @State private var newComment = "\(NSUserName())@\(Host.current().localizedName ?? "mac")"
    @State private var strictBiometry = false
    @State private var error: String?

    // Built-in Secure Enclave keys and the hosts in ~/.ssh/glasshouse.conf.
    private let sshStore = SSHConfigStore.standard()
    @State private var seKeys: [SSHKeyFile] = []
    @State private var securityKeyFiles: [SSHKeyFile] = []
    @State private var managed = ManagedSSHConfig()
    @State private var includeStatus: SSHIncludeStatus = .active
    @State private var conflicts: [SSHConfigConflict] = []
    @State private var newKeyName = "glasshouse"
    @State private var editing: HostDraft?
    @State private var sshError: String?

    private var store: SecureEnclaveKeyStore { .init(accessGroup: AgentConfiguration.keychainAccessGroup) }
    private var agent: SMAppService { .loginItem(identifier: AgentConfiguration.loginItemIdentifier) }

    var body: some View {
        Form {
            Section("Built-in (recommended)") {
                LabeledContent("Secure Enclave provider") {
                    Text(NativeSecureEnclaveSSH.isAvailable ? "Available" : "Not found")
                        .foregroundStyle(NativeSecureEnclaveSSH.isAvailable ? .green : .secondary)
                }
                ForEach(seKeys) { key in
                    HStack {
                        VStack(alignment: .leading) {
                            Text("~/.ssh/\(key.name)")
                            let users = managed.hosts(usingKey: key.name).map(\.alias)
                            Text(users.isEmpty ? key.comment : "\(key.comment) · \(users.joined(separator: ", "))")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Copy Public Key") { copy(key.publicKey) }
                    }
                }
                HStack {
                    TextField("New key name", text: $newKeyName)
                    Button("Create Key in New Tab…") { createSecureEnclaveKey() }
                        .disabled(!NativeSecureEnclaveSSH.isAvailable || !canCreateKey)
                }
                Text("Apple's sc_auth + /usr/lib/ssh-keychain.dylib. No Glasshouse code in the signing path. Servers need OpenSSH ≥ 8.2 (sk- keys). Register a second recovery key: Secure Enclave keys can't be backed up.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("Hosts") {
                includeStatusRow
                ForEach(conflicts) { conflict in
                    Label("~/.ssh/config line \(conflict.line) also defines “\(conflict.alias)”. Glasshouse's entry is read first, so its host, user, port and key win, but that block's other settings still apply. Remove it if it's left over.",
                          systemImage: "exclamationmark.triangle")
                        .font(.caption).foregroundStyle(.orange)
                }
                ForEach(managed.hosts) { host in hostRow(host) }
                HStack {
                    Button("Add Host…") {
                        editing = HostDraft(keyName: seKeys.first?.name ?? newKeyName)
                    }
                    .disabled(seKeys.isEmpty)
                    Spacer()
                    Button("Refresh") { reloadSSH() }
                }
                Text("Hosts are written to ~/.ssh/glasshouse.conf. Each one signs only with its Secure Enclave key, so every login asks for Touch ID, even when other keys in ~/.ssh/config or ssh-agent would work.")
                    .font(.caption).foregroundStyle(.secondary)
                if let sshError { Text(sshError).foregroundStyle(.red).font(.caption) }
            }

            Section("Glasshouse Agent (optional)") {
                Toggle("Run Glasshouse Agent at login", isOn: Binding(
                    get: { agentStatus == .enabled },
                    set: { setAgent($0) }))
                if agentStatus == .requiresApproval {
                    Button("Approve in System Settings…") { SMAppService.openSystemSettingsLoginItems() }
                }
                Text("Adds the destination host and requesting process to the Touch ID prompt, per-host rules, and plain ecdsa-sha2-nistp256 keys for servers that reject sk- keys. Used only by hosts you point at it with IdentityAgent.")
                    .font(.caption).foregroundStyle(.secondary)

                ForEach(keys) { key in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(key.comment)
                            Text(key.fingerprint).font(.caption.monospaced()).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Copy Public Key") { copyPublicKey(key) }
                        Button(role: .destructive) { delete(key) } label: { Image(systemName: "trash") }
                    }
                }
                HStack {
                    TextField("Comment", text: $newComment)
                    Toggle("Strict (re-enrolling a finger invalidates)", isOn: $strictBiometry).toggleStyle(.checkbox)
                    Button("New Key") { create() }
                }
                if let error { Text(error).foregroundStyle(.red).font(.caption) }
            }
        }
        .formStyle(.grouped)
        .onAppear { reload(); reloadSSH() }
        .sheet(item: $editing) { draft in
            HostEditor(draft: draft, keys: seKeys.map(\.name), securityKeys: securityKeyFiles.map(\.name),
                       save: saveHost)
        }
    }

    // MARK: Built-in keys and hosts

    private var canCreateKey: Bool {
        ManagedSSHHost.isValidKeyName(newKeyName) && !seKeys.contains { $0.name == newKeyName }
    }

    @ViewBuilder private var includeStatusRow: some View {
        switch includeStatus {
        case .active:
            Label("~/.ssh/config includes ~/.ssh/glasshouse.conf", systemImage: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .missing:
            VStack(alignment: .leading, spacing: 6) {
                Label("~/.ssh/config doesn't include Glasshouse's hosts yet, so ssh ignores them.",
                      systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                Button("Add Include to ~/.ssh/config") { installInclude() }
                Text("Adds a four-line block at the top of the file. The original is kept as ~/.ssh/config.glasshouse-backup.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        case .inactive(let line):
            VStack(alignment: .leading, spacing: 6) {
                Label("The Include on line \(line) of ~/.ssh/config comes after a Host or Match line, so it only applies inside that block.",
                      systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                Button("Move It to the Top") { installInclude() }
            }
        }
    }

    private func hostRow(_ host: ManagedSSHHost) -> some View {
        HStack {
            VStack(alignment: .leading) {
                Text(host.alias)
                Text("\(host.user.map { "\($0)@" } ?? "")\(host.hostName)\(host.port.map { ":\($0)" } ?? "") · ~/.ssh/\(host.keyName)")
                    .font(.caption).foregroundStyle(.secondary)
                if let securityKeyAlias = host.securityKeyAlias {
                    Text("Security key: ssh \(securityKeyAlias)").font(.caption).foregroundStyle(.secondary)
                }
                if host.backupKeys.isEmpty && host.securityKey == nil {
                    Label("No backup key: without this Mac you'd need another way in", systemImage: "exclamationmark.triangle")
                        .font(.caption).foregroundStyle(.orange)
                } else if !host.backupKeys.isEmpty {
                    Text(host.backupKeys.count == 1 ? "1 backup key" : "\(host.backupKeys.count) backup keys")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            Button("Install Keys…") { installKeys(on: host) }
                .disabled(includeStatus != .active)
                .help(includeStatus == .active
                      ? "Adds this host's keys to authorized_keys on the server, tests a Touch ID login, and lists keys that skip Touch ID"
                      : "Add the Include to ~/.ssh/config first")
            Button("Connect") {
                Workspace.frontmost?.openHost(SSHHost(alias: host.alias))
                NSApp.activate()
            }
            Menu {
                Button("Edit…") { editing = HostDraft(host) }
                Button("Remove from Glasshouse", role: .destructive) { removeHost(host) }
            } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.button).buttonStyle(.borderless).menuIndicator(.hidden).fixedSize()
                .accessibilityLabel("More actions for \(host.alias)")
        }
    }

    private func reloadSSH() {
        seKeys = sshStore.secureEnclaveKeys()
        securityKeyFiles = sshStore.securityKeyFiles()
        do {
            managed = try sshStore.load()
            sshError = nil
        } catch {
            sshError = "Couldn't read \(sshStore.storeURL.path): \(error.localizedDescription)"
        }
        includeStatus = sshStore.includeStatus()
        conflicts = sshStore.conflicts(for: managed)
        if seKeys.contains(where: { $0.name == newKeyName }) {
            newKeyName = Self.freeKeyName(taken: Set(seKeys.map(\.name)))
        }
    }

    /// "glasshouse", then "glasshouse-2", "glasshouse-3", …
    private static func freeKeyName(taken: Set<String>) -> String {
        if !taken.contains("glasshouse") { return "glasshouse" }
        return (2...).lazy.map { "glasshouse-\($0)" }.first { !taken.contains($0) }!
    }

    private func createSecureEnclaveKey() {
        do {
            requestVisibleScript(title: "SSH key setup",
                                 script: try GlasshouseSSHScripts.createSecureEnclaveKey(name: newKeyName))
            sshError = nil
        } catch { sshError = error.localizedDescription }
    }

    private func installInclude() {
        do { try sshStore.installInclude() } catch { sshError = "~/.ssh/config: \(error.localizedDescription)" }
        reloadSSH()
    }

    private func saveHost(_ draft: HostDraft) throws {
        let portText = draft.port.trimmingCharacters(in: .whitespaces)
        let port: Int? = portText.isEmpty ? nil : (Int(portText) ?? -1)   // -1 is reported as out of range
        let securityKey = draft.securityKeyName.isEmpty ? nil
            : ManagedSSHHost.SecurityKey(keyName: draft.securityKeyName, provider: draft.securityKeyProvider)
        let backupKeys = draft.backupKeysText.split(whereSeparator: \.isNewline).map(String.init)
            .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty && !$0.hasPrefix("#") }
        var config = try sshStore.load()
        try config.upsert(ManagedSSHHost(alias: draft.alias, hostName: draft.hostName, user: draft.user,
                                         port: port, keyName: draft.keyName, securityKey: securityKey,
                                         backupKeys: backupKeys),
                          replacing: draft.original)
        try sshStore.save(config)
        reloadSSH()
        Workspace.frontmost?.hosts.reload()
    }

    private func removeHost(_ host: ManagedSSHHost) {
        do {
            var config = try sshStore.load()
            config.remove(alias: host.alias)
            try sshStore.save(config)
        } catch { sshError = error.localizedDescription }
        reloadSSH()
        Workspace.frontmost?.hosts.reload()
    }

    private func installKeys(on host: ManagedSSHHost) {
        do {
            let touchIDKey = try sshStore.publicKey(named: host.keyName)
            let securityKey = try host.securityKey.map { try sshStore.publicKey(named: $0.keyName) }
            requestVisibleScript(title: "Install keys on \(host.alias)",
                                 script: try GlasshouseSSHScripts.installKeys(on: host, touchIDKey: touchIDKey,
                                                                              securityKey: securityKey))
        } catch { sshError = error.localizedDescription }
    }

    private func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    // MARK: Glasshouse Agent

    private func reload() {
        keys = (try? store.listKeys()) ?? []
        agentStatus = agent.status
    }

    private func setAgent(_ on: Bool) {
        do { if on { try agent.register() } else { try agent.unregister() } }
        catch { self.error = "Agent: \(error.localizedDescription)" }
        agentStatus = agent.status
    }

    private func create() {
        do {
            _ = try store.createKey(comment: newComment,
                                    policy: KeyPolicy(requireBiometryCurrentSet: strictBiometry))
            reload()
        } catch { self.error = "Create key: \(error)" }
    }

    private func delete(_ key: AgentKey) {
        // TODO: confirmation sheet — remove from servers first; deletion is irreversible.
        try? store.deleteKey(id: key.id); reload()
    }

    private func copyPublicKey(_ key: AgentKey) {
        copy("\(SSHKeyFormat.ecdsaP256) \(key.publicKeyBlob.base64EncodedString()) \(key.comment)")
    }
}

/// The host sheet's fields as typed. `original` is the alias being edited, or nil for a new host.
private struct HostDraft: Identifiable {
    let id = UUID()
    var original: String?
    var alias = ""
    var hostName = ""
    var user = ""
    var port = ""
    var keyName: String
    /// Empty for none.
    var securityKeyName = ""
    var securityKeyProvider = GlasshouseSSH.internalProvider
    /// One public key per line.
    var backupKeysText = ""

    init(keyName: String) { self.keyName = keyName }

    init(_ host: ManagedSSHHost) {
        original = host.alias
        alias = host.alias
        hostName = host.hostName
        user = host.user ?? ""
        port = host.port.map(String.init) ?? ""
        keyName = host.keyName
        securityKeyName = host.securityKey?.keyName ?? ""
        securityKeyProvider = host.securityKey?.provider ?? GlasshouseSSH.internalProvider
        backupKeysText = host.backupKeys.joined(separator: "\n")
    }
}

private struct HostEditor: View {
    @State var draft: HostDraft
    let keys: [String]
    /// Every sk- key handle; the picker leaves out the one chosen as the Secure Enclave key.
    let securityKeys: [String]
    let save: @MainActor (HostDraft) throws -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?
    @State private var importingBackupKey = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Form {
                TextField("Alias", text: $draft.alias, prompt: Text("myserver"))
                TextField("Host name or IP address", text: $draft.hostName, prompt: Text("myserver.example.com"))
                TextField("User", text: $draft.user, prompt: Text("Optional"))
                TextField("Port", text: $draft.port, prompt: Text("22"))
                Picker("Key", selection: $draft.keyName) {
                    ForEach(keys, id: \.self) { Text("~/.ssh/\($0)").tag($0) }
                }
                Text("You type the alias after ssh, as in “ssh \(draft.alias.isEmpty ? "myserver" : draft.alias)”.")
                    .font(.caption).foregroundStyle(.secondary)

                Section("Security key on this Mac (optional)") {
                    Picker("Key", selection: $draft.securityKeyName) {
                        Text("None").tag("")
                        ForEach(securityKeys.filter { $0 != draft.keyName }, id: \.self) {
                            Text("~/.ssh/\($0)").tag($0)
                        }
                    }
                    if !draft.securityKeyName.isEmpty {
                        TextField("Provider", text: $draft.securityKeyProvider, prompt: Text("internal"))
                        Text("Log in with it as “ssh \(draft.alias.isEmpty ? "myserver" : draft.alias)\(GlasshouseSSH.securityKeyAliasSuffix)” when Touch ID isn't available. “internal” is OpenSSH's built-in FIDO support; if ssh says it isn't enabled, use Homebrew's OpenSSH or a provider library's full path.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }

                Section("Backup keys (from other devices)") {
                    TextEditor(text: $draft.backupKeysText)
                        .font(.caption.monospaced())
                        .frame(minHeight: 70)
                        .accessibilityLabel("Backup public keys, one per line")
                    HStack {
                        Text("One public key per line, as in a .pub file. Install Keys… puts them on the server, so you can log in from those devices if this Mac isn't available.")
                            .font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("Add .pub File…") { importingBackupKey = true }
                    }
                }
                if let error { Text(error).font(.caption).foregroundStyle(.red) }
            }
            .formStyle(.grouped)
            .fileImporter(isPresented: $importingBackupKey, allowedContentTypes: [.item],
                          allowsMultipleSelection: true) { result in addBackupKeys(from: result) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(draft.original == nil ? "Add Host" : "Save") {
                    do { try save(draft); dismiss() } catch { self.error = error.localizedDescription }
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding()
        }
        .frame(width: 520)
    }

    private func addBackupKeys(from result: Result<[URL], Error>) {
        guard case .success(let urls) = result else { return }
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let text = try? String(contentsOf: url, encoding: .utf8) else {
                error = "Couldn't read \(url.lastPathComponent)."; continue
            }
            let lines = text.split(whereSeparator: \.isNewline).map(String.init).filter { !$0.isEmpty }
            if draft.backupKeysText.isEmpty || draft.backupKeysText.hasSuffix("\n") {
                draft.backupKeysText += lines.joined(separator: "\n")
            } else {
                draft.backupKeysText += "\n" + lines.joined(separator: "\n")
            }
        }
    }
}
