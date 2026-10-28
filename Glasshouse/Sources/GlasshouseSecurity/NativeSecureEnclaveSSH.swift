import Foundation

/// Apple's built-in path for Touch ID SSH on macOS 26: a non-exportable Secure Enclave key created
/// by `sc_auth`, surfaced to OpenSSH as an `sk-ecdsa-sha2-nistp256@openssh.com` key through
/// `/usr/lib/ssh-keychain.dylib`. Documented only in the sc_auth(8) and ssh-keychain(8) man pages.
///
/// Like TouchIDSudo, Glasshouse runs these commands in a visible tab rather than hidden subprocesses:
/// `ssh-keygen -K` may ask for a (blank) PIN interactively, and the user should see what happened.
public enum NativeSecureEnclaveSSH {
    public static let providerPath = "/usr/lib/ssh-keychain.dylib"

    public static var isAvailable: Bool {
        FileManager.default.fileExists(atPath: providerPath)
            && FileManager.default.isExecutableFile(atPath: "/usr/sbin/sc_auth")
    }

    /// - Parameter label: also becomes the key's file name, e.g. "glasshouse-github".
    public static func createKeyScript(label: String) -> String {
        let safe = label.filter { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }
        return """
        set -euo pipefail
        # 1. Create a P-256 key inside the Secure Enclave: non-exportable (-ne), Touch ID required (bio).
        sc_auth create-ctk-identity -l \(safe) -k p-256-ne -t bio
        # 2. Download the key *handle* (no secret material) into a scratch dir, then rename it.
        tmp="$(mktemp -d)"; cd "$tmp"
        ssh-keygen -w \(providerPath) -K -N ""     # press Return if asked for a PIN
        install -d -m 0700 ~/.ssh
        mv id_ecdsa_sk_rk ~/.ssh/\(safe) && mv id_ecdsa_sk_rk.pub ~/.ssh/\(safe).pub
        chmod 0600 ~/.ssh/\(safe); rmdir "$tmp" 2>/dev/null || true
        echo; echo "Public key (add to the server's authorized_keys or your Git host):"
        cat ~/.ssh/\(safe).pub
        echo; echo "Register a second (recovery) key too — Secure Enclave keys cannot be backed up."
        """
    }

    /// The ~/.ssh/config block Glasshouse offers to append for a host.
    public static func configBlock(hostAlias: String, hostName: String, user: String, keyLabel: String) -> String {
        """
        Host \(hostAlias)
            HostName \(hostName)
            User \(user)
            IdentityFile ~/.ssh/\(keyLabel)
            SecurityKeyProvider \(providerPath)
            IdentitiesOnly yes
            ForwardAgent no
        """
    }
}
