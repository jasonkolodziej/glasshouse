import AppKit
import DesignSystem
import GlasshouseSecurity
import Observation
import ShellIntegration
import TerminalCore

/// What the approval card shows. Every field says where it came from (design spec: "Source" line).
struct ApprovalContext: Equatable {
    enum Source: String { case kernel = "kernel process table", shell = "reported by shell" }
    enum Phase: Equatable { case waitingForTouchID, passwordFallback }

    var command: String
    var runAs: String
    var host: String
    var tabTitle: String
    var directory: String?
    var source: Source
    var requesterPID: pid_t?
    var phase: Phase
}

/// One terminal tab. Owns the engine, the OSC 133 timeline and the kernel-derived trust state.
@MainActor
@Observable
final class TerminalSession: Identifiable {
    let id = UUID()
    let engine: any TerminalEngine
    var title = "Shell"
    var cwd: URL?
    var exited = false

    private(set) var timeline = CommandTimeline()
    private(set) var trust: TrustState = .idle
    private(set) var approval: ApprovalContext?

    /// Scroll position, for the sticky header and gutter.
    private(set) var topVisibleRow = 0
    private(set) var totalRows = 1
    private(set) var cursorRow = 0
    private(set) var visibleRows = 24

    /// Set by the view: true while this tab's pane is on screen in a key window.
    var isVisibleAndKey = false { didSet { refreshSecureInput() } }

    var onCommandFinished: ((TerminalSession, CommandRecord) -> Void)?
    var onApprovalWhileHidden: ((TerminalSession, ApprovalContext) -> Void)?
    var onExited: ((TerminalSession, Int32?) -> Void)?

    @ObservationIgnored private var pollTimer: Timer?
    @ObservationIgnored private var lastForeground: ProcessSnapshot?

    init(engine: any TerminalEngine = SwiftTermEngine(),
         launch: ShellIntegrationInstaller.LaunchPlan) {
        self.engine = engine
        engine.apply(Theme.current())
        engine.onEvent = { [weak self] in self?.handle($0) }
        engine.start(launch)
    }

    static func local(shellIntegration: Bool = true) -> TerminalSession {
        let env = PTYEnvironment.make(.init(
            appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0",
            agentSocket: nil // per-host IdentityAgent only; never global (see AgentConfiguration)
        ))
        let plan = ShellIntegrationInstaller.plan(baseEnvironment: env, enabled: shellIntegration)
        return TerminalSession(launch: plan)
    }

    /// `ssh <alias>` via the *system* OpenSSH (which honours SSH_SK_PROVIDER / ssh-keychain.dylib).
    static func ssh(alias: String) -> TerminalSession {
        let env = PTYEnvironment.make(.init(appVersion: "0"))
        let plan = ShellIntegrationInstaller.LaunchPlan(executable: "/usr/bin/ssh", args: [alias],
                                                         execName: "ssh", environment: env)
        let s = TerminalSession(launch: plan)
        s.title = alias
        return s
    }

    // MARK: Engine events

    private func handle(_ event: EngineEvent) {
        switch event {
        case .shell(let shellEvent, let reportedRow):
            // Prefer the row the engine captured when the parser saw the mark (SwiftTerm 1.99.1+).
            let row = reportedRow ?? engine.cursorAbsoluteRow()
            if case .inputStart = shellEvent { inputStartRow = row }
            if case .outputStart = shellEvent, let start = inputStartRow {
                // Command text between B and C — display data only, never used for trust.
                timeline.setCommandLine(engine.text(fromRow: start, toRow: row + 1))
            }
            if let finished = timeline.apply(shellEvent, cursorRow: row) {
                onCommandFinished?(self, finished)
            }
            if case .workingDirectory(let url) = shellEvent { cwd = url }
        case .title(let t):
            title = Self.sanitizeTitle(t)
        case .outputActivity:
            refreshGeometry()
            refreshTrust()
        case .resized(_, let rows):
            visibleRows = rows
            refreshGeometry()
        case .bell:
            NSSound.beep()
        case .exited(let code):
            exited = true
            stopPolling()
            setTrust(.idle, approval: nil)
            onExited?(self, code)
        }
    }

    @ObservationIgnored private var inputStartRow: Int?

    private func refreshGeometry() {
        topVisibleRow = engine.topVisibleRow()
        totalRows = max(engine.totalRows(), 1)
        cursorRow = engine.cursorAbsoluteRow()
    }

    // MARK: Trust state (kernel-derived only)

