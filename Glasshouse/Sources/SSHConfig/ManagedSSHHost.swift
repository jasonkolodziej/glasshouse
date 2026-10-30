import Foundation

/// Fixed values Glasshouse writes into SSH config for Secure Enclave keys.
public enum GlasshouseSSH {
    /// Apple's PKCS#11-style provider that signs with `sc_auth` Secure Enclave identities.
    public static let securityKeyProvider = "/usr/lib/ssh-keychain.dylib"
    /// Key type and signature algorithm of a Secure Enclave key reached through `securityKeyProvider`.
    public static let secureEnclaveAlgorithm = "sk-ecdsa-sha2-nistp256@openssh.com"
    /// Key types a FIDO security key (a YubiKey, for example) can hold.
    public static let securityKeyAlgorithms = ["sk-ssh-ed25519@openssh.com", "sk-ecdsa-sha2-nistp256@openssh.com"]
    /// OpenSSH's built-in FIDO support. Homebrew's OpenSSH has it; a build without it needs a provider library.
    public static let internalProvider = "internal"
    /// Added to a host's alias for its security key entry: `myserver` → `myserver-key`.
    public static let securityKeyAliasSuffix = "-key"
    /// The file Glasshouse generates, next to `~/.ssh/config`.
    public static let includeFileName = "glasshouse.conf"
    /// Written exactly like this in `~/.ssh/config`, so it works whatever the home directory is.
    public static let includePath = "~/.ssh/\(includeFileName)"
}

/// One `Host` entry in `~/.ssh/glasshouse.conf`, plus the keys installed on that server. Every field is validated
/// before it is written: values end up in ssh_config, where a stray newline or `%` token would let a field add
/// directives such as `ProxyCommand`, and in shell scripts, where a leading `-` would turn an alias into an option.
public struct ManagedSSHHost: Codable, Hashable, Identifiable, Sendable {
    /// A FIDO security key on this Mac, used through its own entry (`<alias>-key`): `SecurityKeyProvider` is one
    /// value per host, and the main entry's is Apple's Secure Enclave provider.
    public struct SecurityKey: Codable, Hashable, Sendable {
        /// File name under `~/.ssh` of the key handle.
        public var keyName: String
        /// `internal`, or the absolute path of a provider library.
        public var provider: String

        public init(keyName: String, provider: String = GlasshouseSSH.internalProvider) {
            self.keyName = keyName.trimmingCharacters(in: .whitespaces)
            self.provider = provider.trimmingCharacters(in: .whitespaces)
        }
    }

    public var id: String { alias }
    /// What you type after `ssh`.
    public var alias: String
    public var hostName: String
    /// `nil` lets ssh use your local user name.
    public var user: String?
    /// `nil` lets ssh use port 22, or whatever another block sets.
    public var port: Int?
    /// File name under `~/.ssh` of the Secure Enclave key handle: `<keyName>` and `<keyName>.pub`.
    public var keyName: String
    public var securityKey: SecurityKey?
    /// Public keys from devices other than this Mac (another computer, a security key kept elsewhere), installed on
    /// the server so you can still log in without this Mac. One `authorized_keys`-style line each.
    public var backupKeys: [String]

    /// Trims whitespace and turns an empty user into `nil`. Call `validated()` before writing.
    public init(alias: String, hostName: String, user: String? = nil, port: Int? = nil, keyName: String,
                securityKey: SecurityKey? = nil, backupKeys: [String] = []) {
        self.alias = alias.trimmingCharacters(in: .whitespaces)
        self.hostName = hostName.trimmingCharacters(in: .whitespaces)
        let trimmedUser = user?.trimmingCharacters(in: .whitespaces)
        self.user = (trimmedUser?.isEmpty ?? true) ? nil : trimmedUser
        self.port = port
        self.keyName = keyName.trimmingCharacters(in: .whitespaces)
        self.securityKey = securityKey
        self.backupKeys = backupKeys
    }

    private enum CodingKeys: String, CodingKey {
        case alias, hostName, user, port, keyName, securityKey, backupKeys
    }

