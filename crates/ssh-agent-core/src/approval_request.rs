use std::ffi::CString;

use crate::agent_protocol::SessionBind;
use crate::key_store::AgentKey;
use crate::peer_identity::PeerIdentity;

/// Builds the text inside the Touch ID prompt — the one thing Apple's native paths (pam_tid,
/// ssh-keychain.dylib) can't do: name the destination, the key and the requesting process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// The key asked to sign.
    pub key: AgentKey,
    /// The process on the other end of the socket, if it could be identified.
    pub peer: Option<PeerIdentity>,
    /// The connection's most recent `session-bind@openssh.com`, if any.
    pub bind: Option<SessionBind>,
}

impl ApprovalRequest {
    /// Destination from the requesting `ssh` process's argv (e.g. "deploy@prod-db-1").
    pub fn destination(&self) -> Option<&str> {
        self.peer.as_ref()?.process.as_ref()?.ssh_destination()
    }

    /// `LAContext.localizedReason` is shown as "<App> is trying to <reason>." — so it starts with a verb.
    pub fn localized_reason(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        match self.destination() {
            Some(dest) => parts.push(format!("sign in to {dest}")),
            None => parts.push("sign an SSH request".to_owned()),
        }
        parts.push(format!("with key \u{201C}{}\u{201D}", self.key.comment));
        if let Some(peer) = &self.peer {
            let who = if peer.code_is_validly_signed {
                peer.display_name()
            } else {
                format!("{} (unsigned)", peer.display_name())
            };
            let pid = peer
                .pid
                .map(|pid| format!(" [pid {pid}]"))
                .unwrap_or_default();
            parts.push(format!("for {who}{pid}"));
        }
        if let Some(bind) = &self.bind {
            if bind.verified {
                let fingerprint: String = bind.host_key_fingerprint().chars().take(19).collect();
                parts.push(format!("\u{00B7} host key {fingerprint}\u{2026}"));
            } else {
                parts.push("\u{00B7} host key unverified".to_owned());
            }
            if bind.is_forwarding {
                parts.push("\u{00B7} FORWARDED request".to_owned());
            }
        }
        parts.join(" ")
    }

    /// Matches `user@host` or `host` against shell-style globs (e.g. "*.internal", "prod-*").
    pub fn destination_matches(&self, patterns: &[String]) -> bool {
        let Some(dest) = self.destination() else {
            return false;
        };
        let host = dest
            .split('@')
            .rfind(|part| !part.is_empty())
            .unwrap_or(dest);
        let Ok(host) = CString::new(host) else {
            return false;
        };
        patterns.iter().any(|pattern| {
            let Ok(pattern) = CString::new(pattern.as_str()) else {
                return false;
            };
            // SAFETY: both arguments are valid NUL-terminated strings that outlive the call.
            unsafe { libc::fnmatch(pattern.as_ptr(), host.as_ptr(), 0) == 0 }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_store::KeyPolicy;
    use glasshouse_security::ProcessSnapshot;

    fn key() -> AgentKey {
        AgentKey {
            id: "ID".to_owned(),
            comment: "work laptop".to_owned(),
            policy: KeyPolicy::default(),
            public_key_blob: vec![1, 2, 3],
        }
    }

    fn ssh_peer(args: &[&str], signed: bool) -> PeerIdentity {
        PeerIdentity {
            uid: 501,
            pid: Some(4242),
            code_path: Some("/usr/bin/ssh".to_owned()),
            code_is_validly_signed: signed,
            process: Some(ProcessSnapshot {
                pid: 4242,
                executable_path: "/usr/bin/ssh".to_owned(),
                arguments: args.iter().map(|&a| a.to_owned()).collect(),
                effective_user: "me".to_owned(),
            }),
        }
    }

    fn bind(verified: bool, is_forwarding: bool) -> SessionBind {
        SessionBind {
            host_key: b"host key".to_vec(),
            session_id: vec![1],
            signature: vec![2],
            is_forwarding,
            verified,
        }
    }

    #[test]
    fn reason_without_peer_or_bind() {
        let request = ApprovalRequest {
            key: key(),
            peer: None,
            bind: None,
        };
        assert_eq!(
            request.localized_reason(),
            "sign an SSH request with key \u{201C}work laptop\u{201D}"
        );
    }

    #[test]
    fn reason_names_destination_process_and_unverified_forwarded_host() {
        let request = ApprovalRequest {
            key: key(),
            peer: Some(ssh_peer(&["ssh", "-p", "22", "deploy@prod-db-1"], false)),
            bind: Some(bind(false, true)),
        };
        assert_eq!(
            request.localized_reason(),
            "sign in to deploy@prod-db-1 with key \u{201C}work laptop\u{201D} for ssh (unsigned) [pid 4242] \
             \u{00B7} host key unverified \u{00B7} FORWARDED request"
        );
    }

    #[test]
    fn reason_shows_fingerprint_prefix_for_verified_host() {
        let bind = bind(true, false);
        let prefix: String = bind.host_key_fingerprint().chars().take(19).collect();
        let request = ApprovalRequest {
            key: key(),
            peer: Some(ssh_peer(&["ssh", "host"], true)),
            bind: Some(bind),
        };
        assert_eq!(
            request.localized_reason(),
            format!(
                "sign in to host with key \u{201C}work laptop\u{201D} for ssh [pid 4242] \u{00B7} host key {prefix}\u{2026}"
            )
        );
    }

    #[test]
    fn destination_matches_host_part_against_globs() {
        let request = ApprovalRequest {
            key: key(),
            peer: Some(ssh_peer(&["ssh", "deploy@prod-db-1"], true)),
            bind: None,
        };
        assert!(request.destination_matches(&["prod-*".to_owned()]));
        assert!(!request.destination_matches(&["*.internal".to_owned(), "deploy@*".to_owned()]));

        let no_peer = ApprovalRequest {
            key: key(),
            peer: None,
            bind: None,
        };
        assert!(!no_peer.destination_matches(&["*".to_owned()]));
    }
}
