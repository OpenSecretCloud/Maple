import Foundation
import Testing
@testable import MapleSDK

@Suite struct NativeHandoffBoundaryTests {
    @Test(arguments: [FixtureNetwork.Failure.hint, .network, .tamper, .truncated])
    func failedNativeRedemptionNeverRetriesAndRetiresSession(_ failure: FixtureNetwork.Failure) async throws {
        let network = FixtureNetwork([failure])
        let transport = try await EncryptedTransportTests().make(network)
        let prepared = try await transport.prepareNativeOAuthHandoff()
        let grant = try NativeOAuthHandoffGrant(validating: "e30.e30.YQ")

        await #expect(throws: (any Error).self) {
            try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        #expect(await network.handshakes == 1)
        #expect(await network.observations.count == 1)
        #expect(try await transport.credentials.authenticatedUserId() == nil)
        await #expect(throws: TransportError.invalidSession) {
            try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        #expect(await network.observations.count == 1)

        let replacement = try await transport.prepareNativeOAuthHandoff()
        #expect(await network.handshakes == 2)
        #expect(replacement.sessionID != prepared.sessionID)
        #expect(replacement.requestID != prepared.requestID)
        let confirmation = try await transport.redeemNativeOAuthHandoff(replacement, grant: grant)
        #expect(confirmation.principalID == "a")
        #expect(try await transport.credentials.authenticatedUserId() == nil)
    }

    @Test(arguments: [false, true])
    func replacedOrCancelledSessionRejectsPreparedHandoffBeforeDispatch(cancel: Bool) async throws {
        let network = FixtureNetwork()
        let transport = try await EncryptedTransportTests().make(network)
        let prepared = try await transport.prepareNativeOAuthHandoff()
        if cancel {
            try await transport.cancelNativeOAuthHandoff()
        } else {
            try await transport.invalidateSession()
        }
        let replacement = try await transport.prepareNativeOAuthHandoff()
        #expect(replacement.sessionID != prepared.sessionID)
        let grant = try NativeOAuthHandoffGrant(validating: "e30.e30.YQ")
        await #expect(throws: TransportError.invalidSession) {
            try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        #expect(await network.handshakes == 2)
        #expect(await network.observations.isEmpty)
        #expect(try await transport.credentials.authenticatedUserId() == nil)

        let confirmation = try await transport.redeemNativeOAuthHandoff(replacement, grant: grant)
        #expect(confirmation.principalID == "a")
        #expect(await network.observations.count == 1)
    }

    @Test(arguments: ["a", "replacement-account"])
    func nativeRedemptionRejectsAuthorityChangedInFlight(principal: String) async throws {
        let network = DelayedNativeHandoffNetwork(), store = TestSecureStore()
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:3000", store: store)
        let transport = try EncryptedTransport(
            apiURL: "http://127.0.0.1:3000", policy: PCRTrustPolicy(environment: .development),
            credentials: authority, store: store, network: network, now: Date.init)
        let prepared = try await transport.prepareNativeOAuthHandoff()
        let grant = try NativeOAuthHandoffGrant(validating: "e30.e30.YQ")
        let redemption = Task { try await transport.redeemNativeOAuthHandoff(prepared, grant: grant) }
        await network.waitForRedemption()

        let replacement: CredentialSnapshot
        do {
            replacement = try await authority.install(
                accessToken: fixtureJWT(principal: principal, kind: "access"),
                refreshToken: fixtureJWT(principal: principal, kind: "refresh"), expected: authority.snapshot())
        } catch {
            await network.releaseRedemption()
            redemption.cancel()
            throw error
        }
        await network.releaseRedemption()
        await #expect(throws: TransportError.authorityChanged) { try await redemption.value }
        #expect(try await authority.snapshot() == replacement)
        #expect(try await authority.authenticatedUserId() == principal)
        #expect(await network.underlying.observations.count == 1)
        #expect(await network.underlying.handshakes == 1)

        // Returning to anonymous state must use new keys after the rejected response.
        try await authority.clear(expected: replacement)
        let next = try await transport.prepareNativeOAuthHandoff()
        #expect(next.sessionID != prepared.sessionID)
        #expect(await network.underlying.handshakes == 2)
    }
}

/// Hold the encrypted reply after the backend fixture observes redemption, so
/// credential replacement is ordered explicitly without timing-based sleeps.
private actor DelayedNativeHandoffNetwork: HTTPNetworking {
    let underlying = FixtureNetwork()
    private var observedRedemption = false
    private var arrival: CheckedContinuation<Void, Never>?
    private var release: CheckedContinuation<Void, Never>?

    func open(_ request: URLRequest) async throws -> HTTPConnection {
        let response = try await underlying.open(request)
        if request.url?.path == "/v2/request" {
            observedRedemption = true
            arrival?.resume()
            arrival = nil
            await withCheckedContinuation { release = $0 }
        }
        return response
    }

    func waitForRedemption() async {
        if observedRedemption { return }
        await withCheckedContinuation { arrival = $0 }
    }

    func releaseRedemption() {
        release?.resume()
        release = nil
    }
}
