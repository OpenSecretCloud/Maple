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

    @Test func corruptPersistedAuthorityResetsOnlyItsOriginAndPublishesSignedOutState() async throws {
        let origin = "https://example.test", key = "authority:" + origin
        let access = try fixtureJWT(principal: "a", kind: "access")
        let refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let valid: [String: Any] = [
            "version": 1, "origin": origin, "revision": 3, "generation": UUID().uuidString,
            "access": access, "refresh": refresh,
        ]
        let invalid: [[String: Any]] = [
            valid.merging(["version": 2]) { _, new in new },
            valid.merging(["origin": "https://other.test"]) { _, new in new },
            valid.merging(["refresh": NSNull()]) { _, new in new },
            valid.merging(["cacheRoot": Data(repeating: 1, count: 31).base64EncodedString()]) { _, new in new },
            valid.merging(["access": "malformed-jwt"]) { _, new in new },
            valid.merging(["refresh": try fixtureJWT(principal: "b", kind: "refresh")]) { _, new in new },
        ]
        let blobs =
            [Data("malformed JSON".utf8), Data(repeating: 32, count: 96 * 1_024 + 1)]
            + (try invalid.map { try JSONSerialization.data(withJSONObject: $0) })
        for blob in blobs {
            let store = TestSecureStore()
            try store.write(blob, key: key)
            try store.write(Data("untouched".utf8), key: "authority:https://other.test")
            let client = try MapleClient(
                apiURL: origin, clientID: "00000000-0000-0000-0000-000000000001",
                policy: PCRTrustPolicy(environment: .production), store: store)
            let snapshot = try await client.credentials.snapshot()
            #expect(snapshot.principalId == nil && snapshot.revision == 0)
            var changes = await client.credentials.changes().makeAsyncIterator()
            let change = await changes.next()
            #expect(change?.generation == snapshot.generation && change?.requiresCleanup == false)
            #expect(try store.read(key: key) == nil)
            #expect(try store.read(key: "authority:https://other.test") == Data("untouched".utf8))
            _ = try await client.credentials.install(accessToken: access, refreshToken: refresh, expected: snapshot)
            #expect(try await client.credentials.authenticatedUserId() == "a")
        }
    }

    @Test func corruptAuthorityCleanupFailureConstructsButBlocksUseUntilDurableCleanup() async throws {
        let store = TestSecureStore()
        try store.write(Data("malformed JSON".utf8), key: "authority:https://example.test")
        store.setWriteFailure(true)
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: store)
        var changes = await authority.changes().makeAsyncIterator()
        let initial = await changes.next()
        #expect(initial?.requiresCleanup == true)
        await #expect(throws: TransportError.persistence) { try await authority.snapshot() }
        await #expect(throws: TransportError.persistence) { try await authority.completePendingCleanup() }
        store.setWriteFailure(false)
        try await authority.completePendingCleanup()
        let repaired = await changes.next()
        #expect(repaired?.generation == initial?.generation && repaired?.requiresCleanup == false)
        #expect(try await authority.authenticatedUserId() == nil)
        let restored = try CredentialAuthority(apiURL: "https://example.test", store: store)
        #expect(try await restored.authenticatedUserId() == nil)
    }

    @Test func unreadableCredentialAuthorityStillFailsClosed() throws {
        let store = TestSecureStore()
        store.setReadFailure(true)
        #expect(throws: TransportError.persistence) {
            try CredentialAuthority(apiURL: "https://example.test", store: store)
        }
    }

    @Test func cancelledRefreshCallerKeepsSharedRotationAliveForOtherCallers() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let access = try fixtureJWT(principal: "a", kind: "access")
        let refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let expected = try await authority.install(
            accessToken: access, refreshToken: refresh, expected: authority.snapshot())
        let gate = RefreshSuspension()
        let operation: @Sendable () async throws -> Void = {
            await gate.run()
            _ = try await authority.install(
                accessToken: access, refreshToken: refresh, expected: expected, refreshing: true)
        }
        let caller = Task { try await authority.coalescedRefresh(expected: expected, operation: operation) }
        await gate.waitUntilStarted()
        caller.cancel()
        let follower = Task { try await authority.coalescedRefresh(expected: expected, operation: operation) }
        // The shared operation remains suspended while the other waiter joins.
        try await Task.sleep(for: .milliseconds(30))
        #expect(await gate.runs == 1)
        await gate.release()
        await #expect(throws: CancellationError.self) { try await caller.value }
        try await follower.value
        let renewed = try await authority.snapshot()
        #expect(renewed.generation == expected.generation && renewed.revision == expected.revision + 1)
        #expect(await gate.runs == 1)
    }

    @Test func sharedRefreshCannotInstallAfterSameAccountReplacement() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let access = try fixtureJWT(principal: "a", kind: "access")
        let refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let expected = try await authority.install(
            accessToken: access, refreshToken: refresh, expected: authority.snapshot())
        let gate = RefreshSuspension()
        let pending = Task {
            try await authority.coalescedRefresh(expected: expected) {
                await gate.run()
                _ = try await authority.install(
                    accessToken: access, refreshToken: refresh, expected: expected, refreshing: true)
            }
        }
        await gate.waitUntilStarted()
        let replacement = try await authority.install(accessToken: access, refreshToken: refresh, expected: expected)
        await gate.release()
        await #expect(throws: TransportError.authorityChanged) { try await pending.value }
        #expect(try await authority.snapshot() == replacement)
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

private actor RefreshSuspension {
    private(set) var runs = 0
    private var started: CheckedContinuation<Void, Never>?
    private var waiting: [CheckedContinuation<Void, Never>] = []

    func run() async {
        runs += 1
        started?.resume(); started = nil
        await withCheckedContinuation { waiting.append($0) }
    }
    func waitUntilStarted() async {
        if runs > 0 { return }
        await withCheckedContinuation { started = $0 }
    }
    func release() {
        for continuation in waiting { continuation.resume() }
        waiting.removeAll()
    }
}
