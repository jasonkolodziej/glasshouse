import CryptoKit
import Foundation

/// ssh-agent protocol, RFC 9987 (May 2026). Glasshouse implements the minimum a signing-only,
/// hardware-backed agent needs and refuses everything else (no key import, no locking, no removal
/// over the socket — keys are managed in the app).
public enum AgentMessage {
    public static let failure: UInt8 = 5
    public static let success: UInt8 = 6
    public static let requestIdentities: UInt8 = 11
    public static let identitiesAnswer: UInt8 = 12
    public static let signRequest: UInt8 = 13
    public static let signResponse: UInt8 = 14
    public static let `extension`: UInt8 = 27
    public static let extensionFailure: UInt8 = 28
}

public enum AgentRequest: Equatable, Sendable {
    case requestIdentities
    case sign(keyBlob: Data, data: Data, flags: UInt32)
    /// OpenSSH `session-bind@openssh.com` (PROTOCOL.agent, not in RFC 9987).
    case sessionBind(SessionBind)
    case unknownExtension(name: String)
    case unsupported(type: UInt8)

    /// Parses one message body (the bytes after the uint32 length prefix).
    public static func parse(_ body: Data) throws -> AgentRequest {
        var r = SSHReader(body)
        let type = try r.byte()
        switch type {
        case AgentMessage.requestIdentities:
            return .requestIdentities
        case AgentMessage.signRequest:
            let blob = try r.string()
            let data = try r.string()
            let flags = try r.uint32()
            return .sign(keyBlob: Data(blob), data: Data(data), flags: flags)
        case AgentMessage.extension:
            let name = try r.utf8String()
            if name == "session-bind@openssh.com" {
                return .sessionBind(SessionBind(hostKey: Data(try r.string()),
                                                sessionID: Data(try r.string()),
                                                signature: Data(try r.string()),
                                                isForwarding: try r.bool()))
            }
            return .unknownExtension(name: name)
        default:
            return .unsupported(type: type)
        }
    }
}

/// What ssh tells the agent about the connection a signature is for.
/// SECURITY: `hostKey` is display-only until `signature` (over `sessionID`) has been verified with it.
/// Verification is a TODO in this scaffold (needs ed25519/ecdsa/rsa host-key verification); until then
/// Glasshouse uses it only to refuse forwarded requests and to label the prompt as "unverified host".
public struct SessionBind: Equatable, Sendable {
    public var hostKey: Data
    public var sessionID: Data
    public var signature: Data
    public var isForwarding: Bool
    public var verified = false

    /// SHA256 fingerprint in OpenSSH format, for matching against known_hosts.
    public var hostKeyFingerprint: String { SSHKeyFormat.fingerprint(of: hostKey) }
}

public enum AgentResponse {
    public static var failure: Data { frame { $0.byte(AgentMessage.failure) } }
    public static var success: Data { frame { $0.byte(AgentMessage.success) } }
    public static var extensionFailure: Data { frame { $0.byte(AgentMessage.extensionFailure) } }

    public static func identities(_ ids: [(blob: Data, comment: String)]) -> Data {
        frame { w in
            w.byte(AgentMessage.identitiesAnswer)
            w.uint32(UInt32(ids.count))
            for id in ids { w.string(id.blob); w.string(id.comment) }
        }
    }

    public static func signature(_ sshSignature: Data) -> Data {
        frame { w in w.byte(AgentMessage.signResponse); w.string(sshSignature) }
    }

    /// Prefixes the uint32 length.
    static func frame(_ build: (inout SSHWriter) -> Void) -> Data {
        var body = SSHWriter(); build(&body)
        var out = SSHWriter(); out.string(body.data)
        return out.data
    }
}

/// ecdsa-sha2-nistp256 encodings (RFC 5656 §3.1, §3.1.2).
public enum SSHKeyFormat {
    public static let ecdsaP256 = "ecdsa-sha2-nistp256"

    /// string "ecdsa-sha2-nistp256" ‖ string "nistp256" ‖ string Q (uncompressed point 0x04‖X‖Y)
    public static func publicKeyBlob(_ key: P256.Signing.PublicKey) -> Data {
        var w = SSHWriter()
        w.string(ecdsaP256)
        w.string("nistp256")
        w.string(key.x963Representation)
        return w.data
    }

    /// string "ecdsa-sha2-nistp256" ‖ string( mpint r ‖ mpint s )
    /// CryptoKit's `rawRepresentation` is r‖s, 32 bytes each, unsigned big-endian.
    public static func signatureBlob(_ sig: P256.Signing.ECDSASignature) -> Data {
        let raw = sig.rawRepresentation
        var inner = SSHWriter()
        inner.mpint(unsigned: raw.prefix(32))
        inner.mpint(unsigned: raw.suffix(32))
        var w = SSHWriter()
        w.string(ecdsaP256)
        w.string(inner.data)
        return w.data
    }

    /// `ecdsa-sha2-nistp256 AAAA… comment` for authorized_keys.
    public static func authorizedKeysLine(_ key: P256.Signing.PublicKey, comment: String) -> String {
        "\(ecdsaP256) \(publicKeyBlob(key).base64EncodedString()) \(comment)"
    }

    public static func fingerprint(of blob: Data) -> String {
        let digest = SHA256.hash(data: blob)
        let b64 = Data(digest).base64EncodedString().replacingOccurrences(of: "=", with: "")
        return "SHA256:" + b64
    }
}
