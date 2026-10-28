import Foundation

/// One command the shell ran, reconstructed from OSC 133 marks.
public struct CommandRecord: Identifiable, Equatable, Sendable {
    public let id: UUID
    /// Absolute scrollback row where the prompt began (A). Used for jump-to-prompt and gutter marks.
    public var promptRow: Int
    /// Row where output began (C), if the command ran.
    public var outputRow: Int?
    /// Row of the D mark — end of output.
    public var endRow: Int?
    public var commandLine: String?
    public var workingDirectory: URL?
    public var startedAt: Date?
    public var finishedAt: Date?
    public var exitCode: Int32?

    public var isRunning: Bool { startedAt != nil && finishedAt == nil }
    public var duration: TimeInterval? {
        guard let s = startedAt else { return nil }
        return (finishedAt ?? Date()).timeIntervalSince(s)
    }
    public var status: Status {
        if isRunning { return .running }
        guard let code = exitCode else { return .unknown }
        return code == 0 ? .succeeded : .failed
    }
    public enum Status: Sendable { case running, succeeded, failed, unknown }
}

/// Folds shell events into a list of commands for the scrollbar gutter, sticky header,
/// finish notifications and "copy output". The engine supplies the cursor's absolute row.
public struct CommandTimeline: Sendable {
    public private(set) var commands: [CommandRecord] = []
    public private(set) var cwd: URL?
    /// Commands longer than this trigger a "finished" notification when the window isn't key.
    public var notifyAfter: TimeInterval = 5

    public init() {}

    public var current: CommandRecord? { commands.last }

    /// Returns a record when a command just finished, so the UI can notify.
    @discardableResult
    public mutating func apply(_ event: ShellEvent, cursorRow: Int, now: Date = Date()) -> CommandRecord? {
        switch event {
        case .promptStart:
            commands.append(CommandRecord(id: UUID(), promptRow: cursorRow, workingDirectory: cwd))
        case .inputStart:
            break
        case .outputStart:
            guard !commands.isEmpty else { return nil }
            commands[commands.count - 1].outputRow = cursorRow
            commands[commands.count - 1].startedAt = now
        case .commandFinished(let code):
            guard !commands.isEmpty, commands[commands.count - 1].startedAt != nil else { return nil }
            commands[commands.count - 1].endRow = cursorRow
            commands[commands.count - 1].finishedAt = now
            commands[commands.count - 1].exitCode = code
            return commands[commands.count - 1]
        case .workingDirectory(let url):
            cwd = url
        case .notification, .progress:
            break
        }
        return nil
    }

    /// The engine reads the typed command between B and C; the app records it here.
    public mutating func setCommandLine(_ text: String) {
        guard !commands.isEmpty else { return }
        commands[commands.count - 1].commandLine = text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public func previousPrompt(before row: Int) -> CommandRecord? {
        commands.last { $0.promptRow < row }
    }

    public func nextPrompt(after row: Int) -> CommandRecord? {
        commands.first { $0.promptRow > row }
    }
}
