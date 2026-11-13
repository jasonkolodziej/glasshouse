//! Helpers shared by the unit tests.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::ManagedSshHost;

/// The `~/.ssh/config` that logged in to `myserver` without Touch ID: `Host *` comes first and adds an agent-held
/// key, and a second `Host myserver` block adds it again.
pub(crate) const FIXTURE: &str = include_str!("../tests/fixtures/ssh-config.test");

pub(crate) fn myserver() -> ManagedSshHost {
    ManagedSshHost::new("myserver", "10.0.0.5", Some("you"), Some(22), "glasshouse")
}

/// A well-formed public key line of `key_type`: the key data starts with the type name, like a real one.
pub(crate) fn key_line(key_type: &str, comment: &str, seed: u8) -> String {
    fn field(bytes: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(bytes.len()).unwrap().to_be_bytes().to_vec();
        out.extend_from_slice(bytes);
        out
    }
    let mut blob = field(key_type.as_bytes());
    blob.extend(field(&[seed; 32]));
    if key_type.starts_with("sk-") {
        blob.extend(field(b"ssh:"));
    }
    let line = format!("{key_type} {}", STANDARD.encode(blob));
    if comment.is_empty() {
        line
    } else {
        format!("{line} {comment}")
    }
}
