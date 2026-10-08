import Foundation
import MapleSDK
import Synchronization
import Testing
@testable import MapleAppCore

private final class AuthoritySecureStore: SecureStore, Sendable {
    struct State { var data: [String: Data] = [:]; var fail = false }
    let state = Mutex(State())
    func read(key: String) -> Data? { state.withLock { $0.data[key] } }
    func write(_ data: Data, key: String) throws {
        try state.withLock { state in
            if state.fail { throw TransportError.persistence }
            state.data[key] = data
        }
    }
    func remove(key: String) { _ = state.withLock { $0.data.removeValue(forKey: key) } }
    func fail(_ value: Bool) { state.withLock { $0.fail = value } }
}

private actor ObservedSessionService: SessionServing {
    let authority: CredentialAuthority
    var signOuts = 0
    var userGate: ChatTestGate?
    var userFailure: (any Error)?
    var identityCalls = 0
    var heldIdentityCall: Int?
    var identityGate: ChatTestGate?
    func holdIdentity(_ call: Int, gate: ChatTestGate) { heldIdentityCall = call; identityGate = gate }
    init(_ authority: CredentialAuthority) { self.authority = authority }
    func identity() async throws -> SessionIdentity? {
        let value = try await authority.snapshot()
        let captured = value.principalId.map {
            SessionIdentity(origin: value.apiOrigin, principal: $0, generation: value.generation)
        }
        identityCalls += 1
        if identityCalls == heldIdentityCall, let identityGate { await identityGate.hold() }
        return captured
    }
    func authorityChanges() async -> AsyncStream<CredentialAuthorityChange>? { await authority.changes() }
    func authorityState() async -> CredentialAuthorityChange? { await authority.currentState() }
    func completePendingCleanup() async throws { try await authority.completePendingCleanup() }
    func user() async throws -> MapleUser {
        let principal = try await authority.snapshot().principalId ?? "absent"
        if let userGate { await userGate.hold() }
        if let userFailure { throw userFailure }
        return MapleUser(
            id: principal, email: "fixture@example.test", emailVerified: true,
            loginMethod: "email", createdAt: "", updatedAt: "")
    }
    func setUserGate(_ gate: ChatTestGate) { userGate = gate }
    func setUserFailure(_ error: any Error) { userFailure = error }
    func install(_ principal: String) async throws {
        func token(_ kind: String) throws -> String {
            let data = try JSONSerialization.data(withJSONObject: [
                "sub": principal, "exp": 4_000_000_000, "tf": 2,
                "aud": "urn:opensecret:internal:transport-v2:user:\(kind)-token",
            ])
            let payload = data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
                .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
            return "e30.\(payload).c2ln"
        }
        _ = try await authority.install(
            accessToken: token("access"), refreshToken: token("refresh"), expected: authority.snapshot())
    }
    func signIn(email: String, password: String) async throws { try await install(email) }
    func signInGuest(id: String, password: String) async throws { try await install(id) }
    func signUp(email: String, password: String, name: String?) async throws { try await install(email) }
    func signOut(ifOwnedBy identity: SessionIdentity?) async throws {
        guard let identity, try await self.identity() == identity else { return }
        try await signOut()
    }
    func signOut() async throws {
        signOuts += 1
        try await authority.completePendingCleanup()
        try await authority.clear()
    }
}

private actor ObservedLifecycle: AccountDataLifecycle {
    var activations: [AccountScope] = []
    var retirements: [AccountScope] = []
    var clears = 0
    var failure = false
    var clearGate: ChatTestGate?
    func setClearGate(_ gate: ChatTestGate) { clearGate = gate }
    func setFailure(_ value: Bool) { failure = value }
    func activate(_ scope: AccountScope) { activations.append(scope) }
    func retire(_ scope: AccountScope) throws {
        retirements.append(scope)
        if failure { throw ChatStorageError.cleanupRequired }
    }
    func clearSignedOutData() async throws {
        clears += 1
        if let clearGate { await clearGate.hold() }
        if failure { throw ChatStorageError.cleanupRequired }
    }
}

