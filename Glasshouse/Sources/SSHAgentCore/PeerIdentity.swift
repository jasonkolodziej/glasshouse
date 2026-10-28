import Darwin
import Foundation
import GlasshouseSecurity
import Security

/// Who is on the other end of the agent socket.
public struct PeerIdentity: Sendable {
    public let uid: uid_t
    public let pid: pid_t?
    /// Path from the code-signing identity bound to the audit token — not from the PID, which can be reused.
    public let codePath: String?
    public let codeIsValidlySigned: Bool
    /// argv of the peer (normally `ssh …`), for showing the destination in the Touch ID prompt.
    public let process: ProcessSnapshot?

    public var displayName: String {
        process?.executableName ?? codePath.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "unknown process"
    }
}

enum PeerLookup {
    // <sys/un.h>: SOL_LOCAL = 0, LOCAL_PEERPID = 0x002, LOCAL_PEERTOKEN = 0x006.
    // Spelled out because the C macros may not import into Swift. VERIFY against the macOS 26 SDK.
    private static let solLocal: Int32 = 0
    private static let localPeerPID: Int32 = 0x002
    private static let localPeerToken: Int32 = 0x006

    static func identify(fd: Int32) -> PeerIdentity? {
        var uid: uid_t = 0, gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0 else { return nil }

        var pid: pid_t = 0
        var pidLen = socklen_t(MemoryLayout<pid_t>.size)
        let havePID = getsockopt(fd, solLocal, localPeerPID, &pid, &pidLen) == 0

        var token = audit_token_t()
        var tokenLen = socklen_t(MemoryLayout<audit_token_t>.size)
        var codePath: String?
        var valid = false
        if getsockopt(fd, solLocal, localPeerToken, &token, &tokenLen) == 0 {
            let tokenData = withUnsafeBytes(of: &token) { Data($0) }
            (codePath, valid) = codeIdentity(auditToken: tokenData)
        }

        return PeerIdentity(uid: uid, pid: havePID ? pid : nil, codePath: codePath,
                            codeIsValidlySigned: valid,
                            process: havePID ? ProcessInspector.snapshot(pid: pid) : nil)
    }

    private static func codeIdentity(auditToken: Data) -> (String?, Bool) {
        var guest: SecCode?
        let attrs = [kSecGuestAttributeAudit: auditToken] as CFDictionary
        guard SecCodeCopyGuestWithAttributes(nil, attrs, SecCSFlags(), &guest) == errSecSuccess,
              let code = guest else { return (nil, false) }
        let valid = SecCodeCheckValidity(code, SecCSFlags(), nil) == errSecSuccess
        var staticCode: SecStaticCode?
        guard SecCodeCopyStaticCode(code, SecCSFlags(), &staticCode) == errSecSuccess,
              let sc = staticCode else { return (nil, valid) }
        var url: CFURL?
        guard SecCodeCopyPath(sc, SecCSFlags(), &url) == errSecSuccess, let u = url as URL? else { return (nil, valid) }
        return (u.path, valid)
    }
}
