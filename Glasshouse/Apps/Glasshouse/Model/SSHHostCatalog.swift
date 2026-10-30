import Foundation
import Observation
import SSHConfig

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

    /// Aliases from ~/.ssh/config and the files it includes, Glasshouse's ~/.ssh/glasshouse.conf among them.
    nonisolated static func aliases() -> [String] {
        SSHConfigStore.standard().hostAliases()
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