@MainActor @Suite struct SessionAuthorityTests {
    private func fixture() async throws -> (
        SessionController, ObservedSessionService, ObservedLifecycle, AuthoritySecureStore
    ) {
        let secure = AuthoritySecureStore()
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:1234", store: secure)
        let service = ObservedSessionService(authority), storage = ObservedLifecycle()
        try await service.install("original")
        let session = SessionController(service: service, storage: storage)
        await session.restore()
        return (session, service, storage, secure)
    }
    private func settle(_ check: @MainActor () -> Bool) async {
        for _ in 0..<20_000 {
            if check() { return }
            await Task.yield()
        }
        #expect(check())
    }

    @Test func revokedAuthorityImmediatelyRetiresPrivateOwnerWithoutSigningOutAgain() async throws {
        let (session, service, storage, _) = try await fixture()
        let scope = try #require(session.account?.scope)
        let gate = ChatTestGate()
        session.onRetire = { retired in
            #expect(retired == scope); await gate.hold()
        }
        try await service.authority.clear()
        await gate.waitForArrival()
        #expect(session.account == nil && session.phase == .cleaningUp)
        await gate.release()
        await settle { session.phase == .signedOut }
        #expect(await storage.retirements == [scope])
        #expect(await service.signOuts == 0)
    }

    @Test func replacementDuringRetirementResolvesNewestAndOldEventsCannotRetireIt() async throws {
        let (session, service, storage, _) = try await fixture()
        let original = try #require(session.account?.scope)
        let gate = ChatTestGate()
        session.onRetire = { _ in await gate.hold() }
        try await service.install("replacement-one")
        let stale = await service.authority.currentState()
        await gate.waitForArrival()
        try await service.install("replacement-two")
        #expect(session.account == nil)
        await gate.release()
        await settle { session.account?.scope.principalID == "replacement-two" }
        let current = try #require(session.account?.scope)
        await session.authorityChanged(stale)
        #expect(session.account?.scope == current)
        #expect(await service.signOuts == 0)
        #expect(await storage.retirements == [original])
        #expect(await storage.activations.count == 2)
    }

    @Test func failedKeychainClearRevokesAppAndBlocksActivationUntilExplicitRepair() async throws {
        let (session, service, storage, secure) = try await fixture()
        secure.fail(true)
        await #expect(throws: TransportError.persistence) { try await service.authority.clear() }
        await settle { session.phase == .unavailable && session.issue == .cleanup }
        #expect(session.account == nil)
        await session.restore()
        #expect(session.phase == .unavailable && session.issue == .cleanup)
        await session.signIn(email: "blocked", password: "synthetic")
        #expect(await storage.activations.count == 1)
        secure.fail(false)
        await session.restore()
        #expect(session.phase == .signedOut)
        #expect(await service.signOuts == 0)
        await session.signIn(email: "new", password: "synthetic")
        #expect(session.account?.scope.principalID == "new")
    }

    @Test func privateCleanupFailurePreservesReplacementCredentialsUntilRestoreRetry() async throws {
        let (session, service, storage, _) = try await fixture()
        await storage.setFailure(true)
        try await service.install("replacement")
        await settle { session.phase == .unavailable && session.issue == .cleanup }
        #expect(try await service.identity()?.principal == "replacement")
        #expect(session.account == nil)
        #expect(await storage.activations.count == 1)
        await storage.setFailure(false)
        await session.restore()
        #expect(session.account?.scope.principalID == "replacement")
        #expect(await service.signOuts == 0)
    }

