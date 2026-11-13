use std::collections::HashMap;
use std::path::Path;

/// Variables we never forward from the GUI app's own environment into a shell.
const STRIPPED: [&str; 6] = [
    "TERM_SESSION_ID",
    "ITERM_SESSION_ID",
    "__CFBundleIdentifier",
    "XPC_SERVICE_NAME",
    "XPC_FLAGS",
    "SSH_AUTH_SOCK_GLASSHOUSE_INTERNAL",
];

const SSH_KEYCHAIN_PROVIDER: &str = "/usr/lib/ssh-keychain.dylib";

/// Inputs to [`PtyEnvironment::make`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyEnvironmentOptions {
    /// Reported as `TERM_PROGRAM_VERSION`.
    pub app_version: String,
    /// When set, child shells use the Glasshouse Secure Enclave agent (opt-in; see SSH settings).
    pub agent_socket: Option<String>,
    /// When set, OpenSSH uses Apple's native Secure Enclave provider for `-sk` keys.
    pub use_native_secure_enclave_provider: bool,
    /// Locale identifier such as `en_US` used to default `LANG` (`<id>.UTF-8`);
    /// `None` uses `en_US`. The app supplies the system locale here.
    pub locale_identifier: Option<String>,
}

impl PtyEnvironmentOptions {
    /// Options with no agent socket and the native Secure Enclave provider enabled.
    pub fn new(app_version: impl Into<String>) -> Self {
        Self {
            app_version: app_version.into(),
            agent_socket: None,
            use_native_secure_enclave_provider: true,
            locale_identifier: None,
        }
    }
}

/// Builds the child-shell environment.
#[derive(Debug, Clone, Copy)]
pub struct PtyEnvironment;

impl PtyEnvironment {
    /// The child environment derived from `inherited` (usually [`current`](Self::current)).
    pub fn make(
        options: &PtyEnvironmentOptions,
        inherited: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        Self::make_with(options, inherited, |p| p.exists())
    }

    /// Like [`make`](Self::make), with an injectable file-exists check (for tests).
    pub fn make_with(
        options: &PtyEnvironmentOptions,
        inherited: &HashMap<String, String>,
        file_exists: impl Fn(&Path) -> bool,
    ) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = inherited
            .iter()
            .filter(|(k, _)| !STRIPPED.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TERM_PROGRAM".into(), "Glasshouse".into());
        env.insert("TERM_PROGRAM_VERSION".into(), options.app_version.clone());
        env.entry("LANG".into()).or_insert_with(|| {
            let id = options.locale_identifier.as_deref().unwrap_or("en_US");
            format!("{}.UTF-8", id.replace('-', "_"))
        });
        env.entry("HOME".into()).or_insert_with(home_directory);

        if let Some(socket) = &options.agent_socket {
            env.insert("SSH_AUTH_SOCK".into(), socket.clone());
        }
        if options.use_native_secure_enclave_provider
            && file_exists(Path::new(SSH_KEYCHAIN_PROVIDER))
        {
            // Lets plain `ssh` / `ssh-add -K` use Touch ID-gated Secure Enclave keys created with sc_auth.
            env.insert("SSH_SK_PROVIDER".into(), SSH_KEYCHAIN_PROVIDER.into());
        }
        env
    }

    /// This process's environment (non-UTF-8 entries converted lossily).
    pub fn current() -> HashMap<String, String> {
        std::env::vars_os()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect()
    }
}

/// The user's home directory from the password database (what `NSHomeDirectory()` returns), or `/`.
#[cfg(unix)]
fn home_directory() -> String {
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
    if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return "/".into();
    }
    // SAFETY: on success `pw_dir` points to a NUL-terminated string inside `buf`, which is alive here.
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
    dir.to_str().map_or_else(|_| "/".into(), str::to_owned)
}

#[cfg(not(unix))]
fn home_directory() -> String {
    "/".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inherited() -> HashMap<String, String> {
        [
            ("PATH", "/usr/bin"),
            ("TERM_SESSION_ID", "w0t0p0"),
            ("ITERM_SESSION_ID", "x"),
            ("__CFBundleIdentifier", "dev.glasshouse.app"),
            ("XPC_SERVICE_NAME", "0"),
            ("XPC_FLAGS", "0x0"),
            ("SSH_AUTH_SOCK_GLASSHOUSE_INTERNAL", "/tmp/s"),
            ("SSH_AUTH_SOCK", "/tmp/launchd/Listeners"),
            ("TERM", "dumb"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    #[test]
    fn strips_gui_variables_and_sets_terminal_identity() {
        let env =
            PtyEnvironment::make_with(&PtyEnvironmentOptions::new("1.2.3"), &inherited(), |_| {
                false
            });
        for key in STRIPPED {
            assert!(!env.contains_key(key), "{key} should be stripped");
        }
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["COLORTERM"], "truecolor");
        assert_eq!(env["TERM_PROGRAM"], "Glasshouse");
        assert_eq!(env["TERM_PROGRAM_VERSION"], "1.2.3");
        assert_eq!(env["PATH"], "/usr/bin");
        assert_eq!(env["SSH_AUTH_SOCK"], "/tmp/launchd/Listeners");
        assert_eq!(env["LANG"], "en_US.UTF-8");
        assert!(!env["HOME"].is_empty());
        assert!(!env.contains_key("SSH_SK_PROVIDER"));
    }

    #[test]
    fn keeps_existing_lang_and_home_and_uses_locale_identifier() {
        let mut base = inherited();
        base.insert("HOME".into(), "/Users/j".into());
        let env = PtyEnvironment::make_with(&PtyEnvironmentOptions::new("1"), &base, |_| false);
        assert_eq!(env["HOME"], "/Users/j");

        base.insert("LANG".into(), "de_DE.UTF-8".into());
        let env = PtyEnvironment::make_with(&PtyEnvironmentOptions::new("1"), &base, |_| false);
        assert_eq!(env["LANG"], "de_DE.UTF-8");

        let options = PtyEnvironmentOptions {
            locale_identifier: Some("pt-BR".into()),
            ..PtyEnvironmentOptions::new("1")
        };
        let env = PtyEnvironment::make_with(&options, &inherited(), |_| false);
        assert_eq!(env["LANG"], "pt_BR.UTF-8");
    }

    #[test]
    fn agent_socket_and_secure_enclave_provider() {
        let options = PtyEnvironmentOptions {
            agent_socket: Some("/Users/j/.glasshouse/agent.sock".into()),
            ..PtyEnvironmentOptions::new("1")
        };
        let env = PtyEnvironment::make_with(&options, &inherited(), |p| {
            p == Path::new(SSH_KEYCHAIN_PROVIDER)
        });
        assert_eq!(env["SSH_AUTH_SOCK"], "/Users/j/.glasshouse/agent.sock");
        assert_eq!(env["SSH_SK_PROVIDER"], "/usr/lib/ssh-keychain.dylib");

        let options = PtyEnvironmentOptions {
            use_native_secure_enclave_provider: false,
            ..options
        };
        let env = PtyEnvironment::make_with(&options, &inherited(), |_| true);
        assert!(!env.contains_key("SSH_SK_PROVIDER"));
    }
}
