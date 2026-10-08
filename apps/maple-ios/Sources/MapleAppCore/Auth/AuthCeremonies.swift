import Foundation
import MapleSDK
import Observation

public struct BrowserHandoffReservation: Sendable, Equatable {
    public let id: UUID
    public let sessionID: String
    public let requestID: String
    public let expiresAt: Date
    public init(id: UUID, sessionID: String, requestID: String, expiresAt: Date) {
        self.id = id; self.sessionID = sessionID; self.requestID = requestID; self.expiresAt = expiresAt
    }
}

public struct BrowserAccountConfirmation: Sendable, Equatable {
    public let id: UUID
    public let principalID: String
    public let email: String?
    public init(id: UUID, principalID: String, email: String?) {
        self.id = id; self.principalID = principalID; self.email = email
    }
}

public struct BrowserSignInRequest: Sendable, Equatable {
    public let id: UUID
    public let url: URL
    public let callbackScheme: String
    public let expiresAt: Date
}

public struct AppleSignInRequest: Sendable, Equatable {
    public let id: UUID
    public let nonce: String
    public let state: String
    public let expiresAt: Date
}

/// Tokens and the raw nonce never become observable view state or persisted data.
public struct AppleSignInCredential: Sendable {
    public let identityToken: String
    public let userIdentifier: String
    public let state: String?
    public let email: String?
    public let givenName: String?
    public let familyName: String?
    public init(
        identityToken: String, userIdentifier: String, state: String?, email: String? = nil,
        givenName: String? = nil, familyName: String? = nil
    ) {
        self.identityToken = identityToken; self.userIdentifier = userIdentifier; self.state = state
        self.email = email; self.givenName = givenName; self.familyName = familyName
    }
}

public protocol AuthCeremonyServing: Sendable {
    func snapshot() async throws -> CredentialSnapshot
    func requestPasswordReset(email: String, hashedSecret: String, expected: CredentialSnapshot) async throws
    func confirmPasswordReset(
        email: String, code: String, plaintextSecret: String, newPassword: String,
        expected: CredentialSnapshot) async throws
    func verifyEmail(code: String, expected: CredentialSnapshot) async throws
    func resendVerification(expected: CredentialSnapshot) async throws
    func changePassword(currentPassword: String, newPassword: String, expected: CredentialSnapshot) async throws
    func requestAccountDeletion(hashedSecret: String, expected: CredentialSnapshot) async throws
    func confirmAccountDeletion(code: String, plaintextSecret: String, expected: CredentialSnapshot) async throws
    func signInWithApple(
        _ credential: AppleSignInCredential, rawNonce: String,
        expected: CredentialSnapshot) async throws
    func prepareBrowser(id: UUID, expected: CredentialSnapshot) async throws -> BrowserHandoffReservation
    func redeemBrowser(
        _ reservation: BrowserHandoffReservation, grant: NativeOAuthHandoffGrant,
        expected: CredentialSnapshot
    ) async throws -> BrowserAccountConfirmation
    func confirmBrowser(_ confirmation: BrowserAccountConfirmation, expected: CredentialSnapshot) async throws
    func cancelBrowser(id: UUID) async
}

public enum AuthCeremonyOutcome: Sendable, Equatable {
    case verificationSent, emailVerified, passwordReset, passwordChanged, accountDeleted, signedIn
}
public enum AuthCeremonyState: Sendable, Equatable {
    case idle, busy, resetCode(email: String), deletionCode
    case apple(AppleSignInRequest), browser(BrowserSignInRequest), confirmBrowser(BrowserAccountConfirmation)
    case completed(AuthCeremonyOutcome)
}
public enum AuthCeremonyIssue: Error, Sendable, Equatable {
    case invalidInput, expired, changedAccount, invalidCallback, connection, invalidCredentials
    case billingCleanupUnavailable, cleanupRequired, deletionUnconfirmed, browserSignInUnavailable
    case clock(AttestationClockIssue)
}

