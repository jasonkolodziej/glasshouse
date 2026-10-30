import Foundation
#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

/// A security key handle in `~/.ssh`: `<name>` plus `<name>.pub` of an `sk-` type. Secure Enclave keys are
/// `sk-ecdsa-sha2-nistp256@openssh.com`; a FIDO security key may be that type or `sk-ssh-ed25519@openssh.com`.
public struct SSHKeyFile: Hashable, Identifiable, Sendable {
    public var id: String { name }
    public let name: String
    public let type: String
    /// The whole `.pub` line, ready to paste into `authorized_keys`.
    public let publicKey: String
    public let comment: String
}

/// Reads and writes Glasshouse's SSH files:
/// - `storeURL`: the managed hosts as JSON, the source of truth;
/// - `~/.ssh/glasshouse.conf`: generated from that JSON;
/// - `~/.ssh/config`: changed only by `installInclude()`, which adds the managed block at the top.
public struct SSHConfigStore: Sendable {
    public let sshDirectory: URL
    public let storeURL: URL
    /// Expands `~` in `Include` lines.
    public let homeDirectory: URL

    public init(sshDirectory: URL, storeURL: URL, homeDirectory: URL? = nil) {
        self.sshDirectory = sshDirectory
        self.storeURL = storeURL
        self.homeDirectory = homeDirectory ?? sshDirectory.deletingLastPathComponent()
    }

    /// `~/.ssh`, and the host list in Application Support.
    public static func standard(fileManager fm: FileManager = .default) -> SSHConfigStore {
        let home = fm.homeDirectoryForCurrentUser
        let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? home.appendingPathComponent("Library/Application Support")
        return SSHConfigStore(sshDirectory: home.appendingPathComponent(".ssh"),
                              storeURL: support.appendingPathComponent("Glasshouse/ssh-hosts.json"),
                              homeDirectory: home)
    }

    public var mainConfigURL: URL { sshDirectory.appendingPathComponent("config") }
    public var includeFileURL: URL { sshDirectory.appendingPathComponent(GlasshouseSSH.includeFileName) }
    public var backupURL: URL { sshDirectory.appendingPathComponent("config.glasshouse-backup") }

    // MARK: Managed hosts

    public func load() throws -> ManagedSSHConfig {
        guard let data = try? Data(contentsOf: storeURL) else { return ManagedSSHConfig() }
        return try JSONDecoder().decode(ManagedSSHConfig.self, from: data)
    }

