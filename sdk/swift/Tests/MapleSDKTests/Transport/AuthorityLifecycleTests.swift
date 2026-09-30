import Foundation
import Testing
@testable import MapleSDK

@Suite struct AuthorityLifecycleTests {
    @Test func transportRejectsOldAccountLeaseBeforeDispatchEvenForSamePrincipal() async throws {
        let helper = EncryptedTransportTests(), network = FixtureNetwork()
        let transport = try await helper.make(network, signedIn: true)
        let oldScreen = try await transport.credentials.snapshot()
        try await transport.credentials.clear()
        _ = try await transport.credentials.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: transport.credentials.snapshot())
        await #expect(throws: TransportError.authorityChanged) {
            try await transport.send(
                MapleRequest(
                    method: .delete, target: "/v1/conversations/example",
                    expectedPrincipalId: "a"), expectedAccount: oldScreen)
        }
        #expect(await network.handshakes == 0)
        #expect(await network.observations.isEmpty)
    }

    @Test func sameKeychainNamespaceSharesAuthorityAndFencesLateSignIn() async throws {
        let registry = ClientRegistry(), store = TestSecureStore()
        let policy = try PCRTrustPolicy(environment: .development)
        let (first, firstTransport) = try registry.components(
            apiURL: "https://EXAMPLE.test:443/", policy: policy,
            store: store, namespace: "test-keychain")
        let pendingSignIn = try await first.snapshot()
        let (second, secondTransport) = try registry.components(
            apiURL: "https://example.test", policy: policy,
            store: store, namespace: "test-keychain")
        #expect(first === second && firstTransport === secondTransport)
        let (otherPath, otherTransport) = try registry.components(
            apiURL: "https://example.test/other", policy: policy,
            store: store, namespace: "test-keychain")
        #expect(otherPath === first && otherTransport !== firstTransport)
        try await second.clear()
        await #expect(throws: TransportError.authorityChanged) {
            try await first.install(
                accessToken: fixtureJWT(principal: "a", kind: "access"),
                refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: pendingSignIn)
        }
    }

    @Test func canceledSignInCannotCommitTokens() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let snapshot = try await authority.snapshot()
        let attempt = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await authority.install(
                accessToken: fixtureJWT(principal: "a", kind: "access"),
                refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: snapshot)
        }
        await #expect(throws: CancellationError.self) { try await attempt.value }
        #expect(try await authority.snapshot() == snapshot)
    }

    @Test func accountDeletionClearsAcrossRefreshButCannotClearReplacement() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let a = try fixtureJWT(principal: "a", kind: "access"), r = try fixtureJWT(principal: "a", kind: "refresh")
        let original = try await authority.install(accessToken: a, refreshToken: r, expected: authority.snapshot())
        _ = try await authority.install(accessToken: a, refreshToken: r, expected: original, refreshing: true)
        try await authority.clearAccount(expected: original)
        _ = try await authority.install(accessToken: a, refreshToken: r, expected: authority.snapshot())
        await #expect(throws: TransportError.authorityChanged) { try await authority.clearAccount(expected: original) }
        #expect(try await authority.authenticatedUserId() == "a")
    }

    @Test func independentTransportsCoalesceRefreshForSharedAuthority() async throws {
        let network = FixtureNetwork(), store = TestSecureStore()
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:3000", store: store)
        _ = try await authority.install(
            accessToken: fixtureJWT(principal: "a", kind: "access", expiry: 1),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        func transport() throws -> EncryptedTransport {
            try EncryptedTransport(
                apiURL: "http://127.0.0.1:3000", policy: PCRTrustPolicy(environment: .development),
                credentials: authority, store: store, network: network, now: Date.init)
        }
        let first = try transport(), second = try transport()
        let request = MapleRequest(method: .get, target: "/protected/user")
        async let a = first.send(request)
        async let b = second.send(request)
        _ = try await (a, b)
        #expect(await network.observations.filter { $0.target == "/refresh" }.count == 1)
    }
}
