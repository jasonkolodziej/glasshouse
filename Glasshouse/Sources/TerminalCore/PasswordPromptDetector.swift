import Darwin

/// Detects "the foreground program is reading a secret" from the PTY's line discipline,
/// not from screen text — so it can't be spoofed by output that merely *looks* like `Password:`.
///
/// Heuristic used by WezTerm and Ghostty: ECHO off + ICANON on ⇒ a line-buffered, non-echoing read
/// (sudo, ssh password/passphrase, gpg pinentry-tty, `read -s`). Limitation: prompts that originate
/// on a *remote* host or inside tmux are invisible here, because the local tty is in raw mode.
public enum PasswordPromptDetector {
    public enum State: Equatable, Sendable { case normal, secretInput }

    public static func state(masterFD fd: Int32) -> State {
        var t = termios()
        guard tcgetattr(fd, &t) == 0 else { return .normal }
        let echoOff = (t.c_lflag & tcflag_t(ECHO)) == 0
        let canonical = (t.c_lflag & tcflag_t(ICANON)) != 0
        return (echoOff && canonical) ? .secretInput : .normal
    }
}
