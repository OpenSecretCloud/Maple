import Foundation
import Testing
@testable import MapleSDK

@Suite(.enabled(if: ProcessInfo.processInfo.environment["MAPLE_SWIFT_DEV_INTEGRATION"] == "1"))
struct LiveTransportTests {
    @Test func devSessionRenewalAndActualMissingSessionRecovery() async throws {
        let url = "https://enclave.secretgpt.ai"
        let storage = TestSecureStore(), network = DevRecoveryNetwork()
        let policy = try PCRTrustPolicy(environment: .development)
        let authority = try CredentialAuthority(apiURL: url, store: storage)
        func makeTransport() throws -> EncryptedTransport {
            try EncryptedTransport(
                apiURL: url, policy: policy, credentials: authority,
                store: storage, network: network, now: Date.init)
        }
        func consume(_ transport: EncryptedTransport) async throws {
            let response = try await transport.send(MapleRequest(method: .get, target: "/v1/models", auth: .none))
            #expect([200, 401].contains(response.status))
            for try await _ in response.body {}
        }
        let first = try makeTransport()
        // Send one anonymous request with an unknown outer session identifier.
        // The actual enclave must return its exact, unauthenticated recovery hint;
        // the retry then establishes fresh verified keys before sending again.
        try await consume(first)
        #expect(await network.handshakes == 2)
        #expect(await network.requests == 2)
        #expect(await network.receivedMissingSessionHint)
        let storageKey = "session:" + hashSecret(url + "\n" + policy.cacheIdentity)
        let storedData = try #require(try storage.read(key: storageKey))
        let saved = try JSONDecoder().decode(AttestedSession.self, from: storedData)
        let expired = AttestedSession(
            version: saved.version, apiURL: saved.apiURL, policyIdentity: saved.policyIdentity,
            keys: saved.keys, routingKey: saved.routingKey, expiresAt: .distantPast)
        try storage.write(JSONEncoder().encode(expired), key: storageKey)
        try await consume(makeTransport())
        #expect(await network.handshakes == 3)
        #expect(await network.requests == 3)
    }

    @Test func devEnclaveEncryptsAndAuthenticatesLogicalResponses() async throws {
        let storage = TestSecureStore()
        let client = try MapleClient(
            apiURL: "https://enclave.secretgpt.ai",
            clientID: "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6", policy: PCRTrustPolicy(environment: .development),
            store: storage)
        // An anonymous read proves real attestation, X25519, request sealing,
        // incremental decryption and authenticated finality without creating accounts.
        let response = try await client.transport.send(MapleRequest(method: .get, target: "/v1/models", auth: .none))
        #expect([200, 401].contains(response.status))
        var count = 0
        for try await chunk in response.body {
            count += chunk.count
            #expect(count <= 1024 * 1024)
        }
        #expect(count > 0)
        // Reserve a real attested native handoff without minting or redeeming a grant.
        let prepared = try await client.transport.prepareNativeOAuthHandoff()
        #expect(prepared.sessionID.count == 32 && prepared.requestID.count == 32)
        await client.transport.cancelNativeOAuthHandoff()
    }
}

private actor DevRecoveryNetwork: HTTPNetworking {
    let underlying = URLSessionNetworking()
    private(set) var handshakes = 0
    private(set) var requests = 0
    private(set) var receivedMissingSessionHint = false
    func open(_ request: URLRequest) async throws -> HTTPConnection {
        if request.url?.path == "/v2/session" { handshakes += 1 }
        var request = request
        if request.url?.path == "/v2/request" {
            requests += 1
            if requests == 1 { request.setValue(String(repeating: "0", count: 32), forHTTPHeaderField: "x-session-id") }
        }
        let response = try await underlying.open(request)
        if requests == 1, request.url?.path == "/v2/request" {
            receivedMissingSessionHint =
                response.status == 400
                && response.headers["x-opensecret-error-contract"] == "1"
                && response.headers["x-opensecret-error-code"] == "session_not_found"
        }
        return response
    }
}
