//! Runs the agent on a scratch socket and talks to it like `ssh` does.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, SigningKey};
use ssh_agent_core::{
    AgentMessage, AgentServer, AgentServerError, KeyPolicy, SSHReader, SSHWriter, SoftwareKeyStore,
};

/// Helpers live in a `cfg(test)` module so clippy treats them as test code (allow-unwrap-in-tests).
#[cfg(test)]
mod support {
    use super::*;

    pub struct Harness {
        pub _dir: tempfile::TempDir,
        pub socket: PathBuf,
        pub server: AgentServer,
        pub signing_key: SigningKey,
        pub key_blob: Vec<u8>,
    }

    fn scratch_dir() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("agent");
        fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("t")
            .tempdir_in(base)
            .unwrap()
    }

    pub fn harness(
        policy: KeyPolicy,
        approver: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    ) -> Harness {
        let dir = scratch_dir();
        let socket = dir.path().join("sock/agent.sock");
        let store = Arc::new(SoftwareKeyStore::new());
        let signing_key = SigningKey::from_slice(&[0x5A; 32]).unwrap();
        let key = store
            .add_key(signing_key.clone(), "test key", policy)
            .unwrap();
        let mut server = AgentServer::new(&socket, store);
        if let Some(approver) = approver {
            server = server.with_approver(move |_| approver());
        }
        server.start().unwrap();
        Harness {
            _dir: dir,
            socket,
            server,
            signing_key,
            key_blob: key.public_key_blob,
        }
    }

    pub fn round_trip(stream: &mut UnixStream, body: &[u8]) -> Vec<u8> {
        let mut frame = SSHWriter::new();
        frame.string(body);
        stream.write_all(frame.data()).unwrap();
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).unwrap();
        let mut reply = vec![0u8; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut reply).unwrap();
        reply
    }
}

use support::{harness, round_trip};

fn sign_request(key_blob: &[u8], data: &[u8]) -> Vec<u8> {
    let mut w = SSHWriter::new();
    w.byte(AgentMessage::SIGN_REQUEST);
    w.string(key_blob);
    w.string(data);
    w.uint32(0);
    w.into_data()
}

fn session_bind(is_forwarding: bool) -> Vec<u8> {
    let mut w = SSHWriter::new();
    w.byte(AgentMessage::EXTENSION);
    w.string("session-bind@openssh.com");
    w.string([1]);
    w.string([2]);
    w.string([3]);
    w.bool(is_forwarding);
    w.into_data()
}

#[test]
fn lists_identities_and_signs_with_a_verifiable_signature() {
    let h = harness(KeyPolicy::default(), None);
    let mut stream = UnixStream::connect(&h.socket).unwrap();

    let reply = round_trip(&mut stream, &[AgentMessage::REQUEST_IDENTITIES]);
    let mut r = SSHReader::new(&reply);
    assert_eq!(r.byte().unwrap(), AgentMessage::IDENTITIES_ANSWER);
    assert_eq!(r.uint32().unwrap(), 1);
    assert_eq!(r.string().unwrap(), h.key_blob.as_slice());
    assert_eq!(r.utf8_string().unwrap(), "test key");
    assert!(r.is_at_end());

    let data = b"session id || SSH_MSG_USERAUTH_REQUEST";
    let reply = round_trip(&mut stream, &sign_request(&h.key_blob, data));
    let mut r = SSHReader::new(&reply);
    assert_eq!(r.byte().unwrap(), AgentMessage::SIGN_RESPONSE);
    let blob = r.string().unwrap();
    let mut sig = SSHReader::new(blob);
    assert_eq!(sig.utf8_string().unwrap(), "ecdsa-sha2-nistp256");
    let mut inner = SSHReader::new(sig.string().unwrap());
    let mut raw = [0u8; 64];
    for half in raw.chunks_mut(32) {
        let mpint = inner.string().unwrap();
        let magnitude: Vec<u8> = mpint.iter().copied().skip_while(|&b| b == 0).collect();
        half[32 - magnitude.len()..].copy_from_slice(&magnitude);
    }
    let signature = Signature::from_slice(&raw).unwrap();
    assert!(
        h.signing_key
            .verifying_key()
            .verify(data, &signature)
            .is_ok()
    );
}

