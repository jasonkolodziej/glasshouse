//! A signing-only ssh-agent on a Unix-domain socket.
//!
//! - Socket dir 0700, socket 0600, umask 077; peers with a different UID are rejected
//!   (getpeereid on macOS, SO_PEERCRED on Linux).
//! - One connection per thread: a sign request blocks on approval, and ssh waits for us anyway.
//! - No prompt caching: every signature is approved on its own.

use std::fs::{self, DirBuilder};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use crate::agent_protocol::{AgentRequest, AgentResponse, SSHKeyFormat, SessionBind};
use crate::approval_request::ApprovalRequest;
use crate::key_store::{KeyStore, KeyStoreError};
use crate::peer_identity::PeerIdentity;
use crate::ssh_wire::SSHWireError;

/// Decides whether a sign request may proceed (e.g. by showing the approval card). Called after the
/// forwarding and allowed-host checks and before the key store signs.
pub type Approver = dyn Fn(&ApprovalRequest) -> bool + Send + Sync;

/// Failures starting the server.
#[derive(Debug, thiserror::Error)]
pub enum AgentServerError {
    /// The socket path does not fit in `sockaddr_un.sun_path`.
    #[error("socket path {0} is too long for a Unix-domain socket")]
    NameTooLong(PathBuf),
    /// The server is already listening.
    #[error("the agent server is already running")]
    AlreadyRunning,
    /// A filesystem or socket call failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Maximum accepted request frame.
const MAX_FRAME: usize = 256 * 1024;

struct Shared {
    store: Arc<dyn KeyStore>,
    approver: Option<Box<Approver>>,
}

struct Running {
    listener: Arc<UnixListener>,
    stopping: Arc<AtomicBool>,
    accept_thread: JoinHandle<()>,
}

/// Per-connection state (session-bind is sent once per ssh connection, before sign requests).
struct Connection {
    peer: Option<PeerIdentity>,
    binds: Vec<SessionBind>,
}

/// The agent server. Dropping it stops it.
pub struct AgentServer {
    socket_path: PathBuf,
    shared: Arc<Shared>,
    running: Mutex<Option<Running>>,
}

impl std::fmt::Debug for AgentServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentServer")
            .field("socket_path", &self.socket_path)
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

/// Restores the previous umask when dropped.
struct UmaskGuard(libc::mode_t);

impl UmaskGuard {
    fn set(mask: libc::mode_t) -> Self {
        // SAFETY: umask has no preconditions.
        Self(unsafe { libc::umask(mask) })
    }
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        // SAFETY: umask has no preconditions.
        unsafe { libc::umask(self.0) };
    }
}

impl AgentServer {
    /// A stopped server that will listen on `socket_path` and sign with `store`.
    pub fn new(socket_path: impl Into<PathBuf>, store: Arc<dyn KeyStore>) -> Self {
        Self {
            socket_path: socket_path.into(),
            shared: Arc::new(Shared {
                store,
                approver: None,
            }),
            running: Mutex::new(None),
        }
    }

    /// Adds an approval hook consulted before every signature. Must be called before [`Self::start`];
    /// once the server has started the hook is ignored.
    pub fn with_approver(
        mut self,
        approver: impl Fn(&ApprovalRequest) -> bool + Send + Sync + 'static,
    ) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.approver = Some(Box::new(approver));
        }
        self
    }

    /// The socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// True between [`Self::start`] and [`Self::stop`].
    pub fn is_running(&self) -> bool {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Creates the socket directory (0700), binds the socket (0600) and starts accepting connections.
    pub fn start(&self) -> Result<(), AgentServerError> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.is_some() {
            return Err(AgentServerError::AlreadyRunning);
        }
        if let Some(dir) = self
            .socket_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let _ = fs::remove_file(&self.socket_path);

        if self.socket_path.as_os_str().len() >= sun_path_capacity() {
            return Err(AgentServerError::NameTooLong(self.socket_path.clone()));
        }

        let listener = {
            let _umask = UmaskGuard::set(0o077);
            let listener = UnixListener::bind(&self.socket_path)?;
            fs::set_permissions(&self.socket_path, fs::Permissions::from_mode(0o600))?;
            Arc::new(listener)
        };

        let stopping = Arc::new(AtomicBool::new(false));
        let accept_thread = {
            let listener = Arc::clone(&listener);
            let stopping = Arc::clone(&stopping);
            let shared = Arc::clone(&self.shared);
            thread::Builder::new()
                .name("dev.glasshouse.agent.accept".to_owned())
                .spawn(move || accept_loop(&listener, &stopping, &shared))?
        };
        *running = Some(Running {
            listener,
            stopping,
            accept_thread,
        });
        Ok(())
    }

    /// Stops accepting connections and removes the socket. In-flight connections finish on their own.
    pub fn stop(&self) {
        let running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(running) = running {
            running.stopping.store(true, Ordering::SeqCst);
            // Wake the blocked accept(): shutdown works on Linux; the self-connect covers the rest.
            // SAFETY: the listener is kept alive by `running.listener`, so the fd is still ours.
            unsafe { libc::shutdown(running.listener.as_raw_fd(), libc::SHUT_RDWR) };
            let _ = UnixStream::connect(&self.socket_path);
            let _ = running.accept_thread.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        if self.is_running() {
            self.stop();
        }
    }
}

