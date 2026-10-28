import AppKit
import Observation
import ShellIntegration
import UserNotifications

/// One window's worth of tabs plus window-level UI state.
@MainActor
@Observable
final class Workspace {
    var sessions: [TerminalSession] = []
    var selectedID: TerminalSession.ID?
    var showsSidebar = false          // critique U6: collapsed by default — columns matter more
    var showsPalette = false
    var showsFind = false
    var findQuery = ""
    let hosts = SSHHostCatalog()

    /// The workspace whose window was most recently key (target for Settings actions).
    static weak var frontmost: Workspace?

    var selected: TerminalSession? { sessions.first { $0.id == selectedID } }

    init() { newTab() }

    func newTab() {
        add(TerminalSession.local())
    }

    func openHost(_ host: SSHHost) {
        add(TerminalSession.ssh(alias: host.alias))
    }

    /// Runs a script in a new, visible tab (Touch ID setup, Secure Enclave key creation).
    /// The user sees every privileged command — Glasshouse never elevates silently.
    func runVisibleScript(title: String, script: String) {
        let session = TerminalSession.local(shellIntegration: true)
        session.title = title
        add(session)
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("glasshouse-\(UUID().uuidString).sh")
        try? script.write(to: path, atomically: true, encoding: .utf8)
        // Show the script, ask, then run it with bash. Works from zsh, bash and fish prompts.
        let q = "'\(path.path)'"
        session.engine.send(text:
            "clear; cat \(q); echo; printf 'Run these commands? [Return = run, Ctrl-C = cancel] '; " +
            "read _ && /bin/bash \(q); rm -f \(q)\n")
    }

    /// Closing the selected tab selects its right-hand neighbour (else the left), like Safari and Terminal.app.
    /// Closing the last tab leaves `sessions` empty; WorkspaceView then closes the window.
    func close(_ session: TerminalSession) {
        guard let index = sessions.firstIndex(where: { $0.id == session.id }) else { return }
        session.close()
        sessions.remove(at: index)
        if selectedID == session.id {
            selectedID = sessions.isEmpty ? nil : sessions[min(index, sessions.count - 1)].id
        }
    }

    func closeSelected() {
        if let selected { close(selected) }
    }

    private func add(_ s: TerminalSession) {
        s.onCommandFinished = { [weak self] session, record in self?.notifyFinished(session, record) }
        // Terminal.app default: close the tab when the shell exits cleanly; keep it to show a failure.
        s.onExited = { [weak self] session, code in if code == 0 { self?.close(session) } }
        s.onApprovalWhileHidden = { session, ctx in Notifier.approvalWaiting(tab: session.title, ctx: ctx) }
        sessions.append(s)
        selectedID = s.id
    }

    private func notifyFinished(_ s: TerminalSession, _ r: CommandRecord) {
        guard let d = r.duration, d >= s.timeline.notifyAfter,
              !(NSApp.isActive && s.isVisibleAndKey) else { return }
        Notifier.commandFinished(tab: s.title, record: r)
    }
}

enum Notifier {
    static func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    static func commandFinished(tab: String, record: CommandRecord) {
        let c = UNMutableNotificationContent()
        c.title = tab   // always prefixed with the tab name (anti-spoofing)
        let ok = record.exitCode == 0
        c.body = "\(ok ? "✓" : "✗ exit \(record.exitCode ?? -1)") \(record.commandLine ?? "command") · " +
                 String(format: "%.0fs", record.duration ?? 0)
        post(c)
    }

    /// Critique U3: when the requesting pane isn't visible, the card can't be seen — notify instead.
    static func approvalWaiting(tab: String, ctx: ApprovalContext) {
        let c = UNMutableNotificationContent()
        c.title = "\(tab): waiting for Touch ID"
        c.body = "sudo \(ctx.command) (as \(ctx.runAs)) · source: \(ctx.source.rawValue)"
        c.interruptionLevel = .timeSensitive
        post(c)
    }

    private static func post(_ content: UNMutableNotificationContent) {
        let req = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(req)
    }
}
