import AppKit
import ShellIntegration

/// Everything the UI needs from a terminal engine. SwiftTerm implements it today (`SwiftTermEngine`);
/// a libghostty adapter can replace it once libghostty ships a stable C API, without touching the UI.
@MainActor
public protocol TerminalEngine: AnyObject {
    /// The AppKit view that renders the grid. Hosted in SwiftUI via `TerminalHostView`.
    var view: NSView { get }
    /// Engine → session events, delivered on the main actor.
    var onEvent: ((EngineEvent) -> Void)? { get set }

    func start(_ plan: ShellIntegrationInstaller.LaunchPlan)
    func send(text: String)
    func terminate()

    /// PTY master fd — used for termios password-prompt detection and foreground-process lookup.
    var ptyMasterFD: Int32? { get }
    var shellPID: pid_t? { get }

    /// Absolute row (scrollback + screen) of the cursor; anchors OSC 133 marks.
    func cursorAbsoluteRow() -> Int
    /// Total rows (scrollback + visible) — for mapping marks onto the scrollbar gutter.
    func totalRows() -> Int
    /// First visible absolute row.
    func topVisibleRow() -> Int
    func scroll(toAbsoluteRow row: Int)
    /// Plain text of absolute rows [from, to). Used by "Copy output" and command-line capture.
    func text(fromRow: Int, toRow: Int) -> String

    func apply(_ appearance: TerminalAppearance)
}

public enum EngineEvent: Sendable {
    /// `row`: scroll-invariant row of the cursor when the parser saw the sequence, if the engine
    /// knows it (SwiftTerm 1.99.1+); otherwise nil and the session asks for the current cursor row.
    case shell(ShellEvent, row: Int?)
    case title(String)
    case resized(cols: Int, rows: Int)
    /// Output arrived. The session uses this as the cue to re-check termios (cheap ioctl, no polling).
    case outputActivity
    case bell
    case exited(code: Int32?)
}

public enum TerminalCursorShape: Sendable, Equatable {
    case block, underline, bar
}

/// Visual settings the engine controls. The grid is always drawn opaque unless the user
/// opts into translucency AND Reduce Transparency is off — see DesignSystem.GlassPolicy.
public struct TerminalAppearance: Sendable, Equatable {
    public var fontName: String
    public var fontSize: CGFloat
    public var foreground: NSColor
    public var background: NSColor
    public var cursor: NSColor
    /// Text under a block cursor; nil lets the engine choose.
    public var cursorText: NSColor?
    /// Selection highlight; nil uses the system's.
    public var selection: NSColor?
    public var cursorShape: TerminalCursorShape
    public var cursorBlink: Bool
    /// Bold text in the bright ANSI colours.
    public var boldIsBright: Bool
    public var backgroundOpacity: CGFloat
    /// 16 ANSI colors, normal then bright.
    public var ansi: [NSColor]
    /// Space around the grid, drawn by the pane (not the engine) in the background colour.
    public var paddingHorizontal: CGFloat
    public var paddingVertical: CGFloat

    public init(fontName: String = "SF Mono", fontSize: CGFloat = 13,
                foreground: NSColor, background: NSColor, cursor: NSColor,
                cursorText: NSColor? = nil, selection: NSColor? = nil,
                cursorShape: TerminalCursorShape = .block, cursorBlink: Bool = false, boldIsBright: Bool = true,
                backgroundOpacity: CGFloat = 1, ansi: [NSColor],
                paddingHorizontal: CGFloat = 10, paddingVertical: CGFloat = 6) {
        self.fontName = fontName; self.fontSize = fontSize
        self.foreground = foreground; self.background = background; self.cursor = cursor
        self.cursorText = cursorText; self.selection = selection
        self.cursorShape = cursorShape; self.cursorBlink = cursorBlink; self.boldIsBright = boldIsBright
        self.backgroundOpacity = backgroundOpacity; self.ansi = ansi
        self.paddingHorizontal = paddingHorizontal; self.paddingVertical = paddingVertical
    }

    public var font: NSFont {
        NSFont(name: fontName, size: fontSize) ?? .monospacedSystemFont(ofSize: fontSize, weight: .regular)
    }
}
