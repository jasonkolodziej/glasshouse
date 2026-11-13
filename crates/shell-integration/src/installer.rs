use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Embedded scripts, as `(path relative to the integration root, contents)`.
///
/// SwiftPM may drop dotfiles from resources, so the zsh script ships as `zsh/glasshouse.zshenv`
/// and is materialized as `zsh/.zshenv`.
const SCRIPTS: &[(&str, &str)] = &[
    (
        "bash/glasshouse.bash",
        include_str!("../resources/bash/glasshouse.bash"),
    ),
    (
        "fish/vendor_conf.d/glasshouse.fish",
        include_str!("../resources/fish/vendor_conf.d/glasshouse.fish"),
    ),
    (
        "zsh/.zshenv",
        include_str!("../resources/zsh/glasshouse.zshenv"),
    ),
];

const DEFAULT_XDG_DATA_DIRS: &str = "/usr/local/share:/usr/share";
const BUNDLE_ID: &str = "dev.glasshouse.app";

/// Failure to write the integration scripts.
#[derive(Debug, thiserror::Error)]
pub enum InstallerError {
    /// No cache directory could be determined (no `HOME`).
    #[error("no cache directory for shell integration — set HOME or pass a cache root explicitly")]
    NoCacheDirectory,
    /// Creating a directory or writing a script failed.
    #[error("failed to write shell integration script {path}: {source}")]
    Io {
        /// The file or directory being written.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
}

/// Shells with an integration script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// zsh, via `ZDOTDIR`.
    Zsh,
    /// bash, via `--rcfile`.
    Bash,
    /// fish, via `XDG_DATA_DIRS`.
    Fish,
    /// Anything else: runs unmodified.
    Other,
}

/// How to launch the user's shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// Path of the shell binary.
    pub executable: String,
    /// Arguments after argv\[0\].
    pub args: Vec<String>,
    /// argv\[0\]. A leading "-" makes the shell a login shell, as Terminal.app does.
    pub exec_name: String,
    /// The complete child environment.
    pub environment: HashMap<String, String>,
}

/// Injects shell integration without editing the user's dotfiles — the same approach Ghostty and kitty use:
///
/// | Shell | Mechanism |
/// |-------|-----------|
/// | zsh   | Point `ZDOTDIR` at our directory; our `.zshenv` restores the user's `ZDOTDIR`, sources their `.zshenv`, then installs hooks. |
/// | fish  | Prepend our directory to `XDG_DATA_DIRS`; fish auto-loads `fish/vendor_conf.d/*.fish`. |
/// | bash  | Interactive non-login bash with `--rcfile` pointing at our script, which sources the user's profile/bashrc first. |
///
/// Unsupported shells run unmodified (no command marks, everything else still works).
///
/// The scripts are embedded in the binary and written to a cache directory (0700 directories,
/// 0600 files) whenever the embedded copy changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellIntegrationInstaller {
    root: PathBuf,
}

impl ShellIntegrationInstaller {
    /// An installer that materializes scripts under `root` (which will contain `bash/`, `fish/`, `zsh/`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// An installer using [`default_root`](Self::default_root).
    pub fn with_default_root() -> Result<Self, InstallerError> {
        Self::default_root()
            .map(Self::new)
            .ok_or(InstallerError::NoCacheDirectory)
    }

