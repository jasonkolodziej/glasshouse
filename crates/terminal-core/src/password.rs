/// Whether the foreground program is reading a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordPromptState {
    /// Ordinary input.
    Normal,
    /// A line-buffered, non-echoing read (password prompt).
    SecretInput,
}

/// Detects "the foreground program is reading a secret" from the PTY's line discipline,
/// not from screen text — so it can't be spoofed by output that merely *looks* like `Password:`.
///
/// Heuristic used by WezTerm and Ghostty: ECHO off + ICANON on ⇒ a line-buffered, non-echoing read
/// (sudo, ssh password/passphrase, gpg pinentry-tty, `read -s`). Limitation: prompts that originate
/// on a *remote* host or inside tmux are invisible here, because the local tty is in raw mode.
#[derive(Debug, Clone, Copy)]
pub struct PasswordPromptDetector;

impl PasswordPromptDetector {
    /// Reads termios from the PTY master `fd`; [`Normal`](PasswordPromptState::Normal) if that fails.
    #[cfg(unix)]
    pub fn state(master_fd: i32) -> PasswordPromptState {
        // SAFETY: `termios` is plain old data; all-zero is a valid out-param value.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `t` is a valid, writable termios; an invalid fd just makes tcgetattr fail.
        if unsafe { libc::tcgetattr(master_fd, &mut t) } != 0 {
            return PasswordPromptState::Normal;
        }
        Self::state_for_local_flags(t.c_lflag)
    }

    /// Always [`Normal`](PasswordPromptState::Normal): no termios off Unix.
    #[cfg(not(unix))]
    pub fn state(_master_fd: i32) -> PasswordPromptState {
        PasswordPromptState::Normal
    }

    /// Classifies a termios `c_lflag` value.
    #[cfg(unix)]
    pub fn state_for_local_flags(lflag: libc::tcflag_t) -> PasswordPromptState {
        Self::state_for(lflag & libc::ECHO != 0, lflag & libc::ICANON != 0)
    }

    /// The pure decision: secret input iff echo is off and canonical mode is on.
    pub fn state_for(echo: bool, canonical: bool) -> PasswordPromptState {
        if !echo && canonical {
            PasswordPromptState::SecretInput
        } else {
            PasswordPromptState::Normal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_input_only_when_echo_off_and_canonical() {
        assert_eq!(
            PasswordPromptDetector::state_for(false, true),
            PasswordPromptState::SecretInput
        );
        assert_eq!(
            PasswordPromptDetector::state_for(true, true),
            PasswordPromptState::Normal
        );
        assert_eq!(
            PasswordPromptDetector::state_for(false, false),
            PasswordPromptState::Normal
        );
        assert_eq!(
            PasswordPromptDetector::state_for(true, false),
            PasswordPromptState::Normal
        );
    }

    #[cfg(unix)]
    #[test]
    fn classifies_local_flags_and_invalid_fds() {
        assert_eq!(
            PasswordPromptDetector::state_for_local_flags(libc::ICANON | libc::ISIG),
            PasswordPromptState::SecretInput
        );
        assert_eq!(
            PasswordPromptDetector::state_for_local_flags(libc::ICANON | libc::ECHO),
            PasswordPromptState::Normal
        );
        assert_eq!(
            PasswordPromptDetector::state(-1),
            PasswordPromptState::Normal
        );
    }
}
