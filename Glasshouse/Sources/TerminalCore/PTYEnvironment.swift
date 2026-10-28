import Foundation

/// Builds the child-shell environment.
public enum PTYEnvironment {
    /// Variables we never forward from the GUI app's own environment into a shell.
    private static let stripped: Set<String> = [
        "TERM_SESSION_ID", "ITERM_SESSION_ID", "__CFBundleIdentifier",
        "XPC_SERVICE_NAME", "XPC_FLAGS", "SSH_AUTH_SOCK_GLASSHOUSE_INTERNAL",
    ]

    public struct Options: Sendable {
        public var appVersion: String
        /// When set, child shells use the Glasshouse Secure Enclave agent (opt-in; see SSH settings).
        public var agentSocket: String?
        /// When set, OpenSSH uses Apple's native Secure Enclave provider for `-sk` keys.
        public var useNativeSecureEnclaveProvider: Bool

        public init(appVersion: String, agentSocket: String? = nil, useNativeSecureEnclaveProvider: Bool = true) {
            self.appVersion = appVersion
            self.agentSocket = agentSocket
            self.useNativeSecureEnclaveProvider = useNativeSecureEnclaveProvider
        }
    }

    public static func make(_ options: Options,
                            inherited: [String: String] = ProcessInfo.processInfo.environment) -> [String: String] {
        var env = inherited.filter { !stripped.contains($0.key) }
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["TERM_PROGRAM"] = "Glasshouse"
        env["TERM_PROGRAM_VERSION"] = options.appVersion
        env["LANG"] = env["LANG"] ?? Locale.current.identifier.replacingOccurrences(of: "-", with: "_") + ".UTF-8"
        env["HOME"] = env["HOME"] ?? NSHomeDirectory()

        if let socket = options.agentSocket {
            env["SSH_AUTH_SOCK"] = socket
        }
        if options.useNativeSecureEnclaveProvider,
           FileManager.default.fileExists(atPath: "/usr/lib/ssh-keychain.dylib") {
            // Lets plain `ssh` / `ssh-add -K` use Touch ID-gated Secure Enclave keys created with sc_auth.
            env["SSH_SK_PROVIDER"] = "/usr/lib/ssh-keychain.dylib"
        }
        return env
    }
}
