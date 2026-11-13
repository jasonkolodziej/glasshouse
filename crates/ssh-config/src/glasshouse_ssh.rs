//! Fixed values Glasshouse writes into SSH config for Secure Enclave keys.

/// Apple's PKCS#11-style provider that signs with `sc_auth` Secure Enclave identities.
pub const SECURITY_KEY_PROVIDER: &str = "/usr/lib/ssh-keychain.dylib";
/// Key type and signature algorithm of a Secure Enclave key reached through [`SECURITY_KEY_PROVIDER`].
pub const SECURE_ENCLAVE_ALGORITHM: &str = "sk-ecdsa-sha2-nistp256@openssh.com";
/// Key types a FIDO security key (a YubiKey, for example) can hold.
pub const SECURITY_KEY_ALGORITHMS: [&str; 2] = [
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];
/// OpenSSH's built-in FIDO support. Homebrew's OpenSSH has it; a build without it needs a provider library.
pub const INTERNAL_PROVIDER: &str = "internal";
/// Added to a host's alias for its security key entry: `myserver` → `myserver-key`.
pub const SECURITY_KEY_ALIAS_SUFFIX: &str = "-key";
/// The file Glasshouse generates, next to `~/.ssh/config`.
pub const INCLUDE_FILE_NAME: &str = "glasshouse.conf";
/// Written exactly like this in `~/.ssh/config`, so it works whatever the home directory is.
pub const INCLUDE_PATH: &str = "~/.ssh/glasshouse.conf";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_path_points_at_the_include_file() {
        assert_eq!(INCLUDE_PATH, format!("~/.ssh/{INCLUDE_FILE_NAME}"));
    }
}
