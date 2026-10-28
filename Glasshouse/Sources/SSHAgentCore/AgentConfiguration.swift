import Foundation

/// Shared between the app (key management, ssh_config snippets) and the agent (serving).
public enum AgentConfiguration {
    /// `$(AppIdentifierPrefix)dev.glasshouse.shared` — the prefix is your Team ID followed by a dot.
    /// Read from the agent's/app's own entitlements at runtime would be more robust; hard-coded for the scaffold.
    public static var keychainAccessGroup: String {
        (Bundle.main.object(forInfoDictionaryKey: "GHKeychainAccessGroup") as? String)
            ?? "TEAMID.dev.glasshouse.shared"
    }

    /// Short, space-free path well under the 104-byte sun_path limit; directory is created 0700.
    public static var socketPath: String {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".glasshouse/agent.sock").path
    }

    public static let loginItemIdentifier = "dev.glasshouse.agent"

    /// Per-host opt-in: Glasshouse never sets SSH_AUTH_SOCK globally.
    public static func sshConfigBlock(hostAlias: String, hostName: String, user: String) -> String {
        """
        Host \(hostAlias)
            HostName \(hostName)
            User \(user)
            IdentityAgent "\(socketPath)"
            IdentitiesOnly yes
            ForwardAgent no
        """
    }
}
