import CryptoKit
import Foundation
import LocalAuthentication
import Security

/// A Secure Enclave key the agent serves. The private key never exists outside the Secure Enclave;
/// what we persist is CryptoKit's `dataRepresentation` — an opaque blob only *this* Mac's SE can use.
public struct AgentKey: Identifiable, Sendable, Equatable {
    public let id: String            // keychain account
    public var comment: String
    public var policy: KeyPolicy
    public let publicKeyBlob: Data
    public var fingerprint: String { SSHKeyFormat.fingerprint(of: publicKeyBlob) }
}

public struct KeyPolicy: Codable, Sendable, Equatable {
    /// `.biometryCurrentSet` (true): re-enrolling a finger invalidates the key — strictest.
    /// `.userPresence` (false): Touch ID, Apple Watch or login password.
    public var requireBiometryCurrentSet: Bool = false
    /// Refuse requests that arrive through agent forwarding (session-bind is_forwarding = true).
    public var allowForwarded: Bool = false
    /// Host glob patterns this key may be used for; empty = any host.
    public var allowedHosts: [String] = []

    public init(requireBiometryCurrentSet: Bool = false, allowForwarded: Bool = false, allowedHosts: [String] = []) {
        self.requireBiometryCurrentSet = requireBiometryCurrentSet
        self.allowForwarded = allowForwarded
        self.allowedHosts = allowedHosts
    }
}

public enum KeyStoreError: Error { case secureEnclaveUnavailable, keychain(OSStatus), accessControl, notFound }

/// Stores key blobs as generic-password items in the **data protection keychain** (TN3137).
/// Requires the `keychain-access-groups` entitlement backed by a provisioning profile, shared by the
/// app (which creates keys) and the agent (which signs) — otherwise calls fail with -34018.
public struct SecureEnclaveKeyStore: Sendable {
    public let accessGroup: String
    static let service = "dev.glasshouse.agent.keys"

    public init(accessGroup: String) { self.accessGroup = accessGroup }

    // MARK: Create

    public func createKey(comment: String, policy: KeyPolicy = KeyPolicy()) throws -> AgentKey {
        guard SecureEnclave.isAvailable else { throw KeyStoreError.secureEnclaveUnavailable }
        var flags: SecAccessControlCreateFlags = [.privateKeyUsage]
        flags.insert(policy.requireBiometryCurrentSet ? .biometryCurrentSet : .userPresence)
        guard let access = SecAccessControlCreateWithFlags(
            nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, flags, nil) else {
            throw KeyStoreError.accessControl
        }
        let key = try SecureEnclave.P256.Signing.PrivateKey(compactRepresentable: false, accessControl: access)
        let id = UUID().uuidString
        let publicBlob = SSHKeyFormat.publicKeyBlob(key.publicKey)
        let meta = try JSONEncoder().encode(Metadata(comment: comment, policy: policy, publicKeyBlob: publicBlob))

        var query = baseQuery(account: id)
        query[kSecValueData as String] = key.dataRepresentation
        query[kSecAttrLabel as String] = "Glasshouse SSH key: \(comment)"
        query[kSecAttrGeneric as String] = meta
        query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeyStoreError.keychain(status) }

        return AgentKey(id: id, comment: comment, policy: policy, publicKeyBlob: publicBlob)
    }

    // MARK: Read

    /// Lists keys from keychain attributes only — no key data is read and the Secure Enclave is never
    /// touched, so listing (ssh-add -L, REQUEST_IDENTITIES) never triggers a Touch ID prompt.
    /// The public key blob is cached in the item's metadata at creation time.
    public func listKeys() throws -> [AgentKey] {
        var query = baseQuery(account: nil)
        query[kSecMatchLimit as String] = kSecMatchLimitAll
        query[kSecReturnAttributes as String] = true
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return [] }
        guard status == errSecSuccess, let items = result as? [[String: Any]] else {
            throw KeyStoreError.keychain(status)
        }
        return items.compactMap { item in
            guard let id = item[kSecAttrAccount as String] as? String,
                  let metaData = item[kSecAttrGeneric as String] as? Data,
                  let meta = try? JSONDecoder().decode(Metadata.self, from: metaData)
            else { return nil }
            return AgentKey(id: id, comment: meta.comment, policy: meta.policy, publicKeyBlob: meta.publicKeyBlob)
        }
    }

    /// Loads the signing key bound to `context`, whose `localizedReason` becomes the Touch ID prompt text.
    public func signingKey(id: String, context: LAContext) throws -> SecureEnclave.P256.Signing.PrivateKey {
        var query = baseQuery(account: id)
        query[kSecReturnData as String] = true
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let blob = result as? Data else {
            throw status == errSecItemNotFound ? KeyStoreError.notFound : KeyStoreError.keychain(status)
        }
        return try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob, authenticationContext: context)
    }

    public func deleteKey(id: String) throws {
        let status = SecItemDelete(baseQuery(account: id) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeyStoreError.keychain(status) }
    }

    // MARK: Helpers

    private struct Metadata: Codable { var comment: String; var policy: KeyPolicy; var publicKeyBlob: Data }

    private func baseQuery(account: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccessGroup as String: accessGroup,
            kSecUseDataProtectionKeychain as String: true,
        ]
        if let account { q[kSecAttrAccount as String] = account }
        return q
    }
}
