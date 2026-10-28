import AppKit
import SSHAgentCore

/// Glasshouse Agent — an LSUIElement login item that serves Secure Enclave keys over an ssh-agent socket.
/// Opt-in: the main app registers it with SMAppService only when the user enables
/// "Glasshouse Agent" in Settings → SSH. Clients reach it per host via `IdentityAgent`.
@main
@MainActor
enum AgentMain {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory) // no Dock icon, no menu bar; Touch ID sheets still appear

        let store = SecureEnclaveKeyStore(accessGroup: AgentConfiguration.keychainAccessGroup)
        let server = AgentServer(socketPath: AgentConfiguration.socketPath, store: store)
        do {
            try server.start()
        } catch {
            NSLog("Glasshouse Agent failed to start: \(error)")
            exit(1)
        }

        signal(SIGTERM, SIG_IGN)
        let term = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        term.setEventHandler { server.stop(); exit(0) }
        term.resume()

        withExtendedLifetime(term) { app.run() }
    }
}