/// One screen/flow owner. Password reset and deletion secrets remain in memory,
/// bound to the original email/account and a bounded lifetime. Invalidation
/// cancels the actual task, so SDK authority commits observe cancellation too.
@MainActor @Observable public final class AuthCeremonies {
    public private(set) var state: AuthCeremonyState = .idle
    public private(set) var issue: AuthCeremonyIssue?
    public let scope: AccountScope?
    public let accountEmail: String?
    @ObservationIgnored private let service: any AuthCeremonyServing
    @ObservationIgnored private let configuration: AppConfiguration
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private let makeSecret: @Sendable () throws -> String
    @ObservationIgnored private var operation = UUID()
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var expirationTask: Task<Void, Never>?
    @ObservationIgnored private var retired = false
    @ObservationIgnored private var challenge: Challenge?
    @ObservationIgnored private var apple: AppleAttempt?
    @ObservationIgnored private var browser: BrowserAttempt?
    /// Must quiesce account work and retain an admission fence before deletion.
    @ObservationIgnored public var beforeAccountDeletion: (@MainActor (AccountScope) async throws -> Void)?
    /// Release the fence on failure; retire account data only after confirmed success.
    @ObservationIgnored public var onAccountDeletionResult: (@MainActor (AccountScope, Bool) async -> Void)?

    /// Confirmed password reset revokes the captured local account even after screen cancellation.
    @ObservationIgnored public var onPasswordResetResult: (@MainActor (AccountScope) async -> Void)?

    private struct Challenge {
        let secret: String
        let email: String?
        let started: Date
        let expected: CredentialSnapshot
    }
    private struct AppleAttempt {
        let request: AppleSignInRequest
        let rawNonce: String
        let started: Date
        let expected: CredentialSnapshot
    }
    private struct BrowserAttempt {
        let reservation: BrowserHandoffReservation
        let started: Date
        let expected: CredentialSnapshot
    }

    public init(
        service: any AuthCeremonyServing, configuration: AppConfiguration, scope: AccountScope? = nil,
        accountEmail: String? = nil,
        now: @escaping @Sendable () -> Date = { Date() },
        makeSecret: @escaping @Sendable () throws -> String = generateSecureSecret
    ) {
        self.service = service; self.configuration = configuration; self.scope = scope
        self.accountEmail = accountEmail?.trimmingCharacters(in: .whitespacesAndNewlines)
        self.now = now; self.makeSecret = makeSecret
    }

    public var isBusy: Bool { state == .busy }
    private var mayStart: Bool {
        if case .completed = state { return !retired }
        return !retired && state == .idle
    }

