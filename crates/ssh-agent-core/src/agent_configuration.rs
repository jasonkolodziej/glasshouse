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
    ///
    /// `identity_file` is the public key of the agent key this host uses (`~/.ssh/<name>.pub`
    /// written when the key is created). With `IdentitiesOnly yes`, ssh only offers agent keys
    /// whose public key it is given, so without it the agent's key would never be tried.
    ///
    /// # Errors
    /// [`AgentConfigError`] when a value could add or change ssh_config directives: empty, or
    /// containing whitespace, a quote, `#` or a control character (a path may contain spaces).
    pub fn ssh_config_block(
        host_alias: &str,
        host_name: &str,
        user: &str,
        identity_file: &str,
    ) -> Result<String, AgentConfigError> {
        let host_alias = token("host alias", host_alias)?;
        let host_name = token("host name", host_name)?;
        let user = token("user", user)?;
        let identity_file = path("identity file", identity_file)?;
        let socket = Self::socket_path().display().to_string();
        let socket = path("agent socket", &socket)?;
        Ok(format!(
            "Host {host_alias}\n    HostName {host_name}\n    User {user}\n    IdentityAgent \"{socket}\"\n    \
             IdentityFile \"{identity_file}\"\n    IdentitiesOnly yes\n    ForwardAgent no"
        ))
    }
}

/// A value [`AgentConfiguration::ssh_config_block`] refused to write.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentConfigError {
    /// The value is empty.
    #[error("the {0} is empty")]
    Empty(&'static str),
    /// The value contains a character that would end the directive or start another.
    #[error("the {0} contains {1:?}, which ssh_config would read as the end of the value")]
    InvalidCharacter(&'static str, char),
}

/// One ssh_config word: no whitespace, quotes, comments or control characters.
fn token<'a>(what: &'static str, value: &'a str) -> Result<&'a str, AgentConfigError> {
    if value.is_empty() {
        return Err(AgentConfigError::Empty(what));
    }
    match value
        .chars()
        .find(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '#' | '\\'))
    {
        Some(c) => Err(AgentConfigError::InvalidCharacter(what, c)),
        None => Ok(value),
    }
}

/// A quoted path: spaces are fine inside the quotes, line breaks and quotes are not.
fn path<'a>(what: &'static str, value: &'a str) -> Result<&'a str, AgentConfigError> {
    if value.is_empty() {
        return Err(AgentConfigError::Empty(what));
    }
    match value.chars().find(|c| c.is_control() || *c == '"') {
        Some(c) => Err(AgentConfigError::InvalidCharacter(what, c)),
        None => Ok(value),
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
    fn ssh_config_block_names_the_key_and_the_agent() {
        let block = AgentConfiguration::ssh_config_block(
            "prod",
            "prod.example.com",
            "deploy",
            "~/.ssh/prod-agent.pub",
        )
        .unwrap();
        let expected = format!(
            "Host prod
    HostName prod.example.com
    User deploy
    IdentityAgent \"{}\"
    IdentityFile \"~/.ssh/prod-agent.pub\"
    IdentitiesOnly yes
    ForwardAgent no",
            AgentConfiguration::socket_path().display()
        );
        assert_eq!(block, expected);
    }

    #[test]
    fn ssh_config_block_should_refuse_a_host_name_that_adds_a_directive() {
        let injected = "prod.example.com\n    ProxyCommand sh -c evil";
        assert_eq!(
            AgentConfiguration::ssh_config_block("prod", injected, "deploy", "~/.ssh/k.pub"),
            Err(AgentConfigError::InvalidCharacter("host name", '\n'))
        );
    }

    #[test]
    fn ssh_config_block_should_refuse_whitespace_in_a_user() {
        assert_eq!(
            AgentConfiguration::ssh_config_block("prod", "h", "deploy ProxyJump", "~/.ssh/k.pub"),
            Err(AgentConfigError::InvalidCharacter("user", ' '))
        );
    }

    #[test]
    fn ssh_config_block_should_refuse_a_quote_in_the_identity_file() {
        assert_eq!(
            AgentConfiguration::ssh_config_block("prod", "h", "u", "~/.ssh/k\"x"),
            Err(AgentConfigError::InvalidCharacter("identity file", '"'))
        );
    }

    #[test]
    fn ssh_config_block_should_refuse_an_empty_alias() {
        assert_eq!(
            AgentConfiguration::ssh_config_block("", "h", "u", "~/.ssh/k.pub"),
            Err(AgentConfigError::Empty("host alias"))
        );
    }
}
