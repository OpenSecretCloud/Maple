import Foundation
import Testing
@testable import MapleSDK

@Suite struct EncryptedTransportTests {
    @Test func expiryRefreshRequiresAuthenticatedFinalityAndNeverReplays() async throws {
        let truncated = FixtureNetwork([.truncated]), first = try await make(truncated, signedIn: true)
        await truncated.expireNextAccess()
        await #expect(throws: TransportError.truncated) {
            try await first.send(MapleRequest(method: .post, target: "/v1/responses"))
        }
        #expect(await truncated.observations.map(\.target) == ["/v1/responses"])
        let valid = FixtureNetwork(), second = try await make(valid, signedIn: true)
        await valid.expireNextAccess()
        let response = try await second.send(MapleRequest(method: .post, target: "/v1/responses"))
        #expect(response.status == 401)
        _ = try await collect(response)
        #expect(await valid.observations.map(\.target) == ["/v1/responses", "/refresh"])
    }

    func make(
        _ network: FixtureNetwork, store: TestSecureStore = TestSecureStore(), signedIn: Bool = false,
        expired: Bool = false
    ) async throws -> EncryptedTransport {
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:3000", store: store)
        if signedIn {
            _ = try await authority.install(
                accessToken: fixtureJWT(principal: "a", kind: "access", expiry: expired ? 1 : 4_000_000_000),
                refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        }
        return try EncryptedTransport(
            apiURL: "http://127.0.0.1:3000", policy: PCRTrustPolicy(environment: .development),
            credentials: authority, store: store, network: network, now: Date.init)
    }
    func collect(_ response: MapleResponse) async throws -> Data {
        var result = Data()
        for try await chunk in response.body { result += chunk }
        return result
    }
    let request = MapleRequest(method: .get, target: "/test", auth: .none)

    @Test func encryptedBodyAndSessionPersistence() async throws {
        let network = FixtureNetwork(), store = TestSecureStore()
        let first = try await make(network, store: store)
        #expect(try await collect(first.send(request)) == Data("{\"ok\":true}".utf8))
        let restored = try await make(network, store: store)
        _ = try await collect(restored.send(request))
        #expect(await network.handshakes == 1)
        let seen = await network.observations
        #expect(seen.count == 2 && seen[0].requestID != seen[1].requestID)
        #expect(seen.allSatisfy { $0.cacheRoot == nil && $0.credentialKind == nil })
        #expect(!seen[0].bodyPresent)
    }

    @Test func anonymousOAuthRequestsNeverCreateOrAttachCacheRoot() async throws {
        let network = FixtureNetwork(), store = TestSecureStore()
        let transport = try await make(network, store: store)
        let (response, binding) = try await transport.beginOAuth(
            MapleRequest(method: .post, target: "/auth/google", auth: .none))
        _ = try await collect(response)
        _ = try await collect(
            transport.completeOAuth(
                MapleRequest(method: .post, target: "/auth/google/callback", auth: .none), binding: binding))
        #expect(await network.observations.map(\.target) == ["/auth/google", "/auth/google/callback"])
        #expect(await network.observations.allSatisfy { $0.cacheRoot == nil && $0.credentialKind == nil })
        #expect(try store.read(key: "authority:http://127.0.0.1:3000") == nil)
    }

    @Test func cacheWriteFailuresKeepVerifiedSessionAndAuthenticatedNamespaceInMemory() async throws {
        let network = FixtureNetwork(), store = TestSecureStore()
        let transport = try await make(network, store: store, signedIn: true)
        let original = try await transport.credentials.snapshot()
        store.setWriteFailure(true)
        let authenticated = MapleRequest(method: .get, target: "/protected/user", expectedPrincipalId: "a")
        _ = try await collect(transport.send(authenticated))
        _ = try await collect(transport.send(authenticated))
        #expect(await network.handshakes == 1)
        let seen = await network.observations
        #expect(seen.count == 2 && seen.allSatisfy { $0.credentialKind == "bearer" })
        #expect(seen[0].cacheRoot != nil && seen[0].cacheRoot == seen[1].cacheRoot)
        #expect(try await transport.credentials.snapshot() == original)
        await #expect(throws: TransportError.persistence) {
            try await transport.credentials.install(
                accessToken: fixtureJWT(principal: "b", kind: "access"),
                refreshToken: fixtureJWT(principal: "b", kind: "refresh"), expected: original)
        }
        #expect(try await transport.credentials.snapshot() == original)
    }

    @Test func sessionCacheReadAndRemovalFailuresDoNotBlockOrRestoreRetiredSession() async throws {
        let network = FixtureNetwork(), store = TestSecureStore()
        let transport = try await make(network, store: store)
        store.setReadFailure(true)
        _ = try await collect(transport.send(request))
        store.setReadFailure(false)
        store.setWriteFailure(true)
        await transport.invalidateSession()
        _ = try await collect(transport.send(request))
        #expect(await network.handshakes == 2)
        #expect(await network.observations.count == 2)
    }

    @Test func onlyExactRecoveryHintAllowsOneResend() async throws {
        let network = FixtureNetwork([.hint]), transport = try await make(network)
        _ = try await collect(transport.send(request))
        #expect(await network.handshakes == 2)
        #expect(await network.observations.count == 2)
        let failing = FixtureNetwork([.hint, .hint]), other = try await make(failing)
        await #expect(throws: TransportError.untrustedRecoveryHint("session_not_found")) {
            try await other.send(request)
        }
        #expect(await failing.observations.count == 2)
    }

    @Test(arguments: [FixtureNetwork.Failure.generic400, .network, .tamper, .truncated, .extra, .secondStart])
    func uncertainOrInvalidResponseNeverResends(_ failure: FixtureNetwork.Failure) async throws {
        let network = FixtureNetwork([failure]), transport = try await make(network)
        await #expect(throws: (any Error).self) { try await collect(transport.send(request)) }
        #expect(await network.observations.count == 1)
    }

    @Test func inferenceBudgetCountsTransportRecovery() async throws {
        let network = FixtureNetwork([.hint]), transport = try await make(network)
        let inference = MapleRequest(
            method: .post, target: "/v1/responses",
            headers: [openSecretInferenceSendLimitHeader: "1"], auth: .none)
        await #expect(throws: TransportError.untrustedRecoveryHint("session_not_found")) {
            try await transport.send(inference)
        }
        #expect(await network.observations.count == 1)
        #expect(await network.observations.first?.headers.contains(openSecretInferenceSendLimitHeader) == false)
    }

    @Test func refreshCoalescesAndDoesNotInvalidateExistingStream() async throws {
        let network = FixtureNetwork(), transport = try await make(network, signedIn: true, expired: true)
        let protected = MapleRequest(method: .get, target: "/protected/user", expectedPrincipalId: "a")
        async let a = collect(transport.send(protected))
        async let b = collect(transport.send(protected))
        _ = try await (a, b)
        #expect(await network.observations.filter { $0.target == "/refresh" }.count == 1)
        #expect(await network.observations.first?.credentialKind == "resumption")
        #expect(await network.observations.last?.credentialKind == "bearer")
    }

    @Test func signOutRejectsLateStreamEvenAfterSameAccountSignIn() async throws {
        let network = FixtureNetwork(), transport = try await make(network, signedIn: true)
        let response = try await transport.send(
            MapleRequest(method: .get, target: "/protected/user", expectedPrincipalId: "a"))
        try await transport.credentials.clear()
        _ = try await transport.credentials.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: transport.credentials.snapshot())
        await #expect(throws: TransportError.authorityChanged) { try await collect(response) }
    }

    @Test func transientRefreshFailurePreservesCredentialsAndNeverSendsOriginal() async throws {
        let network = FixtureNetwork(), transport = try await make(network, signedIn: true, expired: true)
        await network.setStatus(503)
        await #expect(throws: TransportError.requestRejected(status: 503)) {
            try await transport.send(MapleRequest(method: .post, target: "/v1/responses"))
        }
        #expect(try await transport.credentials.authenticatedUserId() == "a")
        #expect(await network.observations.map(\.target) == ["/refresh"])
    }

    @Test func definitiveRefreshRejectionClearsOnlyCurrentAuthority() async throws {
        let network = FixtureNetwork(), transport = try await make(network, signedIn: true, expired: true)
        await network.setStatus(401)
        await #expect(throws: TransportError.signedOut) {
            try await transport.send(MapleRequest(method: .get, target: "/protected/user"))
        }
        #expect(try await transport.credentials.authenticatedUserId() == nil)
        #expect(await network.observations.count == 1)
    }

    @Test func oauthProviderBindingCannotBeReusedForAnotherProvider() async throws {
        let network = FixtureNetwork(), transport = try await make(network)
        let (response, binding) = try await transport.beginOAuth(
            MapleRequest(method: .post, target: "/auth/google", auth: .none))
        _ = try await collect(response)
        await #expect(throws: TransportError.invalidSession) {
            try await transport.completeOAuth(
                MapleRequest(method: .post, target: "/auth/apple/callback", auth: .none), binding: binding)
        }
        #expect(await network.observations.count == 1)
    }

    @Test func nativeHandoffIsPinnedOneUseAndRequiresConfirmation() async throws {
        let network = FixtureNetwork(), transport = try await make(network)
        let prepared = try await transport.prepareNativeOAuthHandoff()
        let grant = try NativeOAuthHandoffGrant(validating: "e30.e30.YQ")
        let confirmation = try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        #expect(try await transport.credentials.authenticatedUserId() == nil)
        #expect(await network.observations.first?.requestID == prepared.requestID)
        #expect(await network.observations.first?.cacheRoot == nil)
        await #expect(throws: TransportError.invalidSession) {
            try await transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        _ = try await transport.confirmNativeOAuthHandoff(confirmation)
        #expect(try await transport.credentials.authenticatedUserId() == "a")
        await #expect(throws: TransportError.authorityChanged) {
            try await transport.confirmNativeOAuthHandoff(confirmation)
        }
    }
}