    public func requestPasswordReset(email: String) async {
        let email = email.trimmingCharacters(in: .whitespacesAndNewlines)
        guard mayStart else { return }
        guard !email.isEmpty, email.utf8.count <= 320 else { issue = .invalidInput; return }
        if scope != nil {
            guard let accountEmail, !accountEmail.isEmpty, email.caseInsensitiveCompare(accountEmail) == .orderedSame
            else {
                issue = .invalidInput; return
            }
        }
        await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            let secret = try makeSecret(), started = now()
            try await service.requestPasswordReset(email: email, hashedSecret: hashSecret(secret), expected: expected)
            try await check(expected)
            challenge = Challenge(secret: secret, email: email, started: started, expected: expected)
            state = .resetCode(email: email)
            scheduleExpiration(started.addingTimeInterval(900))
        }
    }

    public func confirmPasswordReset(code: String, newPassword: String, confirmation: String) async {
        guard case .resetCode(let email) = state, let challenge, !retired else { return }
        guard validCode(code), validPassword(newPassword), newPassword == confirmation else {
            issue = .invalidInput; return
        }
        await perform(recovering: .resetCode(email: email)) { [self] in
            let ownedOperation = operation
            let completedScope = scope
            let onPasswordResetResult = self.onPasswordResetResult
            try validAge(challenge.started)
            try await check(challenge.expected)
            try await service.confirmPasswordReset(
                email: email, code: code, plaintextSecret: challenge.secret,
                newPassword: newPassword, expected: challenge.expected)
            // A successful response is destructive server confirmation. Preserve
            // cleanup for its original scope even if this screen was dismissed.
            if operation == ownedOperation, !retired {
                do {
                    try await check(challenge.expected)
                    if operation == ownedOperation, !retired {
                        self.challenge = nil; state = .completed(.passwordReset)
                    }
                } catch {
                    if operation == ownedOperation, !retired {
                        self.challenge = nil; state = .idle; issue = .changedAccount
                    }
                }
            }
            if let completedScope { await onPasswordResetResult?(completedScope) }
        }
    }

    public func verifyEmail(code: String) async {
        guard mayStart else { return }
        guard !code.isEmpty, code.utf8.count <= 256,
            code.utf8.allSatisfy({
                (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45
            })
        else { issue = .invalidInput; return }
        await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            try await service.verifyEmail(code: code, expected: expected)
            try await check(expected)
            state = .completed(.emailVerified)
        }
    }

    public func resendVerification() async {
        guard mayStart, scope != nil else { return }
        await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            try await service.resendVerification(expected: expected)
            try await check(expected)
            state = .completed(.verificationSent)
        }
    }

    public func changePassword(currentPassword: String, newPassword: String, confirmation: String) async {
        guard mayStart, scope != nil else { return }
        guard !currentPassword.isEmpty, validPassword(newPassword), newPassword == confirmation else {
            issue = .invalidInput; return
        }
        await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            try await service.changePassword(
                currentPassword: currentPassword, newPassword: newPassword, expected: expected)
            try await check(expected)
            state = .completed(.passwordChanged)
        }
    }

    public func requestAccountDeletion(confirmation: String) async {
        guard mayStart, scope != nil else { return }
        guard confirmation == "DELETE" else { issue = .invalidInput; return }
        await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            let secret = try makeSecret(), started = now()
            try await service.requestAccountDeletion(hashedSecret: hashSecret(secret), expected: expected)
            try await check(expected)
            challenge = Challenge(secret: secret, email: nil, started: started, expected: expected)
            state = .deletionCode
            scheduleExpiration(started.addingTimeInterval(900))
        }
    }

    public func confirmAccountDeletion(code: String) async {
        guard state == .deletionCode, let challenge, let scope, !retired else { return }
        guard UUID(uuidString: code) != nil else { issue = .invalidInput; return }
        await perform(recovering: .deletionCode) { [self] in
            let ownedOperation = operation
            let onAccountDeletionResult = self.onAccountDeletionResult
            try validAge(challenge.started)
            try await check(challenge.expected)
            guard let beforeAccountDeletion else { throw AuthCeremonyIssue.cleanupRequired }
            do {
                try await beforeAccountDeletion(scope)
                try await check(challenge.expected)
                try await service.confirmAccountDeletion(
                    code: code, plaintextSecret: challenge.secret,
                    expected: challenge.expected)
                if operation == ownedOperation, !retired {
                    self.challenge = nil; state = .completed(.accountDeleted)
                }
                await onAccountDeletionResult?(scope, true)
            } catch {
                await onAccountDeletionResult?(scope, false)
                throw error
            }
        }
    }

    public func beginAppleSignIn() async -> AppleSignInRequest? {
        guard mayStart, scope == nil else { return nil }
        let owned = await perform(recovering: .idle) { [self] in
            let expected = try await capture()
            let rawNonce = try makeSecret(), started = now()
            let request = AppleSignInRequest(
                id: UUID(), nonce: hashSecret(rawNonce), state: UUID().uuidString.lowercased(),
                expiresAt: started.addingTimeInterval(900))
            apple = AppleAttempt(request: request, rawNonce: rawNonce, started: started, expected: expected)
            state = .apple(request)
            scheduleExpiration(request.expiresAt)
        }
        guard owned else { return nil }
        if case .apple(let request) = state { return request }
        return nil
    }

    public func completeAppleSignIn(id: UUID, credential: AppleSignInCredential) async {
        guard case .apple(let request) = state, request.id == id, let apple, !retired else { return }
        self.apple = nil  // Every provider callback consumes this attempt before any await.
        await perform(recovering: .idle) { [self] in
            try validAge(apple.started)
            guard credential.state == request.state, !credential.identityToken.isEmpty,
                credential.identityToken.utf8.count <= 64 * 1024, !credential.userIdentifier.isEmpty,
                credential.userIdentifier.utf8.count <= 1024
            else { throw AuthCeremonyIssue.invalidCallback }
            try await check(apple.expected)
            try await service.signInWithApple(credential, rawNonce: apple.rawNonce, expected: apple.expected)
            try Task.checkCancellation()
            state = .completed(.signedIn)
        }
    }

    public func beginBrowserSignIn(provider: OAuthProvider) async -> BrowserSignInRequest? {
        guard mayStart, scope == nil, provider == .google || provider == .github else { return nil }
        // Reject targets the browser broker cannot return to before reserving SDK state.
        guard supportsBrowserCallback else { issue = .browserSignInUnavailable; return nil }
        let owned = await perform(recovering: .idle) { [self] in
            let expected = try await capture(), started = now(), id = UUID()
            // Register the ID before preparation so cancellation also retires a delayed handshake.
            let placeholder = BrowserHandoffReservation(id: id, sessionID: "", requestID: "", expiresAt: started)
            browser = BrowserAttempt(reservation: placeholder, started: started, expected: expected)
            let reservation = try await service.prepareBrowser(id: id, expected: expected)
            try await check(expected)
            let url = try browserURL(provider: provider, reservation: reservation)
            guard reservation.id == id, reservation.expiresAt > now() else { throw AuthCeremonyIssue.expired }
            browser = BrowserAttempt(reservation: reservation, started: started, expected: expected)
            state = .browser(
                BrowserSignInRequest(
                    id: id, url: url, callbackScheme: configuration.callbackScheme,
                    expiresAt: min(reservation.expiresAt, started.addingTimeInterval(900))))
            scheduleExpiration(min(reservation.expiresAt, started.addingTimeInterval(900)))
        }
        guard owned else { return nil }
        if case .browser(let request) = state { return request }
        await discardBrowser()
        return nil
    }

    public func redeemBrowserCallback(id: UUID, url: URL) async {
        guard case .browser(let request) = state, request.id == id, let browser, !retired else { return }
        let owned = await perform(recovering: .idle) { [self] in
            try validBrowser(browser)
            let grant = try NativeOAuthHandoffGrant(callbackURL: url, expectedScheme: configuration.callbackScheme)
            try await check(browser.expected)
            let confirmation = try await service.redeemBrowser(
                browser.reservation, grant: grant, expected: browser.expected)
            try await check(browser.expected)
            try validBrowser(browser)
            guard confirmation.id == id, !confirmation.principalID.isEmpty else {
                throw AuthCeremonyIssue.invalidCallback
            }
            state = .confirmBrowser(confirmation)
        }
        if owned, state == .idle { await discardBrowser() }
    }

    /// Call only from the explicit account-confirmation button, never automatically on redemption.
    public func confirmBrowserAccount(id: UUID) async {
        guard case .confirmBrowser(let confirmation) = state, confirmation.id == id, let browser, !retired else {
            return
        }
        let owned = await perform(recovering: .idle) { [self] in
            try validBrowser(browser)
            try await check(browser.expected)
            try await service.confirmBrowser(confirmation, expected: browser.expected)
            try Task.checkCancellation()
            self.browser = nil; state = .completed(.signedIn)
        }
        if owned, state == .idle { await discardBrowser() }
    }

    public func cancel() async {
        let id = browser?.reservation.id
        reset(retiring: false)
        if let id { await service.cancelBrowser(id: id) }
    }

    public func invalidate() {
        let id = browser?.reservation.id
        reset(retiring: true)
        if let id { Task { await service.cancelBrowser(id: id) } }
    }

    private func reset(retiring: Bool) {
        operation = UUID(); task?.cancel(); task = nil
        expirationTask?.cancel(); expirationTask = nil
        challenge = nil; apple = nil; browser = nil; retired = retiring
        state = .idle; issue = nil
    }

    private func discardBrowser() async {
        let id = browser?.reservation.id; browser = nil
        if let id { await service.cancelBrowser(id: id) }
    }

    @discardableResult private func perform(
        recovering: AuthCeremonyState, action: @escaping @MainActor () async throws -> Void
    ) async -> Bool {
        let ticket = UUID(); operation = ticket; state = .busy; issue = nil
        let pending = Task { [weak self] in
            guard let self else { return }
            do { try await action() } catch {
                guard operation == ticket, !retired else { return }
                state = recovering; issue = Self.present(error)
                if issue == .expired || issue == .changedAccount {
                    challenge = nil; apple = nil; state = .idle
                }
            }
            if operation == ticket {
                task = nil
                if case .completed = state { expirationTask?.cancel(); expirationTask = nil }
                expireIfNeeded()
            }
        }
        task = pending
        await pending.value
        return operation == ticket && !retired
    }

    /// Invoke on foregrounding as well as the timer, since suspension delays timers.
    public func expireIfNeeded() {
        guard !isBusy else { return }
        let expired: Bool
        if let challenge {
            expired = (try? validAge(challenge.started)) == nil
        } else if let apple {
            expired = (try? validAge(apple.started)) == nil
        } else if let browser {
            expired = (try? validBrowser(browser)) == nil
        } else {
            return
        }
        guard expired else { return }
        let id = browser?.reservation.id
        reset(retiring: false); issue = .expired
        if let id { Task { await service.cancelBrowser(id: id) } }
    }

    private func scheduleExpiration(_ deadline: Date) {
        expirationTask?.cancel()
        let delay = max(0, deadline.timeIntervalSince(now()))
        expirationTask = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(delay)) } catch { return }
            self?.expireIfNeeded()
        }
    }

    private func capture() async throws -> CredentialSnapshot {
        let snapshot = try await service.snapshot()
        try Task.checkCancellation()
        guard !retired, snapshot.apiOrigin == configuration.apiURL.absoluteString,
            snapshot.principalId == scope?.principalID,
            scope == nil || scope?.apiOrigin == snapshot.apiOrigin
        else { throw AuthCeremonyIssue.changedAccount }
        return snapshot
    }

    private func check(_ expected: CredentialSnapshot) async throws {
        let current = try await capture()
        guard current.apiOrigin == expected.apiOrigin, current.principalId == expected.principalId,
            current.generation == expected.generation
        else { throw AuthCeremonyIssue.changedAccount }
    }

    private func validAge(_ started: Date) throws {
        let age = now().timeIntervalSince(started)
        guard age >= 0, age < 900 else { throw AuthCeremonyIssue.expired }
    }
    private func validBrowser(_ attempt: BrowserAttempt) throws {
        try validAge(attempt.started)
        guard now() < attempt.reservation.expiresAt else { throw AuthCeremonyIssue.expired }
    }
    private func validCode(_ value: String) -> Bool {
        value.utf8.count == 8 && value.utf8.allSatisfy { (48...57).contains($0) || (65...90).contains($0) }
    }
    private func validPassword(_ value: String) -> Bool { value.utf16.count >= 8 && value.utf8.count <= 16 * 1024 }

    private var supportsBrowserCallback: Bool {
        configuration.callbackScheme == "cloud.opensecret.maple"
            || configuration.callbackScheme == "cloud.opensecret.maple.dev"
    }

    private func browserURL(provider: OAuthProvider, reservation: BrowserHandoffReservation) throws -> URL {
        func validID(_ value: String) -> Bool {
            value.utf8.count == 32 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
        }
        guard validID(reservation.sessionID), validID(reservation.requestID),
            supportsBrowserCallback,
            var parts = URLComponents(url: configuration.authURL, resolvingAgainstBaseURL: false)
        else { throw AuthCeremonyIssue.invalidCallback }
        parts.path = "/desktop-auth"
        parts.queryItems = [
            URLQueryItem(name: "provider", value: provider.rawValue), URLQueryItem(name: "transport", value: "v2"),
            URLQueryItem(name: "native_session_id", value: reservation.sessionID),
            URLQueryItem(name: "native_request_id", value: reservation.requestID),
        ]
        if configuration.callbackScheme == "cloud.opensecret.maple.dev" {
            parts.queryItems?.append(URLQueryItem(name: "native_app_variant", value: "dev"))
        }
        guard let url = parts.url else { throw AuthCeremonyIssue.invalidCallback }
        return url
    }

    private static func present(_ error: Error) -> AuthCeremonyIssue {
        if let clock = AttestationClockIssue.classify(error) { return .clock(clock) }
        if let issue = error as? AuthCeremonyIssue { return issue }
        if let error = error as? ResearchAPIError {
            switch error {
            case .invalidCallback: return .invalidCallback
            case .expiredAuthenticationAttempt: return .expired
            case .invalidArgument: return .invalidInput
            case .http(_, .billingCleanupUnavailable): return .billingCleanupUnavailable
            case .http(_, .invalidLogin): return .invalidCredentials
            default: break
            }
        }
        if let error = error as? TransportError, error == .authorityChanged || error == .signedOut {
            return .changedAccount
        }
        return .connection
    }
}

