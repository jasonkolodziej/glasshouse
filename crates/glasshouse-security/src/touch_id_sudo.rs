use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Whether Touch ID for sudo is configured, as read from `/etc/pam.d/sudo_local`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TouchIDSudoStatus {
    /// An uncommented `auth … sufficient … pam_tid.so` line is present.
    pub pam_tid_enabled: bool,
    /// `pam_reattach.so` is referenced.
    pub pam_reattach_configured: bool,
    /// pam_reattach referenced from a user-writable location (e.g. /opt/homebrew) — a root-escalation risk.
    pub pam_reattach_user_writable: bool,
    /// `/etc/pam.d/sudo_local` exists and is readable.
    pub sudo_local_exists: bool,
}

/// Owner and permission bits of a filesystem item, as used by the root-owned chain check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileOwnership {
    /// Owning user ID.
    pub uid: u32,
    /// Permission bits (`st_mode & 0o7777`).
    pub mode: u32,
}

impl FileOwnership {
    /// Reads the owner and permissions of `path` from the real filesystem.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        Some(Self {
            uid: meta.uid(),
            mode: meta.mode() & 0o7777,
        })
    }
}

/// Reports whether Touch ID for sudo is configured, and produces the hardened setup script.
/// Glasshouse never edits /etc itself: it runs the script in a visible tab so the user sees every
/// privileged command (and approves it — with Touch ID once it's on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchIDSudo;

impl TouchIDSudo {
    /// The PAM include that survives macOS updates.
    pub const SUDO_LOCAL_PATH: &'static str = "/etc/pam.d/sudo_local";
    /// Root-owned install location for pam_reattach used by the setup script.
    pub const SAFE_REATTACH_PATH: &'static str = "/usr/local/lib/pam/pam_reattach.so";

    /// Undoes everything the setup script installs.
    pub const ROLLBACK_SCRIPT: &'static str =
        "sudo rm -f /etc/pam.d/sudo_local /etc/sudoers.d/10-glasshouse-touchid\nsudo visudo -c";

    /// Status of the live system's `/etc/pam.d/sudo_local`.
    pub fn status() -> TouchIDSudoStatus {
        Self::status_with(Path::new(Self::SUDO_LOCAL_PATH), &FileOwnership::of)
    }

    /// Status read from `sudo_local`, with ownership looked up through `ownership` (injectable for tests).
    pub fn status_with(
        sudo_local: &Path,
        ownership: &dyn Fn(&Path) -> Option<FileOwnership>,
    ) -> TouchIDSudoStatus {
        let Ok(text) = fs::read_to_string(sudo_local) else {
            return TouchIDSudoStatus::default();
        };
        let lines: Vec<&str> = text
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(trim_horizontal_whitespace)
            .filter(|line| !line.starts_with('#'))
            .collect();
        let tid = lines.iter().any(|line| {
            line.starts_with("auth") && line.contains("pam_tid.so") && line.contains("sufficient")
        });
        let reattach_line = lines.iter().find(|line| line.contains("pam_reattach.so"));
        let user_writable = reattach_line
            .and_then(|line| {
                line.split(' ')
                    .find(|word| word.ends_with("pam_reattach.so"))
            })
            .is_some_and(|path| !Self::is_root_owned_chain(Path::new(path), ownership));
        TouchIDSudoStatus {
            pam_tid_enabled: tid,
            pam_reattach_configured: reattach_line.is_some(),
            pam_reattach_user_writable: user_writable,
            sudo_local_exists: true,
        }
    }

