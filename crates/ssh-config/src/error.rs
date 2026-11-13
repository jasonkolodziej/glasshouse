use std::io;
use std::path::PathBuf;

/// A value that can't be written into ssh_config or a setup script. Messages are shown to the user as is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum SshConfigError {
    /// The alias isn't a plain host name token.
    #[error(
        "“{0}” can't be used as an alias. Use letters, digits, “.”, “_” and “-”, not starting with “-”."
    )]
    InvalidAlias(String),
    /// The host name isn't a host name or IP address.
    #[error(
        "“{0}” isn't a host name or IP address. Use letters, digits, “.”, “-” and “:”, without spaces."
    )]
    InvalidHostName(String),
    /// The user name contains characters ssh_config can't hold safely.
    #[error("“{0}” can't be used as a user name. Use letters, digits, “.”, “_”, “-” and “@”.")]
    InvalidUser(String),
    /// The port is outside 1–65535.
    #[error("Port {0} is out of range. Use 1–65535, or leave it empty for 22.")]
    InvalidPort(i64),
    /// The key name isn't a plain file name in `~/.ssh`, or is reused for two roles.
    #[error(
        "“{0}” can't be used as a key name. Use letters, digits, “.”, “_” and “-”, not starting with “.” or “-”, \
         and pick a different key for each role."
    )]
    InvalidKeyName(String),
    /// The security key provider is neither `internal` nor an absolute library path.
    #[error(
        "“{0}” isn't a security key provider. Use “internal” or the full path of a provider library."
    )]
    InvalidProvider(String),
    /// The text isn't one public key line; holds its first 40 characters.
    #[error(
        "“{0}…” isn't a public key. Paste one line from a .pub file: type, key data and an optional comment."
    )]
    InvalidPublicKey(String),
    /// More than 16 backup keys.
    #[error("A host can have at most 16 backup keys.")]
    TooManyBackupKeys,
    /// Another host already writes this alias.
    #[error("Another host is already called “{0}”.")]
    DuplicateAlias(String),
    /// A key file with this name already exists in `~/.ssh`.
    #[error("~/.ssh/{0} already exists. Choose another name.")]
    KeyAlreadyExists(String),
    /// `~/.ssh/<name>.pub` is missing or isn't a usable public key.
    #[error("~/.ssh/{0}.pub is missing or isn't a public key.")]
    MissingPublicKey(String),
}

/// Failure reading or writing Glasshouse's SSH files.
#[derive(Debug, thiserror::Error)]
pub enum SshConfigStoreError {
    /// A saved value failed validation.
    #[error(transparent)]
    Config(#[from] SshConfigError),
    /// The saved host list isn't valid JSON of the expected shape.
    #[error("The saved SSH host list isn't valid: {0}. Fix or delete the file, then try again.")]
    Json(#[from] serde_json::Error),
    /// A file or directory couldn't be read or written.
    #[error("Couldn't write {}: {source}. Check that the folder exists and you can write to it.", path.display())]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// Neither `$HOME` nor the password database names a home directory.
    #[error(
        "Couldn't find your home directory. Set the HOME environment variable, then try again."
    )]
    NoHomeDirectory,
}

impl SshConfigStoreError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
