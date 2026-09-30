import CryptoKit
import Foundation
import Security

/// Errors deliberately contain no input values, decrypted content, or credentials.
public enum TransportError: Error, Sendable, Equatable {
    case invalidEndpoint, invalidEnvelope, invalidSession, expiredSession
    case invalidFrame, authentication, truncated, postTerminal, remote(String)
    case untrustedRecoveryHint(String), unauthenticatedResponse
    case authorityChanged, invalidCredentials, signedOut, persistence, resourceLimit
    case requestRejected(status: Int), establishmentTimeout
}

enum TransportCrypto {
    static let prefix = "opensecret/transport-v2/"

    static func random(count: Int) throws -> Data {
        guard count > 0, count <= 50 * 1024 * 1024 else { throw TransportError.resourceLimit }
        var bytes = Data(count: count)
        let result = bytes.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, count, $0.baseAddress!) }
        guard result == errSecSuccess else { throw TransportError.authentication }
        return bytes
    }

    static func domain(_ label: String) -> Data { Data((prefix + label).utf8) + Data([0]) }

    static func derive(shared: Data, challenge: Data, client: Data, server: Data) throws -> SessionKeys {
        guard shared.count == 32, shared.contains(where: { $0 != 0 }),
            challenge.count == 32, client.count == 32, server.count == 32
        else {
            throw TransportError.authentication
        }
        let digest = Data(SHA256.hash(data: domain("session/v1") + challenge + client + server))
        func key(_ label: String, _ count: Int) -> Data {
            HKDF<SHA256>.deriveKey(
                inputKeyMaterial: SymmetricKey(data: shared), salt: challenge,
                info: domain(label) + digest, outputByteCount: count
            )
            .withUnsafeBytes { Data($0) }
        }
        return SessionKeys(
            id: key("session-id/v1", 16), request: key("request-key/v1", 32),
            response: key("response-key/v1", 32))
    }

    static func subkey(_ key: Data, label: String, session: Data, request: Data) throws -> SymmetricKey {
        guard key.count == 32, session.count == 16, request.count == 16 else { throw TransportError.invalidSession }
        let info = domain(label) + session + request + Data([1])
        return SymmetricKey(data: Data(HMAC<SHA256>.authenticationCode(for: info, using: SymmetricKey(data: key))))
    }

    static func sealRequest(_ plaintext: Data, keys: SessionKeys, request: Data) throws -> Data {
        let key = try subkey(keys.request, label: "request-subkey/v1", session: keys.id, request: request)
        let box = try ChaChaPoly.seal(
            plaintext, using: key, nonce: ChaChaPoly.Nonce(data: Data(count: 12)),
            authenticating: domain("request-record/v1") + keys.id + request)
        return request + box.ciphertext + box.tag
    }

    static func openResponse(_ ciphertext: Data, keys: SessionKeys, request: Data, sequence: UInt64) throws -> Data {
        guard ciphertext.count >= 16 else { throw TransportError.invalidFrame }
        let key = try subkey(keys.response, label: "response-subkey/v1", session: keys.id, request: request)
        do {
            let box = try ChaChaPoly.SealedBox(
                nonce: ChaChaPoly.Nonce(data: Data(count: 4) + sequence.bigEndianData),
                ciphertext: ciphertext.dropLast(16), tag: ciphertext.suffix(16))
            return try ChaChaPoly.open(
                box, using: key,
                authenticating: domain("response-record/v1") + keys.id + request + sequence.bigEndianData)
        } catch { throw TransportError.authentication }
    }
}

struct SessionKeys: Sendable, Codable {
    let id: Data
    let request: Data
    let response: Data
}

extension FixedWidthInteger {
    var bigEndianData: Data {
        var value = bigEndian
        return withUnsafeBytes(of: &value) { Data($0) }
    }
}

extension Data {
    var lowercaseHex: String { map { String(format: "%02x", $0) }.joined() }
    init?(canonicalBase64 value: String, count: Int) {
        guard value.utf8.count <= ((count + 2) / 3) * 4,
            let bytes = Data(base64Encoded: value), bytes.count == count,
            bytes.base64EncodedString() == value
        else { return nil }
        self = bytes
    }
    func constantTimeEquals(_ other: Data) -> Bool {
        guard count == other.count else { return false }
        return zip(self, other).reduce(UInt8(0)) { $0 | ($1.0 ^ $1.1) } == 0
    }
}

/// Generates the password-reset ceremony's secure random secret as lowercase hex.
public func generateSecureSecret() throws -> String { try TransportCrypto.random(count: 32).lowercaseHex }

/// SHA-256 of the UTF-8 secret, matching the other Maple SDKs.
public func hashSecret(_ secret: String) -> String { Data(SHA256.hash(data: Data(secret.utf8))).lowercaseHex }
