import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

private actor SessionFixture: SessionServing {
    struct SignupRequest: Equatable, Sendable { let email: String; let password: String; let name: String? }
    let principal = "test-principal"
    var current: SessionIdentity?
    var failUser = false
    var holdUser = false
    var requested = false
    var userWaiter: CheckedContinuation<Void, Never>?
    var entryWaiter: CheckedContinuation<Void, Never>?
    var loginCount = 0
    var signupRequests: [SignupRequest] = []
    var verified = false
    init(signedIn: Bool = true) {
        if signedIn {
            current = SessionIdentity(origin: "http://127.0.0.1:1234", principal: principal, generation: UUID())
        }
    }
    func identity() -> SessionIdentity? { current }
    func user() async throws -> MapleUser {
        requested = true; entryWaiter?.resume(); entryWaiter = nil
        if holdUser { await withCheckedContinuation { userWaiter = $0 } }
        if failUser { throw URLError(.notConnectedToInternet) }
        return MapleUser(
            id: principal, email: "fixture@example.test", emailVerified: verified,
            loginMethod: "email", createdAt: "", updatedAt: "")
    }
    func configure(fail: Bool = false, hold: Bool = false) { failUser = fail; holdUser = hold; requested = false }
    func setVerified() { verified = true }
    func awaitUserRequest() async {
        if !requested { await withCheckedContinuation { entryWaiter = $0 } }
    }
    func releaseUser() { userWaiter?.resume(); userWaiter = nil; holdUser = false }
    func signIn(email: String, password: String) {
        loginCount += 1
        current = SessionIdentity(origin: "http://127.0.0.1:1234", principal: principal, generation: UUID())
    }
    func signInGuest(id: String, password: String) { signIn(email: "", password: password) }
    func signUp(email: String, password: String, name: String?) {
        signupRequests.append(.init(email: email, password: password, name: name))
        signIn(email: email, password: password)
    }
    func signOut() { current = nil }
}

private actor LifecycleFixture: AccountDataLifecycle {
    var activations: [AccountScope] = []
    var retirements: [AccountScope] = []
    var clears = 0
    var failure = false
    func activate(_ scope: AccountScope) { activations.append(scope) }
    func retire(_ scope: AccountScope) throws {
        if failure { throw ChatStorageError.cleanupRequired }
        retirements.append(scope)
    }
    func clearSignedOutData() throws {
        if failure { throw ChatStorageError.cleanupRequired }
        clears += 1
    }
    func setFailure(_ value: Bool) { failure = value }
}

@MainActor private final class RetirementFailureControl { var shouldFail = true }

enum SignupPasswordSample: String, Sendable {
    case empty, sevenASCII, sixUTF16, aboveByteLimit, eightASCII, eightUTF16, maximumBytes

    var value: String {
        switch self {
        case .empty: ""
        case .sevenASCII: "seven77"
        case .sixUTF16: "😀😀😀"
        case .aboveByteLimit: String(repeating: "a", count: 16 * 1024 + 1)
        case .eightASCII: "eight888"
        case .eightUTF16: "😀😀😀😀"
        case .maximumBytes: String(repeating: "a", count: 16 * 1024)
        }
    }
}

@MainActor struct SessionControllerTests {
    @Test(arguments: [SignupPasswordSample.empty, .sevenASCII, .sixUTF16, .aboveByteLimit])
    func signupRejectsInvalidPasswordWithoutSendingIt(sample: SignupPasswordSample) async {
        let service = SessionFixture(signedIn: false), storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        await controller.signUp(email: "fixture@example.test", password: sample.value)
        #expect(controller.phase == .signedOut && controller.issue == .invalidInput)
        #expect(await service.signupRequests.isEmpty)
        #expect(await storage.activations.isEmpty)
        #expect(await service.identity() == nil)
    }

