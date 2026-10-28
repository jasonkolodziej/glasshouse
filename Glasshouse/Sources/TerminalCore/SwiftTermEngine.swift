import AppKit
import SwiftTerm
import ShellIntegration
#if !SWIFTTERM_1
import Synchronization
#endif

/// SwiftTerm adapter. Supports two API generations, selected at build time:
///
/// | SwiftTerm | Build | Output / OSC hook | Geometry & text |
/// |-----------|-------|-------------------|-----------------|
/// | **1.99.1+** (default; 2.0 preview) | no trait | `observeOscEvents` + `setProcessOutputHandler` | `terminalContentSnapshot(region:)` (lock-safe copies) |
/// | **1.20.x** (legacy) | trait `SwiftTerm1` → `SWIFTTERM_1` | override `dataReceived(slice:)` | `getTerminal()` + `buffer` |
///
/// Why two paths: 1.99's new IO pipeline ("New io", #627) removed `getTerminal()` / public `terminal`
/// ("you can no longer hold on to the underlying terminal"), and `LocalProcess` now delivers bytes to an
/// internal adapter that feeds the renderer directly — `LocalProcessTerminalView.dataReceived(slice:)` is
/// still `open` but is **no longer called**, so overriding it would compile and silently see nothing.
/// 1.99.0 lacks `terminalContentSnapshot`; the 1.99 path needs **1.99.1 or later**.
/// Verified against SwiftTerm source at tags v1.20.0 and v1.99.1.
///
/// Row coordinates everywhere are SwiftTerm's *scroll-invariant* rows (`linesTop + index`), so marks stay
/// valid as scrollback is trimmed.
@MainActor
public final class SwiftTermEngine: NSObject, TerminalEngine {
    private let terminalView: GlasshouseTerminalView
    public var onEvent: ((EngineEvent) -> Void)?
    public var view: NSView { terminalView }

    #if SWIFTTERM_1
    private var scanner = OSCScanner()
    #else
    private var oscObservation: TerminalOscObservation?
    private let outputCoalescer = OutputCoalescer()
    #endif

    public override init() {
        terminalView = GlasshouseTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        super.init()
        terminalView.processDelegate = self
        installHooks()
    }

    // MARK: Output and OSC hooks

    #if SWIFTTERM_1
    private func installHooks() {
        terminalView.onBytes = { [weak self] slice in
            guard let self else { return }
            for event in self.scanner.scan(slice) { self.onEvent?(.shell(event, row: nil)) }
            self.onEvent?(.outputActivity)
        }
    }
    #else
    private func installHooks() {
        // OSC events arrive on a private serial queue, in parser order, with the cursor position
        // (screen-relative) at the moment the parser saw them.
        oscObservation = terminalView.observeOscEvents { [weak self] event in
            guard Self.isInteresting(event.code) else { return }
            let payload = "\(event.code);" + String(decoding: event.payload, as: UTF8.self)
            let screenRow = event.cursor.row
            Task { @MainActor [weak self] in
                guard let self, let shellEvent = OSCScanner.parse(payload) else { return }
                // Screen row → scroll-invariant row. Exact unless output scrolled between the parse
                // and this hop; OSC 133 marks are emitted at prompt boundaries, when output is idle.
                let absolute = self.viewport().map { $0.liveTopRow + screenRow }
                self.onEvent?(.shell(shellEvent, row: absolute))
            }
        }
        // Runs on the IO parse thread after each applied output batch. Heavy output produces many
        // batches; coalesce them so at most one main-actor hop is in flight at a time.
        let coalescer = outputCoalescer
        terminalView.setProcessOutputHandler { [weak self] in
            guard coalescer.claim() else { return }
            Task { @MainActor [weak self] in
                coalescer.release()
                self?.onEvent?(.outputActivity)
            }
        }
    }

    nonisolated private static func isInteresting(_ code: Int) -> Bool {
        code == 133 || code == 7 || code == 9 || code == 777
    }

    private func viewport() -> TerminalContentSnapshot? {
        terminalView.terminalContentSnapshot(region: .viewport)
    }
    #endif

    // MARK: Lifecycle

    public func start(_ plan: ShellIntegrationInstaller.LaunchPlan) {
        let env = plan.environment.map { "\($0.key)=\($0.value)" }
        terminalView.startProcess(executable: plan.executable, args: plan.args,
                                  environment: env, execName: plan.execName)
    }

    public func send(text: String) { terminalView.send(txt: text) }

    public func terminate() {
        if let pid = shellPID, pid > 0 { kill(pid, SIGHUP) } // like closing a Terminal.app tab
    }

    // Public in both 1.20 and 1.99.1 (1.99 reads them under the session lock).
    public var ptyMasterFD: Int32? { terminalView.process.map { $0.childfd } }
    public var shellPID: pid_t? { terminalView.process.map { $0.shellPid } }

    // MARK: Geometry (scroll-invariant rows)

    #if SWIFTTERM_1
    private var term: Terminal { terminalView.getTerminal() }

