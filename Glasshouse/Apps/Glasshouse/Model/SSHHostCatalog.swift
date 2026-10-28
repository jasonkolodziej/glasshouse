import Foundation
import Observation

struct SSHHost: Identifiable, Hashable {
    var id: String { alias }
    let alias: String
    var hostName: String?
    var user: String?
    var port: String?
    var credential: Credential = .other

    enum Credential: String { case secureEnclave = "SE", agent = "Agent", other = "Key" }
}

/// Hosts from ~/.ssh/config, resolved with `ssh -G` so what we show is what ssh will actually use
/// (Include, Match, defaults all applied). Wildcard-only `Host` patterns are skipped.
@MainActor
@Observable
final class SSHHostCatalog {
    private(set) var hosts: [SSHHost] = []

    func reload() {
        Task.detached(priority: .utility) {
            let aliases = Self.aliases()
            var result: [SSHHost] = []
            for alias in aliases { if let h = Self.resolve(alias) { result.append(h) } }
            await MainActor.run { self.hosts = result }
        }
    }

    nonisolated static func aliases() -> [String] {
        let url = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh/config")
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return [] }
        // Top-level file only; `ssh -G` follows Include for each alias. TODO: walk Include'd files for aliases.
        return text.split(separator: "\n").flatMap { line -> [String] in
            let parts = line.trimmingCharacters(in: .whitespaces).split(whereSeparator: \.isWhitespace)
            guard parts.first?.lowercased() == "host" else { return [] }
            return parts.dropFirst().map(String.init).filter { !$0.contains("*") && !$0.contains("?") && !$0.hasPrefix("!") }
        }
    }

    nonisolated static func resolve(_ alias: String) -> SSHHost? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        p.arguments = ["-G", alias]
        let out = Pipe(); p.standardOutput = out; p.standardError = Pipe()
        do { try p.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0, let text = String(data: data, encoding: .utf8) else { return nil }

        var host = SSHHost(alias: alias)
        var identityFiles: [String] = []
        var provider: String?, identityAgent: String?
        for line in text.split(separator: "\n") {
            let kv = line.split(separator: " ", maxSplits: 1).map(String.init)
            guard kv.count == 2 else { continue }
            switch kv[0] {
            case "hostname": host.hostName = kv[1]
            case "user": host.user = kv[1]
            case "port": host.port = kv[1]
            case "identityfile": identityFiles.append(kv[1])
            case "securitykeyprovider": provider = kv[1]
            case "identityagent": identityAgent = kv[1]
            default: break
            }
        }
        if provider == "/usr/lib/ssh-keychain.dylib" { host.credential = .secureEnclave }
        else if identityAgent?.contains(".glasshouse/agent.sock") == true { host.credential = .agent }
        return host
    }
}
