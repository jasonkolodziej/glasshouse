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
        echo; echo "Next: Settings › SSH Keys › Add Host…, then Install Keys… to put it on a server."
        echo "Add a backup key for each host too: Secure Enclave keys cannot be backed up."
        """
    }

    /// One key to put in the server's `authorized_keys`, and what to call it in messages.
    struct InstallItem: Equatable {
        let key: SSHPublicKey
        let label: String
    }

    /// Every key a host keeps on its server: the Secure Enclave key, its security key, then its backup keys.
    static func installItems(for host: ManagedSSHHost, touchIDKey: SSHPublicKey,
                             securityKey: SSHPublicKey?) -> [InstallItem] {
        var items = [InstallItem(key: touchIDKey, label: "Touch ID key ~/.ssh/\(host.keyName)")]
        if let securityKey, let name = host.securityKey?.keyName {
            items.append(InstallItem(key: securityKey, label: "security key ~/.ssh/\(name)"))
        }
        items += host.parsedBackupKeys.map { InstallItem(key: $0, label: "backup key \($0.label)") }
        var seen = Set<String>()
        return items.filter { seen.insert($0.key.blob).inserted }
    }

    /// Installs the host's keys on the server, then logs in with Touch ID. Then it lists the server's keys that log
    /// in without a touch (Touch ID or a security-key tap) and, only if the host has a backup, offers to remove them.
    ///
    /// The first login has to work the way you already log in, so it undoes the managed entry's restrictions for
    /// that one command: options on the command line win over the config file. The removal runs over the Touch ID
    /// login, after that login has just worked.
    public static func installKeys(on host: ManagedSSHHost, touchIDKey: SSHPublicKey,
                                   securityKey: SSHPublicKey?) throws -> String {
        let host = try host.validated()
        guard touchIDKey.type == GlasshouseSSH.secureEnclaveAlgorithm else {
            throw SSHConfigError.missingPublicKey(host.keyName)
        }
        let items = installItems(for: host, touchIDKey: touchIDKey, securityKey: securityKey)
        let alias = shellQuote(host.alias)
        let target = (host.user.map { "\($0)@" } ?? "") + host.hostName + (host.port.map { ":\($0)" } ?? "")
        let hasBackup = host.securityKey != nil || !host.backupKeys.isEmpty
        let listing = items.map { "echo \(shellQuote("  - " + $0.label))" }.joined(separator: "\n")

        var script = #"""
        set -euo pipefail
        # Without the managed entry in effect, the test login below could succeed with another key.
        if ! ssh -G \#(alias) | grep -qx 'pubkeyacceptedalgorithms \#(GlasshouseSSH.secureEnclaveAlgorithm)'; then
          echo "Glasshouse's settings for \#(host.alias) are not in effect."
          echo "In Settings › SSH Keys, add the Include to ~/.ssh/config, then try again."; exit 1
        fi
        echo "Installing keys on \#(host.alias) (\#(target)):"
        \#(listing)
        echo "This step logs in the way you do today (password or another key), not with Touch ID."
        report="$(ssh -o IdentityAgent=SSH_AUTH_SOCK -o IdentitiesOnly=no \
          -o PubkeyAcceptedAlgorithms=+\#(GlasshouseSSH.secureEnclaveAlgorithm) \#(alias) 'sh -s' <<'GLASSHOUSE_REMOTE'
        \#(remoteInstallScript(items: items))
        GLASSHOUSE_REMOTE
        )"
        printf '%s\n' "$report" | awk -F'\t' '$1 == "added" { print "  added: " $2 } $1 == "present" { print "  already there: " $2 }'

        echo; echo "Testing: this login must ask for Touch ID."
        if ssh \#(alias) true; then echo "✓ \#(host.alias): Touch ID login works"
        else echo "✗ \#(host.alias): the Touch ID login failed. Run: ssh -v \#(host.alias)"; exit 1; fi

        notouch="$(printf '%s\n' "$report" | awk -F'\t' '$1 == "notouch"')"
        if [ -z "$notouch" ]; then echo "✓ No other keys on \#(host.alias) log in without Touch ID or a security-key tap."; exit 0; fi
        echo; echo "These other keys on \#(host.alias) log in without Touch ID or a security-key tap:"
        printf '%s\n' "$notouch" | awk -F'\t' '{ print "  - " $3 $4 }'

        """#
        if hasBackup {
            script += #"""
            read -r -p "Remove them from \#(host.alias)? A copy of authorized_keys stays on the server. [y/N] " answer
            case "$answer" in [yY]*) ;; *) echo "Kept."; exit 0 ;; esac
            blobs="$(printf '%s\n' "$notouch" | awk -F'\t' '{ printf "%s ", $2 }')"
            echo "Removing them over the Touch ID login."
            ssh \#(alias) "sh -s $blobs" <<'GLASSHOUSE_REMOVE'
            \#(remoteRemovalScript)
            GLASSHOUSE_REMOVE
            """#
        } else {
            script += #"""
            echo "Glasshouse won't offer to remove them: \#(host.alias) has no backup key, so without this Mac"
            echo "they may be your only way in. Add a backup key in Settings › SSH Keys, then run Install Keys… again."
            """#
        }
        return script
    }

    /// Runs on the server with `sh -s`. Appends each key unless its key data is already there (so running it twice
    /// adds nothing), then prints one tab-separated line per key that logs in without a touch:
    /// `notouch`, key data, type, comment. Keys in `items` are never listed. Prints only data, never runs it.
    static func remoteInstallScript(items: [InstallItem]) -> String {
        let adds = items.map {
            "add \(shellQuote($0.key.line)) \(shellQuote($0.key.blob)) \(shellQuote($0.label))"
        }.joined(separator: "\n")
        let keep = items.map(\.key.blob).joined(separator: " ")
        return #"""
        set -eu
        umask 077
        f="$HOME/.ssh/authorized_keys"
        mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh"
        touch "$f" && chmod 600 "$f"
        if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi
        add() {
          if grep -qF -- "$2" "$f"; then printf 'present\t%s\n' "$3"
          else printf '%s\n' "$1" >> "$f"; printf 'added\t%s\n' "$3"; fi
        }
        \#(adds)
        awk -v keep='\#(keep)' '
          BEGIN { n = split(keep, k, " "); for (i = 1; i <= n; i++) kept[k[i]] = 1 }
          /^[ \t]*(#|$)/ { next }
          {
            for (i = 1; i < NF; i++) if ($i ~ /^(ssh-|ecdsa-sha2-|sk-)/) break
            if (i >= NF) next
            type = $i; blob = $(i + 1)
            if ((blob in kept) || blob !~ /^[A-Za-z0-9+\/=]+$/) next
            options = ""; for (j = 1; j < i; j++) options = options $j " "
            comment = ""; for (j = i + 2; j <= NF; j++) comment = comment " " $j
            gsub(/[^ -~]/, "?", comment); gsub(/[^ -~]/, "?", type)
            if (type !~ /^sk-/ || options ~ /no-touch-required/) printf "notouch\t%s\t%s\t%s\n", blob, type, comment
          }' "$f"
        """#
    }

    /// Runs on the server with `sh -s BLOB…`. Copies `authorized_keys` aside, then removes every line holding one of
    /// the given key data strings.
    static let remoteRemovalScript = #"""
        set -eu
        f="$HOME/.ssh/authorized_keys"
        copy="$f.glasshouse-$(date +%Y%m%d%H%M%S)"
        cp -p "$f" "$copy"
        tmp="$f.glasshouse-tmp"
        cp -p "$f" "$tmp"
        for blob in "$@"; do grep -vF -- "$blob" "$tmp" > "$tmp.next" || true; mv "$tmp.next" "$tmp"; done
        chmod 600 "$tmp" && mv "$tmp" "$f"
        echo "Removed $# key(s). The previous file is $copy"
        """#

    static func shellQuote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: #"'\''"#) + "'"
    }
}
