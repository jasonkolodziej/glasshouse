import Foundation

/// Scripts Glasshouse runs in a visible tab, so you see every command before it runs.
public enum GlasshouseSSHScripts {
    /// Creates a Secure Enclave identity with `sc_auth` and downloads its key handle to `~/.ssh/<name>`.
    /// `ssh-keygen -K` may ask for a PIN interactively, which is why this runs in a tab.
    public static func createSecureEnclaveKey(name: String) throws -> String {
        guard ManagedSSHHost.isValidKeyName(name) else { throw SSHConfigError.invalidKeyName(name) }
        let key = "~/.ssh/\(name)"
        return """
        set -euo pipefail
        if [ -e \(key) ] || [ -e \(key).pub ]; then
          echo "\(key) already exists. Choose another key name in Settings › SSH Keys."; exit 1
        fi
        # 1. Create a P-256 key inside the Secure Enclave: non-exportable (-ne), Touch ID required (bio).
        sc_auth create-ctk-identity -l \(name) -k p-256-ne -t bio
        # 2. Download the key *handle* (no secret material) into a scratch folder, then move it into place.
        tmp="$(mktemp -d)"; cd "$tmp"
        ssh-keygen -w \(GlasshouseSSH.securityKeyProvider) -K -N ""     # press Return if asked for a PIN
        install -d -m 0700 ~/.ssh
        mv id_ecdsa_sk_rk \(key) && mv id_ecdsa_sk_rk.pub \(key).pub
        chmod 0600 \(key); cd - >/dev/null; rmdir "$tmp" 2>/dev/null || true
        echo; echo "Public key:"
        cat \(key).pub
        echo; echo "Next: Settings › SSH Keys › Add Host…, then Install Key… to put it on a server."
        echo "Register a second (recovery) key too: Secure Enclave keys cannot be backed up."
        """
    }

    /// Appends the host's public key to `authorized_keys` on the server, then logs in with it.
    ///
    /// The copy has to log in the way you already do, so it undoes the managed entry's restrictions for this
    /// one command: options on the command line win over the config file.
    public static func installKey(on host: ManagedSSHHost) throws -> String {
        let host = try host.validated()
        let alias = shellQuote(host.alias)
        let key = "~/.ssh/\(host.keyName).pub"
        let target = (host.user.map { "\($0)@" } ?? "") + host.hostName + (host.port.map { ":\($0)" } ?? "")
        return """
        set -euo pipefail
        # Without the managed entry in effect, the test login below could succeed with another key.
        if ! ssh -G \(alias) | grep -qx 'pubkeyacceptedalgorithms \(GlasshouseSSH.secureEnclaveAlgorithm)'; then
          echo "Glasshouse's settings for \(host.alias) are not in effect."
          echo "In Settings › SSH Keys, add the Include to ~/.ssh/config, then try again."; exit 1
        fi
        echo "Installing \(key) on \(host.alias) (\(target))."
        echo "This step logs in the way you do today (password or another key), not with Touch ID."
        ssh-copy-id -f -i \(key) -o IdentityAgent=SSH_AUTH_SOCK -o IdentitiesOnly=no \\
          -o PubkeyAcceptedAlgorithms=+\(GlasshouseSSH.secureEnclaveAlgorithm) \(alias)
        echo; echo "Testing: this login must ask for Touch ID."
        if ssh \(alias) true; then echo "✓ \(host.alias): Touch ID login works"
        else echo "✗ \(host.alias): the Touch ID login failed. Run: ssh -v \(host.alias)"; exit 1; fi
        """
    }

    static func shellQuote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: #"'\''"#) + "'"
    }
}
