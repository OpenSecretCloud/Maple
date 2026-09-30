import CryptoKit
import Foundation
import Security
import Testing
@testable import MapleSDK

/// Opt-in anonymous development-enclave checks. Never records the document,
/// session identifiers, keys, or other captures. No user or inference call occurs.
struct LiveAttestationTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["MAPLE_RUN_LIVE_ATTESTATION"] == "1"))
    func developmentNitroAndSignedHistory() async throws {
        let url = URL(string: "https://enclave.secretgpt.ai/v2/session")!
        var challenge = Data(count: 32)
        let randomResult = challenge.withUnsafeMutableBytes {
            SecRandomCopyBytes(kSecRandomDefault, $0.count, $0.baseAddress!)
        }
        try #require(randomResult == errSecSuccess)
        let key = Curve25519.KeyAgreement.PrivateKey()
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue(challenge.base64EncodedString(), forHTTPHeaderField: "x-opensecret-routing-key")
        request.httpBody = try JSONSerialization.data(withJSONObject: [
            "version": 2,
            "challenge": challenge.base64EncodedString(),
            "client_public_key": key.publicKey.rawRepresentation.base64EncodedString(),
        ])
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 20
        configuration.timeoutIntervalForResource = 20
        let session = URLSession(configuration: configuration, delegate: LiveNoRedirect(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        let (bytes, response) = try await session.bytes(for: request)
        let http = try #require(response as? HTTPURLResponse)
        try #require(http.statusCode == 200)
        try #require(http.url == url)
        var data = Data()
        for try await byte in bytes {
            try #require(data.count < 128 * 1_024)
            data.append(byte)
        }
        struct SessionResponse: Decodable {
            let attestation_document: String; let version: Int; let expires_in_seconds: Int
        }
        let envelope = try JSONDecoder().decode(SessionResponse.self, from: data)
        try #require(envelope.version == 2 && envelope.expires_in_seconds > 30)
        let verifier = AttestationVerifier()
        let now = Date()
        let document = try verifier.verify(base64: envelope.attestation_document, nonce: challenge, deviceTime: now)
        try #require(document.publicKey?.count == 32)
        let clientBindingMatches =
            document.userData == Data("opensecret/transport-v2/session/v1/client-public-key".utf8) + Data([0])
            + key.publicKey.rawRepresentation
        #expect(clientBindingMatches)
        let policy = try PCRTrustPolicy(environment: .development)
        let hash = try await policy.requireTrustedPCR0(document.pcrs[0])
        // Fetch even when embedded roots match, so this checks the live signed
        // history and pinned signature in addition to the ordinary fast path.
        let history = try await SignedPCRHistory.fetch(PCRTrustPolicy.historyURL(.development))
        #expect(try SignedPCRHistory.verify(hash: hash, data: history))
        for interval in [-86_400.0 * 3, 86_400.0 * 3] {
            #expect(throws: AttestationClockSkewError.self) {
                try verifier.verify(
                    base64: envelope.attestation_document, nonce: challenge,
                    deviceTime: now.addingTimeInterval(interval))
            }
        }
    }
}

private final class LiveNoRedirect: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}
