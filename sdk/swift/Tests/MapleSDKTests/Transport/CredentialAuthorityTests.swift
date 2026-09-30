import Foundation
import Testing
@testable import MapleSDK

@Suite struct CredentialAuthorityTests {
    @Test func tokenHintsRejectWrongAudienceFormatAndPrincipal() throws {
        let a = try fixtureJWT(principal: "a", kind: "access")
        let r = try fixtureJWT(principal: "a", kind: "refresh")
        #expect(try CredentialPair(access: a, refresh: r).principal == "a")
        #expect(throws: TransportError.invalidCredentials) {
            try CredentialPair(access: a, refresh: fixtureJWT(principal: "b", kind: "refresh"))
        }
        #expect(throws: TransportError.invalidCredentials) { try CredentialPair(access: r, refresh: a) }
        #expect(throws: TransportError.invalidCredentials) {
            try CredentialPair(access: fixtureJWT(principal: "a", kind: "access", format: 1), refresh: r)
        }
        #expect(throws: TransportError.invalidCredentials) { try CredentialPair(access: a + "=", refresh: r) }
    }

    @Test func CASAndSameAccountReloginFenceOldWork() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let anonymous = try await authority.snapshot()
        let access = try fixtureJWT(principal: "a", kind: "access"),
            refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let first = try await authority.install(accessToken: access, refreshToken: refresh, expected: anonymous)
        await #expect(throws: TransportError.authorityChanged) {
            try await authority.install(accessToken: access, refreshToken: refresh, expected: anonymous)
        }
        try await authority.clear(expected: first)
        let second = try await authority.install(
            accessToken: access, refreshToken: refresh, expected: authority.snapshot())
        #expect(first.generation != second.generation)
        await #expect(throws: TransportError.authorityChanged) {
            try await authority.assertCurrent(first, exactRevision: false)
        }
        #expect(second.revision > first.revision)
    }

    @Test func refreshPreservesAccountLeaseButRequiresExactRevision() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let a = try fixtureJWT(principal: "a", kind: "access"), r = try fixtureJWT(principal: "a", kind: "refresh")
        let original = try await authority.install(accessToken: a, refreshToken: r, expected: authority.snapshot())
        let renewed = try await authority.install(accessToken: a, refreshToken: r, expected: original, refreshing: true)
        #expect(original.generation == renewed.generation)
        try await authority.assertCurrent(original, exactRevision: false)
        await #expect(throws: TransportError.authorityChanged) { try await authority.assertCurrent(original) }
    }

    @Test func durableFailuresNeverPublishAndCleanupFailsClosed() async throws {
        let storage = TestSecureStore()
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: storage)
        let anonymous = try await authority.snapshot()
        let a = try fixtureJWT(principal: "a", kind: "access"), r = try fixtureJWT(principal: "a", kind: "refresh")
        storage.setWriteFailure(true)
        await #expect(throws: TransportError.persistence) {
            try await authority.install(accessToken: a, refreshToken: r, expected: anonymous)
        }
        #expect(try await authority.snapshot() == anonymous)
        storage.setWriteFailure(false)
        _ = try await authority.install(accessToken: a, refreshToken: r, expected: anonymous)
        storage.setWriteFailure(true)
        await #expect(throws: TransportError.persistence) { try await authority.clear() }
        await #expect(throws: TransportError.persistence) { try await authority.snapshot() }
        storage.setWriteFailure(false)
        try await authority.clear()
        #expect(try await authority.authenticatedUserId() == nil)
    }

    @Test func restoreIsOriginScopedAndPreservesCacheRoot() async throws {
        let storage = TestSecureStore()
        let first = try CredentialAuthority(apiURL: "https://EXAMPLE.test:443/service/", store: storage)
        let root = try await first.cacheRoot()
        _ = try await first.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: first.snapshot())
        let restored = try CredentialAuthority(apiURL: "https://example.test/another", store: storage)
        #expect(try await restored.authenticatedUserId() == "a")
        #expect(try await restored.cacheRoot() == root)
        let other = try CredentialAuthority(apiURL: "https://other.test", store: storage)
        #expect(try await other.authenticatedUserId() == nil)
        #expect(try await other.cacheRoot() != root)
    }

    @Test func endpointRejectsUnsafeOrigins() throws {
        for value in [
            "http://example.test", "https://user@example.test", "https://example.test?token=x",
            "https://example.test/#x", "http://127.1", "http://127.0.0.2",
        ] {
            #expect(throws: TransportError.invalidEndpoint) { try APIEndpoint(value) }
        }
        #expect(try APIEndpoint("http://127.0.0.1:3000").isLoopback)
        #expect(try APIEndpoint("http://[::1]:3000").isLoopback)
    }
}
