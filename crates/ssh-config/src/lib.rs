//! Glasshouse's managed SSH configuration: the hosts it manages, the `~/.ssh/glasshouse.conf` it generates, the
//! `Include` block it keeps at the top of `~/.ssh/config`, and the scripts that create and install Secure Enclave
//! keys.
//!
//! Port of the Swift `SSHConfig` module.
#![warn(missing_docs)]

mod error;
pub mod glasshouse_ssh;
pub mod glasshouse_ssh_scripts;
mod json;
mod managed_ssh_config;
mod managed_ssh_host;
mod ssh_config_store;
pub mod ssh_config_text;
mod swift_text;

#[cfg(test)]
mod test_support;

pub use error::{SshConfigError, SshConfigStoreError};
pub use managed_ssh_config::ManagedSshConfig;
pub use managed_ssh_host::{ManagedSshHost, SecurityKey, SshPublicKey};
pub use ssh_config_store::{SshConfigStore, SshKeyFile};
pub use ssh_config_text::{SshConfigConflict, SshConfigDirective, SshIncludeStatus};