    /// Re-evaluates who owns the foreground and whether the tty is in secret-input mode.
    /// Called on output activity, on keystrokes, and every 100 ms while sudo/ssh is in the foreground.
    func refreshTrust() {
        guard let fd = engine.ptyMasterFD else { return }
        let fg = ProcessInspector.foreground(masterFD: fd)
        let secret = PasswordPromptDetector.state(masterFD: fd) == .secretInput

        if let fg, fg.isSSH {
            startPolling()
            let dest = fg.sshDestination ?? "remote host"
            // Local secret mode while ssh is foreground = ssh's own passphrase/password prompt.
            setTrust(secret ? .secureInput : .remoteSession(dest), approval: nil)
        } else if let fg, fg.isSudo {
            startPolling()
            let ctx = approvalContext(for: fg, phase: secret ? .passwordFallback : .waitingForTouchID)
            setTrust(secret ? .secureInput : .touchIDSudo, approval: ctx)
        } else if secret {
            setTrust(.secureInput, approval: nil)
        } else {
            stopPolling()
            setTrust(.idle, approval: nil)
        }
        lastForeground = fg
    }

    private func approvalContext(for p: ProcessSnapshot, phase: ApprovalContext.Phase) -> ApprovalContext {
        // KERN_PROCARGS2 on setuid-root sudo may fail (UNVERIFIED). If argv is just ["sudo"]-ish,
        // fall back to the shell-reported command line and say so on the card.
        let kernelArgs = p.arguments.count > 1 ? Array(p.arguments.dropFirst()) : []
        let (command, source): (String, ApprovalContext.Source) = kernelArgs.isEmpty
            ? (timeline.current?.commandLine ?? "sudo", .shell)
            : (kernelArgs.map { ProcessSnapshot.shellQuoteForDisplay($0) }.joined(separator: " "), .kernel)
        let runAs = Self.sudoTargetUser(kernelArgs) ?? "root"
        return ApprovalContext(command: command, runAs: runAs, host: Host.current().localizedName ?? "This Mac",
                               tabTitle: title, directory: cwd?.path, source: source,
                               requesterPID: p.pid, phase: phase)
    }

    private func setTrust(_ new: TrustState, approval newApproval: ApprovalContext?) {
        let appeared = approval == nil && newApproval != nil
        trust = new
        approval = newApproval
        refreshSecureInput()
        if appeared, let ctx = newApproval {
            if isVisibleAndKey {
                NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested, userInfo: [
                    .announcement: "sudo is waiting for Touch ID: \(ctx.command), as \(ctx.runAs)",
                    .priority: NSAccessibilityPriorityLevel.high.rawValue,
                ])
            } else {
                onApprovalWhileHidden?(self, ctx)   // critique U3: notify instead of an unseen card
            }
        }
    }

    private func refreshSecureInput() {
        SecureKeyboardEntry.shared.update(secretPromptFocused: isVisibleAndKey && trust == .secureInput)
    }

    private func startPolling() {
        guard pollTimer == nil else { return }
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshTrust() }
        }
    }

    private func stopPolling() { pollTimer?.invalidate(); pollTimer = nil }

    // MARK: Actions

    func jumpToPreviousPrompt() {
        if let r = timeline.previousPrompt(before: topVisibleRow) { engine.scroll(toAbsoluteRow: r.promptRow) }
    }
    func jumpToNextPrompt() {
        if let r = timeline.nextPrompt(after: topVisibleRow) { engine.scroll(toAbsoluteRow: r.promptRow) }
    }
    func jump(to record: CommandRecord) { engine.scroll(toAbsoluteRow: record.promptRow) }

    func output(of record: CommandRecord) -> String {
        guard let start = record.outputRow else { return "" }
        return engine.text(fromRow: start, toRow: record.endRow ?? engine.cursorAbsoluteRow())
    }

    func copyLastOutput() {
        guard let last = timeline.commands.last(where: { $0.finishedAt != nil }) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(output(of: last), forType: .string)
    }

    func clearScrollback() { engine.send(text: "\u{0C}") } // TODO: engine-level scrollback clear (⌘K)

    func close() {
        if !exited { engine.terminate() } // an exited shell's PID may already belong to another process
        stopPolling()
        SecureKeyboardEntry.shared.update(secretPromptFocused: false)
    }

    // MARK: Helpers

    /// Program-set titles (OSC 0/2) never reach trusted chrome; strip glyphs that imitate it anyway.
    static func sanitizeTitle(_ raw: String) -> String {
        let banned: Set<Character> = ["🔒", "🔐", "🔑", "🛡", "✅", "☑", "✔"]
        let cleaned = raw.filter { !banned.contains($0) && !$0.isNewline }
        return String(cleaned.prefix(80))
    }

    static func sudoTargetUser(_ args: [String]) -> String? {
        if let i = args.firstIndex(of: "-u"), i + 1 < args.count { return args[i + 1] }
        if let a = args.first(where: { $0.hasPrefix("--user=") }) { return String(a.dropFirst(7)) }
        return nil
    }
}

extension ProcessSnapshot {
    static func shellQuoteForDisplay(_ s: String) -> String {
        s.contains(" ") ? "'\(s)'" : s
    }
}
