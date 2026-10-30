import Foundation

/// One keyword line of an ssh_config file.
public struct SSHConfigDirective: Equatable, Sendable {
    /// 1-based line number.
    public let line: Int
    /// Lowercased, as ssh treats keywords case-insensitively.
    public let keyword: String
    public let arguments: [String]
}

/// Whether `~/.ssh/config` pulls in `~/.ssh/glasshouse.conf` where it takes effect.
public enum SSHIncludeStatus: Equatable, Sendable {
    case missing
    /// Before every `Host` and `Match` line: the managed hosts apply, and their values win over later blocks.
    case active
    /// After a `Host` or `Match` line, so it only applies inside that block.
    case inactive(line: Int)
}

/// Another `Host` block in `~/.ssh/config` that names a managed alias.
public struct SSHConfigConflict: Hashable, Identifiable, Sendable {
    public var id: String { "\(alias):\(line)" }
    public let alias: String
    public let line: Int
}

/// Pure text operations on ssh_config files. Nothing here touches the disk.
public enum SSHConfigText {
    public static let beginMarker = "# >>> glasshouse >>>"
    public static let endMarker = "# <<< glasshouse <<<"

    /// Goes first in `~/.ssh/config`. For most options ssh keeps the first value it finds, and an `Include`
    /// after a `Host` line would only apply inside that block.
    public static let includeBlock = """
        \(beginMarker)
        # Managed by Glasshouse (Settings › SSH Keys). Keep this block above every Host and Match line.
        Include \(GlasshouseSSH.includePath)
        \(endMarker)

        """

    public static func directives(in text: String) -> [SSHConfigDirective] {
        lines(text).enumerated().compactMap { index, line in
            parse(line).map { SSHConfigDirective(line: index + 1, keyword: $0.keyword, arguments: $0.arguments) }
        }
    }

    /// Concrete `Host` names, in file order without repeats. Patterns (`*`, `?`) and negations are skipped.
    public static func hostAliases(in text: String) -> [String] {
        var seen = Set<String>()
        return directives(in: text)
            .filter { $0.keyword == "host" }
            .flatMap(\.arguments)
            .filter { !$0.contains("*") && !$0.contains("?") && !$0.hasPrefix("!") && seen.insert($0).inserted }
    }

    /// Every argument of every `Include` line, unexpanded.
    public static func includePatterns(in text: String) -> [String] {
        directives(in: text).filter { $0.keyword == "include" }.flatMap(\.arguments)
    }

    public static func includeStatus(in text: String) -> SSHIncludeStatus {
        let all = directives(in: text)
        guard let include = all.first(where: { $0.keyword == "include" && $0.arguments.contains(where: isOurInclude) })
        else { return .missing }
        if let firstBlock = all.first(where: { $0.keyword == "host" || $0.keyword == "match" }),
           firstBlock.line < include.line {
            return .inactive(line: include.line)
        }
        return .active
    }

    /// Puts the managed block at the very top, moving it there if it is already further down.
    /// Everything else is kept byte for byte. Running it twice gives the same text.
    public static func installingInclude(in text: String) -> String {
        let rest = removingInclude(from: text)
        return rest.isEmpty ? includeBlock : includeBlock + "\n" + rest
    }

    /// Removes the managed block, and the blank line `installingInclude` put after it.
    public static func removingInclude(from text: String) -> String {
        var raw = rawLines(text)
        let plain = raw.map(stripCR)
        guard let begin = plain.firstIndex(of: beginMarker),
              let end = plain[begin...].firstIndex(of: endMarker) else { return text }
        raw.removeSubrange(begin...end)
        if begin < raw.count, stripCR(raw[begin]).isEmpty, begin == 0 || stripCR(raw[begin - 1]).isEmpty {
            raw.remove(at: begin)
        }
        return raw.joined(separator: "\n")
    }

    /// `Host` lines outside the managed block that name a managed alias. Glasshouse's entry is read first,
    /// so its HostName, User and Port win, but the other block's remaining settings still apply.
    public static func conflicts(in text: String, aliases: some Collection<String>) -> [SSHConfigConflict] {
        let wanted = Set(aliases)
        let all = lines(text)
        let managed: ClosedRange<Int>? = all.firstIndex(of: beginMarker).flatMap { begin in
            all[begin...].firstIndex(of: endMarker).map { (begin + 1)...($0 + 1) }
        }
        return directives(in: text)
            .filter { $0.keyword == "host" && !(managed?.contains($0.line) ?? false) }
            .flatMap { d in d.arguments.filter(wanted.contains).map { SSHConfigConflict(alias: $0, line: d.line) } }
    }

    // MARK: Parsing

    static func isOurInclude(_ argument: String) -> Bool {
        argument == GlasshouseSSH.includePath || argument == GlasshouseSSH.includeFileName
            || argument.hasSuffix("/.ssh/\(GlasshouseSSH.includeFileName)")
    }

    /// Split at "\n" only, scalar by scalar. Swift treats "\r\n" as one Character, so `split` and
    /// `components(separatedBy:)` would leave CRLF lines joined. Each line keeps its "\r", so joining with "\n"
    /// rebuilds a CRLF file unchanged.
    static func rawLines(_ text: String) -> [String] {
        var result: [String] = []
        var current = String.UnicodeScalarView()
        for scalar in text.unicodeScalars {
            if scalar == "\n" { result.append(String(current)); current = String.UnicodeScalarView() }
            else { current.append(scalar) }
        }
        result.append(String(current))
        return result
    }

    /// Lines without their "\r", for parsing.
    static func lines(_ text: String) -> [String] {
        rawLines(text).map(stripCR)
    }

    static func stripCR(_ line: String) -> String {
        line.unicodeScalars.last == "\r" ? String(String.UnicodeScalarView(line.unicodeScalars.dropLast())) : line
    }

    /// `Keyword arg …` or `Keyword=arg …`. Double quotes group an argument; `#` starts a comment.
    static func parse(_ line: String) -> (keyword: String, arguments: [String])? {
        let isSpace: (Character) -> Bool = { $0 == " " || $0 == "\t" }
        let body = line.drop(while: isSpace)
        guard let first = body.first, first != "#" else { return nil }
        let keywordEnd = body.firstIndex { isSpace($0) || $0 == "=" } ?? body.endIndex
        var rest = body[keywordEnd...].drop(while: isSpace)
        if rest.first == "=" { rest = rest.dropFirst().drop(while: isSpace) }

        var arguments: [String] = []
        var current = ""
        var quoted = false, inToken = false
        for char in rest {
            if char == "\"" {
                quoted.toggle(); inToken = true
            } else if isSpace(char), !quoted {
                if inToken { arguments.append(current); current = ""; inToken = false }
            } else if char == "#", !quoted, !inToken {
                break
            } else {
                current.append(char); inToken = true
            }
        }
        if inToken { arguments.append(current) }
        return (body[..<keywordEnd].lowercased(), arguments)
    }
}
