import DesignSystem
import GlasshouseSecurity
import ServiceManagement
import SSHAgentCore
import SwiftUI

/// Settings split into three short panes (critique of Terminal.app's six-sub-pane profile editor).
struct SettingsView: View {
    var body: some View {
        TabView {
            Tab("Security", systemImage: "touchid") { SecuritySettings() }
            Tab("SSH Keys", systemImage: "key.horizontal") { SSHSettings() }
            Tab("Appearance", systemImage: "paintbrush") { AppearanceSettings() }
        }
        .frame(width: 560, height: 440)
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

    private var store: SecureEnclaveKeyStore { .init(accessGroup: AgentConfiguration.keychainAccessGroup) }
    private var agent: SMAppService { .loginItem(identifier: AgentConfiguration.loginItemIdentifier) }

    var body: some View {
        Form {
            Section("Built-in (recommended)") {
                LabeledContent("Secure Enclave provider") {
                    Text(NativeSecureEnclaveSSH.isAvailable ? "Available" : "Not found")
                        .foregroundStyle(NativeSecureEnclaveSSH.isAvailable ? .green : .secondary)
                }
                Button("Create Secure Enclave Key in New Tab…") {
                    requestVisibleScript(title: "SSH key setup",
                                         script: NativeSecureEnclaveSSH.createKeyScript(label: "glasshouse"))
                }
                .disabled(!NativeSecureEnclaveSSH.isAvailable)
                Text("Apple's sc_auth + /usr/lib/ssh-keychain.dylib. No Glasshouse code in the signing path. Servers need OpenSSH ≥ 8.2 (sk- keys). Register a second recovery key: Secure Enclave keys can't be backed up.")
                    .font(.caption).foregroundStyle(.secondary)
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
        .onAppear(perform: reload)
    }

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
        let line = "\(SSHKeyFormat.ecdsaP256) \(key.publicKeyBlob.base64EncodedString()) \(key.comment)"
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(line, forType: .string)
    }
}

// MARK: - Appearance

private struct AppearanceSettings: View {
    @AppStorage("translucentGrid") private var translucentGrid = false
    @AppStorage("themeFaithfulColors") private var themeFaithful = false
    @AppStorage("fontSize") private var fontSize = 13.0
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast

    var body: some View {
        Form {
            Section("Text") {
                Stepper("Font size: \(Int(fontSize)) pt", value: $fontSize, in: 9...32)
                Toggle("Theme-faithful colors (lower ANSI contrast floor, Lc 45)", isOn: $themeFaithful)
                Text("By default Glasshouse nudges ANSI colors to at least APCA Lc 60 and the default text to Lc 75 against the background. Increase Contrast raises both.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Window") {
                Toggle("Translucent terminal background (Liquid Glass)", isOn: $translucentGrid)
                    .disabled(reduceTransparency || contrast == .increased)
                if reduceTransparency || contrast == .increased {
                    Text("Unavailable while Reduce Transparency or Increase Contrast is on.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Text("Off by default: glass is for controls, the terminal is content. When on, uses Regular glass only and keeps the contrast floor.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }
}
