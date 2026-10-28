import Foundation

/// Reports whether Touch ID for sudo is configured, and produces the hardened setup script.
/// Glasshouse never edits /etc itself: it runs the script in a visible tab so the user sees every
/// privileged command (and approves it — with Touch ID once it's on).
public enum TouchIDSudo {
    public struct Status: Equatable, Sendable {
        public var pamTidEnabled: Bool
        public var pamReattachConfigured: Bool
        /// pam_reattach referenced from a user-writable location (e.g. /opt/homebrew) — a root-escalation risk.
        public var pamReattachUserWritable: Bool
        public var sudoLocalExists: Bool
    }

    public static let sudoLocalPath = "/etc/pam.d/sudo_local"
    public static let safeReattachPath = "/usr/local/lib/pam/pam_reattach.so"

    public static func status(fileManager fm: FileManager = .default) -> Status {
        guard let text = try? String(contentsOfFile: sudoLocalPath, encoding: .utf8) else {
            return Status(pamTidEnabled: false, pamReattachConfigured: false,
                          pamReattachUserWritable: false, sudoLocalExists: false)
        }
        let lines = text.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.hasPrefix("#") }
        let tid = lines.contains { $0.hasPrefix("auth") && $0.contains("pam_tid.so") && $0.contains("sufficient") }
        let reattachLine = lines.first { $0.contains("pam_reattach.so") }
        var userWritable = false
        if let line = reattachLine,
           let path = line.split(separator: " ", omittingEmptySubsequences: true)
               .map(String.init).first(where: { $0.hasSuffix("pam_reattach.so") }) {
            userWritable = !isRootOwnedChain(path, fm: fm)
        }
        return Status(pamTidEnabled: tid, pamReattachConfigured: reattachLine != nil,
                      pamReattachUserWritable: userWritable, sudoLocalExists: true)
    }

    /// The file and every ancestor directory must be root-owned and not group/other-writable.
    static func isRootOwnedChain(_ path: String, fm: FileManager) -> Bool {
        var url = URL(fileURLWithPath: path).resolvingSymlinksInPath()
        while true {
            guard let attrs = try? fm.attributesOfItem(atPath: url.path),
                  let owner = attrs[.ownerAccountID] as? NSNumber,
                  let perms = attrs[.posixPermissions] as? NSNumber else { return false }
            if owner.intValue != 0 || (perms.intValue & 0o022) != 0 { return false }
            if url.path == "/" { return true }
            url.deleteLastPathComponent()
        }
    }

    /// Hardened setup, matching the earlier "Touch ID for sudo and SSH" report.
    /// - Parameter includeReattach: only needed for tmux/screen users.
    public static func setupScript(includeReattach: Bool) -> String {
        var s = """
        set -euo pipefail
        echo "Glasshouse: enabling Touch ID for sudo (password remains the fallback)."
        sudo -v   # authenticate once up front; keep this tab open until verified

        """
        if includeReattach {
            s += """
            if ! brew list pam-reattach >/dev/null 2>&1; then brew install pam-reattach; fi
            sudo install -d -o root -g wheel -m 0755 /usr/local/lib/pam
            sudo install -o root -g wheel -m 0444 "$(brew --prefix)/lib/pam/pam_reattach.so" \(safeReattachPath)

            """
        }
        let reattachLine = includeReattach ? "auth       optional       \(safeReattachPath) ignore_ssh\n" : ""
        s += """
        sudo tee \(sudoLocalPath) >/dev/null <<'EOF'
        # sudo_local: local config file which survives system update and is included for sudo
        \(reattachLine)auth       sufficient     pam_tid.so
        EOF
        sudo chown root:wheel \(sudoLocalPath) && sudo chmod 0444 \(sudoLocalPath)

        # One tap = one command: no cached sudo ticket (DISA STIG for macOS 26).
        echo 'Defaults timestamp_timeout=0' | sudo tee /etc/sudoers.d/10-glasshouse-touchid >/dev/null
        sudo chmod 0440 /etc/sudoers.d/10-glasshouse-touchid
        sudo visudo -c

        sudo -k && echo "Verify: the next command should ask for Touch ID." && sudo true && echo "✓ Touch ID sudo works"
        """
        return s
    }

    public static let rollbackScript = """
    sudo rm -f /etc/pam.d/sudo_local /etc/sudoers.d/10-glasshouse-touchid
    sudo visudo -c
    """
}
