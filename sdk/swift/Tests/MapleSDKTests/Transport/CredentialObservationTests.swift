import Foundation
import Testing
@testable import MapleSDK

private actor AuthorityEvents {
    var values: [CredentialAuthorityChange] = []
    func append(_ value: CredentialAuthorityChange) { values.append(value) }
}

@Suite struct CredentialObservationTests {
    @Test func lateSubscriberStartsWithReplacementAndSlowSubscriberGetsNewest() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let early = await authority.changes()
        let first = try await authority.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        let late = await authority.changes()
        var lateIterator = late.makeAsyncIterator()
        let initial = await lateIterator.next()
        #expect(initial?.generation == first.generation)
        try await authority.clear()
        let second = try await authority.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        var earlyIterator = early.makeAsyncIterator()
        let newest = await earlyIterator.next()
        #expect(newest?.generation == second.generation)
        #expect(newest?.generation != first.generation)
        #expect(newest?.requiresCleanup == false)
    }

    @Test func refreshDoesNotInvalidateObserversAndCancellationUnregisters() async throws {
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: TestSecureStore())
        let access = try fixtureJWT(principal: "a", kind: "access")
        let refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let first = try await authority.install(
            accessToken: access, refreshToken: refresh, expected: authority.snapshot())
        let stream = await authority.changes(), events = AuthorityEvents()
        let observation = Task {
            for await value in stream { await events.append(value) }
        }
        for _ in 0..<1_000 {
            if await events.values.count == 1 { break }
            await Task.yield()
        }
        _ = try await authority.install(accessToken: access, refreshToken: refresh, expected: first, refreshing: true)
        // Allow delivery of any incorrectly published refresh event while the
        // observer is already waiting. This is not a server or polling delay.
        try await Task.sleep(for: .milliseconds(30))
        #expect(await events.values.count == 1)
        #expect(await authority.observerCount == 1)
        observation.cancel(); await observation.value
        for _ in 0..<1_000 {
            if await authority.observerCount == 0 { break }
            await Task.yield()
        }
        #expect(await authority.observerCount == 0)
    }

    @Test func failedClearPublishesRevocationAndRetryNeverClearsReplacement() async throws {
        let store = TestSecureStore()
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: store)
        let first = try await authority.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        var changes = await authority.changes().makeAsyncIterator()
        _ = await changes.next()
        store.setWriteFailure(true)
        await #expect(throws: TransportError.persistence) { try await authority.clear(expected: first) }
        let revokedValue = await changes.next()
        let revoked = try #require(revokedValue)
        #expect(revoked.generation != first.generation && revoked.requiresCleanup)
        #expect(await authority.currentState() == revoked)
        await #expect(throws: TransportError.persistence) { try await authority.snapshot() }
        await #expect(throws: TransportError.persistence) { try await authority.completePendingCleanup() }
        store.setWriteFailure(false)
        try await authority.completePendingCleanup()
        let repairedValue = await changes.next()
        let repaired = try #require(repairedValue)
        #expect(repaired.generation == revoked.generation && !repaired.requiresCleanup)
        #expect(try await authority.authenticatedUserId() == nil)
        let replacement = try await authority.install(
            accessToken: fixtureJWT(principal: "b", kind: "access"),
            refreshToken: fixtureJWT(principal: "b", kind: "refresh"), expected: authority.snapshot())
        try await authority.completePendingCleanup()
        #expect(try await authority.snapshot() == replacement)
    }

    @Test(arguments: [401, 403])
    func confirmedRefreshRejectionPublishesRevocation(status: Int) async throws {
        let network = FixtureNetwork()
        await network.setStatus(status)
        let transport = try await EncryptedTransportTests().make(network, signedIn: true, expired: true)
        let prior = await transport.credentials.currentState()
        var changes = await transport.credentials.changes().makeAsyncIterator()
        _ = await changes.next()
        await #expect(throws: TransportError.signedOut) {
            try await transport.send(MapleRequest(method: .get, target: "/test"))
        }
        let nextValue = await changes.next()
        let next = try #require(nextValue)
        #expect(next.generation != prior.generation && !next.requiresCleanup)
        #expect(try await transport.credentials.authenticatedUserId() == nil)
        #expect(await network.observations.map(\.target) == ["/refresh"])
    }

    @Test func ordinaryUnauthorizedResponseAndTransientRefreshPreserveAuthority() async throws {
        let network = FixtureNetwork()
        await network.setStatus(401)
        let transport = try await EncryptedTransportTests().make(network, signedIn: true)
        let prior = await transport.credentials.currentState()
        let response = try await transport.send(MapleRequest(method: .get, target: "/test"))
        _ = try await EncryptedTransportTests().collect(response)
        #expect(response.status == 401)
        #expect(await transport.credentials.currentState() == prior)
        let transient = FixtureNetwork()
        await transient.setStatus(503)
        let other = try await EncryptedTransportTests().make(transient, signedIn: true, expired: true)
        let old = await other.credentials.currentState()
        do {
            _ = try await other.send(MapleRequest(method: .get, target: "/test"));
            Issue.record("Expected refresh failure")
        } catch { #expect(await other.credentials.currentState() == old) }
    }

    @Test func scopedSignOutRejectsSamePrincipalReplacementAndRevokeAllowsRoutineRefresh() async throws {
        let client = try MapleClient(
            apiURL: "http://127.0.0.1:1234", clientID: "00000000-0000-0000-0000-000000000001",
            policy: PCRTrustPolicy(environment: .development), store: TestSecureStore())
        let access = try fixtureJWT(principal: "a", kind: "access"),
            refresh = try fixtureJWT(principal: "a", kind: "refresh")
        let first = try await client.credentials.install(
            accessToken: access, refreshToken: refresh, expected: client.credentials.snapshot())
        let replacement = try await client.credentials.install(
            accessToken: access, refreshToken: refresh, expected: first)
        await #expect(throws: TransportError.authorityChanged) { try await client.signOut(expected: first) }
        #expect(try await client.credentials.snapshot() == replacement)
        _ = try await client.credentials.install(
            accessToken: access, refreshToken: refresh, expected: replacement, refreshing: true)
        let revoked = try await client.credentials.revoke(expected: replacement)
        #expect(revoked == refresh)
        #expect(try await client.credentials.authenticatedUserId() == nil)
    }

    @Test func clientSignOutCanFinishAPreviouslyFailedDurableClear() async throws {
        let store = TestSecureStore()
        let client = try MapleClient(
            apiURL: "http://127.0.0.1:1234", clientID: "00000000-0000-0000-0000-000000000001",
            policy: PCRTrustPolicy(environment: .development), store: store)
        _ = try await client.credentials.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: client.credentials.snapshot())
        store.setWriteFailure(true)
        await #expect(throws: TransportError.persistence) { try await client.credentials.clear() }
        store.setWriteFailure(false)
        // The revoked in-memory pair is absent, so this retry needs no request
        // and cannot leak the previously persisted refresh token to a new caller.
        try await client.signOut()
        #expect(try await client.credentials.authenticatedUserId() == nil)
        #expect(await client.credentials.currentState().requiresCleanup == false)
    }

    @Test func failedInstallLeavesObserverAuthorityUnchanged() async throws {
        let store = TestSecureStore()
        let authority = try CredentialAuthority(apiURL: "https://example.test", store: store)
        let original = await authority.currentState()
        store.setWriteFailure(true)
        await #expect(throws: TransportError.persistence) {
            try await authority.install(
                accessToken: fixtureJWT(principal: "a", kind: "access"),
                refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: authority.snapshot())
        }
        #expect(await authority.currentState() == original)
        var changes = await authority.changes().makeAsyncIterator()
        let first = await changes.next()
        #expect(first == original)
    }
}
