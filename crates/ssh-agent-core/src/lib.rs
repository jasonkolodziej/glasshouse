//! Port of the Swift `SSHAgentCore` module: a signing-only ssh-agent (RFC 9987 plus OpenSSH's
//! `session-bind@openssh.com`) that serves ECDSA P-256 keys over a Unix-domain socket and names the
//! destination, key and requesting process in every approval prompt.

mod agent_configuration;
mod agent_protocol;
mod agent_server;
mod approval_request;
mod key_store;
mod peer_identity;
mod ssh_wire;

pub use agent_configuration::AgentConfiguration;
pub use agent_protocol::{AgentMessage, AgentRequest, AgentResponse, SSHKeyFormat, SessionBind};
pub use agent_server::{AgentServer, AgentServerError, Approver};
pub use approval_request::ApprovalRequest;
pub use key_store::{
    AgentKey, KeyPolicy, KeyStore, KeyStoreError, SecureEnclaveKeyStore, SoftwareKeyStore,
};
pub use peer_identity::PeerIdentity;
pub use ssh_wire::{SSHReader, SSHWireError, SSHWriter};
