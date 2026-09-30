import Foundation
import Testing
@testable import MapleSDK

@Suite struct HandshakeBoundaryTests {
    @Test(arguments: ["https://localhost", "https://127.0.0.1", "https://[::1]"])
    func httpsLoopbackMustVerifyRealAttestation(_ endpoint: String) async throws {
        let store = TestSecureStore(), network = FixtureNetwork()
        let authority = try CredentialAuthority(apiURL: endpoint, store: store)
        let transport = try EncryptedTransport(
            apiURL: endpoint, policy: PCRTrustPolicy(environment: .development),
            credentials: authority, store: store, network: network, now: Date.init)
        #expect(try !APIEndpoint(endpoint).isLoopback)
        await #expect(throws: (any Error).self) {
            try await transport.send(MapleRequest(method: .get, target: "/test", auth: .none))
        }
        #expect(await network.handshakes == 1)
        #expect(await network.observations.isEmpty)
    }

    @Test func nativeAttemptAndConfirmationExpireAtFifteenMinutes() async throws {
        let clock = FixtureClock(), store = TestSecureStore(), network = FixtureNetwork()
        let authority = try CredentialAuthority(apiURL: "http://localhost", store: store)
        let transport = try EncryptedTransport(
            apiURL: "http://localhost", policy: PCRTrustPolicy(environment: .development),
            credentials: authority, store: store, network: network, now: { clock.now() })
        let prepared = try await transport.prepareNativeOAuthHandoff()
        #expect(prepared.expiresAt.timeIntervalSince(clock.now()) == 900)
        clock.advance(901)
        let grant = try NativeOAuthHandoffGrant(validating: "e30.e30.YQ")
        await #expect(throws: TransportError.invalidSession) {
            try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        #expect(await network.observations.isEmpty)
        let second = try await transport.prepareNativeOAuthHandoff()
        let confirmation = try await transport.redeemNativeOAuthHandoff(second, grant: grant)
        clock.advance(901)
        await #expect(throws: TransportError.authorityChanged) {
            try await transport.confirmNativeOAuthHandoff(confirmation)
        }
        #expect(try await authority.authenticatedUserId() == nil)
    }

    @Test func ordinaryHeadersAreCanonicalizedAndDuplicatesRejected() async throws {
        let helper = EncryptedTransportTests(), network = FixtureNetwork()
        let transport = try await helper.make(network)
        _ = try await helper.collect(
            transport.send(
                MapleRequest(
                    method: .post, target: "/test",
                    headers: ["Content-Type": "application/json"], auth: .none)))
        #expect(await network.observations.first?.headers == ["content-type"])
        await #expect(throws: TransportError.invalidEnvelope) {
            try await transport.send(
                MapleRequest(
                    method: .post, target: "/test",
                    headers: ["Content-Type": "application/json", "content-type": "text/plain"], auth: .none))
        }
        #expect(await network.observations.count == 1)
    }
}

private final class FixtureClock: @unchecked Sendable {
    private let lock = NSLock()
    private var date = Date()
    func now() -> Date { lock.lock(); defer { lock.unlock() }; return date }
    func advance(_ seconds: TimeInterval) { lock.lock(); defer { lock.unlock() }; date.addTimeInterval(seconds) }
}
