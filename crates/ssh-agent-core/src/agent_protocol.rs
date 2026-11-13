//! ssh-agent protocol, RFC 9987 (May 2026). Glasshouse implements the minimum a signing-only,
//! hardware-backed agent needs and refuses everything else (no key import, no locking, no removal
//! over the socket — keys are managed in the app).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::ssh_wire::{SSHReader, SSHWireError, SSHWriter};

/// ssh-agent message numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentMessage;

impl AgentMessage {
    /// `SSH_AGENT_FAILURE`.
    pub const FAILURE: u8 = 5;
    /// `SSH_AGENT_SUCCESS`.
    pub const SUCCESS: u8 = 6;
    /// `SSH_AGENTC_REQUEST_IDENTITIES`.
    pub const REQUEST_IDENTITIES: u8 = 11;
    /// `SSH_AGENT_IDENTITIES_ANSWER`.
    pub const IDENTITIES_ANSWER: u8 = 12;
    /// `SSH_AGENTC_SIGN_REQUEST`.
    pub const SIGN_REQUEST: u8 = 13;
    /// `SSH_AGENT_SIGN_RESPONSE`.
    pub const SIGN_RESPONSE: u8 = 14;
    /// `SSH_AGENTC_EXTENSION`.
    pub const EXTENSION: u8 = 27;
    /// `SSH_AGENT_EXTENSION_FAILURE`.
    pub const EXTENSION_FAILURE: u8 = 28;
}

/// The extension name OpenSSH uses to bind a connection to its host key.
const SESSION_BIND: &str = "session-bind@openssh.com";

/// A parsed client request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRequest {
    /// List the public keys the agent holds.
    RequestIdentities,
    /// Sign `data` with the key whose public blob is `key_blob`.
    Sign {
        /// Public key blob of the key to sign with.
        key_blob: Vec<u8>,
        /// The data to sign.
        data: Vec<u8>,
        /// Signature flags (ignored: only ecdsa-sha2-nistp256 is served).
        flags: u32,
    },
    /// OpenSSH `session-bind@openssh.com` (PROTOCOL.agent, not in RFC 9987).
    SessionBind(SessionBind),
    /// Any other extension; answered with `SSH_AGENT_EXTENSION_FAILURE`.
    UnknownExtension {
        /// The extension name.
        name: String,
    },
    /// Any other message type; answered with `SSH_AGENT_FAILURE`.
    Unsupported {
        /// The message number.
        message_type: u8,
    },
}

impl AgentRequest {
    /// Parses one message body (the bytes after the uint32 length prefix).
    pub fn parse(body: &[u8]) -> Result<Self, SSHWireError> {
        let mut r = SSHReader::new(body);
        let message_type = r.byte()?;
        match message_type {
            AgentMessage::REQUEST_IDENTITIES => Ok(Self::RequestIdentities),
            AgentMessage::SIGN_REQUEST => {
                let key_blob = r.string()?.to_vec();
                let data = r.string()?.to_vec();
                let flags = r.uint32()?;
                Ok(Self::Sign {
                    key_blob,
                    data,
                    flags,
                })
            }
            AgentMessage::EXTENSION => {
                let name = r.utf8_string()?;
                if name == SESSION_BIND {
                    let host_key = r.string()?.to_vec();
                    let session_id = r.string()?.to_vec();
                    let signature = r.string()?.to_vec();
                    let is_forwarding = r.bool()?;
                    return Ok(Self::SessionBind(SessionBind {
                        host_key,
                        session_id,
                        signature,
                        is_forwarding,
                        verified: false,
                    }));
                }
                Ok(Self::UnknownExtension { name })
            }
            _ => Ok(Self::Unsupported { message_type }),
        }
    }
}

/// What ssh tells the agent about the connection a signature is for.
///
/// SECURITY: `host_key` is display-only until `signature` (over `session_id`) has been verified with it.
/// Verification is a TODO in this scaffold (needs ed25519/ecdsa/rsa host-key verification); until then
/// Glasshouse uses it only to refuse forwarded requests and to label the prompt as "unverified host".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBind {
    /// The server's host key blob.
    pub host_key: Vec<u8>,
    /// The SSH session identifier (exchange hash).
    pub session_id: Vec<u8>,
    /// The server's signature over `session_id`.
    pub signature: Vec<u8>,
    /// True when the request arrived through agent forwarding.
    pub is_forwarding: bool,
    /// True once `signature` has been verified with `host_key` (never, yet).
    pub verified: bool,
}

impl SessionBind {
    /// SHA256 fingerprint in OpenSSH format, for matching against known_hosts.
    pub fn host_key_fingerprint(&self) -> String {
        SSHKeyFormat::fingerprint(&self.host_key)
    }
}

/// Framed agent replies (uint32 length prefix included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentResponse;

