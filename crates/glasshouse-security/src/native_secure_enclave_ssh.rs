use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Apple's built-in path for Touch ID SSH on macOS 26: a non-exportable Secure Enclave key created
/// by `sc_auth`, surfaced to OpenSSH as an `sk-ecdsa-sha2-nistp256@openssh.com` key through
/// `/usr/lib/ssh-keychain.dylib`. Documented only in the sc_auth(8) and ssh-keychain(8) man pages.
///
/// Key creation, host entries and key installation live in the `ssh-config` crate, which the app
/// uses for Settings › SSH Keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSecureEnclaveSSH;

impl NativeSecureEnclaveSSH {
    /// The PKCS#11-style security key provider shipped with macOS.
    pub const PROVIDER_PATH: &'static str = "/usr/lib/ssh-keychain.dylib";
    /// The smart card utility that creates the Secure Enclave key.
    pub const SC_AUTH_PATH: &'static str = "/usr/sbin/sc_auth";

    /// True when the provider dylib exists and `sc_auth` is executable.
    pub fn is_available() -> bool {
        Self::is_available_in(Path::new("/"))
    }

    /// Same check as [`Self::is_available`], with both paths resolved under `root` (for tests).
    pub fn is_available_in(root: &Path) -> bool {
        let provider = rooted(root, Self::PROVIDER_PATH);
        let sc_auth = rooted(root, Self::SC_AUTH_PATH);
        provider.exists() && is_executable_file(&sc_auth)
    }
}

fn rooted(root: &Path, absolute: &str) -> PathBuf {
    root.join(absolute.trim_start_matches('/'))
}

/// Mirrors `FileManager.isExecutableFile(atPath:)`: an `access(X_OK)` check on a non-directory.
fn is_executable_file(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn scratch_dir() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    fn make_root(provider: bool, sc_auth_mode: Option<u32>) -> tempfile::TempDir {
        let root = scratch_dir();
        fs::create_dir_all(root.path().join("usr/lib")).unwrap();
        fs::create_dir_all(root.path().join("usr/sbin")).unwrap();
        if provider {
            fs::write(root.path().join("usr/lib/ssh-keychain.dylib"), b"").unwrap();
        }
        if let Some(mode) = sc_auth_mode {
            let path = root.path().join("usr/sbin/sc_auth");
            fs::write(&path, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
        root
    }

    #[test]
    fn available_when_provider_exists_and_sc_auth_is_executable() {
        let root = make_root(true, Some(0o755));
        assert!(NativeSecureEnclaveSSH::is_available_in(root.path()));
    }

    #[test]
    fn unavailable_without_provider_dylib() {
        let root = make_root(false, Some(0o755));
        assert!(!NativeSecureEnclaveSSH::is_available_in(root.path()));
    }

    #[test]
    fn unavailable_without_sc_auth() {
        let root = make_root(true, None);
        assert!(!NativeSecureEnclaveSSH::is_available_in(root.path()));
    }
}