    @Test func ordinaryUnauthorizedProfileFailureDoesNotInventAuthorityRevocation() async throws {
        let (session, service, storage, _) = try await fixture()
        let scope = session.account?.scope
        await service.setUserFailure(ResearchAPIError.http(status: 401, reason: .invalidLogin))
        await session.refreshUser()
        #expect(session.account?.scope == scope && session.phase == .signedIn)
        #expect(await storage.retirements.isEmpty)
        #expect(await service.signOuts == 0)
    }

    @Test func explicitLogoutCleanupRetryThroughRestoreStillRevokesAuthority() async throws {
        let (session, service, storage, _) = try await fixture()
        await storage.setFailure(true)
        await session.signOut()
        #expect(session.phase == .unavailable && session.issue == .cleanup)
        await storage.setFailure(false)
        await session.restore()
        #expect(session.phase == .signedOut && session.account == nil)
        #expect(await service.signOuts == 1)
    }

    @Test func installDuringSignedOutCleanupIsResolvedEvenWhenItsEventWasAlreadyHandled() async throws {
        let secure = AuthoritySecureStore()
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:1234", store: secure)
        let service = ObservedSessionService(authority), storage = ObservedLifecycle(), gate = ChatTestGate()
        await storage.setClearGate(gate)
        let session = SessionController(service: service, storage: storage)
        let restore = Task { await session.restore() }
        await gate.waitForArrival()
        try await service.install("installed-during-cleanup")
        await session.authorityChanged(authority.currentState())
        await gate.release(); await restore.value
        #expect(session.account?.scope.principalID == "installed-during-cleanup")
        #expect(await storage.activations.count == 1)
    }

    @Test func installAfterFinalNilReadButBeforePublicationCannotBeLost() async throws {
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:1234", store: AuthoritySecureStore())
        let service = ObservedSessionService(authority), storage = ObservedLifecycle(), gate = ChatTestGate()
        await service.holdIdentity(2, gate: gate)
        let session = SessionController(service: service, storage: storage)
        let restore = Task { await session.restore() }
        await gate.waitForArrival()
        try await service.install("replacement")
        await session.authorityChanged(authority.currentState())
        await gate.release(); await restore.value
        #expect(session.account?.scope.principalID == "replacement")
        #expect(await storage.activations.count == 1)
    }

    @Test func explicitLogoutNeverRevokesReplacementInstalledDuringRetirement() async throws {
        let (session, service, storage, _) = try await fixture()
        let original = try #require(session.account?.scope), gate = ChatTestGate()
        session.onRetire = { _ in await gate.hold() }
        let logout = Task { await session.signOut() }
        await gate.waitForArrival()
        try await service.install("replacement")
        await gate.release(); await logout.value
        #expect(session.account?.scope.principalID == "replacement")
        #expect(await service.signOuts == 0)
        #expect(await storage.retirements == [original])
    }

    @Test func completedLogoutDoesNotRevokeNewAuthorityWhenOnlyPrivateCleanupIsRetried() async throws {
        let (session, service, storage, _) = try await fixture()
        await storage.setFailure(true)
        await session.signOut()
        #expect(session.issue == .cleanup && session.account == nil)
        try await service.install("replacement")
        await storage.setFailure(false)
        await session.restore()
        #expect(session.account?.scope.principalID == "replacement")
        #expect(await service.signOuts == 1)
    }

    @Test func revocationDuringRestoreRejectsLateProfilePublication() async throws {
        let secure = AuthoritySecureStore()
        let authority = try CredentialAuthority(apiURL: "http://127.0.0.1:1234", store: secure)
        let service = ObservedSessionService(authority), storage = ObservedLifecycle(), gate = ChatTestGate()
        try await service.install("original"); await service.setUserGate(gate)
        let session = SessionController(service: service, storage: storage)
        let restore = Task { await session.restore() }
        await gate.waitForArrival()
        try await authority.clear()
        await settle { session.phase == .signedOut }
        await gate.release(); await restore.value
        #expect(session.phase == .signedOut && session.account == nil)
        #expect(await storage.activations.isEmpty)
    }
}
