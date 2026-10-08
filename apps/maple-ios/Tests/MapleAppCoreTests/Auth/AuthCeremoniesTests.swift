import Foundation
import MapleSDK
import Synchronization
import Testing
@testable import MapleAppCore

private final class CeremonySecureStore: SecureStore, Sendable {
    private let values = Mutex<[String: Data]>([:])
    func read(key: String) throws -> Data? { values.withLock { $0[key] } }
    func write(_ data: Data, key: String) throws { values.withLock { $0[key] = data } }
    func remove(key: String) throws { values.withLock { _ = $0.removeValue(forKey: key) } }
}
private final class CeremonyClock: Sendable {
    private let date = Mutex(Date(timeIntervalSince1970: 1_800_000_000))
    func now() -> Date { date.withLock { $0 } }
    func advance(_ interval: TimeInterval) { date.withLock { $0 = $0.addingTimeInterval(interval) } }
}
private func ceremonyToken(_ kind: String, principal: String) throws -> String {
    func url(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
    let body = try JSONSerialization.data(withJSONObject: [
        "sub": principal, "exp": 4_000_000_000, "tf": 2,
        "aud": "urn:opensecret:internal:transport-v2:user:\(kind)-token",
    ])
    return url(Data("{}".utf8)) + "." + url(body) + "." + url(Data("fixture".utf8))
}
private actor CeremonyFixture: AuthCeremonyServing {
    let authority: CredentialAuthority
    let clock: CeremonyClock
    var resetRequests: [(String, String)] = []
    var resetConfirmations: [(String, String, String, String)] = []
    var deletionRequests: [String] = []
    var deletionConfirmations = 0
    var appleNonces: [String] = []
    var snapshotReads = 0
    var browserPreparations: [UUID] = []
    var browserRedeems = 0
    var browserInstalls = 0
    var browserCancels: [UUID] = []
    var deletionError: ResearchAPIError?
    var resetError: (any Error)?
    var gate: ChatTestGate?
    var providerGate: ChatTestGate?
    var deletedGate: ChatTestGate?
    var resetCompletedGate: ChatTestGate?
    init(clock: CeremonyClock) throws {
        self.clock = clock
        authority = try CredentialAuthority(apiURL: "http://127.0.0.1:1234", store: CeremonySecureStore())
    }
    func configure(gate: ChatTestGate? = nil, providerGate: ChatTestGate? = nil, deletionError: ResearchAPIError? = nil)
    {
        self.gate = gate; self.providerGate = providerGate; self.deletionError = deletionError
    }
    func install(_ principal: String = "account-a") async throws {
        try await authority.clear()
        let snapshot = try await authority.snapshot()
        _ = try await authority.install(
            accessToken: ceremonyToken("access", principal: principal),
            refreshToken: ceremonyToken("refresh", principal: principal), expected: snapshot)
    }
    func setResetError(_ error: (any Error)?) { resetError = error }
    func holdResetResult(_ gate: ChatTestGate) { resetCompletedGate = gate }
    func holdDeletedResult(_ gate: ChatTestGate) { deletedGate = gate }
    func snapshot() async throws -> CredentialSnapshot {
        snapshotReads += 1
        return try await authority.snapshot()
    }
    func check(_ expected: CredentialSnapshot) async throws {
        try Task.checkCancellation()
        guard try await snapshot() == expected else { throw TransportError.authorityChanged }
    }
    func requestPasswordReset(email: String, hashedSecret: String, expected: CredentialSnapshot) async throws {
        try await check(expected)
        if let resetError { throw resetError }
        resetRequests.append((email, hashedSecret))
        if let gate { await gate.hold() }
    }
    func confirmPasswordReset(
        email: String, code: String, plaintextSecret: String, newPassword: String,
        expected: CredentialSnapshot
    ) async throws {
        try await check(expected); resetConfirmations.append((email, code, plaintextSecret, newPassword))
        if let resetCompletedGate { await resetCompletedGate.hold() }
    }
    func verifyEmail(code: String, expected: CredentialSnapshot) async throws {
        try await check(expected); if let gate { await gate.hold() }
    }
    func resendVerification(expected: CredentialSnapshot) async throws { try await check(expected) }
    func changePassword(currentPassword: String, newPassword: String, expected: CredentialSnapshot) async throws {
        try await check(expected)
    }
    func requestAccountDeletion(hashedSecret: String, expected: CredentialSnapshot) async throws {
        try await check(expected); deletionRequests.append(hashedSecret)
    }
    func confirmAccountDeletion(code: String, plaintextSecret: String, expected: CredentialSnapshot) async throws {
        try await check(expected); deletionConfirmations += 1
        if let deletionError { throw deletionError }
        try await authority.clear(expected: expected)
        if let deletedGate { await deletedGate.hold() }
    }
    func signInWithApple(_ credential: AppleSignInCredential, rawNonce: String, expected: CredentialSnapshot)
        async throws
    {
        try await check(expected); appleNonces.append(rawNonce)
        if let providerGate { await providerGate.hold() }
        try await check(expected)
        _ = try await authority.install(
            accessToken: ceremonyToken("access", principal: "apple-account"),
            refreshToken: ceremonyToken("refresh", principal: "apple-account"), expected: expected)
    }
    func prepareBrowser(id: UUID, expected: CredentialSnapshot) async throws -> BrowserHandoffReservation {
        browserPreparations.append(id)
        try await check(expected)
        return BrowserHandoffReservation(
            id: id, sessionID: String(repeating: "a", count: 32),
            requestID: String(repeating: "b", count: 32), expiresAt: clock.now().addingTimeInterval(600))
    }
    func redeemBrowser(
        _ reservation: BrowserHandoffReservation, grant: NativeOAuthHandoffGrant,
        expected: CredentialSnapshot
    ) async throws -> BrowserAccountConfirmation {
        try await check(expected); browserRedeems += 1
        if let providerGate { await providerGate.hold() }
        try await check(expected)
        return BrowserAccountConfirmation(
            id: reservation.id, principalID: "browser-account", email: "browser@example.test")
    }
    func confirmBrowser(_ confirmation: BrowserAccountConfirmation, expected: CredentialSnapshot) async throws {
        try await check(expected); browserInstalls += 1
        _ = try await authority.install(
            accessToken: ceremonyToken("access", principal: confirmation.principalID),
            refreshToken: ceremonyToken("refresh", principal: confirmation.principalID), expected: expected)
    }
    func cancelBrowser(id: UUID) { browserCancels.append(id) }
}

@MainActor struct AuthCeremoniesTests {
    private let secret = String(repeating: "d", count: 64)
    private func make(signedIn: Bool = false, callbackScheme: String = "cloud.opensecret.maple.dev") async throws
        -> (AuthCeremonies, CeremonyFixture, CeremonyClock)
    {
        let clock = CeremonyClock(), service = try CeremonyFixture(clock: clock)
        if signedIn { try await service.install() }
        let config = try AppConfiguration(
            profile: .local, apiURL: "http://127.0.0.1:1234", billingURL: "http://127.0.0.1:1235",
            flagsURL: "http://127.0.0.1:1236", authURL: "http://127.0.0.1:1237",
            clientID: UUID().uuidString, callbackScheme: callbackScheme)
        let scope = signedIn ? AccountScope(apiOrigin: config.apiURL.absoluteString, principalID: "account-a") : nil
        let value = secret
        return (
            AuthCeremonies(
                service: service, configuration: config, scope: scope,
                accountEmail: signedIn ? "account@example.test" : nil, now: { clock.now() }, makeSecret: { value }),
            service, clock
        )
    }
    private func callback(scheme: String = "cloud.opensecret.maple.dev") -> URL {
        URL(string: "\(scheme)://auth?handoff_grant=e30.e30.e30")!
    }

    @Test func typedClockFailureDoesNotCreateChallengeAndCanBeRetried() async throws {
        let (flow, service, clock) = try await make()
        let error = try #require(
            AttestationClockSkewError(
                validatingDeviceTime: clock.now().addingTimeInterval(-600), enclaveTime: clock.now(),
                certificateIndex: 0, notBefore: clock.now().addingTimeInterval(-60),
                notAfter: clock.now().addingTimeInterval(3_600)))
        await service.setResetError(NSError(domain: "Fixture", code: 1, userInfo: [NSUnderlyingErrorKey: error]))
        await flow.requestPasswordReset(email: "first@example.test")
        #expect(flow.state == .idle)
        #expect(flow.issue == .clock(.deviceTime))
        #expect(await service.resetRequests.isEmpty)
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(await service.resetConfirmations.isEmpty)
        await service.setResetError(nil)
        await flow.requestPasswordReset(email: "first@example.test")
        #expect(flow.issue == nil)
        #expect(flow.state == .resetCode(email: "first@example.test"))
        #expect(await service.resetRequests.count == 1)
    }

    @Test func resetBindsOriginalEmailAndSendsHashBeforePrivateSecret() async throws {
        let (flow, service, _) = try await make()
        await flow.requestPasswordReset(email: "  first@example.test  ")
        #expect(flow.state == .resetCode(email: "first@example.test"))
        #expect(await service.resetRequests.first?.0 == "first@example.test")
        #expect(await service.resetRequests.first?.1 == hashSecret(secret))
        await flow.requestPasswordReset(email: "replacement@example.test")
        await flow.confirmPasswordReset(code: "BAD", newPassword: "password", confirmation: "password")
        #expect(await service.resetConfirmations.isEmpty)
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(await service.resetConfirmations.first?.0 == "first@example.test")
        #expect(await service.resetConfirmations.first?.2 == secret)
        #expect(flow.state == .completed(.passwordReset))
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(await service.resetConfirmations.count == 1)
    }

    @Test func accountResetPreservesSessionUntilSuccessAndRejectsReplacementGeneration() async throws {
        let (flow, service, _) = try await make(signedIn: true)
        let original = try await service.snapshot()
        await flow.requestPasswordReset(email: "account@example.test")
        #expect(flow.state == .resetCode(email: "account@example.test"))
        #expect(try await service.snapshot() == original)
        try await service.install()
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(flow.issue == .changedAccount)
        #expect(await service.resetConfirmations.isEmpty)
    }

    @Test func accountResetRejectsAnotherDestinationWithoutRequestOrCleanup() async throws {
        let (flow, service, _) = try await make(signedIn: true)
        var cleanupCalls = 0
        flow.onPasswordResetResult = { _ in cleanupCalls += 1 }
        await flow.requestPasswordReset(email: "different@example.test")
        #expect(flow.issue == .invalidInput && flow.state == .idle)
        #expect(await service.resetRequests.isEmpty)
        #expect(cleanupCalls == 0)
    }

    @Test func accountResetSucceedsWithCapturedEmailAndOriginalAuthority() async throws {
        let (flow, service, _) = try await make(signedIn: true)
        await flow.requestPasswordReset(email: "account@example.test")
        await flow.requestPasswordReset(email: "different@example.test")
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(flow.state == .completed(.passwordReset))
        #expect(await service.resetConfirmations.first?.0 == "account@example.test")
    }

    @Test func confirmedResetAfterCancellationCleansCapturedScopeWithoutReplacingNewState() async throws {
        let (flow, service, _) = try await make(signedIn: true), gate = ChatTestGate()
        var cleaned: [AccountScope] = []
        flow.onPasswordResetResult = { cleaned.append($0) }
        await service.holdResetResult(gate)
        await flow.requestPasswordReset(email: "account@example.test")
        let confirming = Task {
            await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        }
        await gate.waitForArrival(); await flow.cancel()
        await flow.requestAccountDeletion(confirmation: "DELETE")
        #expect(flow.state == .deletionCode)
        await gate.release(); await confirming.value
        #expect(cleaned == [try #require(flow.scope)])
        #expect(flow.state == .deletionCode)
    }

    @Test(arguments: [901.0, -1.0]) func expiredOrBackwardsClockDropsResetSecret(age: Double) async throws {
        let (flow, service, clock) = try await make()
        await flow.requestPasswordReset(email: "first@example.test")
        clock.advance(age)
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(flow.issue == .expired)
        #expect(flow.state == .idle)
        #expect(await service.resetConfirmations.isEmpty)
    }

    @Test func canceledResetRequestCannotResurrectConfirmation() async throws {
        let (flow, service, _) = try await make(), gate = ChatTestGate()
        await service.configure(gate: gate)
        let request = Task { await flow.requestPasswordReset(email: "first@example.test") }
        await gate.waitForArrival()
        await flow.cancel()
        await gate.release(); await request.value
        #expect(flow.state == .idle)
        await flow.confirmPasswordReset(code: "ABCD1234", newPassword: "password", confirmation: "password")
        #expect(await service.resetConfirmations.isEmpty)
    }

    @Test func deletionRequiresFenceAndBillingFailureRemainsSignedInForRetry() async throws {
        let (flow, service, _) = try await make(signedIn: true)
        await flow.requestAccountDeletion(confirmation: "delete")
        #expect(await service.deletionRequests.isEmpty)
        await flow.requestAccountDeletion(confirmation: "DELETE")
        #expect(await service.deletionRequests == [hashSecret(secret)])
        let code = UUID().uuidString
        await flow.confirmAccountDeletion(code: code)
        #expect(flow.issue == .cleanupRequired)
        #expect(await service.deletionConfirmations == 0)
        var fenced = 0, results: [Bool] = []
        flow.beforeAccountDeletion = { _ in fenced += 1 }
        flow.onAccountDeletionResult = { _, deleted in results.append(deleted) }
        await service.configure(deletionError: .http(status: 503, reason: .billingCleanupUnavailable))
        await flow.confirmAccountDeletion(code: code)
        #expect(flow.issue == .billingCleanupUnavailable)
        #expect(flow.state == .deletionCode)
        #expect(try await service.snapshot().principalId == "account-a")
        await service.configure()
        await flow.confirmAccountDeletion(code: code)
        #expect(flow.state == .completed(.accountDeleted))
        #expect(try await service.snapshot().principalId == nil)
        #expect(fenced == 2 && results == [false, true])
    }

    @Test func sameAccountReloginInvalidatesDeletionChallenge() async throws {
        let (flow, service, _) = try await make(signedIn: true)
        await flow.requestAccountDeletion(confirmation: "DELETE")
        try await service.install()
        flow.beforeAccountDeletion = { _ in Issue.record("Stale deletion must not acquire a fence") }
        await flow.confirmAccountDeletion(code: UUID().uuidString)
        #expect(flow.issue == .changedAccount)
        #expect(await service.deletionConfirmations == 0)
    }

    @Test func confirmedDeletionAfterCancellationCleansOldScopeWithoutReplacingNewState() async throws {
        let (flow, service, _) = try await make(signedIn: true), gate = ChatTestGate()
        var deletedResults: [Bool] = []
        flow.beforeAccountDeletion = { _ in }
        flow.onAccountDeletionResult = { _, deleted in deletedResults.append(deleted) }
        await flow.requestAccountDeletion(confirmation: "DELETE")
        await service.holdDeletedResult(gate)
        let deletion = Task { await flow.confirmAccountDeletion(code: UUID().uuidString) }
        await gate.waitForArrival()
        await flow.cancel()
        try await service.install()
        await flow.requestAccountDeletion(confirmation: "DELETE")
        #expect(flow.state == .deletionCode)
        await gate.release(); await deletion.value
        #expect(flow.state == .deletionCode)
        #expect(deletedResults == [true])
        #expect(try await service.snapshot().principalId == "account-a")
    }

    @Test func appleStateAndNonceAreOwnedAndCallbackIsSingleUse() async throws {
        let (flow, service, _) = try await make()
        let request = try #require(await flow.beginAppleSignIn())
        #expect(request.nonce == hashSecret(secret) && request.nonce != secret)
        await flow.completeAppleSignIn(
            id: UUID(), credential: .init(identityToken: "fixture", userIdentifier: "id", state: request.state))
        #expect(await service.appleNonces.isEmpty)
        await flow.completeAppleSignIn(
            id: request.id, credential: .init(identityToken: "fixture", userIdentifier: "id", state: "wrong"))
        #expect(flow.issue == .invalidCallback)
        #expect(await service.appleNonces.isEmpty)
        await flow.completeAppleSignIn(
            id: request.id, credential: .init(identityToken: "fixture", userIdentifier: "id", state: request.state))
        #expect(await service.appleNonces.isEmpty)
        let next = try #require(await flow.beginAppleSignIn())
        await flow.completeAppleSignIn(
            id: next.id, credential: .init(identityToken: "fixture", userIdentifier: "id", state: next.state))
        #expect(await service.appleNonces == [secret])
        #expect(flow.state == .completed(.signedIn))
    }

    @Test func canceledAppleNetworkCompletionCannotInstall() async throws {
        let (flow, service, _) = try await make(), gate = ChatTestGate()
        await service.configure(providerGate: gate)
        let request = try #require(await flow.beginAppleSignIn())
        let completion = Task {
            await flow.completeAppleSignIn(
                id: request.id, credential: .init(identityToken: "fixture", userIdentifier: "id", state: request.state))
        }
        await gate.waitForArrival()
        flow.invalidate()
        await gate.release(); await completion.value
        #expect(try await service.snapshot().principalId == nil)
        #expect(flow.state == .idle)
    }

    @Test(arguments: [OAuthProvider.google, .github])
    func unsupportedBrowserTargetDoesNotReserveOrConsumeAuthority(_ provider: OAuthProvider) async throws {
        // Synthetic unsupported scheme; not a selected application bundle identity.
        let (flow, service, _) = try await make(callbackScheme: "cloud.opensecret.maple.fixture-unregistered")
        let original = try await service.snapshot()
        let reads = await service.snapshotReads
        for _ in 0..<2 {
            #expect(await flow.beginBrowserSignIn(provider: provider) == nil)
            #expect(flow.state == .idle)
            #expect(flow.issue == .browserSignInUnavailable)
        }
        await flow.redeemBrowserCallback(id: UUID(), url: callback())
        await flow.confirmBrowserAccount(id: UUID())
        await flow.cancel()
        #expect(await service.snapshotReads == reads)
        #expect(await service.browserPreparations.isEmpty)
        #expect(await service.browserRedeems == 0)
        #expect(await service.browserInstalls == 0)
        #expect(await service.browserCancels.isEmpty)
        #expect(try await service.snapshot() == original)
        // The unavailable provider target does not retire unrelated account flows.
        await flow.requestPasswordReset(email: "first@example.test")
        #expect(flow.state == .resetCode(email: "first@example.test"))
        #expect(flow.issue == nil)
        #expect(await service.resetRequests.count == 1)
    }

    @Test(arguments: ["cloud.opensecret.maple", "cloud.opensecret.maple.dev"])
    func supportedBrowserTargetsKeepTheirExactBrokerMapping(_ scheme: String) async throws {
        let (flow, service, _) = try await make(callbackScheme: scheme)
        let request = try #require(await flow.beginBrowserSignIn(provider: .github))
        let parts = try #require(URLComponents(url: request.url, resolvingAgainstBaseURL: false))
        let variant = parts.queryItems?.first(where: { $0.name == "native_app_variant" })?.value
        #expect(request.callbackScheme == scheme)
        #expect(parts.path == "/desktop-auth")
        #expect(parts.queryItems?.first(where: { $0.name == "provider" })?.value == "github")
        #expect(parts.queryItems?.first(where: { $0.name == "transport" })?.value == "v2")
        #expect(variant == (scheme == "cloud.opensecret.maple.dev" ? "dev" : nil))
        #expect(await service.browserPreparations == [request.id])
        await flow.cancel()
        #expect(await service.browserCancels == [request.id])
    }

    @Test func browserLocalV2RouteRequiresExplicitVerifiedAccountConfirmation() async throws {
        let (flow, service, _) = try await make()
        let request = try #require(await flow.beginBrowserSignIn(provider: .google))
        let parts = try #require(URLComponents(url: request.url, resolvingAgainstBaseURL: false))
        #expect(parts.host == "127.0.0.1" && parts.port == 1237 && parts.path == "/desktop-auth")
        #expect(parts.queryItems?.first(where: { $0.name == "native_app_variant" })?.value == "dev")
        #expect(parts.queryItems?.first(where: { $0.name == "transport" })?.value == "v2")
        await flow.redeemBrowserCallback(id: request.id, url: callback())
        #expect(
            flow.state
                == .confirmBrowser(.init(id: request.id, principalID: "browser-account", email: "browser@example.test"))
        )
        #expect(await service.browserInstalls == 0)
        #expect(try await service.snapshot().principalId == nil)
        await flow.redeemBrowserCallback(id: request.id, url: callback())
        #expect(await service.browserRedeems == 1)
        await flow.confirmBrowserAccount(id: request.id)
        await flow.confirmBrowserAccount(id: request.id)
        #expect(await service.browserInstalls == 1)
        #expect(flow.state == .completed(.signedIn))
    }

    @Test func wrongSchemeConsumesBrowserAttemptWithoutRedemption() async throws {
        let (flow, service, _) = try await make()
        let request = try #require(await flow.beginBrowserSignIn(provider: .github))
        await flow.redeemBrowserCallback(id: request.id, url: callback(scheme: "cloud.opensecret.maple"))
        #expect(flow.issue == .invalidCallback)
        #expect(await service.browserRedeems == 0)
        #expect(await service.browserCancels == [request.id])
        await flow.redeemBrowserCallback(id: request.id, url: callback())
        #expect(await service.browserRedeems == 0)
    }

    @Test func confirmationExpiryNeverInstallsRedeemedBrowserCredentials() async throws {
        let (flow, service, clock) = try await make()
        let request = try #require(await flow.beginBrowserSignIn(provider: .google))
        await flow.redeemBrowserCallback(id: request.id, url: callback())
        clock.advance(601)
        await flow.confirmBrowserAccount(id: request.id)
        #expect(flow.issue == .expired)
        #expect(await service.browserInstalls == 0)
        #expect(await service.browserCancels == [request.id])
    }

    @Test func delayedVerificationCannotPublishForReplacementAccount() async throws {
        let (flow, service, _) = try await make(signedIn: true), gate = ChatTestGate()
        await service.configure(gate: gate)
        let verification = Task { await flow.verifyEmail(code: UUID().uuidString) }
        await gate.waitForArrival()
        try await service.install()
        await gate.release(); await verification.value
        #expect(flow.issue == .changedAccount)
        #expect(flow.state == .idle)
    }
}
