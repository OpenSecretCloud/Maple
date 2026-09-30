import Foundation

/// Keychain-backed clients share live authority for their entire process.
/// The lock protects synchronous construction only; network work stays actor-isolated.
final class ClientRegistry: @unchecked Sendable {
    static let shared = ClientRegistry()
    private let lock = NSLock()
    private var authorities: [String: CredentialAuthority] = [:]
    private var transports: [String: EncryptedTransport] = [:]

    func components(
        apiURL: String, policy: PCRTrustPolicy, store: any SecureStore,
        namespace: String
    ) throws -> (CredentialAuthority, EncryptedTransport) {
        lock.lock()
        defer { lock.unlock() }
        let endpoint = try APIEndpoint(apiURL)
        let authorityKey = hashSecret(namespace + "\n" + endpoint.origin)
        let authority: CredentialAuthority
        if let saved = authorities[authorityKey] {
            authority = saved
        } else {
            authority = try CredentialAuthority(apiURL: apiURL, store: store)
            authorities[authorityKey] = authority
        }
        let transportKey = hashSecret(namespace + "\n" + endpoint.url.absoluteString + "\n" + policy.cacheIdentity)
        let transport: EncryptedTransport
        if let saved = transports[transportKey] {
            transport = saved
        } else {
            transport = try EncryptedTransport(apiURL: apiURL, policy: policy, credentials: authority, store: store)
            transports[transportKey] = transport
        }
        return (authority, transport)
    }
}