fn sun_path_capacity() -> usize {
    // SAFETY: sockaddr_un is plain old data; all-zero is a valid value.
    let addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_path.len()
}

fn accept_loop(listener: &UnixListener, stopping: &AtomicBool, shared: &Arc<Shared>) {
    loop {
        let accepted = listener.accept();
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        match accepted {
            Ok((stream, _)) => {
                let shared = Arc::clone(shared);
                let spawned = thread::Builder::new()
                    .name("dev.glasshouse.agent.connection".to_owned())
                    .spawn(move || serve(stream, &shared));
                if spawned.is_err() {
                    continue;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn serve(mut stream: UnixStream, shared: &Shared) {
    let Some(peer) = PeerIdentity::identify(stream.as_raw_fd()) else {
        return;
    };
    // SAFETY: getuid has no preconditions.
    if peer.uid != unsafe { libc::getuid() } {
        return;
    }
    let mut conn = Connection {
        peer: Some(peer),
        binds: Vec::new(),
    };
    while let Some(body) = read_frame(&mut stream) {
        let reply = AgentRequest::parse(&body)
            .map_err(HandleError::from)
            .and_then(|request| handle(request, &mut conn, shared))
            .unwrap_or_else(|_| AgentResponse::failure());
        if stream.write_all(&reply).is_err() {
            return;
        }
    }
}

#[derive(Debug)]
enum HandleError {
    Wire,
    KeyStore,
}

impl From<SSHWireError> for HandleError {
    fn from(_: SSHWireError) -> Self {
        Self::Wire
    }
}

impl From<KeyStoreError> for HandleError {
    fn from(_: KeyStoreError) -> Self {
        Self::KeyStore
    }
}

fn handle(
    request: AgentRequest,
    conn: &mut Connection,
    shared: &Shared,
) -> Result<Vec<u8>, HandleError> {
    match request {
        AgentRequest::RequestIdentities => {
            let keys = shared.store.list_keys().unwrap_or_default();
            let ids: Vec<(&[u8], &str)> = keys
                .iter()
                .map(|key| (key.public_key_blob.as_slice(), key.comment.as_str()))
                .collect();
            Ok(AgentResponse::identities(&ids))
        }
        AgentRequest::SessionBind(bind) => {
            conn.binds.push(bind); // TODO: verify bind.signature over session_id with bind.host_key
            Ok(AgentResponse::success())
        }
        AgentRequest::Sign { key_blob, data, .. } => {
            let Some(key) = shared
                .store
                .list_keys()?
                .into_iter()
                .find(|key| key.public_key_blob == key_blob)
            else {
                return Ok(AgentResponse::failure());
            };
            let forwarded = conn.binds.last().is_some_and(|bind| bind.is_forwarding);
            if forwarded && !key.policy.allow_forwarded {
                return Ok(AgentResponse::failure());
            }

            let request = ApprovalRequest {
                key,
                peer: conn.peer.clone(),
                bind: conn.binds.last().cloned(),
            };
            // A host restriction needs an authenticated host: the session-bind host key, its
            // signature verified and matched to a known_hosts name allowed by the policy. The
            // peer's argv can't stand in for that (`ssh -o HostName=elsewhere allowed`, or any
            // client calling itself ssh), and that verification doesn't exist yet, so a
            // restricted key fails closed: it signs nothing rather than sign for the wrong host.
            if !request.key.policy.allowed_hosts.is_empty() {
                return Ok(AgentResponse::failure());
            }
            if let Some(approver) = &shared.approver
                && !approver(&request)
            {
                return Ok(AgentResponse::failure());
            }
            let signature = shared.store.sign(&request.key.id, &data, &request)?;
            Ok(AgentResponse::signature(&SSHKeyFormat::signature_blob(
                &signature,
            )))
        }
        AgentRequest::UnknownExtension { .. } => Ok(AgentResponse::extension_failure()),
        AgentRequest::Unsupported { .. } => Ok(AgentResponse::failure()),
    }
}

fn read_frame(stream: &mut UnixStream) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).ok()?;
    let len = usize::try_from(u32::from_be_bytes(header)).ok()?;
    if len == 0 || len > MAX_FRAME {
        return None;
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).ok()?;
    Some(body)
}
