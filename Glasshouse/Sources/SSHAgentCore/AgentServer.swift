import CryptoKit
import Darwin
import Foundation
import LocalAuthentication

/// A signing-only ssh-agent on a Unix-domain socket.
///
/// - BSD sockets rather than `NWListener`: `sun_path` is limited to 104 bytes and container paths can
///   exceed it; Apple's advice for that case is plain BSD sockets.
/// - Socket dir 0700, socket 0600, umask 077; peers with a different UID are rejected (getpeereid).
/// - One connection per thread: a sign request blocks on Touch ID, and ssh waits for us anyway.
/// - No prompt caching: every signature gets a fresh `LAContext`.
public final class AgentServer: @unchecked Sendable {
    public let socketPath: String
    private let store: SecureEnclaveKeyStore
    private var listenFD: Int32 = -1
    private let queue = DispatchQueue(label: "dev.glasshouse.agent.accept")
    private let maxFrame = 256 * 1024

    /// Per-connection state (session-bind is sent once per ssh connection, before sign requests).
    private struct Connection { var peer: PeerIdentity?; var binds: [SessionBind] = [] }

    public init(socketPath: String, store: SecureEnclaveKeyStore) {
        self.socketPath = socketPath
        self.store = store
    }

    public func start() throws {
        let dir = (socketPath as NSString).deletingLastPathComponent
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        unlink(socketPath)

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(socketPath.utf8)
        let capacity = MemoryLayout.size(ofValue: addr.sun_path)
        guard pathBytes.count < capacity else { close(fd); throw POSIXError(.ENAMETOOLONG) }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            raw.copyBytes(from: pathBytes); raw[pathBytes.count] = 0
        }
        addr.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)

        let oldMask = umask(0o077)
        defer { umask(oldMask) }
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, chmod(socketPath, 0o600) == 0, listen(fd, 16) == 0 else {
            close(fd); throw POSIXError(.init(rawValue: errno) ?? .EIO)
        }
        listenFD = fd
        queue.async { [weak self] in self?.acceptLoop() }
    }

    public func stop() {
        if listenFD >= 0 { close(listenFD); listenFD = -1 }
        unlink(socketPath)
    }

    private func acceptLoop() {
        while listenFD >= 0 {
            let client = accept(listenFD, nil, nil)
            if client < 0 { if errno == EINTR { continue } else { break } }
            Thread.detachNewThread { [weak self] in self?.serve(client) }
        }
    }

    // MARK: Connection

    private func serve(_ fd: Int32) {
        defer { close(fd) }
        guard let peer = PeerLookup.identify(fd: fd), peer.uid == getuid() else { return }
        var conn = Connection(peer: peer)

        while let body = readFrame(fd) {
            let reply: Data
            do {
                reply = try handle(AgentRequest.parse(body), conn: &conn)
            } catch {
                reply = AgentResponse.failure
            }
            guard writeAll(fd, reply) else { return }
        }
    }

    private func handle(_ request: AgentRequest, conn: inout Connection) throws -> Data {
        switch request {
        case .requestIdentities:
            let keys = (try? store.listKeys()) ?? []
            return AgentResponse.identities(keys.map { ($0.publicKeyBlob, $0.comment) })

        case .sessionBind(let bind):
            conn.binds.append(bind)   // TODO: verify bind.signature over sessionID with bind.hostKey
            return AgentResponse.success

        case .sign(let blob, let data, _):
            guard let key = try store.listKeys().first(where: { $0.publicKeyBlob == blob }) else {
                return AgentResponse.failure
            }
            let forwarded = conn.binds.last?.isForwarding ?? false
            if forwarded && !key.policy.allowForwarded { return AgentResponse.failure }

            let request = ApprovalRequest(key: key, peer: conn.peer, bind: conn.binds.last)
            if !key.policy.allowedHosts.isEmpty,
               !request.destinationMatches(key.policy.allowedHosts) { return AgentResponse.failure }

            let context = LAContext()
            context.localizedReason = request.localizedReason
            context.localizedCancelTitle = "Deny"
            let privateKey = try store.signingKey(id: key.id, context: context)
            let signature = try privateKey.signature(for: data)   // ← Touch ID prompt happens here
            return AgentResponse.signature(SSHKeyFormat.signatureBlob(signature))

        case .unknownExtension:
            return AgentResponse.extensionFailure

        case .unsupported:
            return AgentResponse.failure
        }
    }

    // MARK: Framing

    private func readFrame(_ fd: Int32) -> Data? {
        guard let header = readExactly(fd, 4) else { return nil }
        let len = header.reduce(0) { ($0 << 8) | Int($1) }
        guard len > 0, len <= maxFrame else { return nil }
        return readExactly(fd, len)
    }

    private func readExactly(_ fd: Int32, _ n: Int) -> Data? {
        var buf = [UInt8](repeating: 0, count: n)
        var got = 0
        while got < n {
            let r = buf.withUnsafeMutableBytes { read(fd, $0.baseAddress! + got, n - got) }
            if r < 0 && errno == EINTR { continue }
            if r <= 0 { return nil }
            got += r
        }
        return Data(buf)
    }

    private func writeAll(_ fd: Int32, _ data: Data) -> Bool {
        data.withUnsafeBytes { raw in
            var sent = 0
            while sent < raw.count {
                let w = write(fd, raw.baseAddress! + sent, raw.count - sent)
                if w < 0 && errno == EINTR { continue }
                if w <= 0 { return false }
                sent += w
            }
            return true
        }
    }
}
