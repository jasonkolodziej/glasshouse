use std::fs::File;
use std::io::Read as _;
use std::sync::{Mutex, MutexGuard, PoisonError};

use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use serde::{Deserialize, Serialize};

use crate::agent_protocol::SSHKeyFormat;
use crate::approval_request::ApprovalRequest;

/// A key the agent serves. For the Secure Enclave the private key never exists outside the enclave;
/// only the public key blob and metadata are visible here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentKey {
    /// Stable identifier (the keychain account for Secure Enclave keys).
    pub id: String,
    /// Shown in `ssh-add -L` and in the approval prompt.
    pub comment: String,
    /// Restrictions applied by the agent before signing.
    pub policy: KeyPolicy,
    /// `ecdsa-sha2-nistp256` public key blob.
    pub public_key_blob: Vec<u8>,
}

impl AgentKey {
    /// OpenSSH SHA256 fingerprint of the public key.
    pub fn fingerprint(&self) -> String {
        SSHKeyFormat::fingerprint(&self.public_key_blob)
    }
}

/// Per-key signing policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPolicy {
    /// `.biometryCurrentSet` (true): re-enrolling a finger invalidates the key — strictest.
    /// `.userPresence` (false): Touch ID, Apple Watch or login password.
    pub require_biometry_current_set: bool,
    /// Refuse requests that arrive through agent forwarding (session-bind is_forwarding = true).
    pub allow_forwarded: bool,
    /// Host glob patterns this key may be used for; empty = any host.
    pub allowed_hosts: Vec<String>,
}

/// Key store failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyStoreError {
    /// No Secure Enclave on this machine, or no Secure Enclave backend in this build.
    #[error("the Secure Enclave is unavailable on this system")]
    SecureEnclaveUnavailable,
    /// A keychain call failed with this `OSStatus`.
    #[error("keychain error (OSStatus {0})")]
    Keychain(i32),
    /// The access control object could not be created.
    #[error("could not create the key's access control")]
    AccessControl,
    /// No key with the requested ID.
    #[error("key not found")]
    NotFound,
    /// Key generation or signing failed.
    #[error("cryptographic operation failed: {0}")]
    Crypto(String),
}

/// Where the agent's keys live. Listing must never prompt; signing may (and for hardware keys does).
pub trait KeyStore: Send + Sync {
    /// Lists the served keys without touching private key material.
    fn list_keys(&self) -> Result<Vec<AgentKey>, KeyStoreError>;

    /// Signs `data` (ECDSA P-256 over SHA-256) with key `key_id`. `request` describes the destination,
    /// key and requesting process; a hardware store uses its `localized_reason` as the prompt text.
    fn sign(
        &self,
        key_id: &str,
        data: &[u8],
        request: &ApprovalRequest,
    ) -> Result<Signature, KeyStoreError>;
}

/// Secure Enclave key store (keychain service `dev.glasshouse.agent.keys`, data protection keychain,
/// shared access group).
///
/// NOT PORTED: the Swift version relies on CryptoKit's `SecureEnclave.P256`, the data protection
/// keychain and `LAContext`, which have no lightweight Rust bindings. This type keeps the Swift API
/// shape but every operation returns [`KeyStoreError::SecureEnclaveUnavailable`]; the agent therefore
/// serves no keys until a native backend is added. Use [`SoftwareKeyStore`] for tests and development.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureEnclaveKeyStore {
    /// `keychain-access-groups` entitlement value shared by the app and the agent.
    pub access_group: String,
}

impl SecureEnclaveKeyStore {
    /// Keychain service name for the stored key blobs.
    pub const SERVICE: &'static str = "dev.glasshouse.agent.keys";

    /// A store bound to `access_group`.
    pub fn new(access_group: impl Into<String>) -> Self {
        Self {
            access_group: access_group.into(),
        }
    }

    /// Creates a Secure Enclave key. Always unavailable in this port.
    pub fn create_key(
        &self,
        _comment: &str,
        _policy: KeyPolicy,
    ) -> Result<AgentKey, KeyStoreError> {
        Err(KeyStoreError::SecureEnclaveUnavailable)
    }

    /// Deletes a key. Always unavailable in this port.
    pub fn delete_key(&self, _id: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::SecureEnclaveUnavailable)
    }
}

impl KeyStore for SecureEnclaveKeyStore {
    fn list_keys(&self) -> Result<Vec<AgentKey>, KeyStoreError> {
        Err(KeyStoreError::SecureEnclaveUnavailable)
    }

    fn sign(
        &self,
        _key_id: &str,
        _data: &[u8],
        _request: &ApprovalRequest,
    ) -> Result<Signature, KeyStoreError> {
        Err(KeyStoreError::SecureEnclaveUnavailable)
    }
}

/// In-memory P-256 keys: the encoding path is identical to the Secure Enclave's, without the hardware.
/// It never prompts; pair it with an [`crate::Approver`] on the server to gate signatures.
#[derive(Debug, Default)]
pub struct SoftwareKeyStore {
    keys: Mutex<Vec<(AgentKey, SigningKey)>>,
}