impl AgentResponse {
    /// `SSH_AGENT_FAILURE`.
    pub fn failure() -> Vec<u8> {
        Self::frame(|w| w.byte(AgentMessage::FAILURE))
    }

    /// `SSH_AGENT_SUCCESS`.
    pub fn success() -> Vec<u8> {
        Self::frame(|w| w.byte(AgentMessage::SUCCESS))
    }

    /// `SSH_AGENT_EXTENSION_FAILURE`.
    pub fn extension_failure() -> Vec<u8> {
        Self::frame(|w| w.byte(AgentMessage::EXTENSION_FAILURE))
    }

    /// `SSH_AGENT_IDENTITIES_ANSWER` listing `(public key blob, comment)` pairs.
    pub fn identities(ids: &[(&[u8], &str)]) -> Vec<u8> {
        Self::frame(|w| {
            w.byte(AgentMessage::IDENTITIES_ANSWER);
            w.uint32(u32::try_from(ids.len()).unwrap_or(u32::MAX));
            for (blob, comment) in ids {
                w.string(blob);
                w.string(comment);
            }
        })
    }

    /// `SSH_AGENT_SIGN_RESPONSE` carrying an SSH signature blob.
    pub fn signature(ssh_signature: &[u8]) -> Vec<u8> {
        Self::frame(|w| {
            w.byte(AgentMessage::SIGN_RESPONSE);
            w.string(ssh_signature);
        })
    }

    /// Prefixes the uint32 length.
    fn frame(build: impl FnOnce(&mut SSHWriter)) -> Vec<u8> {
        let mut body = SSHWriter::new();
        build(&mut body);
        let mut out = SSHWriter::new();
        out.string(body.data());
        out.into_data()
    }
}

/// ecdsa-sha2-nistp256 encodings (RFC 5656 §3.1, §3.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SSHKeyFormat;

impl SSHKeyFormat {
    /// The SSH key/signature algorithm name.
    pub const ECDSA_P256: &'static str = "ecdsa-sha2-nistp256";

    /// string "ecdsa-sha2-nistp256" ‖ string "nistp256" ‖ string Q (uncompressed point 0x04‖X‖Y)
    pub fn public_key_blob(key: &VerifyingKey) -> Vec<u8> {
        let mut w = SSHWriter::new();
        w.string(Self::ECDSA_P256);
        w.string("nistp256");
        w.string(key.to_encoded_point(false).as_bytes());
        w.into_data()
    }

    /// string "ecdsa-sha2-nistp256" ‖ string( mpint r ‖ mpint s )
    /// The signature's fixed-size encoding is r‖s, 32 bytes each, unsigned big-endian.
    pub fn signature_blob(sig: &Signature) -> Vec<u8> {
        let raw = sig.to_bytes();
        let (r, s) = raw.split_at(32);
        let mut inner = SSHWriter::new();
        inner.mpint_unsigned(r);
        inner.mpint_unsigned(s);
        let mut w = SSHWriter::new();
        w.string(Self::ECDSA_P256);
        w.string(inner.data());
        w.into_data()
    }

    /// `ecdsa-sha2-nistp256 AAAA… comment` for authorized_keys.
    pub fn authorized_keys_line(key: &VerifyingKey, comment: &str) -> String {
        let blob = base64::engine::general_purpose::STANDARD.encode(Self::public_key_blob(key));
        format!("{} {blob} {comment}", Self::ECDSA_P256)
    }

