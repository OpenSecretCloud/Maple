import CryptoKit
import Foundation
@testable import MapleSDK

actor FixtureNetwork: HTTPNetworking {
    enum Failure: Sendable { case hint, generic400, tamper, truncated, extra, secondStart, network }
    struct Observation: Sendable {
        let target: String
        let credentialKind: String?
        let cacheRoot: String?
        let requestID: String
        let headers: [String]
        let bodyPresent: Bool
    }
    private var sessions: [String: SessionKeys] = [:]
    private(set) var handshakes = 0
    private(set) var observations: [Observation] = []
    private var failures: [Failure]
    private var responseStatus = 200
    private var expireNext = false
    init(_ failures: [Failure] = []) { self.failures = failures }
    func setStatus(_ status: Int) { responseStatus = status }
    func expireNextAccess() { expireNext = true }

    func open(_ request: URLRequest) async throws -> HTTPConnection {
        if request.url?.path == "/v2/session" { return try handshake(request) }
        guard let id = request.value(forHTTPHeaderField: "x-session-id"), let keys = sessions[id],
            let body = request.httpBody, body.count >= 32
        else { throw TransportError.invalidSession }
        let requestID = Data(body.prefix(16))
        let key = try TransportCrypto.subkey(
            keys.request, label: "request-subkey/v1", session: keys.id, request: requestID)
        let box = try ChaChaPoly.SealedBox(
            nonce: ChaChaPoly.Nonce(data: Data(count: 12)),
            ciphertext: body.dropFirst(16).dropLast(16), tag: body.suffix(16))
        let plaintext = try ChaChaPoly.open(
            box, using: key,
            authenticating: TransportCrypto.domain("request-record/v1") + keys.id + requestID)
        let length = plaintext.prefix(4).reduce(0) { $0 * 256 + Int($1) }
        let metadata = try JSONSerialization.jsonObject(with: plaintext.subdata(in: 4..<(4 + length))) as! [String: Any]
        let target = metadata["target"] as! String
        observations.append(
            Observation(
                target: target, credentialKind: (metadata["credential"] as? [String: String])?["kind"],
                cacheRoot: metadata["cache_namespace_root"] as? String, requestID: requestID.lowercaseHex,
                headers: (metadata["headers"] as! [[String: String]]).compactMap { $0["name"] },
                bodyPresent: metadata["body_present"] as! Bool))
        let failure = failures.isEmpty ? nil : failures.removeFirst()
        if failure == .network { throw URLError(.networkConnectionLost) }
        if failure == .hint {
            return connection(
                status: 400,
                headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "session_not_found"],
                data: Data())
        }
        if failure == .generic400 { return connection(status: 400, headers: [:], data: Data()) }
        let payload: Data
        if target == "/refresh" || target == "/auth/native-handoff/redeem" || target == "/auth/google/callback" {
            payload = try JSONSerialization.data(withJSONObject: [
                "id": "a",
                "access_token": fixtureJWT(principal: "a", kind: "access"),
                "refresh_token": fixtureJWT(principal: "a", kind: "refresh"),
            ])
        } else {
            payload = Data("{\"ok\":true}".utf8)
        }
        let expired = expireNext && target != "/refresh"
        if expired { expireNext = false }
        let responseHeaders =
            expired
            ? [
                ["name": "x-opensecret-error-contract", "value": "1"],
                ["name": "x-opensecret-error-code", "value": "access_token_expired"],
            ] : []
        let start =
            Data([1])
            + (try JSONSerialization.data(withJSONObject: [
                "status": expired ? 401 : responseStatus, "headers": responseHeaders,
            ]))
        var records = [start, Data([2]) + payload, Data([3])]
        if failure == .secondStart { records.insert(start, at: 1) }
        var framed = Data()
        for (index, record) in records.enumerated() {
            let cipher = try sealResponse(record, keys: keys, request: requestID, sequence: UInt64(index))
            framed += UInt32(cipher.count).bigEndianData + cipher
        }
        if failure == .truncated { framed.removeLast(5) }
        if failure == .tamper { framed[framed.count - 1] ^= 1 }
        if failure == .extra { framed.append(0) }
        return connection(status: 200, headers: ["content-type": "application/octet-stream"], data: framed)
    }

    private func handshake(_ request: URLRequest) throws -> HTTPConnection {
        handshakes += 1
        let fields = try JSONSerialization.jsonObject(with: request.httpBody!) as! [String: Any]
        let challenge = Data(base64Encoded: fields["challenge"] as! String)!
        let client = Data(base64Encoded: fields["client_public_key"] as! String)!
        let secret = Curve25519.KeyAgreement.PrivateKey()
        let server = secret.publicKey.rawRepresentation
        let shared = try secret.sharedSecretFromKeyAgreement(
            with: Curve25519.KeyAgreement.PublicKey(rawRepresentation: client))
        let keys = try TransportCrypto.derive(
            shared: shared.withUnsafeBytes { Data($0) }, challenge: challenge, client: client, server: server)
        sessions[keys.id.lowercaseHex] = keys
        let payload = TrustCBOR.map([
            .init(.text("nonce"), .bytes(challenge)), .init(.text("public_key"), .bytes(server)),
            .init(.text("user_data"), .bytes(TransportCrypto.domain("session/v1/client-public-key") + client)),
        ]).encode()
        let document = TrustCBOR.array([.bytes(Data()), .map([]), .bytes(payload), .bytes(Data())]).encode()
        let json = try JSONSerialization.data(withJSONObject: [
            "version": 2, "session_id": keys.id.lowercaseHex,
            "expires_in_seconds": 3600, "attestation_document": document.base64EncodedString(),
        ])
        return connection(status: 200, headers: ["content-type": "application/json"], data: json)
    }
}

func sealResponse(_ data: Data, keys: SessionKeys, request: Data, sequence: UInt64) throws -> Data {
    let key = try TransportCrypto.subkey(keys.response, label: "response-subkey/v1", session: keys.id, request: request)
    let box = try ChaChaPoly.seal(
        data, using: key,
        nonce: ChaChaPoly.Nonce(data: Data(count: 4) + sequence.bigEndianData),
        authenticating: TransportCrypto.domain("response-record/v1") + keys.id + request + sequence.bigEndianData)
    return box.ciphertext + box.tag
}

func connection(status: Int, headers: [String: String], data: Data) -> HTTPConnection {
    let source = FixtureBytes(data)
    return HTTPConnection(status: status, headers: headers, read: { await source.read($0) }, cancel: {})
}

private actor FixtureBytes {
    var data: Data
    init(_ data: Data) { self.data = data }
    func read(_ maximum: Int) -> Data {
        let size = min(maximum, data.count)
        defer { data = Data(data.dropFirst(size)) }
        return Data(data.prefix(size))
    }
}