    /// The file and every ancestor directory must be root-owned and not group/other-writable.
    pub fn is_root_owned_chain(
        path: &Path,
        ownership: &dyn Fn(&Path) -> Option<FileOwnership>,
    ) -> bool {
        let mut current = resolve(path);
        loop {
            let Some(attrs) = ownership(&current) else {
                return false;
            };
            if attrs.uid != 0 || attrs.mode & 0o022 != 0 {
                return false;
            }
            if current == Path::new("/") {
                return true;
            }
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => return false,
            }
        }
    }

    /// Hardened setup, matching the earlier "Touch ID for sudo and SSH" report.
    /// `include_reattach` is only needed for tmux/screen users.
    pub fn setup_script(include_reattach: bool) -> String {
        let mut s = String::from(concat!(
            "set -euo pipefail\n",
            "echo \"Glasshouse: enabling Touch ID for sudo (password remains the fallback).\"\n",
            "sudo -v   # authenticate once up front; keep this tab open until verified\n",
        ));
        if include_reattach {
            s += concat!(
                "if ! brew list pam-reattach >/dev/null 2>&1; then brew install pam-reattach; fi\n",
                "sudo install -d -o root -g wheel -m 0755 /usr/local/lib/pam\n",
                "sudo install -o root -g wheel -m 0444 \"$(brew --prefix)/lib/pam/pam_reattach.so\" ",
            );
            s += Self::SAFE_REATTACH_PATH;
            s += "\n";
        }
        let reattach_line = if include_reattach {
            format!(
                "auth       optional       {} ignore_ssh\n",
                Self::SAFE_REATTACH_PATH
            )
        } else {
            String::new()
        };
        let sudo_local = Self::SUDO_LOCAL_PATH;
        s += &format!(
            concat!(
                "sudo tee {sudo_local} >/dev/null <<'EOF'\n",
                "# sudo_local: local config file which survives system update and is included for sudo\n",
                "{reattach_line}auth       sufficient     pam_tid.so\n",
                "EOF\n",
                "sudo chown root:wheel {sudo_local} && sudo chmod 0444 {sudo_local}\n",
                "\n",
                "# One tap = one command: no cached sudo ticket (DISA STIG for macOS 26).\n",
                "echo 'Defaults timestamp_timeout=0' | sudo tee /etc/sudoers.d/10-glasshouse-touchid >/dev/null\n",
                "sudo chmod 0440 /etc/sudoers.d/10-glasshouse-touchid\n",
                "sudo visudo -c\n",
                "\n",
                "sudo -k && echo \"Verify: the next command should ask for Touch ID.\" && sudo true && ",
                "echo \"\u{2713} Touch ID sudo works\"",
            ),
            sudo_local = sudo_local,
            reattach_line = reattach_line,
        );
        s
    }
}