    /// `SHA256:` followed by the unpadded base64 SHA-256 of `blob` (OpenSSH format).
    pub fn fingerprint(blob: &[u8]) -> String {
        format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(blob)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::SigningKey;
    use p256::ecdsa::signature::{Signer as _, Verifier as _};

    fn test_signing_key() -> SigningKey {
        SigningKey::from_slice(&[0x42; 32]).unwrap()
    }

    #[test]
    fn mpint_encoding() {
        assert_eq!(SSHWriter::mpint_bytes(&[0x00, 0x00, 0x7F]), vec![0x7F]);
        assert_eq!(
            SSHWriter::mpint_bytes(&[0x80, 0x01]),
            vec![0x00, 0x80, 0x01]
        );
        assert!(SSHWriter::mpint_bytes(&[0x00, 0x00]).is_empty());
    }

    #[test]
    fn public_key_blob_layout() {
        let key = *test_signing_key().verifying_key();
        let blob = SSHKeyFormat::public_key_blob(&key);
        let mut r = SSHReader::new(&blob);
        assert_eq!(r.utf8_string().unwrap(), "ecdsa-sha2-nistp256");
        assert_eq!(r.utf8_string().unwrap(), "nistp256");
        let q = r.string().unwrap();
        assert_eq!(q.len(), 65);
        assert_eq!(q[0], 0x04);
        assert!(r.is_at_end());
    }

    /// Software key stands in for the Secure Enclave: the encoding path is identical.
    #[test]
    fn signature_blob_round_trips_and_verifies() {
        let key = test_signing_key();
        let message = b"session-id || userauth request";
        let sig: Signature = key.sign(message);

        let blob = SSHKeyFormat::signature_blob(&sig);
        let mut r = SSHReader::new(&blob);
        assert_eq!(r.utf8_string().unwrap(), "ecdsa-sha2-nistp256");
        let mut inner = SSHReader::new(r.string().unwrap());
        let r_int = inner.string().unwrap();
        let s_int = inner.string().unwrap();

        fn pad32(d: &[u8]) -> Vec<u8> {
            let trimmed: Vec<u8> = d.iter().copied().skip_while(|&b| b == 0).collect();
            let mut out = vec![0; 32 - trimmed.len()];
            out.extend(trimmed);
            out
        }
        let raw = [pad32(r_int), pad32(s_int)].concat();
        let rebuilt = Signature::from_slice(&raw).unwrap();
        assert!(key.verifying_key().verify(message, &rebuilt).is_ok());
    }

    #[test]
    fn parses_sign_request() {
        let mut w = SSHWriter::new();
        w.byte(AgentMessage::SIGN_REQUEST);
        w.string([1, 2, 3]);
        w.string("payload");
        w.uint32(0);
        assert_eq!(
            AgentRequest::parse(w.data()).unwrap(),
            AgentRequest::Sign {
                key_blob: vec![1, 2, 3],
                data: b"payload".to_vec(),
                flags: 0
            }
        );
    }

    #[test]
    fn parses_session_bind_and_flags_forwarding() {
        let mut w = SSHWriter::new();
        w.byte(AgentMessage::EXTENSION);
        w.string("session-bind@openssh.com");
        w.string([9]);
        w.string([8]);
        w.string([7]);
        w.bool(true);
        let AgentRequest::SessionBind(bind) = AgentRequest::parse(w.data()).unwrap() else {
            panic!("expected session-bind");
        };
        assert!(bind.is_forwarding && !bind.verified);
        assert_eq!(
            (bind.host_key, bind.session_id, bind.signature),
            (vec![9], vec![8], vec![7])
        );
    }

    #[test]
    fn unknown_extension_and_types() {
        let mut w = SSHWriter::new();
        w.byte(AgentMessage::EXTENSION);
        w.string("query");
        assert_eq!(
            AgentRequest::parse(w.data()).unwrap(),
            AgentRequest::UnknownExtension {
                name: "query".to_owned()
            }
        );
        // ADD_IDENTITY refused
        assert_eq!(
            AgentRequest::parse(&[17]).unwrap(),
            AgentRequest::Unsupported { message_type: 17 }
        );
    }

    #[test]
    fn rejects_truncated_and_oversized() {
        assert_eq!(
            AgentRequest::parse(&[13, 0, 0, 0, 9, 1]),
            Err(SSHWireError::Truncated)
        );
        let mut w = SSHWriter::new();
        w.byte(13);
        w.uint32(u32::try_from(SSHReader::MAX_STRING + 1).unwrap());
        assert_eq!(AgentRequest::parse(w.data()), Err(SSHWireError::TooLarge));
    }

    #[test]
    fn identities_answer_is_framed() {
        let frame = AgentResponse::identities(&[(&[1], "k")]);
        let len = frame[..4]
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
        assert_eq!(len, frame.len() - 4);
        assert_eq!(frame[4], AgentMessage::IDENTITIES_ANSWER);
    }

    #[test]
    fn fingerprint_format() {
        let fp = SSHKeyFormat::fingerprint(b"x");
        assert!(fp.starts_with("SHA256:"));
        assert!(!fp.ends_with('='));
        assert_eq!(fp, "SHA256:LXEWQrcmsEQBYnyp+6wy9chTD7GQPMTbAiWHF5IaSIE");
    }

    #[test]
    fn simple_responses_are_single_byte_frames() {
        assert_eq!(AgentResponse::failure(), vec![0, 0, 0, 1, 5]);
        assert_eq!(AgentResponse::success(), vec![0, 0, 0, 1, 6]);
        assert_eq!(AgentResponse::extension_failure(), vec![0, 0, 0, 1, 28]);
    }

    #[test]
    fn authorized_keys_line_has_algorithm_blob_and_comment() {
        let key = *test_signing_key().verifying_key();
        let line = SSHKeyFormat::authorized_keys_line(&key, "me@mac");
        let parts: Vec<&str> = line.split(' ').collect();
        assert_eq!(parts[0], "ecdsa-sha2-nistp256");
        assert!(parts[1].starts_with("AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY"));
        assert_eq!(parts[2], "me@mac");
    }
}
