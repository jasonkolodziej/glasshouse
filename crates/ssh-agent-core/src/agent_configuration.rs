use std::path::PathBuf;

/// Shared between the app (key management, ssh_config snippets) and the agent (serving).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConfiguration;

impl AgentConfiguration {
    /// `$(AppIdentifierPrefix)dev.glasshouse.shared` — the prefix is your Team ID followed by a dot.
    pub const DEFAULT_KEYCHAIN_ACCESS_GROUP: &'static str = "TEAMID.dev.glasshouse.shared";

    /// Environment override for the access group. The Swift app read `GHKeychainAccessGroup` from its
    /// Info.plist; a Rust binary has no bundle, so the value comes from the environment instead.
    pub const KEYCHAIN_ACCESS_GROUP_ENV: &'static str = "GH_KEYCHAIN_ACCESS_GROUP";

    /// Login item / launchd label of the agent.
    pub const LOGIN_ITEM_IDENTIFIER: &'static str = "dev.glasshouse.agent";

    /// The keychain access group shared by the app and the agent.
    pub fn keychain_access_group() -> String {
        std::env::var(Self::KEYCHAIN_ACCESS_GROUP_ENV)
            .ok()
            .filter(|group| !group.is_empty())
            .unwrap_or_else(|| Self::DEFAULT_KEYCHAIN_ACCESS_GROUP.to_owned())
    }

    /// Short, space-free path well under the 104-byte sun_path limit; directory is created 0700.
    pub fn socket_path() -> PathBuf {
        std::env::home_dir()
            .unwrap_or_default()
            .join(".glasshouse/agent.sock")
    }

    /// Per-host opt-in: Glasshouse never sets SSH_AUTH_SOCK globally.
    pub fn ssh_config_block(host_alias: &str, host_name: &str, user: &str) -> String {
        format!(
            "Host {host_alias}\n    HostName {host_name}\n    User {user}\n    IdentityAgent \"{}\"\n    \
             IdentitiesOnly yes\n    ForwardAgent no",
            Self::socket_path().display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_lives_in_dot_glasshouse_under_home() {
        let path = AgentConfiguration::socket_path();
        assert!(path.ends_with(".glasshouse/agent.sock"));
        assert_eq!(
            path.parent().unwrap().parent().unwrap(),
            std::env::home_dir().unwrap()
        );
    }

    #[test]
    fn ssh_config_block_matches_swift_layout() {
        let block = AgentConfiguration::ssh_config_block("prod", "prod.example.com", "deploy");
        let expected = format!(
            "Host prod
    HostName prod.example.com
    User deploy
    IdentityAgent \"{}\"
    IdentitiesOnly yes
    ForwardAgent no",
            AgentConfiguration::socket_path().display()
        );
        assert_eq!(block, expected);
    }
}