    public func topVisibleRow() -> Int { term.buffer.totalLinesTrimmed + term.buffer.yDisp }
    /// 1.20 doesn't expose `yBase`, so this is exact only while the view is at the live bottom
    /// (which is when OSC marks arrive). Scrolled back, it reports the row under the viewport.
    public func cursorAbsoluteRow() -> Int { topVisibleRow() + term.getCursorLocation().y }
    public func totalRows() -> Int { topVisibleRow() + term.rows }

    public func text(fromRow: Int, toRow: Int) -> String {
        guard toRow > fromRow else { return "" }
        let trimmed = term.buffer.totalLinesTrimmed
        // VERIFY: getText's Position.row is a buffer-line index (not screen-relative) in 1.20.
        return term.getText(start: Position(col: 0, row: fromRow - trimmed),
                            end: Position(col: term.cols, row: toRow - 1 - trimmed))
    }
    #else
    public func topVisibleRow() -> Int { viewport()?.capturedRange.lowerBound ?? 0 }
    public func cursorAbsoluteRow() -> Int { viewport()?.cursorRow ?? 0 }
    public func totalRows() -> Int {
        guard let v = viewport() else { return 1 }
        return v.liveTopRow + v.inputState.dimensions.rows
    }

    public func text(fromRow: Int, toRow: Int) -> String {
        guard toRow > fromRow, let v = viewport() else { return "" }
        // Copy only as much history as the range needs (bounded, lock-safe copy).
        let needed = max(0, v.liveTopRow - fromRow)
        guard let snap = terminalView.terminalContentSnapshot(
            region: .history(maximumScrollbackRows: needed)) else { return "" }
        return snap.rows
            .filter { $0.absoluteRow >= fromRow && $0.absoluteRow < toRow }
            .map { $0.text.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression) }
            .joined(separator: "\n")
    }
    #endif

    /// Relative scrolling avoids converting between invariant rows and `scrollTo(row:)`'s
    /// buffer-index coordinates (which would need the non-public `linesTop`).
    public func scroll(toAbsoluteRow row: Int) {
        let delta = row - topVisibleRow()
        if delta > 0 { terminalView.scrollDown(lines: delta) }
        else if delta < 0 { terminalView.scrollUp(lines: -delta) }
    }

    // MARK: Appearance

    public func apply(_ a: TerminalAppearance) {
        terminalView.font = a.font
        terminalView.nativeForegroundColor = a.foreground
        terminalView.nativeBackgroundColor = a.background.withAlphaComponent(a.backgroundOpacity)
        terminalView.caretColor = a.cursor
        terminalView.installColors(a.ansi.map(Self.swiftTermColor))
        // Glasshouse applies its own APCA floor (DesignSystem) to the palette; SwiftTerm 1.99.1's
        // WCAG-based `minimumContrastRatio` stays at its default (1 = off) to avoid double adjustment.
        terminalView.layer?.isOpaque = a.backgroundOpacity >= 1
    }

    private static func swiftTermColor(_ c: NSColor) -> SwiftTerm.Color {
        let rgb = c.usingColorSpace(.sRGB) ?? c
        return SwiftTerm.Color(red: UInt16(rgb.redComponent * 65535),
                               green: UInt16(rgb.greenComponent * 65535),
                               blue: UInt16(rgb.blueComponent * 65535))
    }
}

// The protocol is @MainActor in 1.99 and non-isolated in 1.20; `nonisolated` + assumeIsolated
// satisfies both (SwiftTerm delivers these on the main queue in both versions).
extension SwiftTermEngine: LocalProcessTerminalViewDelegate {
    nonisolated public func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {
        MainActor.assumeIsolated { onEvent?(.resized(cols: newCols, rows: newRows)) }
    }
    nonisolated public func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        MainActor.assumeIsolated { onEvent?(.title(title)) }
    }
    nonisolated public func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {
        // OSC 7 is handled through Glasshouse's own OSC path.
    }
    nonisolated public func processTerminated(source: TerminalView, exitCode: Int32?) {
        MainActor.assumeIsolated { onEvent?(.exited(code: exitCode)) }
    }
}

#if !SWIFTTERM_1
/// Lets the parse thread post "output happened" without flooding the main actor.
final class OutputCoalescer: Sendable {
    private let pending = Mutex(false)
    /// True if the caller should post (no post currently in flight).
    func claim() -> Bool { pending.withLock { p in if p { return false }; p = true; return true } }
    func release() { pending.withLock { $0 = false } }
}
#endif

/// On 1.20, taps the PTY byte stream before SwiftTerm parses it. On 1.99 the override would never
/// be called (see above), so the subclass is empty there.
final class GlasshouseTerminalView: LocalProcessTerminalView {
    #if SWIFTTERM_1
    var onBytes: ((ArraySlice<UInt8>) -> Void)?

    override func dataReceived(slice: ArraySlice<UInt8>) {
        super.dataReceived(slice: slice)
        onBytes?(slice)
    }
    #endif
    // Accessibility: SwiftTerm provides its own NSAccessibility implementation (MacAccessibilityService
    // in 1.99); don't override it here.
}
