import CryptoKit
import Foundation
import Testing
@testable import SSHAgentCore

@Suite("ssh-agent wire protocol")
struct AgentProtocolTests {
    @Test func mpintEncoding() {
        #expect(SSHWriter.mpintBytes(Data([0x00, 0x00, 0x7F])) == Data([0x7F]))
        #expect(SSHWriter.mpintBytes(Data([0x80, 0x01])) == Data([0x00, 0x80, 0x01])) // keep positive
        #expect(SSHWriter.mpintBytes(Data([0x00, 0x00])) == Data())                    // zero = empty
    }

    @Test func publicKeyBlobLayout() throws {
        let key = P256.Signing.PrivateKey().publicKey
        var r = SSHReader(SSHKeyFormat.publicKeyBlob(key))
        #expect(try r.utf8String() == "ecdsa-sha2-nistp256")
        #expect(try r.utf8String() == "nistp256")
        let q = try r.string()
        #expect(q.count == 65 && q.first == 0x04)
        #expect(r.isAtEnd)
    }

    /// Software key stands in for the Secure Enclave: the encoding path is identical.
    @Test func signatureBlobRoundTripsAndVerifies() throws {
        let key = P256.Signing.PrivateKey()
        let message = Data("session-id || userauth request".utf8)
        let sig = try key.signature(for: message)

        var r = SSHReader(SSHKeyFormat.signatureBlob(sig))
        #expect(try r.utf8String() == "ecdsa-sha2-nistp256")
        var inner = SSHReader(try r.string())
        let rInt = Data(try inner.string()), sInt = Data(try inner.string())

        func pad32(_ d: Data) -> Data { let t = d.drop { $0 == 0 }; return Data(repeating: 0, count: 32 - t.count) + t }
        let raw = pad32(rInt) + pad32(sInt)
        let rebuilt = try P256.Signing.ECDSASignature(rawRepresentation: raw)
        #expect(key.publicKey.isValidSignature(rebuilt, for: message))
    }

    @Test func parsesSignRequest() throws {
        var w = SSHWriter()
        w.byte(AgentMessage.signRequest)
        w.string(Data([1, 2, 3]))
        w.string(Data("payload".utf8))
        w.uint32(0)
        #expect(try AgentRequest.parse(w.data) == .sign(keyBlob: Data([1, 2, 3]), data: Data("payload".utf8), flags: 0))
    }

    @Test func parsesSessionBindAndFlagsForwarding() throws {
        var w = SSHWriter()
        w.byte(AgentMessage.extension)
        w.string("session-bind@openssh.com")
        w.string(Data([9])); w.string(Data([8])); w.string(Data([7])); w.bool(true)
        guard case .sessionBind(let bind) = try AgentRequest.parse(w.data) else {
            Issue.record("expected session-bind"); return
        }
        #expect(bind.isForwarding && !bind.verified)
    }

    @Test func unknownExtensionAndTypes() throws {
        var w = SSHWriter(); w.byte(AgentMessage.extension); w.string("query")
        #expect(try AgentRequest.parse(w.data) == .unknownExtension(name: "query"))
        #expect(try AgentRequest.parse(Data([17])) == .unsupported(type: 17)) // ADD_IDENTITY refused
    }

    @Test func rejectsTruncatedAndOversized() {
        #expect(throws: SSHWireError.self) { try AgentRequest.parse(Data([13, 0, 0, 0, 9, 1])) }
        var w = SSHWriter(); w.byte(13); w.uint32(UInt32(SSHReader.maxString + 1))
        #expect(throws: SSHWireError.tooLarge) { try AgentRequest.parse(w.data) }
    }

    @Test func identitiesAnswerIsFramed() {
        let frame = AgentResponse.identities([(Data([1]), "k")])
        let len = frame.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
        #expect(len == frame.count - 4)
        #expect(frame[4] == AgentMessage.identitiesAnswer)
    }

    @Test func fingerprintFormat() {
        #expect(SSHKeyFormat.fingerprint(of: Data("x".utf8)).hasPrefix("SHA256:"))
        #expect(!SSHKeyFormat.fingerprint(of: Data("x".utf8)).hasSuffix("="))
    }
}