    @Test(arguments: [SignupPasswordSample.eightASCII, .eightUTF16, .maximumBytes])
    func signupUsesResearchNameAndPasswordUTF16Contract(sample: SignupPasswordSample) async {
        let service = SessionFixture(signedIn: false), storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        await controller.signUp(email: "fixture@example.test", password: sample.value)
        #expect(controller.phase == .signedIn && controller.issue == nil)
        #expect(
            await service.signupRequests
                == [.init(email: "fixture@example.test", password: sample.value, name: "ANON")])
        #expect(await storage.activations.count == 1)
    }

    @Test func verificationRefreshPreservesScopeAndIgnoresReplacementAuthority() async throws {
        let service = SessionFixture(); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        let scope = try #require(controller.account?.scope)
        await service.setVerified()
        await controller.refreshUser()
        #expect(controller.account?.scope == scope)
        #expect(controller.account?.user.emailVerified == true)
        #expect(await storage.activations.count == 1)
        await service.signOut()
        await controller.refreshUser()
        #expect(controller.account?.scope == scope)
        #expect(controller.phase == .signedIn)
    }

    @Test func delayedProfileCannotRestoreSignedOutAccount() async {
        let service = SessionFixture(); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        await service.configure(hold: true)
        let refresh = Task { await controller.refreshUser() }
        await service.awaitUserRequest()
        await controller.signOut()
        await service.releaseUser()
        await refresh.value
        #expect(controller.account == nil)
        #expect(controller.phase == .signedOut)
    }

    @Test func externalAuthenticationOwnsOneTicketAndResolvesCommittedAuthority() async throws {
        let service = SessionFixture(signedIn: false); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        let ticket = try #require(controller.beginExternalAuthentication())
        #expect(controller.beginExternalAuthentication() == nil)
        await service.signIn(email: "synthetic", password: "synthetic")
        await controller.finishExternalAuthentication(ticket)
        #expect(controller.phase == .signedIn)
        await controller.cancelExternalAuthentication(ticket)
        #expect(controller.phase == .signedIn)
    }

    @Test func cancelledExternalAttemptReturnsToSignInAndStaleCompletionCannotPublish() async throws {
        let service = SessionFixture(signedIn: false); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        var cancelled = 0
        controller.onCancelExternal = { cancelled += 1 }
        let ticket = try #require(controller.beginExternalAuthentication())
        await controller.cancelExternalAuthentication(ticket)
        #expect(controller.phase == .signedOut)
        #expect(cancelled == 1)
        await controller.signOut()
        await controller.finishExternalAuthentication(ticket)
        #expect(controller.account == nil)
        #expect(await storage.activations.isEmpty)
    }

    @Test func offlineRestoreDoesNotDeleteCredentialsOrDrafts() async {
        let service = SessionFixture(); let storage = LifecycleFixture()
        await service.configure(fail: true)
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        #expect(controller.phase == .unavailable)
        #expect(controller.issue == .connection)
        #expect(await service.identity() != nil)
        #expect(await storage.clears == 0)
        #expect(await storage.activations.isEmpty)
        await service.configure()
        await controller.restore()
        #expect(controller.phase == .signedIn)
    }

    @Test func samePrincipalReloginOwnsNewAppGeneration() async throws {
        let service = SessionFixture(); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        let first = try #require(controller.account?.scope)
        await controller.signOut()
        #expect(controller.phase == .signedOut)
        await controller.signIn(email: "fixture@example.test", password: "synthetic")
        let second = try #require(controller.account?.scope)
        #expect(first.principalID == second.principalID)
        #expect(first.generation != second.generation)
        #expect(await storage.retirements == [first])
    }

    @Test func delayedRestoreCannotPublishAfterSignOut() async {
        let service = SessionFixture(); let storage = LifecycleFixture()
        await service.configure(hold: true)
        let controller = SessionController(service: service, storage: storage)
        let restore = Task { await controller.restore() }
        await service.awaitUserRequest()
        await controller.signOut()
        await service.releaseUser()
        await restore.value
        #expect(controller.phase == .signedOut)
        #expect(controller.account == nil)
        #expect(await storage.activations.isEmpty)
        #expect(await service.identity() == nil)
    }

    @Test func externalRetirementFailureStillRevokesCredentialsAndCleansDrafts() async throws {
        let service = SessionFixture(); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        let scope = try #require(controller.account?.scope)
        let failure = RetirementFailureControl()
        controller.onRetire = { retired in
            #expect(retired == scope)
            if failure.shouldFail { throw ChatStorageError.cleanupRequired }
        }
        await controller.signOut()
        #expect(controller.account == nil)
        #expect(await service.identity() == nil)
        #expect(await storage.retirements == [scope])
        #expect(controller.phase == .unavailable)
        #expect(controller.issue == .cleanup)
        await controller.signIn(email: "replacement@example.test", password: "synthetic")
        #expect(await service.loginCount == 0)
        failure.shouldFail = false
        await controller.signOut()
        #expect(controller.phase == .signedOut)
        await controller.signIn(email: "replacement@example.test", password: "synthetic")
        #expect(await service.loginCount == 1)
    }

    @Test func cleanupFailureBlocksReplacementSignInUntilRetry() async {
        let service = SessionFixture(); let storage = LifecycleFixture()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        await storage.setFailure(true)
        await controller.signOut()
        #expect(controller.phase == .unavailable)
        #expect(controller.issue == .cleanup)
        #expect(controller.account == nil)
        #expect(await service.identity() == nil)
        await controller.signIn(email: "replacement@example.test", password: "synthetic")
        #expect(await service.loginCount == 0)
        await storage.setFailure(false)
        await controller.signOut()
        #expect(controller.phase == .signedOut)
        await controller.signIn(email: "replacement@example.test", password: "synthetic")
        #expect(await service.loginCount == 1)
        #expect(controller.phase == .signedIn)
    }
}