    /// Saves the host list, then regenerates `~/.ssh/glasshouse.conf` from it.
    public func save(_ config: ManagedSSHConfig) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try write(encoder.encode(config), to: storeURL, directoryPermissions: 0o700)
        try write(Data(config.renderIncludeFile().utf8), to: includeFileURL, directoryPermissions: 0o700)
    }

    // MARK: ~/.ssh/config

    public func includeStatus() -> SSHIncludeStatus {
        SSHConfigText.includeStatus(in: mainConfigText())
    }

    /// Adds the managed block at the top of `~/.ssh/config`, or moves it there. Creates the file if needed.
    /// The first time it changes an existing file, it keeps a copy as `config.glasshouse-backup`.
    public func installInclude() throws {
        let fm = FileManager.default
        let target = mainConfigURL.resolvingSymlinksInPath()   // keep a dotfiles symlink pointing where it did
        let original = mainConfigText()
        let updated = SSHConfigText.installingInclude(in: original)
        guard updated != original else { return }
        if fm.fileExists(atPath: target.path), !fm.fileExists(atPath: backupURL.path) {
            try fm.copyItem(at: target, to: backupURL)
        }
        let permissions = (try? fm.attributesOfItem(atPath: target.path)[.posixPermissions] as? NSNumber)?.int16Value
        try write(Data(updated.utf8), to: target, permissions: permissions ?? 0o600, directoryPermissions: 0o700)
    }

    public func conflicts(for config: ManagedSSHConfig) -> [SSHConfigConflict] {
        SSHConfigText.conflicts(in: mainConfigText(), aliases: config.hosts.flatMap(\.aliases))
    }

    func mainConfigText() -> String {
        (try? String(contentsOf: mainConfigURL, encoding: .utf8)) ?? ""
    }

    // MARK: Keys

    /// Secure Enclave key handles in `~/.ssh`, by name. A `.pub` without its handle file is skipped.
    public func secureEnclaveKeys() -> [SSHKeyFile] {
        securityKeyFiles().filter { $0.type == GlasshouseSSH.secureEnclaveAlgorithm }
    }

    /// Every `sk-` key handle in `~/.ssh`, by name: Secure Enclave keys and FIDO security keys alike.
    public func securityKeyFiles() -> [SSHKeyFile] {
        let fm = FileManager.default
        let names = (try? fm.contentsOfDirectory(atPath: sshDirectory.path)) ?? []
        return names.filter { $0.hasSuffix(".pub") }.sorted().compactMap { pubName in
            let name = String(pubName.dropLast(4))
            guard fm.fileExists(atPath: sshDirectory.appendingPathComponent(name).path),
                  let text = try? String(contentsOf: sshDirectory.appendingPathComponent(pubName), encoding: .utf8),
                  let line = text.split(whereSeparator: \.isNewline).first.map(String.init) else { return nil }
            let fields = line.split(separator: " ", maxSplits: 2).map(String.init)
            guard fields.count >= 2, GlasshouseSSH.securityKeyAlgorithms.contains(fields[0]) else { return nil }
            return SSHKeyFile(name: name, type: fields[0], publicKey: line, comment: fields.count == 3 ? fields[2] : "")
        }
    }

    /// The parsed public half of `~/.ssh/<name>`.
    public func publicKey(named name: String) throws -> SSHPublicKey {
        let url = sshDirectory.appendingPathComponent(name + ".pub")
        guard ManagedSSHHost.isValidKeyName(name),
              let text = try? String(contentsOf: url, encoding: .utf8),
              let line = text.split(whereSeparator: \.isNewline).first,
              let key = try? SSHPublicKey(parsing: String(line)) else { throw SSHConfigError.missingPublicKey(name) }
        return key
    }

    // MARK: Host aliases across Include files

    /// Concrete host aliases from `~/.ssh/config` and every file it includes, the way ssh reads them:
    /// relative `Include` paths start in `~/.ssh`, and `*` or `?` may appear in the last path component.
    public func hostAliases() -> [String] {
        var seen = Set<String>(), visited = Set<String>(), result: [String] = []
        func visit(_ url: URL, depth: Int) {
            guard depth <= 16, visited.insert(url.standardizedFileURL.path).inserted,
                  let text = try? String(contentsOf: url, encoding: .utf8) else { return }
            for alias in SSHConfigText.hostAliases(in: text) where seen.insert(alias).inserted { result.append(alias) }
            for pattern in SSHConfigText.includePatterns(in: text) {
                for file in expandInclude(pattern) { visit(file, depth: depth + 1) }
            }
        }
        visit(mainConfigURL, depth: 0)
        return result
    }

    func expandInclude(_ pattern: String) -> [URL] {
        let path: String
        if pattern.hasPrefix("~/") {
            path = homeDirectory.appendingPathComponent(String(pattern.dropFirst(2))).path
        } else if pattern.hasPrefix("/") {
            path = pattern
        } else {
            path = sshDirectory.appendingPathComponent(pattern).path
        }
        let url = URL(fileURLWithPath: path)
        let last = url.lastPathComponent
        guard last.contains("*") || last.contains("?") else { return [url] }
        let directory = url.deletingLastPathComponent()
        let entries = (try? FileManager.default.contentsOfDirectory(atPath: directory.path)) ?? []
        return entries.sorted().filter { fnmatch(last, $0, 0) == 0 }.map { directory.appendingPathComponent($0) }
    }

    // MARK: Writing

    /// Atomic write, then `permissions`. Creates missing parent directories with `directoryPermissions`.
    private func write(_ data: Data, to url: URL, permissions: Int16 = 0o600, directoryPermissions: Int16) throws {
        let fm = FileManager.default
        let directory = url.deletingLastPathComponent()
        if !fm.fileExists(atPath: directory.path) {
            try fm.createDirectory(at: directory, withIntermediateDirectories: true,
                                   attributes: [.posixPermissions: NSNumber(value: directoryPermissions)])
        }
        try data.write(to: url, options: .atomic)
        try fm.setAttributes([.posixPermissions: NSNumber(value: permissions)], ofItemAtPath: url.path)
    }
}
