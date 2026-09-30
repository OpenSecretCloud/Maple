import Foundation
import Testing
@testable import MapleSDK

@Suite struct InferenceBudgetHeaderTests {
    private func transport(_ network: FixtureNetwork) throws -> EncryptedTransport {
        let store = TestSecureStore()
        let credentials = try CredentialAuthority(apiURL: "http://127.0.0.1:3000", store: store)
        return try EncryptedTransport(
            apiURL: "http://127.0.0.1:3000", policy: PCRTrustPolicy(environment: .development),
            credentials: credentials, store: store, network: network, now: Date.init)
    }

    @Test(arguments: [
        "X-OpenSecret-Client-Inference-Send-Limit", "X-OPENSECRET-CLIENT-INFERENCE-SEND-LIMIT",
        openSecretInferenceSendLimitHeader,
    ])
    func caseInsensitiveBudgetIsEnforcedAndNeverForwarded(_ header: String) async throws {
        let network = FixtureNetwork([.hint])
        let client = try transport(network)
        let request = MapleRequest(method: .post, target: "/v1/responses", headers: [header: "1"], auth: .none)
        await #expect(throws: TransportError.untrustedRecoveryHint("session_not_found")) {
            try await client.send(request)
        }
        let observations = await network.observations
        #expect(observations.count == 1)
        #expect(
            observations.allSatisfy { observation in
                !observation.headers.contains {
                    $0.caseInsensitiveCompare(openSecretInferenceSendLimitHeader) == .orderedSame
                }
            })
    }

    @Test(arguments: ["1", "2"])
    func caseDuplicateBudgetHeadersRejectBeforeDispatch(_ second: String) async throws {
        let network = FixtureNetwork()
        let client = try transport(network)
        let request = MapleRequest(
            method: .post, target: "/v1/responses",
            headers: [openSecretInferenceSendLimitHeader: "1", "X-OpenSecret-Client-Inference-Send-Limit": second],
            auth: .none)
        await #expect(throws: TransportError.invalidEnvelope) { try await client.send(request) }
        #expect(await network.observations.isEmpty)
        #expect(await network.handshakes == 0)
    }
}
