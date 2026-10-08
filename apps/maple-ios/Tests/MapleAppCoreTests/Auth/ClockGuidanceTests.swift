import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

private func clockDiagnostic(seconds: TimeInterval = 3 * 86_400) throws -> AttestationClockSkewError {
    let signedAt = Date(timeIntervalSince1970: 1_800_000_000)
    return try #require(
        AttestationClockSkewError(
            validatingDeviceTime: signedAt.addingTimeInterval(seconds), enclaveTime: signedAt,
            certificateIndex: 0, notBefore: signedAt.addingTimeInterval(-600),
            notAfter: signedAt.addingTimeInterval(3_600)))
}

private actor ClockSessionService: SessionServing {
    private var current: SessionIdentity?
    private var failure: (any Error)?
    private(set) var revocations = 0
    init(signedIn: Bool, failure: any Error) {
        self.failure = failure
        if signedIn { current = Self.identityValue() }
    }
    private static func identityValue() -> SessionIdentity {
        SessionIdentity(origin: "http://127.0.0.1:1234", principal: "clock-fixture", generation: UUID())
    }
    func identity() -> SessionIdentity? { current }
    func clearFailure() { failure = nil }
    func user() throws -> MapleUser {
        if let failure { throw failure }
        return MapleUser(
            id: "clock-fixture", email: "clock@example.test", emailVerified: true,
            loginMethod: "email", createdAt: "", updatedAt: "")
    }
    func signIn(email: String, password: String) throws {
        if let failure { throw failure }
        current = Self.identityValue()
    }
    func signInGuest(id: String, password: String) throws { try signIn(email: id, password: password) }
    func signUp(email: String, password: String, name: String?) throws { try signIn(email: email, password: password) }
    func signOut() { revocations += 1; current = nil }
}
private actor ClockStorage: AccountDataLifecycle {
    private(set) var clears = 0
    private(set) var retirements = 0
    private(set) var activations = 0
    func activate(_ scope: AccountScope) { activations += 1 }
    func retire(_ scope: AccountScope) { retirements += 1 }
    func clearSignedOutData() { clears += 1 }
}

@MainActor struct ClockGuidanceTests {
    @Test func classificationUsesTypedBoundedChainAndDistinguishesNearCertificateFailure() throws {
        #expect(AttestationClockIssue.classify(try clockDiagnostic()) == .deviceTime)
        #expect(AttestationClockIssue.classify(try clockDiagnostic(seconds: 30)) == .certificateTiming)
        var wrapped: any Error = try clockDiagnostic()
        for _ in 0..<7 { wrapped = NSError(domain: "Fixture", code: 1, userInfo: [NSUnderlyingErrorKey: wrapped]) }
        #expect(AttestationClockIssue.classify(wrapped) == .deviceTime)
        wrapped = NSError(domain: "Fixture", code: 1, userInfo: [NSUnderlyingErrorKey: wrapped])
        #expect(AttestationClockIssue.classify(wrapped) == nil)
        let spoofed = NSError(
            domain: "AttestationClockSkewError", code: 1,
            userInfo: [NSLocalizedDescriptionKey: "Check the device date, time and time zone settings"])
        #expect(AttestationClockIssue.classify(spoofed) == nil)
        #expect(AttestationClockIssue.classify(TrustError.certificateChain) == nil)
        #expect(AttestationClockIssue.classify(ResearchAPIError.http(status: 401, reason: .invalidLogin)) == nil)
    }

    @Test func failedLoginKeepsFormAvailableAndExplicitRetryInstallsAccount() async throws {
        let wrapped = NSError(domain: "Fixture", code: 1, userInfo: [NSUnderlyingErrorKey: try clockDiagnostic()])
        let service = ClockSessionService(signedIn: false, failure: wrapped), storage = ClockStorage()
        let controller = SessionController(service: service, storage: storage)
        await controller.restore()
        await controller.signIn(email: "clock@example.test", password: "synthetic")
        #expect(controller.phase == .signedOut)
        #expect(controller.issue == .clock(.deviceTime))
        #expect(controller.account == nil)
        #expect(await service.identity() == nil)
        #expect(await service.revocations == 0)
        #expect(await storage.activations == 0)
        await service.clearFailure()
        await controller.signIn(email: "clock@example.test", password: "synthetic")
        #expect(controller.phase == .signedIn)
        #expect(controller.issue == nil)
        #expect(await storage.activations == 1)
    }

    @Test func clockFailureDuringRestorePreservesCredentialsAndPrivateWorkForRetry() async throws {
        let service = ClockSessionService(signedIn: true, failure: try clockDiagnostic(seconds: 30))
        let storage = ClockStorage()
        let ownedController = SessionController(service: service, storage: storage)
        let identity = await service.identity()
        await ownedController.restore()
        #expect(ownedController.phase == .unavailable)
        #expect(ownedController.issue == .clock(.certificateTiming))
        #expect(await service.identity() == identity)
        #expect(await service.revocations == 0)
        #expect(await storage.clears == 0)
        #expect(await storage.retirements == 0)
        #expect(await storage.activations == 0)
        await service.clearFailure()
        await ownedController.restore()
        #expect(ownedController.phase == .signedIn)
        #expect(await service.identity() == identity)
        #expect(await storage.activations == 1)
    }
}
