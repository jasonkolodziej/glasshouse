import Foundation

/// Semantic events emitted by the shell through OSC escape sequences.
///
/// - OSC 133 (FinalTerm semantic prompts): A = prompt start, B = input start,
///   C = output start (command executing), D[;exit] = command finished.
/// - OSC 7: current working directory as a file:// URL.
/// - OSC 9 / OSC 777;notify: desktop notifications (OSC 9 is ambiguous with ConEmu progress `9;4;…`).
public enum ShellEvent: Equatable, Sendable {
    case promptStart
    case inputStart
    case outputStart
    case commandFinished(exitCode: Int32?)
    case workingDirectory(URL)
    case notification(title: String?, body: String)
    case progress(state: Int, percent: Int?)
}

/// Incremental scanner that pulls OSC sequences out of the PTY byte stream without
/// consuming them — the bytes still go to the terminal engine unchanged.
///
/// Handles sequences split across reads, and both terminators (BEL `0x07` and ST `ESC \`).
public struct OSCScanner: Sendable {
    private enum State: Sendable { case ground, escape, osc, oscEscape }
    private var state: State = .ground
    private var buffer: [UInt8] = []
    /// Guard against unterminated sequences eating memory.
    private let maxLength = 4096

    public init() {}

    public mutating func scan<S: Sequence>(_ bytes: S) -> [ShellEvent] where S.Element == UInt8 {
        var events: [ShellEvent] = []
        for byte in bytes {
            switch state {
            case .ground:
                if byte == 0x1B { state = .escape }
                else if byte == 0x9D { state = .osc; buffer.removeAll(keepingCapacity: true) } // C1 OSC
            case .escape:
                if byte == 0x5D { state = .osc; buffer.removeAll(keepingCapacity: true) } // ']'
                else { state = byte == 0x1B ? .escape : .ground }
            case .osc:
                if byte == 0x07 { finish(into: &events) }
                else if byte == 0x1B { state = .oscEscape }
                else if buffer.count < maxLength { buffer.append(byte) }
                else { state = .ground; buffer.removeAll() }
            case .oscEscape:
                if byte == 0x5C { finish(into: &events) } // ST = ESC \
                else { state = .ground; buffer.removeAll() }
            }
        }
        return events
    }

    private mutating func finish(into events: inout [ShellEvent]) {
        state = .ground
        defer { buffer.removeAll(keepingCapacity: true) }
        guard let payload = String(bytes: buffer, encoding: .utf8) else { return }
        if let event = Self.parse(payload) { events.append(event) }
    }

    /// Parses an OSC payload (the text between `ESC ]` and the terminator).
    public static func parse(_ payload: String) -> ShellEvent? {
        let parts = payload.split(separator: ";", omittingEmptySubsequences: false).map(String.init)
        guard let code = parts.first else { return nil }
        switch code {
        case "133":
            guard parts.count >= 2 else { return nil }
            switch parts[1].prefix(1) {
            case "A": return .promptStart
            case "B": return .inputStart
            case "C": return .outputStart
            case "D": return .commandFinished(exitCode: parts.count >= 3 ? Int32(parts[2]) : nil)
            default: return nil
            }
        case "7":
            let raw = parts.dropFirst().joined(separator: ";")
            guard let url = URL(string: raw), url.isFileURL else { return nil }
            return .workingDirectory(url)
        case "9":
            // ConEmu progress: 9;4;state;percent
            if parts.count >= 3, parts[1] == "4", let st = Int(parts[2]) {
                return .progress(state: st, percent: parts.count >= 4 ? Int(parts[3]) : nil)
            }
            return .notification(title: nil, body: parts.dropFirst().joined(separator: ";"))
        case "777":
            // 777;notify;title;body
            guard parts.count >= 4, parts[1] == "notify" else { return nil }
            return .notification(title: parts[2], body: parts.dropFirst(3).joined(separator: ";"))
        default:
            return nil
        }
    }
}