    /// Hosts saved before security keys and backup keys existed have neither field.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(alias: try c.decode(String.self, forKey: .alias),
                  hostName: try c.decode(String.self, forKey: .hostName),
                  user: try c.decodeIfPresent(String.self, forKey: .user),
                  port: try c.decodeIfPresent(Int.self, forKey: .port),
                  keyName: try c.decode(String.self, forKey: .keyName),
                  securityKey: try c.decodeIfPresent(SecurityKey.self, forKey: .securityKey),
                  backupKeys: try c.decodeIfPresent([String].self, forKey: .backupKeys) ?? [])
    }

    /// `alias-key` when the host has a security key.
    public var securityKeyAlias: String? {
        securityKey.map { _ in alias + GlasshouseSSH.securityKeyAliasSuffix }
    }

    /// Every alias this host writes to the config file.
    public var aliases: [String] { [alias] + (securityKeyAlias.map { [$0] } ?? []) }

    /// Checks every field, and returns the host with its backup keys parsed, rewritten as `type blob comment`
    /// and without repeats.
    public func validated() throws -> ManagedSSHHost {
        guard Self.isToken(alias, extra: "._-") else { throw SSHConfigError.invalidAlias(alias) }
        guard Self.isToken(hostName, extra: "._-:") else { throw SSHConfigError.invalidHostName(hostName) }
        if let user, !Self.isToken(user, extra: "._-@") { throw SSHConfigError.invalidUser(user) }
        if let port, !(1...65_535).contains(port) { throw SSHConfigError.invalidPort(port) }
        guard Self.isValidKeyName(keyName) else { throw SSHConfigError.invalidKeyName(keyName) }
        if let securityKey {
            guard Self.isValidKeyName(securityKey.keyName), securityKey.keyName != keyName else {
                throw SSHConfigError.invalidKeyName(securityKey.keyName)
            }
            guard Self.isValidProvider(securityKey.provider) else {
                throw SSHConfigError.invalidProvider(securityKey.provider)
            }
        }
        var host = self
        var seen = Set<String>()
        host.backupKeys = try backupKeys.compactMap { line in
            let key = try SSHPublicKey(parsing: line)
            return seen.insert(key.blob).inserted ? key.line : nil
        }
        guard host.backupKeys.count <= 16 else { throw SSHConfigError.tooManyBackupKeys }
        return host
    }

    public var parsedBackupKeys: [SSHPublicKey] {
        backupKeys.compactMap { try? SSHPublicKey(parsing: $0) }
    }

    /// A plain file name in `~/.ssh`: no path separators, not hidden, not the `.pub` half.
    public static func isValidKeyName(_ name: String) -> Bool {
        isToken(name, extra: "._-") && !name.hasPrefix(".") && !name.hasSuffix(".pub")
    }

    /// `internal`, or an absolute path without spaces or `%` tokens.
    public static func isValidProvider(_ provider: String) -> Bool {
        provider == GlasshouseSSH.internalProvider || (provider.hasPrefix("/") && isToken(provider, extra: "._-/+"))
    }

    /// ASCII letters and digits plus `extra`, at most 255 characters, not starting with `-`.
    static func isToken(_ value: String, extra: String) -> Bool {
        guard let first = value.first, first != "-", value.count <= 255 else { return false }
        return value.allSatisfy { ($0.isASCII && ($0.isLetter || $0.isNumber)) || extra.contains($0) }
    }

    /// The `Host` blocks. `IdentityAgent none` and the `PubkeyAcceptedAlgorithms` restriction are what make
    /// every login need its key: `IdentityFile` lines from other matching blocks (`Host *` included) still add up,
    /// and ssh offers keys held by an agent before any key file.
    public var configBlock: String {
        var text = block(alias: alias, keyName: keyName, provider: GlasshouseSSH.securityKeyProvider,
                         algorithms: [GlasshouseSSH.secureEnclaveAlgorithm])
        if let securityKey, let securityKeyAlias {
            text += "\n" + block(alias: securityKeyAlias, keyName: securityKey.keyName, provider: securityKey.provider,
                                 algorithms: GlasshouseSSH.securityKeyAlgorithms)
        }
        return text
    }

    private func block(alias: String, keyName: String, provider: String, algorithms: [String]) -> String {
        var lines = ["Host \(alias)", "    HostName \(hostName)"]
        if let user { lines.append("    User \(user)") }
        if let port { lines.append("    Port \(port)") }
        lines += [
            "    IdentityFile ~/.ssh/\(keyName)",
            "    SecurityKeyProvider \(provider)",
            "    IdentitiesOnly yes",
            "    IdentityAgent none",
            "    PubkeyAcceptedAlgorithms \(algorithms.joined(separator: ","))",
            "    ForwardAgent no",
        ]
        return lines.joined(separator: "\n") + "\n"
    }
}

