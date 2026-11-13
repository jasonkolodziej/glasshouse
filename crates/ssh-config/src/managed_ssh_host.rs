use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::SshConfigError;
use crate::glasshouse_ssh;
use crate::swift_text::{is_newline, split_max, trim_whitespace};

/// A FIDO security key on this Mac, used through its own entry (`<alias>-key`): `SecurityKeyProvider` is one
/// value per host, and the main entry's is Apple's Secure Enclave provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecurityKey {
    /// File name under `~/.ssh` of the key handle.
    pub key_name: String,
    /// `internal`, or the absolute path of a provider library.
    pub provider: String,
}

impl SecurityKey {
    /// A key used through OpenSSH's built-in FIDO support. Trims whitespace.
    pub fn new(key_name: &str) -> Self {
        Self::with_provider(key_name, glasshouse_ssh::INTERNAL_PROVIDER)
    }

    /// A key used through `provider`. Trims whitespace from both values.
    pub fn with_provider(key_name: &str, provider: &str) -> Self {
        Self {
            key_name: trim_whitespace(key_name).to_owned(),
            provider: trim_whitespace(provider).to_owned(),
        }
    }
}

/// One `Host` entry in `~/.ssh/glasshouse.conf`, plus the keys installed on that server. Every field is validated
/// before it is written: values end up in ssh_config, where a stray newline or `%` token would let a field add
/// directives such as `ProxyCommand`, and in shell scripts, where a leading `-` would turn an alias into an option.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ManagedSshHost {
    /// What you type after `ssh`.
    pub alias: String,
    /// The server's host name or IP address.
    pub host_name: String,
    /// `None` lets ssh use your local user name.
    pub user: Option<String>,
    /// `None` lets ssh use port 22, or whatever another block sets.
    pub port: Option<i64>,
    /// File name under `~/.ssh` of the Secure Enclave key handle: `<key_name>` and `<key_name>.pub`.
    pub key_name: String,
    /// The FIDO security key this host also accepts, if any.
    pub security_key: Option<SecurityKey>,
    /// Public keys from devices other than this Mac (another computer, a security key kept elsewhere), installed on
    /// the server so you can still log in without this Mac. One `authorized_keys`-style line each.
    pub backup_keys: Vec<String>,
}

impl ManagedSshHost {
    /// Trims whitespace and turns an empty user into `None`. Call [`validated`](Self::validated) before writing.
    pub fn new(
        alias: &str,
        host_name: &str,
        user: Option<&str>,
        port: Option<i64>,
        key_name: &str,
    ) -> Self {
        Self {
            alias: trim_whitespace(alias).to_owned(),
            host_name: trim_whitespace(host_name).to_owned(),
            user: user
                .map(trim_whitespace)
                .filter(|user| !user.is_empty())
                .map(str::to_owned),
            port,
            key_name: trim_whitespace(key_name).to_owned(),
            security_key: None,
            backup_keys: Vec::new(),
        }
    }

    /// The same host with `security_key`.
    pub fn with_security_key(mut self, security_key: Option<SecurityKey>) -> Self {
        self.security_key = security_key;
        self
    }

    /// The same host with `backup_keys`.
    pub fn with_backup_keys(mut self, backup_keys: Vec<String>) -> Self {
        self.backup_keys = backup_keys;
        self
    }

    /// Stable identity: the alias.
    pub fn id(&self) -> &str {
        &self.alias
    }

    /// `alias-key` when the host has a security key.
    pub fn security_key_alias(&self) -> Option<String> {
        self.security_key.as_ref().map(|_| {
            format!(
                "{}{}",
                self.alias,
                glasshouse_ssh::SECURITY_KEY_ALIAS_SUFFIX
            )
        })
    }

    /// Every alias this host writes to the config file.
    pub fn aliases(&self) -> Vec<String> {
        std::iter::once(self.alias.clone())
            .chain(self.security_key_alias())
            .collect()
    }