impl SoftwareKeyStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Generates a new random key.
    pub fn create_key(&self, comment: &str, policy: KeyPolicy) -> Result<AgentKey, KeyStoreError> {
        let signing_key = loop {
            let mut secret = [0u8; 32];
            random_bytes(&mut secret)?;
            if let Ok(key) = SigningKey::from_slice(&secret) {
                break key;
            }
        };
        self.add_key(signing_key, comment, policy)
    }

    /// Adds an existing private key.
    pub fn add_key(
        &self,
        signing_key: SigningKey,
        comment: &str,
        policy: KeyPolicy,
    ) -> Result<AgentKey, KeyStoreError> {
        let key = AgentKey {
            id: new_id()?,
            comment: comment.to_owned(),
            policy,
            public_key_blob: SSHKeyFormat::public_key_blob(signing_key.verifying_key()),
        };
        self.lock().push((key.clone(), signing_key));
        Ok(key)
    }

    /// Removes a key; removing an unknown ID is not an error (like the Swift keychain store).
    pub fn delete_key(&self, id: &str) {
        self.lock().retain(|(key, _)| key.id != id);
    }

    fn lock(&self) -> MutexGuard<'_, Vec<(AgentKey, SigningKey)>> {
        self.keys.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl KeyStore for SoftwareKeyStore {
    fn list_keys(&self) -> Result<Vec<AgentKey>, KeyStoreError> {
        Ok(self.lock().iter().map(|(key, _)| key.clone()).collect())
    }

    fn sign(
        &self,
        key_id: &str,
        data: &[u8],
        _request: &ApprovalRequest,
    ) -> Result<Signature, KeyStoreError> {
        let keys = self.lock();
        let (_, signing_key) = keys
            .iter()
            .find(|(key, _)| key.id == key_id)
            .ok_or(KeyStoreError::NotFound)?;
        signing_key
            .try_sign(data)
            .map_err(|error| KeyStoreError::Crypto(error.to_string()))
    }
}

fn random_bytes(buf: &mut [u8]) -> Result<(), KeyStoreError> {
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .map_err(|error| KeyStoreError::Crypto(format!("could not read /dev/urandom: {error}")))
}

/// Uppercase UUID string, like Foundation's `UUID().uuidString`.
fn new_id() -> Result<String, KeyStoreError> {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes)?;
    Ok(uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .hyphenated()
        .to_string()
        .to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier as _;

    fn request_for(key: &AgentKey) -> ApprovalRequest {
        ApprovalRequest {
            key: key.clone(),
            peer: None,
            bind: None,
        }
    }

    #[test]
    fn software_store_signs_with_listed_keys() {
        let store = SoftwareKeyStore::new();
        let key = store.create_key("laptop", KeyPolicy::default()).unwrap();
        assert_eq!(store.list_keys().unwrap(), vec![key.clone()]);
        assert_eq!(key.id.len(), 36);
        assert_eq!(key.id, key.id.to_uppercase());

        let sig = store.sign(&key.id, b"data", &request_for(&key)).unwrap();
        let signing_key = SigningKey::from_slice(&[0x11; 32]).unwrap();
        let other = store
            .add_key(signing_key.clone(), "other", KeyPolicy::default())
            .unwrap();
        assert_ne!(key.public_key_blob, other.public_key_blob);
        let other_sig = store
            .sign(&other.id, b"data", &request_for(&other))
            .unwrap();
        assert!(
            signing_key
                .verifying_key()
                .verify(b"data", &other_sig)
                .is_ok()
        );
        assert!(signing_key.verifying_key().verify(b"data", &sig).is_err());
    }

    #[test]
    fn software_store_reports_missing_and_deleted_keys() {
        let store = SoftwareKeyStore::new();
        let key = store.create_key("k", KeyPolicy::default()).unwrap();
        store.delete_key(&key.id);
        assert!(store.list_keys().unwrap().is_empty());
        assert_eq!(
            store.sign(&key.id, b"x", &request_for(&key)),
            Err(KeyStoreError::NotFound)
        );
    }

    #[test]
    fn secure_enclave_store_is_unavailable() {
        let store = SecureEnclaveKeyStore::new("TEAMID.dev.glasshouse.shared");
        assert_eq!(
            store.list_keys(),
            Err(KeyStoreError::SecureEnclaveUnavailable)
        );
        assert_eq!(
            store.create_key("k", KeyPolicy::default()),
            Err(KeyStoreError::SecureEnclaveUnavailable)
        );
    }

    #[test]
    fn key_policy_uses_swift_codable_field_names() {
        let policy = KeyPolicy {
            require_biometry_current_set: true,
            allow_forwarded: false,
            allowed_hosts: vec!["*.internal".to_owned()],
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(
            json,
            r#"{"requireBiometryCurrentSet":true,"allowForwarded":false,"allowedHosts":["*.internal"]}"#
        );
        assert_eq!(serde_json::from_str::<KeyPolicy>(&json).unwrap(), policy);
    }
}
