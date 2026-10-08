import Foundation
import MapleSDK
import Synchronization
import Testing
@testable import MapleAppCore

private actor LinkRestoreService: SessionServing {
    var current: SessionIdentity?
    let gate: ChatTestGate?
    init(signedIn: Bool, gate: ChatTestGate?) {
        self.gate = gate
        if signedIn {
            current = SessionIdentity(origin: "http://127.0.0.1:1234", principal: "fixture", generation: UUID())
        }
    }
    func identity() -> SessionIdentity? { current }
    func user() async -> MapleUser {
        let id = current?.principal ?? "absent"
        if let gate { await gate.hold() }
        return MapleUser(
            id: id, email: "synthetic@example.test", emailVerified: false, loginMethod: "email", createdAt: "",
            updatedAt: "")
    }
    func signIn(email: String, password: String) {}
    func signInGuest(id: String, password: String) {}
    func signUp(email: String, password: String, name: String?) {}
    func signOut() { current = nil }
}
private actor LinkRestoreStorage: AccountDataLifecycle {
    let gate: ChatTestGate?
    init(gate: ChatTestGate?) { self.gate = gate }
    func activate(_ scope: AccountScope) {}
    func retire(_ scope: AccountScope) {}
    func clearSignedOutData() async { if let gate { await gate.hold() } }
}

@MainActor @Suite struct AccountLinkInboxTests {
    private let verify = IncomingAccountLink.verify(code: "00000000-0000-4000-8000-000000000042")

    @Test(arguments: [false, true])
    func coldLinkSurvivesSuspendedRestoreAndDeliversExactlyOnce(signedIn: Bool) async {
        let gate = ChatTestGate(), inbox = AccountLinkInbox()
        let session = SessionController(
            service: LinkRestoreService(signedIn: signedIn, gate: signedIn ? gate : nil),
            storage: LinkRestoreStorage(gate: signedIn ? nil : gate))
        session.onCancelExternal = { inbox.retire() }
        // Equivalent to receipt before the root has constructed its environment.
        #expect(inbox.offer(verify, phase: .restoring, scope: nil) == nil)
        let restore = Task { await session.restore() }
        await gate.waitForArrival()
        #expect(inbox.take(phase: session.phase, scope: session.account?.scope) == nil)
        await gate.release(); await restore.value
        #expect(session.phase == (signedIn ? .signedIn : .signedOut))
        #expect(inbox.take(phase: session.phase, scope: session.account?.scope) == verify)
        #expect(inbox.take(phase: session.phase, scope: session.account?.scope) == nil)
    }

    @Test func retirementDuringSuspendedRestoreRejectsItsWaitingLink() async {
        let gate = ChatTestGate(), inbox = AccountLinkInbox()
        let session = SessionController(
            service: LinkRestoreService(signedIn: true, gate: gate), storage: LinkRestoreStorage(gate: nil))
        session.onCancelExternal = { inbox.retire() }
        let restore = Task { await session.restore() }
        await gate.waitForArrival()
        #expect(inbox.offer(verify, phase: session.phase, scope: nil) == nil)
        await session.signOut(); await gate.release(); await restore.value
        #expect(session.phase == .signedOut)
        #expect(inbox.take(phase: session.phase, scope: nil) == nil)
    }

    @Test func newestValidLinkWinsButForeignOrOAuthInputCannotReplaceIt() throws {
        let config = try AppConfiguration(
            profile: .local, apiURL: "http://127.0.0.1:1234", billingURL: "http://127.0.0.1:1235",
            flagsURL: "http://127.0.0.1:1236", authURL: "http://127.0.0.1:1237",
            clientID: "00000000-0000-4000-8000-000000000001", callbackScheme: "cloud.opensecret.maple.native.dev")
        let inbox = AccountLinkInbox()
        #expect(inbox.offer(verify, phase: .restoring, scope: nil) == nil)
        for value in [
            "http://127.0.0.1:1238/password-reset", "http://127.0.0.1:1237/password-reset?code=ABCD1234&code=EFGH5678",
            "cloud.opensecret.maple.native.dev://auth/callback?grant=synthetic",
        ] {
            let parsed = IncomingAccountLink(url: try #require(URL(string: value)), configuration: config)
            #expect(parsed == nil)
            if let parsed { _ = inbox.offer(parsed, phase: .restoring, scope: nil) }
        }
        #expect(inbox.take(phase: .signedOut, scope: nil) == verify)
        #expect(inbox.offer(verify, phase: .restoring, scope: nil) == nil)
        let resetURL = try #require(
            URL(string: "http://127.0.0.1:1237/password-reset?code=ABCD1234&secret=synthetic-external&email=other"))
        let reset = try #require(IncomingAccountLink(url: resetURL, configuration: config))
        #expect(inbox.offer(reset, phase: .restoring, scope: nil) == nil)
        #expect(inbox.take(phase: .signedOut, scope: nil) == .passwordReset(code: "ABCD1234"))
    }

    @Test func knownScopeCannotFollowSameAccountReloginOrAnonymousRetirement() {
        let inbox = AccountLinkInbox()
        let original = AccountScope(apiOrigin: "http://127.0.0.1:1234", principalID: "same")
        let replacement = AccountScope(apiOrigin: original.apiOrigin, principalID: original.principalID)
        #expect(inbox.offer(verify, phase: .restoring, scope: original) == nil)
        #expect(inbox.take(phase: .signedIn, scope: replacement) == nil)
        #expect(inbox.offer(verify, phase: .restoring, scope: original) == nil)
        #expect(inbox.take(phase: .signedOut, scope: nil) == nil)
        #expect(inbox.offer(verify, phase: .restoring, scope: original) == nil)
        #expect(inbox.take(phase: .signedIn, scope: original) == verify)
    }

    @Test(arguments: [SessionPhase.authenticating, .cleaningUp])
    func receiptDuringAuthorityTransitionCannotWaitAndAdoptReplacement(_ phase: SessionPhase) {
        let inbox = AccountLinkInbox()
        let original = AccountScope(apiOrigin: "http://127.0.0.1:1234", principalID: "same")
        let replacement = AccountScope(apiOrigin: original.apiOrigin, principalID: original.principalID)
        // The root invokes this admission synchronously at receipt, before any
        // subsequently scheduled restoration can publish a replacement account.
        #expect(inbox.offer(verify, phase: phase, scope: original) == nil)
        #expect(inbox.take(phase: .signedIn, scope: replacement) == nil)
    }

    @Test func pendingRouteExpiresAndDoesNotCrossAnExternalSignIn() {
        let elapsed = Mutex(0.0), inbox = AccountLinkInbox(monotonicNow: { elapsed.withLock { $0 } })
        #expect(inbox.offer(verify, phase: .restoring, scope: nil) == nil)
        elapsed.withLock { $0 = 301 }
        #expect(inbox.take(phase: .signedOut, scope: nil) == nil)
        #expect(inbox.offer(verify, phase: .restoring, scope: nil) == nil)
        #expect(inbox.take(phase: .unavailable, scope: nil) == nil)
        #expect(inbox.take(phase: .authenticating, scope: nil) == nil)
        #expect(inbox.take(phase: .signedOut, scope: nil) == nil)
    }
}