/// One public key in `authorized_keys` form: `type base64 [comment]`, without options.
public struct SSHPublicKey: Hashable, Sendable {
    public static let allowedTypes: Set<String> = [
        "ssh-ed25519", "ecdsa-sha2-nistp256", "ecdsa-sha2-nistp384", "ecdsa-sha2-nistp521", "ssh-rsa",
        "sk-ssh-ed25519@openssh.com", "sk-ecdsa-sha2-nistp256@openssh.com",
    ]

    public let type: String
    public let blob: String
    public let comment: String

    /// Checks that the key data really is a key of the stated type. A leading options field, more than one line or
    /// an unknown type is refused. The comment keeps printable ASCII; quotes and backslashes become `_`, because
    /// the key is written into shell scripts.
    public init(parsing text: String) throws {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let fields = trimmed.split(maxSplits: 2, whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
        guard !trimmed.contains(where: \.isNewline), fields.count >= 2, Self.allowedTypes.contains(fields[0]),
              fields[1].allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || "+/=".contains($0)) }),
              let data = Data(base64Encoded: fields[1]), data.count >= 4 else {
            throw SSHConfigError.invalidPublicKey(String(trimmed.prefix(40)))
        }
        let length = data.prefix(4).reduce(0) { $0 << 8 | Int($1) }
        guard data.count >= 4 + length, String(decoding: data.dropFirst(4).prefix(length), as: UTF8.self) == fields[0] else {
            throw SSHConfigError.invalidPublicKey(String(trimmed.prefix(40)))
        }
        type = fields[0]
        blob = fields[1]
        comment = String((fields.count == 3 ? fields[2] : "").prefix(200).map { c -> Character in
            guard c.isASCII, let v = c.asciiValue, (0x20...0x7E).contains(v), c != "'", c != "\\", c != "\"" else { return "_" }
            return c
        }).trimmingCharacters(in: .whitespaces)
    }

    public var line: String { comment.isEmpty ? "\(type) \(blob)" : "\(type) \(blob) \(comment)" }

    /// Every use needs a touch on a device: Touch ID for a Secure Enclave key, a tap for a security key.
    public var needsTouch: Bool { type.hasPrefix("sk-") }

    /// What to call the key in messages.
    public var label: String { comment.isEmpty ? type : "\(comment) (\(type))" }
}

public enum SSHConfigError: Error, Equatable, LocalizedError {
    case invalidAlias(String)
    case invalidHostName(String)
    case invalidUser(String)
    case invalidPort(Int)
    case invalidKeyName(String)
    case invalidProvider(String)
    case invalidPublicKey(String)
    case tooManyBackupKeys
    case duplicateAlias(String)
    case keyAlreadyExists(String)
    case missingPublicKey(String)

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
            "“\(v)” can't be used as a key name. Use letters, digits, “.”, “_” and “-”, not starting with “.” or “-”, and pick a different key for each role."
        case .invalidProvider(let v):
            "“\(v)” isn't a security key provider. Use “internal” or the full path of a provider library."
        case .invalidPublicKey(let v):
            "“\(v)…” isn't a public key. Paste one line from a .pub file: type, key data and an optional comment."
        case .tooManyBackupKeys:
            "A host can have at most 16 backup keys."
        case .duplicateAlias(let v):
            "Another host is already called “\(v)”."
        case .keyAlreadyExists(let v):
            "~/.ssh/\(v) already exists. Choose another name."
        case .missingPublicKey(let v):
            "~/.ssh/\(v).pub is missing or isn't a public key."
        }
    }
}
