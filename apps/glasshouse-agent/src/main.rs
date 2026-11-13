//! Glasshouse Agent — a login item that serves Secure Enclave keys over an ssh-agent socket.
//! Opt-in: the main app registers it only when the user enables "Glasshouse Agent" in
//! Settings → SSH. Clients reach it per host via `IdentityAgent`.
//!
//! The Swift agent ran an accessory `NSApplication` so Touch ID sheets could appear; this port has no
//! UI of its own. It blocks SIGTERM/SIGINT in every thread and waits for them synchronously, then
//! stops the server (removing the socket) and exits 0.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use ssh_agent_core::{AgentConfiguration, AgentServer, SecureEnclaveKeyStore};

fn main() {
    if let Err(error) = run() {
        eprintln!("Glasshouse Agent failed to start: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Block before any thread is spawned so every server thread inherits the mask and the signals
    // are delivered only to `sigwait` below.
    let signals = termination_signals()?;
    // SAFETY: `signals` is an initialised sigset_t; a null old-set pointer is allowed.
    let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut()) };
    if rc != 0 {
        bail!("could not block SIGTERM/SIGINT (error {rc})");
    }

    let store = Arc::new(SecureEnclaveKeyStore::new(
        AgentConfiguration::keychain_access_group(),
    ));
    let server = AgentServer::new(AgentConfiguration::socket_path(), store);
    server
        .start()
        .with_context(|| format!("could not listen on {}", server.socket_path().display()))?;

    let signal = wait_for(&signals)?;
    eprintln!("Glasshouse Agent stopping on signal {signal}");
    server.stop();
    Ok(())
}

fn termination_signals() -> Result<libc::sigset_t> {
    // SAFETY: sigset_t is plain old data; sigemptyset initialises it before use.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a valid, writable sigset_t.
    let ok = unsafe {
        libc::sigemptyset(&mut set) == 0
            && libc::sigaddset(&mut set, libc::SIGTERM) == 0
            && libc::sigaddset(&mut set, libc::SIGINT) == 0
    };
    if !ok {
        bail!("could not build the termination signal set");
    }
    Ok(set)
}

fn wait_for(signals: &libc::sigset_t) -> Result<libc::c_int> {
    loop {
        let mut signal: libc::c_int = 0;
        // SAFETY: `signals` is an initialised set blocked in this thread; `signal` is writable.
        let rc = unsafe { libc::sigwait(signals, &mut signal) };
        match rc {
            0 => return Ok(signal),
            libc::EINTR => continue,
            error => bail!("sigwait failed (error {error})"),
        }
    }
}