    /// `~/Library/Caches/dev.glasshouse.app/shell-integration` on macOS;
    /// `$XDG_CACHE_HOME` (or `~/.cache`)`/dev.glasshouse.app/shell-integration` elsewhere.
    pub fn default_root() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from);
        let caches = if cfg!(target_os = "macos") {
            home?.join("Library/Caches")
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .filter(|p| Path::new(p).is_absolute())
                .map(PathBuf::from)
                .or_else(|| home.map(|h| h.join(".cache")))?
        };
        Some(caches.join(BUNDLE_ID).join("shell-integration"))
    }

    /// The integration root (contains `fish/vendor_conf.d`, `bash/glasshouse.bash`, `zsh/.zshenv`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Which integration applies to `shell_path`, by its file name.
    pub fn detect(shell_path: &str) -> Shell {
        match Path::new(shell_path).file_name().and_then(OsStr::to_str) {
            Some("zsh") => Shell::Zsh,
            Some("bash") => Shell::Bash,
            Some("fish") => Shell::Fish,
            _ => Shell::Other,
        }
    }

    /// The user's login shell from the password database, falling back to `$SHELL`, then zsh.
    pub fn user_shell() -> String {
        passwd_shell()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/zsh".to_owned())
    }

    /// Writes every embedded script under [`root`](Self::root), rewriting only files that changed.
    pub fn materialize(&self) -> Result<(), InstallerError> {
        for (relative, contents) in SCRIPTS {
            let target = self.root.join(relative);
            let dir = target.parent().unwrap_or(&self.root);
            create_private_dir(dir).map_err(|source| InstallerError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
            if fs::read(&target).is_ok_and(|existing| existing == contents.as_bytes()) {
                continue;
            }
            write_private_atomic(&target, contents.as_bytes()).map_err(|source| {
                InstallerError::Io {
                    path: target.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }

    /// Builds the launch plan for `shell_path` on top of `base_environment`.
    ///
    /// With `enabled == false`, or if the scripts can't be written, the shell runs unmodified
    /// as a login shell.
    pub fn plan(
        &self,
        shell_path: &str,
        base_environment: &HashMap<String, String>,
        enabled: bool,
    ) -> LaunchPlan {
        let name = Path::new(shell_path)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or(shell_path)
            .to_owned();
        let mut plan = LaunchPlan {
            executable: shell_path.to_owned(),
            args: Vec::new(),
            exec_name: format!("-{name}"),
            environment: base_environment.clone(),
        };
        let shell = Self::detect(shell_path);
        // Unsupported shells need no scripts on disk.
        if !enabled || (shell != Shell::Other && self.materialize().is_err()) {
            return plan;
        }

        let env = &mut plan.environment;
        let root = self.root.to_string_lossy().into_owned();
        match shell {
            Shell::Zsh => {
                if let Some(orig) = env.get("ZDOTDIR").cloned() {
                    env.insert("GLASSHOUSE_ORIG_ZDOTDIR".into(), orig);
                }
                env.insert(
                    "ZDOTDIR".into(),
                    self.root.join("zsh").to_string_lossy().into_owned(),
                );
            }
            Shell::Fish => {
                // The root contains fish/vendor_conf.d.
                let existing = env
                    .get("XDG_DATA_DIRS")
                    .map_or(DEFAULT_XDG_DATA_DIRS, String::as_str);
                let value = format!("{root}:{existing}");
                env.insert("XDG_DATA_DIRS".into(), value);
                env.insert("GLASSHOUSE_XDG_DATA_DIRS_PREFIX".into(), root);
            }
            Shell::Bash => {
                // --rcfile is ignored for login shells, so run interactive non-login; our rc sources the profile.
                let rcfile = self
                    .root
                    .join("bash/glasshouse.bash")
                    .to_string_lossy()
                    .into_owned();
                plan.args = vec!["--rcfile".into(), rcfile, "-i".into()];
                plan.exec_name = name;
            }
            Shell::Other => {}
        }
        plan.environment
            .insert("GLASSHOUSE_SHELL_INTEGRATION".into(), "1".into());
        plan
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Writes through a sibling temp file and renames, so a shell never sources a half-written script.
fn write_private_atomic(target: &Path, contents: &[u8]) -> io::Result<()> {
    let file_name = target
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_default();
    let mut tmp_name = file_name;
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = target.with_file_name(tmp_name);
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        set_private_file_permissions(&tmp)?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn passwd_shell() -> Option<String> {
    use std::ffi::CStr;

    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: `passwd` is plain old data; an all-zero value is valid and only used as an out-param.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the duration of the call and `buf.len()` is its true size.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || pwd.pw_shell.is_null() {
        return None;
    }
    // SAFETY: on success `pw_shell` points to a NUL-terminated string inside `buf`, which is alive here.
    let shell = unsafe { CStr::from_ptr(pwd.pw_shell) };
    shell.to_str().ok().map(str::to_owned)
}

#[cfg(not(unix))]
fn passwd_shell() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installer() -> (tempfile::TempDir, ShellIntegrationInstaller) {
        let dir = tempfile::tempdir().unwrap();
        let installer = ShellIntegrationInstaller::new(dir.path().join("shell-integration"));
        (dir, installer)
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn detects_shells_by_file_name() {
        assert_eq!(ShellIntegrationInstaller::detect("/bin/zsh"), Shell::Zsh);
        assert_eq!(
            ShellIntegrationInstaller::detect("/opt/homebrew/bin/fish"),
            Shell::Fish
        );
        assert_eq!(ShellIntegrationInstaller::detect("bash"), Shell::Bash);
        assert_eq!(ShellIntegrationInstaller::detect("/bin/tcsh"), Shell::Other);
    }

    #[test]
    fn zsh_points_zdotdir_at_materialized_script_and_keeps_original() {
        let (_tmp, inst) = installer();
        let plan = inst.plan(
            "/bin/zsh",
            &env(&[("ZDOTDIR", "/Users/j/.config/zsh")]),
            true,
        );
        let zdotdir = inst.root().join("zsh");
        assert_eq!(plan.environment["ZDOTDIR"], zdotdir.to_string_lossy());
        assert_eq!(
            plan.environment["GLASSHOUSE_ORIG_ZDOTDIR"],
            "/Users/j/.config/zsh"
        );
        assert_eq!(plan.environment["GLASSHOUSE_SHELL_INTEGRATION"], "1");
        assert_eq!((plan.exec_name.as_str(), plan.args.len()), ("-zsh", 0));
        let script = fs::read_to_string(zdotdir.join(".zshenv")).unwrap();
        assert!(script.contains("133;A"));
    }

    #[test]
    fn zsh_without_zdotdir_sets_no_original() {
        let (_tmp, inst) = installer();
        let plan = inst.plan("/bin/zsh", &HashMap::new(), true);
        assert!(!plan.environment.contains_key("GLASSHOUSE_ORIG_ZDOTDIR"));
    }

    #[cfg(unix)]
    #[test]
    fn materialized_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, inst) = installer();
        inst.materialize().unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&inst.root().join("zsh")), 0o700);
        assert_eq!(mode(&inst.root().join("fish/vendor_conf.d")), 0o700);
        assert_eq!(mode(&inst.root().join("zsh/.zshenv")), 0o600);
        assert_eq!(mode(&inst.root().join("bash/glasshouse.bash")), 0o600);
        // A changed copy is rewritten.
        fs::write(inst.root().join("zsh/.zshenv"), "stale").unwrap();
        inst.materialize().unwrap();
        assert_ne!(
            fs::read_to_string(inst.root().join("zsh/.zshenv")).unwrap(),
            "stale"
        );
    }

    #[test]
    fn fish_prefixes_xdg_data_dirs() {
        let (_tmp, inst) = installer();
        let root = inst.root().to_string_lossy().into_owned();
        let plan = inst.plan("/usr/bin/fish", &HashMap::new(), true);
        assert_eq!(
            plan.environment["XDG_DATA_DIRS"],
            format!("{root}:/usr/local/share:/usr/share")
        );
        assert_eq!(plan.environment["GLASSHOUSE_XDG_DATA_DIRS_PREFIX"], root);
        assert!(
            inst.root()
                .join("fish/vendor_conf.d/glasshouse.fish")
                .is_file()
        );

        let plan = inst.plan(
            "/usr/bin/fish",
            &env(&[("XDG_DATA_DIRS", "/opt/share")]),
            true,
        );
        assert_eq!(
            plan.environment["XDG_DATA_DIRS"],
            format!("{root}:/opt/share")
        );
        assert_eq!(plan.exec_name, "-fish");
    }

    #[test]
    fn bash_runs_interactive_non_login_with_rcfile() {
        let (_tmp, inst) = installer();
        let plan = inst.plan("/bin/bash", &HashMap::new(), true);
        let rcfile = inst.root().join("bash/glasshouse.bash");
        assert_eq!(
            plan.args,
            [
                "--rcfile".to_owned(),
                rcfile.to_string_lossy().into_owned(),
                "-i".to_owned()
            ]
        );
        assert_eq!(plan.exec_name, "bash");
        assert!(rcfile.is_file());
    }

    #[test]
    fn other_shells_and_disabled_integration_run_unmodified() {
        let (tmp, inst) = installer();
        let base = env(&[("PATH", "/usr/bin")]);
        let plan = inst.plan("/bin/tcsh", &base, true);
        assert_eq!(plan.exec_name, "-tcsh");
        assert_eq!(plan.environment["GLASSHOUSE_SHELL_INTEGRATION"], "1");
        assert_eq!(plan.environment.len(), 2);
        assert!(!inst.root().exists());

        let plan = inst.plan("/bin/zsh", &base, false);
        assert_eq!(plan.environment, base);
        assert_eq!(plan.exec_name, "-zsh");
        assert!(!inst.root().exists());

        // Scripts that can't be written leave the shell unmodified.
        let blocker = tmp.path().join("file");
        fs::write(&blocker, "").unwrap();
        let plan =
            ShellIntegrationInstaller::new(blocker.join("root")).plan("/bin/zsh", &base, true);
        assert_eq!(plan.environment, base);
    }
}