/// Swift's `trimmingCharacters(in: .whitespaces)`: spaces and tabs, but not line breaks.
fn trim_horizontal_whitespace(line: &str) -> &str {
    line.trim_matches(|c: char| {
        c.is_whitespace()
            && !matches!(
                c,
                '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
    })
}

/// Like `URL(fileURLWithPath:).resolvingSymlinksInPath()`: absolute, with symlinks resolved when the
/// path exists and left as-is otherwise.
fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn scratch_dir() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    fn fake_fs(entries: &[(&str, u32, u32)]) -> impl Fn(&Path) -> Option<FileOwnership> + use<> {
        let map: HashMap<PathBuf, FileOwnership> = entries
            .iter()
            .map(|&(p, uid, mode)| (PathBuf::from(p), FileOwnership { uid, mode }))
            .collect();
        move |p: &Path| map.get(p).copied()
    }

    fn root_chain() -> Vec<(&'static str, u32, u32)> {
        vec![
            ("/", 0, 0o755),
            ("/usr", 0, 0o755),
            ("/usr/local", 0, 0o755),
            ("/usr/local/lib", 0, 0o755),
            ("/usr/local/lib/pam", 0, 0o755),
            ("/usr/local/lib/pam/pam_reattach.so", 0, 0o444),
        ]
    }

    #[test]
    fn root_owned_chain_accepts_root_owned_non_writable_ancestors() {
        let fs = fake_fs(&root_chain());
        assert!(TouchIDSudo::is_root_owned_chain(
            Path::new(TouchIDSudo::SAFE_REATTACH_PATH),
            &fs
        ));
    }

    #[test]
    fn root_owned_chain_rejects_user_owned_or_group_writable_ancestors() {
        let mut user_owned = root_chain();
        user_owned[3] = ("/usr/local/lib", 501, 0o755);
        let path = Path::new(TouchIDSudo::SAFE_REATTACH_PATH);
        assert!(!TouchIDSudo::is_root_owned_chain(
            path,
            &fake_fs(&user_owned)
        ));

        let mut group_writable = root_chain();
        group_writable[1] = ("/usr", 0, 0o775);
        assert!(!TouchIDSudo::is_root_owned_chain(
            path,
            &fake_fs(&group_writable)
        ));

        assert!(!TouchIDSudo::is_root_owned_chain(
            path,
            &fake_fs(&root_chain()[1..])
        ));
    }

    #[test]
    fn status_of_missing_sudo_local_reports_nothing_configured() {
        let dir = scratch_dir();
        let status = TouchIDSudo::status_with(&dir.path().join("sudo_local"), &fake_fs(&[]));
        assert_eq!(status, TouchIDSudoStatus::default());
    }

    #[test]
    fn status_detects_pam_tid_and_user_writable_reattach() {
        let dir = scratch_dir();
        let file = dir.path().join("sudo_local");
        fs::write(
            &file,
            "# sudo_local\n#auth sufficient pam_tid.so\n  auth       optional       \
             /opt/homebrew/lib/pam/pam_reattach.so ignore_ssh\n\tauth       sufficient     pam_tid.so\n",
        )
        .unwrap();
        let status = TouchIDSudo::status_with(&file, &fake_fs(&root_chain()));
        assert_eq!(
            status,
            TouchIDSudoStatus {
                pam_tid_enabled: true,
                pam_reattach_configured: true,
                pam_reattach_user_writable: true,
                sudo_local_exists: true,
            }
        );
    }

    #[test]
    fn status_accepts_reattach_from_root_owned_path_and_ignores_comments() {
        let dir = scratch_dir();
        let file = dir.path().join("sudo_local");
        fs::write(
            &file,
            "auth       optional       /usr/local/lib/pam/pam_reattach.so ignore_ssh\n# auth sufficient pam_tid.so\n",
        )
        .unwrap();
        let status = TouchIDSudo::status_with(&file, &fake_fs(&root_chain()));
        assert!(!status.pam_tid_enabled);
        assert!(status.pam_reattach_configured);
        assert!(!status.pam_reattach_user_writable);
        assert!(status.sudo_local_exists);
    }

    #[test]
    fn setup_script_without_reattach_matches_swift() {
        let expected = "set -euo pipefail
echo \"Glasshouse: enabling Touch ID for sudo (password remains the fallback).\"
sudo -v   # authenticate once up front; keep this tab open until verified
sudo tee /etc/pam.d/sudo_local >/dev/null <<'EOF'
# sudo_local: local config file which survives system update and is included for sudo
auth       sufficient     pam_tid.so
EOF
sudo chown root:wheel /etc/pam.d/sudo_local && sudo chmod 0444 /etc/pam.d/sudo_local

# One tap = one command: no cached sudo ticket (DISA STIG for macOS 26).
echo 'Defaults timestamp_timeout=0' | sudo tee /etc/sudoers.d/10-glasshouse-touchid >/dev/null
sudo chmod 0440 /etc/sudoers.d/10-glasshouse-touchid
sudo visudo -c

sudo -k && echo \"Verify: the next command should ask for Touch ID.\" && sudo true && echo \"✓ Touch ID sudo works\"";
        assert_eq!(TouchIDSudo::setup_script(false), expected);
    }

    #[test]
    fn setup_script_with_reattach_installs_root_owned_copy_first() {
        let script = TouchIDSudo::setup_script(true);
        let expected_block = "sudo -v   # authenticate once up front; keep this tab open until verified
if ! brew list pam-reattach >/dev/null 2>&1; then brew install pam-reattach; fi
sudo install -d -o root -g wheel -m 0755 /usr/local/lib/pam
sudo install -o root -g wheel -m 0444 \"$(brew --prefix)/lib/pam/pam_reattach.so\" /usr/local/lib/pam/pam_reattach.so
sudo tee /etc/pam.d/sudo_local >/dev/null <<'EOF'
# sudo_local: local config file which survives system update and is included for sudo
auth       optional       /usr/local/lib/pam/pam_reattach.so ignore_ssh
auth       sufficient     pam_tid.so
EOF
";
        assert!(script.contains(expected_block), "{script}");
    }

    #[test]
    fn rollback_script_removes_both_files() {
        assert_eq!(
            TouchIDSudo::ROLLBACK_SCRIPT,
            "sudo rm -f /etc/pam.d/sudo_local /etc/sudoers.d/10-glasshouse-touchid\nsudo visudo -c"
        );
    }
}
