import Foundation

/// Injects shell integration without editing the user's dotfiles — the same approach Ghostty and kitty use:
///
/// | Shell | Mechanism |
/// |-------|-----------|
/// | zsh   | Point `ZDOTDIR` at our directory; our `.zshenv` restores the user's `ZDOTDIR`, sources their `.zshenv`, then installs hooks. |
/// | fish  | Prepend our directory to `XDG_DATA_DIRS`; fish auto-loads `fish/vendor_conf.d/*.fish`. |
/// | bash  | Interactive non-login bash with `--rcfile` pointing at our script, which sources the user's profile/bashrc first. |
///
/// Unsupported shells run unmodified (no command marks, everything else still works).
public enum ShellIntegrationInstaller {
    public enum Shell: String, Sendable { case zsh, bash, fish, other }

    public struct LaunchPlan: Sendable {
        public var executable: String
        public var args: [String]
        /// argv[0]. A leading "-" makes the shell a login shell, as Terminal.app does.
        public var execName: String
        public var environment: [String: String]

        public init(executable: String, args: [String], execName: String, environment: [String: String]) {
            self.executable = executable; self.args = args
            self.execName = execName; self.environment = environment
        }
    }

    public static func detect(_ shellPath: String) -> Shell {
        Shell(rawValue: URL(fileURLWithPath: shellPath).lastPathComponent) ?? .other
    }

    /// The user's login shell from the directory service, falling back to $SHELL, then zsh.
    public static func userShell() -> String {
        if let pw = getpwuid(getuid()), let shell = pw.pointee.pw_shell {
            let s = String(cString: shell)
            if !s.isEmpty { return s }
        }
        return ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
    }

    public static func plan(shellPath: String = userShell(),
                            baseEnvironment: [String: String],
                            enabled: Bool = true) -> LaunchPlan {
        let name = URL(fileURLWithPath: shellPath).lastPathComponent
        var env = baseEnvironment
        var args: [String] = []
        var execName = "-" + name

        guard enabled, let root = Bundle.module.resourceURL else {
            return LaunchPlan(executable: shellPath, args: args, execName: execName, environment: env)
        }

        switch detect(shellPath) {
        case .zsh:
            guard let zdotdir = materializeZshDir(from: root) else { break }
            if let orig = env["ZDOTDIR"] { env["GLASSHOUSE_ORIG_ZDOTDIR"] = orig }
            env["ZDOTDIR"] = zdotdir.path
        case .fish:
            let ours = root.path // contains fish/vendor_conf.d
            let existing = env["XDG_DATA_DIRS"] ?? "/usr/local/share:/usr/share"
            env["XDG_DATA_DIRS"] = ours + ":" + existing
            env["GLASSHOUSE_XDG_DATA_DIRS_PREFIX"] = ours
        case .bash:
            // --rcfile is ignored for login shells, so run interactive non-login; our rc sources the profile.
            args = ["--rcfile", root.appendingPathComponent("bash/glasshouse.bash").path, "-i"]
            execName = name
        case .other:
            break
        }
        env["GLASSHOUSE_SHELL_INTEGRATION"] = "1"
        return LaunchPlan(executable: shellPath, args: args, execName: execName, environment: env)
    }

    /// SwiftPM may drop dotfiles from resources, so the script ships as `zsh/glasshouse.zshenv` and is
    /// written to `~/Library/Caches/<bundle id>/shell-integration/zsh/.zshenv` (0700 dir, 0600 file)
    /// whenever the bundled copy changes.
    static func materializeZshDir(from root: URL) -> URL? {
        let fm = FileManager.default
        guard let caches = fm.urls(for: .cachesDirectory, in: .userDomainMask).first,
              let source = try? Data(contentsOf: root.appendingPathComponent("zsh/glasshouse.zshenv"))
        else { return nil }
        let bundleID = Bundle.main.bundleIdentifier ?? "dev.glasshouse.app"
        let dir = caches.appendingPathComponent("\(bundleID)/shell-integration/zsh", isDirectory: true)
        let target = dir.appendingPathComponent(".zshenv")
        do {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true,
                                   attributes: [.posixPermissions: 0o700])
            if (try? Data(contentsOf: target)) != source {
                try source.write(to: target, options: .atomic)
                try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: target.path)
            }
            return dir
        } catch {
            return nil
        }
    }
}
