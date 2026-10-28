import Foundation

/// Builds the text inside the Touch ID prompt — the one thing Apple's native paths (pam_tid,
/// ssh-keychain.dylib) can't do: name the destination, the key and the requesting process.
public struct ApprovalRequest: Sendable {
    public let key: AgentKey
    public let peer: PeerIdentity?
    public let bind: SessionBind?

    /// Destination from the requesting `ssh` process's argv (e.g. "deploy@prod-db-1").
    public var destination: String? { peer?.process?.sshDestination }

    /// LAContext.localizedReason is shown as "<App> is trying to <reason>." — so it starts with a verb.
    public var localizedReason: String {
        var parts: [String] = []
        if let dest = destination {
            parts.append("sign in to \(dest)")
        } else {
            parts.append("sign an SSH request")
        }
        parts.append("with key “\(key.comment)”")
        if let peer {
            let who = peer.codeIsValidlySigned ? peer.displayName : "\(peer.displayName) (unsigned)"
            parts.append("for \(who)" + (peer.pid.map { " [pid \($0)]" } ?? ""))
        }
        if let bind {
            parts.append(bind.verified ? "· host key \(bind.hostKeyFingerprint.prefix(19))…"
                                       : "· host key unverified")
            if bind.isForwarding { parts.append("· FORWARDED request") }
        }
        return parts.joined(separator: " ")
    }

    /// Matches `user@host` or `host` against shell-style globs (e.g. "*.internal", "prod-*").
    public func destinationMatches(_ patterns: [String]) -> Bool {
        guard let dest = destination else { return false }
        let host = dest.split(separator: "@").last.map(String.init) ?? dest
        return patterns.contains { fnmatch($0, host, 0) == 0 }
    }
}
