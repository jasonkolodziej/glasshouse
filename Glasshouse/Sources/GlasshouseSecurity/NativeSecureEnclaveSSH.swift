import Foundation

/// Apple's built-in path for Touch ID SSH on macOS 26: a non-exportable Secure Enclave key created
/// by `sc_auth`, surfaced to OpenSSH as an `sk-ecdsa-sha2-nistp256@openssh.com` key through
/// `/usr/lib/ssh-keychain.dylib`. Documented only in the sc_auth(8) and ssh-keychain(8) man pages.
public enum NativeSecureEnclaveSSH {
    public static let providerPath = "/usr/lib/ssh-keychain.dylib"

    public static var isAvailable: Bool {
        FileManager.default.fileExists(atPath: providerPath)
            && FileManager.default.isExecutableFile(atPath: "/usr/sbin/sc_auth")
    }

    // Key creation, host entries and key installation: see the SSHConfig module (GlasshouseSSHScripts,
    // ManagedSSHConfig), which the app uses for Settings › SSH Keys.
}
