//! Port of the Swift `GlasshouseSecurity` module: kernel-sourced process snapshots for the approval
//! card, scoped Secure Keyboard Entry, Touch ID for sudo status and scripts, and detection of Apple's
//! native Secure Enclave SSH path.

mod native_secure_enclave_ssh;
mod process_inspector;
mod secure_keyboard_entry;
mod touch_id_sudo;

pub use native_secure_enclave_ssh::NativeSecureEnclaveSSH;
pub use process_inspector::{ProcessInspector, ProcessSnapshot};
pub use secure_keyboard_entry::{SecureInputBackend, SecureKeyboardEntry, SystemSecureInput};
pub use touch_id_sudo::{FileOwnership, TouchIDSudo, TouchIDSudoStatus};
