//! Hosts from ~/.ssh/config, resolved with `ssh -G` so what we show is what ssh will actually use
//! (Include, Match, defaults all applied). Resolution runs off the UI thread.

use std::process::{Command, Stdio};

/// Which credential a host signs with, as the sidebar badge shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Credential {
    SecureEnclave,
    Agent,
    Other,
}

impl Credential {
    pub fn badge(self) -> &'static str {
        match self {
            Credential::SecureEnclave => "SE",
            Credential::Agent => "Agent",
            Credential::Other => "Key",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Credential::SecureEnclave => {
                "Secure Enclave key via /usr/lib/ssh-keychain.dylib — Touch ID per use"
            }
            Credential::Agent => "Glasshouse Agent — Touch ID prompt names host and process",
            Credential::Other => "Key file or another agent",
        }
    }
}

/// One resolved host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<String>,
    pub credential: Credential,
}

/// Parses `ssh -G <alias>` output.
pub fn parse_resolved(alias: &str, text: &str) -> SshHost {
    let mut host = SshHost {
        alias: alias.to_owned(),
        host_name: None,
        user: None,
        port: None,
        credential: Credential::Other,
    };
    let mut provider = None;
    let mut identity_agent = None;
    let mut algorithms = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key {
            "hostname" => host.host_name = Some(value.to_owned()),
            "user" => host.user = Some(value.to_owned()),
            "port" => host.port = Some(value.to_owned()),
            "securitykeyprovider" => provider = Some(value),
            "identityagent" => identity_agent = Some(value),
            "pubkeyacceptedalgorithms" => algorithms = Some(value),
            _ => {}
        }
    }
    // The provider alone only says which library handles security keys: an IdentityFile or an
    // agent key could still sign without Touch ID. A Glasshouse host also turns the agent off and
    // accepts only sk- algorithms, so every signature is a security-key one through the provider.
    let only_security_keys = algorithms
        .is_some_and(|list| !list.is_empty() && list.split(',').all(|a| a.starts_with("sk-")));
    if provider == Some(ssh_config::glasshouse_ssh::SECURITY_KEY_PROVIDER)
        && identity_agent == Some("none")
        && only_security_keys
    {
        host.credential = Credential::SecureEnclave;
    } else if identity_agent.is_some_and(|a| a.contains(".glasshouse/agent.sock")) {
        host.credential = Credential::Agent;
    }
    host
}

/// Runs `/usr/bin/ssh -G alias`; `None` if ssh is missing or fails.
pub fn resolve(alias: &str) -> Option<SshHost> {
    let out = Command::new("/usr/bin/ssh")
        .args(["-G", alias])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_resolved(alias, &String::from_utf8_lossy(&out.stdout)))
}

/// Every alias in ~/.ssh/config (following Includes), resolved. Blocking: call from a worker.
pub fn load_all() -> Vec<SshHost> {
    let Ok(store) = ssh_config::SshConfigStore::standard() else {
        return Vec::new();
    };
    store
        .host_aliases()
        .iter()
        .filter_map(|alias| resolve(alias))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resolved_should_mark_secure_enclave_hosts() {
        let text = "hostname h.example\nuser me\nport 22\nsecuritykeyprovider /usr/lib/ssh-keychain.dylib\n\
                    identityagent none\npubkeyacceptedalgorithms sk-ecdsa-sha2-nistp256@openssh.com\n";
        let host = parse_resolved("box", text);
        assert_eq!(
            host,
            SshHost {
                alias: "box".into(),
                host_name: Some("h.example".into()),
                user: Some("me".into()),
                port: Some("22".into()),
                credential: Credential::SecureEnclave,
            }
        );
    }

    #[test]
    fn parse_resolved_should_not_promise_touch_id_when_other_keys_can_sign() {
        // The provider is set, but ssh-agent and ordinary key types are still allowed.
        let text = "securitykeyprovider /usr/lib/ssh-keychain.dylib\nidentityagent SSH_AUTH_SOCK\n\
                    pubkeyacceptedalgorithms ssh-ed25519,sk-ecdsa-sha2-nistp256@openssh.com\n";
        assert_eq!(parse_resolved("box", text).credential, Credential::Other);
    }

    #[test]
    fn parse_resolved_should_not_promise_touch_id_with_only_a_provider() {
        let text =
            "securitykeyprovider /usr/lib/ssh-keychain.dylib\nidentityfile ~/.ssh/id_ed25519\n";
        assert_eq!(parse_resolved("box", text).credential, Credential::Other);
    }

    #[test]
    fn parse_resolved_should_mark_glasshouse_agent_hosts() {
        let host = parse_resolved("a", "identityagent /Users/me/.glasshouse/agent.sock\n");
        assert_eq!(host.credential, Credential::Agent);
    }
}