    /// Checks every field, and returns the host with its backup keys parsed, rewritten as `type blob comment`
    /// and without repeats.
    pub fn validated(&self) -> Result<Self, SshConfigError> {
        if !is_token(&self.alias, "._-") {
            return Err(SshConfigError::InvalidAlias(self.alias.clone()));
        }
        if !is_token(&self.host_name, "._-:") {
            return Err(SshConfigError::InvalidHostName(self.host_name.clone()));
        }
        if let Some(user) = &self.user
            && !is_token(user, "._-@")
        {
            return Err(SshConfigError::InvalidUser(user.clone()));
        }
        if let Some(port) = self.port
            && !(1..=65_535).contains(&port)
        {
            return Err(SshConfigError::InvalidPort(port));
        }
        if !Self::is_valid_key_name(&self.key_name) {
            return Err(SshConfigError::InvalidKeyName(self.key_name.clone()));
        }
        if let Some(security_key) = &self.security_key {
            if !Self::is_valid_key_name(&security_key.key_name)
                || security_key.key_name == self.key_name
            {
                return Err(SshConfigError::InvalidKeyName(
                    security_key.key_name.clone(),
                ));
            }
            if !Self::is_valid_provider(&security_key.provider) {
                return Err(SshConfigError::InvalidProvider(
                    security_key.provider.clone(),
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut backup_keys = Vec::new();
        for line in &self.backup_keys {
            let key = SshPublicKey::parse(line)?;
            if seen.insert(key.blob.clone()) {
                backup_keys.push(key.line());
            }
        }
        if backup_keys.len() > 16 {
            return Err(SshConfigError::TooManyBackupKeys);
        }
        Ok(Self {
            backup_keys,
            ..self.clone()
        })
    }

    /// The backup keys that parse, in order.
    pub fn parsed_backup_keys(&self) -> Vec<SshPublicKey> {
        self.backup_keys
            .iter()
            .filter_map(|line| SshPublicKey::parse(line).ok())
            .collect()
    }

    /// A plain file name in `~/.ssh`: no path separators, not hidden, not the `.pub` half.
    pub fn is_valid_key_name(name: &str) -> bool {
        is_token(name, "._-") && !name.starts_with('.') && !name.ends_with(".pub")
    }

    /// `internal`, or an absolute path without spaces or `%` tokens.
    pub fn is_valid_provider(provider: &str) -> bool {
        provider == glasshouse_ssh::INTERNAL_PROVIDER
            || (provider.starts_with('/') && is_token(provider, "._-/+"))
    }

    /// The `Host` blocks. `IdentityAgent none` and the `PubkeyAcceptedAlgorithms` restriction are what make
    /// every login need its key: `IdentityFile` lines from other matching blocks (`Host *` included) still add up,
    /// and ssh offers keys held by an agent before any key file.
    pub fn config_block(&self) -> String {
        let mut text = self.block(
            &self.alias,
            &self.key_name,
            glasshouse_ssh::SECURITY_KEY_PROVIDER,
            &[glasshouse_ssh::SECURE_ENCLAVE_ALGORITHM],
        );
        if let (Some(security_key), Some(alias)) = (&self.security_key, self.security_key_alias()) {
            text.push('\n');
            text += &self.block(
                &alias,
                &security_key.key_name,
                &security_key.provider,
                &glasshouse_ssh::SECURITY_KEY_ALGORITHMS,
            );
        }
        text
    }

    fn block(&self, alias: &str, key_name: &str, provider: &str, algorithms: &[&str]) -> String {
        let mut lines = vec![
            format!("Host {alias}"),
            format!("    HostName {}", self.host_name),
        ];
        if let Some(user) = &self.user {
            lines.push(format!("    User {user}"));
        }
        if let Some(port) = self.port {
            lines.push(format!("    Port {port}"));
        }
        lines.extend([
            format!("    IdentityFile ~/.ssh/{key_name}"),
            format!("    SecurityKeyProvider {provider}"),
            "    IdentitiesOnly yes".to_owned(),
            "    IdentityAgent none".to_owned(),
            format!("    PubkeyAcceptedAlgorithms {}", algorithms.join(",")),
            "    ForwardAgent no".to_owned(),
        ]);
        lines.join("\n") + "\n"
    }
}

/// ASCII letters and digits plus `extra`, at most 255 characters, not starting with `-`.
pub(crate) fn is_token(value: &str, extra: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.chars().count() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// Foundation's `Data(base64Encoded:)`: padding required, stray trailing bits ignored.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
);

/// One public key in `authorized_keys` form: `type base64 [comment]`, without options.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SshPublicKey {
    key_type: String,
    blob: String,
    comment: String,
}

impl SshPublicKey {
    /// Key types accepted as a public key.
    pub const ALLOWED_TYPES: [&str; 7] = [
        "ssh-ed25519",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "ssh-rsa",
        "sk-ssh-ed25519@openssh.com",
        "sk-ecdsa-sha2-nistp256@openssh.com",
    ];

    /// Checks that the key data really is a key of the stated type. A leading options field, more than one line or
    /// an unknown type is refused. The comment keeps printable ASCII; quotes and backslashes become `_`, because
    /// the key is written into shell scripts.
    pub fn parse(text: &str) -> Result<Self, SshConfigError> {
        let trimmed = text.trim();
        let invalid = || SshConfigError::InvalidPublicKey(trimmed.chars().take(40).collect());
        let fields = split_max(trimmed, 2, |c| c == ' ' || c == '\t');
        if trimmed.contains(is_newline)
            || fields.len() < 2
            || !Self::ALLOWED_TYPES.contains(&fields[0])
            || !fields[1]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
        {
            return Err(invalid());
        }
        let data = BASE64.decode(fields[1]).map_err(|_| invalid())?;
        let Some((length, rest)) = data.split_first_chunk::<4>() else {
            return Err(invalid());
        };
        let length = usize::try_from(u32::from_be_bytes(*length)).map_err(|_| invalid())?;
        if rest.get(..length) != Some(fields[0].as_bytes()) {
            return Err(invalid());
        }
        let comment: String = fields
            .get(2)
            .unwrap_or(&"")
            .chars()
            .take(200)
            .map(|c| {
                if matches!(c, ' '..='~') && !matches!(c, '\'' | '\\' | '"') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Ok(Self {
            key_type: fields[0].to_owned(),
            blob: fields[1].to_owned(),
            comment: trim_whitespace(&comment).to_owned(),
        })
    }

    /// The key type, such as `ssh-ed25519`.
    pub fn key_type(&self) -> &str {
        &self.key_type
    }

    /// The base64 key data.
    pub fn blob(&self) -> &str {
        &self.blob
    }

    /// The sanitized comment, possibly empty.
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /// `type blob [comment]`.
    pub fn line(&self) -> String {
        if self.comment.is_empty() {
            format!("{} {}", self.key_type, self.blob)
        } else {
            format!("{} {} {}", self.key_type, self.blob, self.comment)
        }
    }

    /// Every use needs a touch on a device: Touch ID for a Secure Enclave key, a tap for a security key.
    pub fn needs_touch(&self) -> bool {
        self.key_type.starts_with("sk-")
    }

    /// What to call the key in messages.
    pub fn label(&self) -> String {
        if self.comment.is_empty() {
            self.key_type.clone()
        } else {
            format!("{} ({})", self.comment, self.key_type)
        }
    }
}

impl std::str::FromStr for SshPublicKey {
    type Err = SshConfigError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key_line, myserver};

    fn host(
        alias: &str,
        host_name: &str,
        user: Option<&str>,
        port: Option<i64>,
        key_name: &str,
    ) -> ManagedSshHost {
        ManagedSshHost::new(alias, host_name, user, port, key_name)
    }

    // Managed host validation

    #[test]
    fn accepts_an_ordinary_host() {
        let validated = host(" myserver ", "fe80::1", Some(""), None, "glasshouse")
            .validated()
            .unwrap();
        assert_eq!(validated.alias, "myserver");
        assert_eq!(validated.user, None);
        let other = host(
            "db-1.prod",
            "db.example.com",
            Some("ops@corp"),
            Some(2222),
            "work_key-2",
        );
        assert_eq!(other.validated().unwrap().port, Some(2222));
    }

    #[test]
    fn rejects_values_that_would_add_directives() {
        let injected = "myserver\n    ProxyCommand sh -c id";
        assert_eq!(
            host(injected, "h", None, None, "k").validated(),
            Err(SshConfigError::InvalidAlias(injected.into()))
        );
        assert_eq!(
            host("a", "10.0.0.5 ProxyCommand", None, None, "k").validated(),
            Err(SshConfigError::InvalidHostName(
                "10.0.0.5 ProxyCommand".into()
            ))
        );
        assert_eq!(
            host("a", "h", Some("you\nProxyCommand"), None, "k").validated(),
            Err(SshConfigError::InvalidUser("you\nProxyCommand".into()))
        );
        assert_eq!(
            host("a", "%h.evil", None, None, "k").validated(),
            Err(SshConfigError::InvalidHostName("%h.evil".into()))
        );
    }

    #[test]
    fn rejects_values_that_look_like_options() {
        assert_eq!(
            host("-oProxyCommand=id", "h", None, None, "k").validated(),
            Err(SshConfigError::InvalidAlias("-oProxyCommand=id".into()))
        );
        assert_eq!(
            host("a", "-p", None, None, "k").validated(),
            Err(SshConfigError::InvalidHostName("-p".into()))
        );
    }

    #[test]
    fn rejects_patterns_and_odd_aliases() {
        for alias in ["*", "my*", "!myserver", "a b", "", "my\"server"] {
            assert!(
                host(alias, "h", None, None, "k").validated().is_err(),
                "{alias:?}"
            );
        }
    }

    #[test]
    fn rejects_key_names_outside_the_ssh_directory() {
        for name in ["../id_rsa", "a/b", ".hidden", "glasshouse.pub", "-k", ""] {
            assert!(!ManagedSshHost::is_valid_key_name(name), "{name:?}");
        }
    }

    #[test]
    fn checks_the_port_range() {
        assert_eq!(
            host("a", "h", None, Some(0), "k").validated(),
            Err(SshConfigError::InvalidPort(0))
        );
        assert_eq!(
            host("a", "h", None, Some(65_536), "k").validated(),
            Err(SshConfigError::InvalidPort(65_536))
        );
        host("a", "h", None, Some(65_535), "k").validated().unwrap();
    }

    // Rendering

    #[test]
    fn block_forces_the_secure_enclave_key() {
        assert_eq!(
            myserver().config_block(),
            "Host myserver
    HostName 10.0.0.5
    User you
    Port 22
    IdentityFile ~/.ssh/glasshouse
    SecurityKeyProvider /usr/lib/ssh-keychain.dylib
    IdentitiesOnly yes
    IdentityAgent none
    PubkeyAcceptedAlgorithms sk-ecdsa-sha2-nistp256@openssh.com
    ForwardAgent no
"
        );
    }

    #[test]
    fn leaves_out_user_and_port_when_unset() {
        let block = host("gh", "github.com", None, None, "glasshouse").config_block();
        assert!(!block.contains("User "));
        assert!(!block.contains("Port "));
    }

    // Public keys

    #[test]
    fn parses_and_normalizes_a_line() {
        let line = key_line("ssh-ed25519", "laptop", 2);
        let key = SshPublicKey::parse(&format!("  {}\n", line.replace(' ', "\t"))).unwrap();
        assert_eq!(key.key_type(), "ssh-ed25519");
        assert_eq!(key.comment(), "laptop");
        assert_eq!(key.line(), line);
        assert!(!key.needs_touch());
        assert!(
            SshPublicKey::parse(&key_line("sk-ssh-ed25519@openssh.com", "", 1))
                .unwrap()
                .needs_touch()
        );
    }

    #[test]
    fn refuses_anything_but_one_key() {
        let plain = key_line("ssh-ed25519", "", 1);
        let blob = plain.split(' ').nth(1).unwrap();
        for text in [
            format!("no-pty {plain}"),
            format!("command=\"id\" {plain}"),
            "ssh-dss AAAAB3NzaC1kc3M=".to_owned(),
            "ssh-ed25519 not*base64".to_owned(),
            format!("{plain}\n{}", key_line("ssh-ed25519", "", 9)),
            format!("ecdsa-sha2-nistp256 {blob}"),
            String::new(),
        ] {
            assert!(
                matches!(
                    SshPublicKey::parse(&text),
                    Err(SshConfigError::InvalidPublicKey(_))
                ),
                "{text:?}"
            );
        }
    }

    #[test]
    fn makes_comments_safe_for_shell_quoting() {
        let key = SshPublicKey::parse(&key_line("ssh-ed25519", r#"jo's "key" \x"#, 1)).unwrap();
        assert_eq!(key.comment(), "jo_s _key_ _x");
    }

    // Backup and security keys

    #[test]
    fn normalizes_and_deduplicates_backup_keys() {
        let line = key_line("ssh-ed25519", "laptop", 2);
        let mut host = myserver();
        host.backup_keys = vec![
            format!("  {line}  "),
            line.replace(" laptop", " other-name"),
        ];
        assert_eq!(
            host.validated().unwrap().backup_keys,
            std::slice::from_ref(&line)
        );

        host.backup_keys = vec![format!("no-pty {line}")];
        assert!(host.validated().is_err());
        host.backup_keys = (0..17)
            .map(|seed| key_line("ssh-ed25519", "", seed))
            .collect();
        assert_eq!(host.validated(), Err(SshConfigError::TooManyBackupKeys));
    }

    #[test]
    fn security_key_gets_its_own_entry() {
        let host = myserver().with_security_key(Some(SecurityKey::new("id_ed25519_sk")));
        let validated = host.validated().unwrap();
        assert_eq!(validated.aliases(), ["myserver", "myserver-key"]);
        assert!(validated.config_block().ends_with(
            "
Host myserver-key
    HostName 10.0.0.5
    User you
    Port 22
    IdentityFile ~/.ssh/id_ed25519_sk
    SecurityKeyProvider internal
    IdentitiesOnly yes
    IdentityAgent none
    PubkeyAcceptedAlgorithms sk-ssh-ed25519@openssh.com,sk-ecdsa-sha2-nistp256@openssh.com
    ForwardAgent no
"
        ));
    }

    #[test]
    fn checks_the_provider() {
        for (provider, valid) in [
            ("internal", true),
            ("/opt/homebrew/lib/libsk-libfido2.dylib", true),
            ("libsk.dylib", false),
            ("/a b", false),
            ("/x\nProxyCommand id", false),
            ("%d/sk", false),
        ] {
            assert_eq!(
                ManagedSshHost::is_valid_provider(provider),
                valid,
                "{provider:?}"
            );
        }
    }

    #[test]
    fn security_key_must_be_a_different_key() {
        let host = myserver().with_security_key(Some(SecurityKey::new("glasshouse")));
        assert_eq!(
            host.validated(),
            Err(SshConfigError::InvalidKeyName("glasshouse".into()))
        );
    }
}