#[test]
fn socket_and_directory_are_owner_only_and_removed_on_stop() {
    let h = harness(KeyPolicy::default(), None);
    let socket_mode = fs::metadata(&h.socket).unwrap().permissions().mode() & 0o777;
    let dir_mode = fs::metadata(h.socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(socket_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
    assert!(matches!(
        h.server.start(),
        Err(AgentServerError::AlreadyRunning)
    ));

    h.server.stop();
    assert!(!h.server.is_running());
    assert!(!h.socket.exists());
    assert!(UnixStream::connect(&h.socket).is_err());
}

#[test]
fn unknown_keys_unsupported_messages_and_extensions_fail() {
    let h = harness(KeyPolicy::default(), None);
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    assert_eq!(
        round_trip(&mut stream, &sign_request(b"nope", b"x")),
        [AgentMessage::FAILURE]
    );
    assert_eq!(round_trip(&mut stream, &[17]), [AgentMessage::FAILURE]);
    assert_eq!(
        round_trip(&mut stream, &[13, 0, 0, 0, 9, 1]),
        [AgentMessage::FAILURE]
    );
    let mut ext = SSHWriter::new();
    ext.byte(AgentMessage::EXTENSION);
    ext.string("query");
    assert_eq!(
        round_trip(&mut stream, ext.data()),
        [AgentMessage::EXTENSION_FAILURE]
    );
}

#[test]
fn forwarded_requests_are_refused_unless_the_key_allows_them() {
    let h = harness(KeyPolicy::default(), None);
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    assert_eq!(
        round_trip(&mut stream, &session_bind(true)),
        [AgentMessage::SUCCESS]
    );
    assert_eq!(
        round_trip(&mut stream, &sign_request(&h.key_blob, b"x")),
        [AgentMessage::FAILURE]
    );

    let allowing = harness(
        KeyPolicy {
            allow_forwarded: true,
            ..KeyPolicy::default()
        },
        None,
    );
    let mut stream = UnixStream::connect(&allowing.socket).unwrap();
    assert_eq!(
        round_trip(&mut stream, &session_bind(true)),
        [AgentMessage::SUCCESS]
    );
    let reply = round_trip(&mut stream, &sign_request(&allowing.key_blob, b"x"));
    assert_eq!(reply[0], AgentMessage::SIGN_RESPONSE);
}

#[test]
fn allowed_hosts_require_a_matching_ssh_destination() {
    // The peer is this test binary, not `ssh`, so there is no destination to match.
    let h = harness(
        KeyPolicy {
            allowed_hosts: vec!["*".to_owned()],
            ..KeyPolicy::default()
        },
        None,
    );
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    assert_eq!(
        round_trip(&mut stream, &sign_request(&h.key_blob, b"x")),
        [AgentMessage::FAILURE]
    );
}

#[test]
fn approver_is_consulted_for_every_signature() {
    let calls = Arc::new(AtomicUsize::new(0));
    let allow = Arc::new(Mutex::new(false));
    let (calls_in, allow_in) = (Arc::clone(&calls), Arc::clone(&allow));
    let h = harness(
        KeyPolicy::default(),
        Some(Box::new(move || {
            calls_in.fetch_add(1, Ordering::SeqCst);
            *allow_in.lock().unwrap()
        })),
    );
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    assert_eq!(
        round_trip(&mut stream, &sign_request(&h.key_blob, b"x")),
        [AgentMessage::FAILURE]
    );
    *allow.lock().unwrap() = true;
    assert_eq!(
        round_trip(&mut stream, &sign_request(&h.key_blob, b"x"))[0],
        AgentMessage::SIGN_RESPONSE
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn oversized_frames_close_the_connection() {
    let h = harness(KeyPolicy::default(), None);
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    stream
        .write_all(&(256 * 1024 + 1u32).to_be_bytes())
        .unwrap();
    let mut buf = [0u8; 1];
    assert_eq!(stream.read(&mut buf).unwrap(), 0);
}
