import Foundation

/// Fixed values Glasshouse writes into SSH config for Secure Enclave keys.
public enum GlasshouseSSH {
    /// Apple's PKCS#11-style provider that signs with `sc_auth` Secure Enclave identities.
    public static let securityKeyProvider = "/usr/lib/ssh-keychain.dylib"
    /// Key type and signature algorithm of a Secure Enclave key reached through `securityKeyProvider`.
    public static let secureEnclaveAlgorithm = "sk-ecdsa-sha2-nistp256@openssh.com"
    /// The file Glasshouse generates, next to `~/.ssh/config`.
    public static let includeFileName = "glasshouse.conf"
    /// Written exactly like this in `~/.ssh/config`, so it works whatever the home directory is.
    public static let includePath = "~/.ssh/\(includeFileName)"
}

/// One `Host` entry in `~/.ssh/glasshouse.conf`. Every field is validated before it is written:
/// values end up in ssh_config, where a stray newline or `%` token would let a field add directives
/// such as `ProxyCommand`, and in shell scripts, where a leading `-` would turn an alias into an option.
public struct ManagedSSHHost: Codable, Hashable, Identifiable, Sendable {
    public var id: String { alias }
    /// What you type after `ssh`.
    public var alias: String
    public var hostName: String
    /// `nil` lets ssh use your local user name.
    public var user: String?
    /// `nil` lets ssh use port 22, or whatever another block sets.
    public var port: Int?
    /// File name under `~/.ssh` of the key handle: `<keyName>` and `<keyName>.pub`.
    public var keyName: String

    /// Trims whitespace and turns an empty user into `nil`. Call `validated()` before writing.
    public init(alias: String, hostName: String, user: String? = nil, port: Int? = nil, keyName: String) {
        self.alias = alias.trimmingCharacters(in: .whitespaces)
        self.hostName = hostName.trimmingCharacters(in: .whitespaces)
        let trimmedUser = user?.trimmingCharacters(in: .whitespaces)
        self.user = (trimmedUser?.isEmpty ?? true) ? nil : trimmedUser
        self.port = port
        self.keyName = keyName.trimmingCharacters(in: .whitespaces)
    }

    public func validated() throws -> ManagedSSHHost {
        guard Self.isToken(alias, extra: "._-") else { throw SSHConfigError.invalidAlias(alias) }
        guard Self.isToken(hostName, extra: "._-:") else { throw SSHConfigError.invalidHostName(hostName) }
        if let user, !Self.isToken(user, extra: "._-@") { throw SSHConfigError.invalidUser(user) }
        if let port, !(1...65_535).contains(port) { throw SSHConfigError.invalidPort(port) }
        guard Self.isValidKeyName(keyName) else { throw SSHConfigError.invalidKeyName(keyName) }
        return self
    }

    /// A plain file name in `~/.ssh`: no path separators, not hidden, not the `.pub` half.
    public static func isValidKeyName(_ name: String) -> Bool {
        isToken(name, extra: "._-") && !name.hasPrefix(".") && !name.hasSuffix(".pub")
    }

    /// ASCII letters and digits plus `extra`, at most 255 characters, not starting with `-`.
    static func isToken(_ value: String, extra: String) -> Bool {
        guard let first = value.first, first != "-", value.count <= 255 else { return false }
        return value.allSatisfy { ($0.isASCII && ($0.isLetter || $0.isNumber)) || extra.contains($0) }
    }

    /// The `Host` block. `IdentityAgent none` and the `PubkeyAcceptedAlgorithms` restriction are what make
    /// every login ask for Touch ID: `IdentityFile` lines from other matching blocks (`Host *` included)
    /// still add up, and ssh offers keys held by an agent before any key file.
    public var configBlock: String {
        var lines = ["Host \(alias)", "    HostName \(hostName)"]
        if let user { lines.append("    User \(user)") }
        if let port { lines.append("    Port \(port)") }
        lines += [
            "    IdentityFile ~/.ssh/\(keyName)",
            "    SecurityKeyProvider \(GlasshouseSSH.securityKeyProvider)",
            "    IdentitiesOnly yes",
            "    IdentityAgent none",
            "    PubkeyAcceptedAlgorithms \(GlasshouseSSH.secureEnclaveAlgorithm)",
            "    ForwardAgent no",
        ]
        return lines.joined(separator: "\n") + "\n"
    }
}

public enum SSHConfigError: Error, Equatable, LocalizedError {
    case invalidAlias(String)
    case invalidHostName(String)
    case invalidUser(String)
    case invalidPort(Int)
    case invalidKeyName(String)
    case duplicateAlias(String)
    case keyAlreadyExists(String)

    public var errorDescription: String? {
        switch self {
        case .invalidAlias(let v):
            "“\(v)” can't be used as an alias. Use letters, digits, “.”, “_” and “-”, not starting with “-”."
        case .invalidHostName(let v):
            "“\(v)” isn't a host name or IP address. Use letters, digits, “.”, “-” and “:”, without spaces."
        case .invalidUser(let v):
            "“\(v)” can't be used as a user name. Use letters, digits, “.”, “_”, “-” and “@”."
        case .invalidPort(let v):
            "Port \(v) is out of range. Use 1–65535, or leave it empty for 22."
        case .invalidKeyName(let v):
            "“\(v)” can't be used as a key name. Use letters, digits, “.”, “_” and “-”, not starting with “.” or “-”."
        case .duplicateAlias(let v):
            "Another host is already called “\(v)”."
        case .keyAlreadyExists(let v):
            "~/.ssh/\(v) already exists. Choose another name."
        }
    }
}