/// SDK handles remain private to this actor. The app receives only public
/// routing identifiers and the verified account shown for confirmation.
public actor SDKAuthCeremonyService: AuthCeremonyServing {
    private let client: MapleClient
    private var activeBrowserID: UUID?
    private var prepared: PreparedNativeOAuthHandoff?
    private var confirmation: NativeSignInConfirmation?

    public init(client: MapleClient) { self.client = client }
    public func snapshot() async throws -> CredentialSnapshot { try await client.credentials.snapshot() }

    public func requestPasswordReset(email: String, hashedSecret: String, expected: CredentialSnapshot) async throws {
        try await check(expected)
        try await client.anonymousAPI.requestPasswordReset(
            email: email, hashedSecret: hashedSecret, clientId: client.clientID)
        try await check(expected)
    }
    public func confirmPasswordReset(
        email: String, code: String, plaintextSecret: String, newPassword: String,
        expected: CredentialSnapshot
    ) async throws {
        try await check(expected)
        try await client.anonymousAPI.confirmPasswordReset(
            email: email, code: code, plaintextSecret: plaintextSecret,
            newPassword: newPassword, clientId: client.clientID)
        // Do not discard a confirmed destructive result after an account change.
        // The caller cleans only its captured scope and fences UI publication.
    }
    public func verifyEmail(code: String, expected: CredentialSnapshot) async throws {
        try await check(expected)
        try await client.anonymousAPI.verifyEmail(code: code)
        try await check(expected)
    }
    public func resendVerification(expected: CredentialSnapshot) async throws {
        try await accountAPI(expected).requestNewVerificationEmail()
        try await check(expected)
    }
    public func changePassword(currentPassword: String, newPassword: String, expected: CredentialSnapshot) async throws
    {
        try await client.changePassword(currentPassword: currentPassword, newPassword: newPassword, expected: expected)
    }
    public func requestAccountDeletion(hashedSecret: String, expected: CredentialSnapshot) async throws {
        try await accountAPI(expected).requestAccountDeletion(hashedSecret: hashedSecret)
        try await check(expected)
    }
    public func confirmAccountDeletion(code: String, plaintextSecret: String, expected: CredentialSnapshot) async throws
    {
        try await client.confirmAccountDeletion(code: code, plaintextSecret: plaintextSecret, expected: expected)
    }
    public func signInWithApple(
        _ credential: AppleSignInCredential, rawNonce: String,
        expected: CredentialSnapshot
    ) async throws {
        guard expected.principalId == nil else { throw TransportError.authorityChanged }
        try await check(expected)
        let login = try await client.anonymousAPI.handleAppleNativeSignIn(
            identityToken: credential.identityToken, userIdentifier: credential.userIdentifier, rawNonce: rawNonce,
            clientId: client.clientID, email: credential.email, givenName: credential.givenName,
            familyName: credential.familyName)
        try Task.checkCancellation()
        _ = try await client.install(login, expected: expected)
    }

    public func prepareBrowser(id: UUID, expected: CredentialSnapshot) async throws -> BrowserHandoffReservation {
        guard activeBrowserID == nil, expected.principalId == nil else { throw TransportError.authorityChanged }
        activeBrowserID = id
        do {
            try await check(expected)
            let result = try await client.transport.prepareNativeOAuthHandoff()
            try await check(expected)
            guard activeBrowserID == id else { throw TransportError.authorityChanged }
            prepared = result
            return BrowserHandoffReservation(
                id: id, sessionID: result.sessionID, requestID: result.requestID,
                expiresAt: result.expiresAt)
        } catch {
            await cancelBrowser(id: id)
            throw error
        }
    }

    public func redeemBrowser(
        _ reservation: BrowserHandoffReservation, grant: NativeOAuthHandoffGrant,
        expected: CredentialSnapshot
    ) async throws -> BrowserAccountConfirmation {
        guard activeBrowserID == reservation.id, let prepared,
            prepared.sessionID == reservation.sessionID, prepared.requestID == reservation.requestID,
            prepared.expiresAt == reservation.expiresAt
        else { throw TransportError.authorityChanged }
        self.prepared = nil
        do {
            try await check(expected)
            let result = try await client.transport.redeemNativeOAuthHandoff(prepared, grant: grant)
            try await check(expected)
            guard activeBrowserID == reservation.id else { throw TransportError.authorityChanged }
            confirmation = result
            return BrowserAccountConfirmation(id: reservation.id, principalID: result.principalID, email: result.email)
        } catch {
            await cancelBrowser(id: reservation.id)
            throw error
        }
    }

    public func confirmBrowser(_ account: BrowserAccountConfirmation, expected: CredentialSnapshot) async throws {
        guard activeBrowserID == account.id, let confirmation,
            confirmation.principalID == account.principalID, confirmation.email == account.email
        else {
            throw TransportError.authorityChanged
        }
        self.confirmation = nil
        do {
            try await check(expected)
            _ = try await client.transport.confirmNativeOAuthHandoff(confirmation)
            if activeBrowserID == account.id { activeBrowserID = nil }
        } catch {
            await cancelBrowser(id: account.id)
            throw error
        }
    }

    public func cancelBrowser(id: UUID) async {
        guard activeBrowserID == id else { return }
        activeBrowserID = nil; prepared = nil; confirmation = nil
        try? await client.transport.cancelNativeOAuthHandoff()
    }

    private func accountAPI(_ expected: CredentialSnapshot) async throws -> ResearchAPI {
        try await check(expected)
        let api = try await client.accountAPI()
        try await check(expected)
        return api
    }
    private func check(_ expected: CredentialSnapshot) async throws {
        try Task.checkCancellation()
        let current = try await client.credentials.snapshot()
        try Task.checkCancellation()
        guard current.apiOrigin == expected.apiOrigin, current.principalId == expected.principalId,
            current.generation == expected.generation,
            expected.principalId != nil || current.revision == expected.revision
        else {
            throw TransportError.authorityChanged
        }
    }
}
