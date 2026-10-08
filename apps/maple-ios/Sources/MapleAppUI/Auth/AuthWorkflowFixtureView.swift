#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import Synchronization
    import UIKit

    /// Exercises the shipping forms, session owner and native callback coordinator.
    /// Only the remote account service and browser presentation are synthetic;
    /// no fixture credentials, mail, provider requests or disk writes leave memory.
    struct AuthWorkflowFixtureView: View {
        @State private var model = AuthWorkflowFixtureModel()

        var body: some View {
            VStack(spacing: 4) {
                diagnostics
                if let account = model.session.account {
                    NavigationStack {
                        List {
                            Text(account.user.email ?? account.user.id).accessibilityIdentifier("auth.fixture.account")
                            Text(account.user.loginMethod).accessibilityIdentifier("auth.fixture.method")
                            Button("Verify email") { model.open(.verification) }
                                .accessibilityIdentifier("auth.fixture.verify")
                            Button("Change password") { model.open(.changePassword) }
                                .accessibilityIdentifier("auth.fixture.change")
                            Button("Reset password") { model.open(.reset) }
                                .accessibilityIdentifier("auth.fixture.reset")
                            Button("Delete account") { model.open(.deletion) }
                                .accessibilityIdentifier("auth.fixture.delete")
                            if model.service.expiryFixture {
                                Button("Inject terminal refresh rejection") {
                                    Task { await model.rejectRefresh() }
                                }.accessibilityIdentifier("auth.fixture.expire")
                            }
                            Button("Sign out") { Task { await model.session.signOut() } }
                                .accessibilityIdentifier("auth.fixture.sign-out")
                        }.navigationTitle("Synthetic account")
                    }
                } else {
                    SignInView(session: model.session, coordinator: model.coordinator) { model.open(.reset) }
                }
            }
            .task { await model.session.restore() }
            .sheet(item: $model.screen) { screen in
                NavigationStack {
                    Group {
                        switch screen.kind {
                        case .reset: PasswordResetView(ceremonies: screen.flow)
                        case .verification: EmailVerificationView(ceremonies: screen.flow)
                        case .changePassword: ChangePasswordView(ceremonies: screen.flow)
                        case .deletion: DeleteAccountView(ceremonies: screen.flow)
                        }
                    }
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Close") { model.screen = nil }.accessibilityIdentifier("auth.fixture.close")
                        }
                    }
                }
            }
        }

        private var diagnostics: some View {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(model.phase).accessibilityIdentifier("auth.fixture.phase")
                    Text("Redemptions \(model.service.redemptions)").accessibilityIdentifier("auth.fixture.redemptions")
                    Text("Installs \(model.service.installs)").accessibilityIdentifier("auth.fixture.installs")
                }
                if !model.browser.presentations.isEmpty {
                    HStack {
                        Button("Deliver twice") { model.browser.deliver(duplicate: true) }
                            .accessibilityIdentifier("auth.fixture.callback-twice")
                        Button("Release") { model.service.releaseRedemption() }
                            .accessibilityIdentifier("auth.fixture.release")
                        Button("Wrong callback") { model.browser.deliverWrong() }
                            .accessibilityIdentifier("auth.fixture.wrong-callback")
                    }
                    HStack {
                        Button("Provider cancel") { model.browser.cancelFromProvider() }
                            .accessibilityIdentifier("auth.fixture.provider-cancel")
                        Button("Old callback") { model.browser.deliverPrevious() }
                            .accessibilityIdentifier("auth.fixture.old-callback")
                        Text(model.browser.validRequest ? "Local request valid" : "Invalid request")
                            .accessibilityIdentifier("auth.fixture.request")
                    }
                }
                Text("Reset requests \(model.service.resetRequests); confirmations \(model.service.resetConfirmations)")
                    .accessibilityIdentifier("auth.fixture.resets")
                if model.service.expiryFixture {
                    Text(
                        "Revocations \(model.service.revocations); retirements \(model.authorityRetirements); signouts \(model.service.signOuts)"
                    )
                    .accessibilityIdentifier("auth.fixture.authority")
                    Text(model.privateWork.isEmpty ? "Owned work cleared" : "Owned work retained")
                        .accessibilityIdentifier("auth.fixture.work")
                    if model.revocationFailed {
                        Text("Fixture revocation failed").accessibilityIdentifier("auth.fixture.revocation-error")
                    }
                }
                Text("Deletion fences \(model.deletionFences); cleanups \(model.deletionCleanups)")
                    .accessibilityIdentifier("auth.fixture.deletions")
            }.font(.caption2).buttonStyle(.bordered).padding(.horizontal, 8)
        }
    }

    @MainActor @Observable private final class AuthWorkflowFixtureModel {
        enum Kind { case reset, verification, changePassword, deletion }
        struct Screen: Identifiable { let id = UUID(); let kind: Kind; let flow: AuthCeremonies }
        let configuration: AppConfiguration
        let service: AuthWorkflowFixtureService
        let session: SessionController
        let browser: AuthWorkflowFixtureBrowser
        let coordinator: NativeSignInCoordinator
        var screen: Screen?
        var deletionFences = 0
        var deletionCleanups = 0
        var authorityRetirements = 0
        var privateWork = "Synthetic owned work"
        var revocationFailed = false

        init() {
            do {
                configuration = try AppConfiguration(
                    profile: .local, apiURL: "http://127.0.0.1:1234", billingURL: "http://127.0.0.1:1235",
                    flagsURL: "http://127.0.0.1:1236", authURL: "http://127.0.0.1:1237",
                    clientID: "11111111-2222-4333-8444-555555555555", callbackScheme: "cloud.opensecret.maple.dev")
                service = try AuthWorkflowFixtureService()
                session = SessionController(service: service, storage: AuthWorkflowFixtureStorage())
                browser = AuthWorkflowFixtureBrowser()
                let browser = browser
                coordinator = NativeSignInCoordinator(
                    session: session, ceremonies: AuthCeremonies(service: service, configuration: configuration),
                    browserFactory: { url, scheme, _, completion in
                        browser.make(url: url, scheme: scheme, completion: completion)
                    })
                let coordinator = coordinator
                session.onCancelExternal = { [weak coordinator] in await coordinator?.cancelPresentation() }
                if service.expiryFixture {
                    session.onRetire = { [weak self] _ in
                        self?.authorityRetirements += 1
                        self?.privateWork = ""
                    }
                }
            } catch { preconditionFailure("Invalid synthetic authentication fixture configuration") }
        }

        var phase: String {
            switch session.phase {
            case .restoring: "restoring"
            case .signedOut: "signedOut"
            case .authenticating: "authenticating"
            case .signedIn: "signedIn"
            case .unavailable: "unavailable"
            case .cleaningUp: "cleaningUp"
            }
        }

        func rejectRefresh() async {
            do { try await service.rejectRefresh() } catch { revocationFailed = true }
        }

        func open(_ kind: Kind) {
            let flow = AuthCeremonies(
                service: service, configuration: configuration, scope: session.account?.scope,
                accountEmail: session.account?.user.email)
            flow.beforeAccountDeletion = { [weak self] _ in self?.deletionFences += 1 }
            flow.onAccountDeletionResult = { [weak self] _, deleted in
                guard let self, deleted else { return }
                self.deletionCleanups += 1
                await self.session.signOut()
            }
            flow.onPasswordResetResult = { [weak self] _ in await self?.session.signOut() }
            screen = Screen(kind: kind, flow: flow)
        }
    }

    private actor AuthWorkflowFixtureStorage: AccountDataLifecycle {
        func activate(_ scope: AccountScope) async throws {}
        func retire(_ scope: AccountScope) async throws {}
        func clearSignedOutData() async throws {}
    }

    private final class AuthWorkflowFixtureSecureStore: SecureStore, Sendable {
        private let values = Mutex<[String: Data]>([:])
        func read(key: String) throws -> Data? { values.withLock { $0[key] } }
        func write(_ data: Data, key: String) throws { values.withLock { $0[key] = data } }
        func remove(key: String) throws { values.withLock { _ = $0.removeValue(forKey: key) } }
    }

    @MainActor @Observable private final class AuthWorkflowFixtureService: SessionServing, AuthCeremonyServing {
        let authority: CredentialAuthority
        let expiryFixture = ProcessInfo.processInfo.arguments.contains("--maple-auth-expiry-fixture")
        var revocations = 0
        var signOuts = 0
        var redemptions = 0
        var installs = 0
        var resetRequests = 0
        var resetConfirmations = 0
        private var profile: MapleUser?
        private var password = "fixture-password"
        private var clockFailurePending = ProcessInfo.processInfo.arguments.contains("--maple-auth-clock-fixture")
        private var resetHash: String?
        private var resetEmail: String?
        private var deletionHash: String?
        private var redemption: CheckedContinuation<Void, Never>?

        init() throws {
            authority = try CredentialAuthority(
                apiURL: "http://127.0.0.1:1234", store: AuthWorkflowFixtureSecureStore())
        }
        func snapshot() async throws -> CredentialSnapshot { try await authority.snapshot() }
        private func check(_ expected: CredentialSnapshot) async throws {
            try Task.checkCancellation()
            guard try await snapshot() == expected else { throw TransportError.authorityChanged }
        }
        func identity() async throws -> SessionIdentity? {
            let snapshot = try await snapshot()
            guard let principal = snapshot.principalId else { return nil }
            return SessionIdentity(origin: snapshot.apiOrigin, principal: principal, generation: snapshot.generation)
        }
        func user() async throws -> MapleUser {
            guard let profile else { throw ResearchAPIError.invalidResponse }
            return profile
        }
        func signIn(email: String, password: String) async throws {
            if clockFailurePending {
                clockFailurePending = false
                let signedAt = Date(timeIntervalSince1970: 1_800_000_000)
                guard
                    let error = AttestationClockSkewError(
                        validatingDeviceTime: signedAt.addingTimeInterval(3 * 86_400), enclaveTime: signedAt,
                        certificateIndex: 0, notBefore: signedAt.addingTimeInterval(-600),
                        notAfter: signedAt.addingTimeInterval(3_600))
                else { throw ResearchAPIError.invalidResponse }
                throw NSError(domain: "SyntheticAttestationTransport", code: 1, userInfo: [NSUnderlyingErrorKey: error])
            }
            guard password == self.password else { throw ResearchAPIError.http(status: 401, reason: .invalidLogin) }
            try await install(principal: "email-account", email: email, method: "email")
        }
        func signInGuest(id: String, password: String) async throws {
            guard id == "existing-guest", password == self.password else {
                throw ResearchAPIError.http(status: 401, reason: .invalidLogin)
            }
            try await install(principal: id, email: nil, method: "guest")
        }
        func signUp(email: String, password: String, name: String?) async throws {
            guard password.utf16.count >= 8 else { throw ResearchAPIError.invalidArgument }
            self.password = password
            try await install(principal: "new-email-account", email: email, method: "email")
        }
        func signOut() async throws { signOuts += 1; try await authority.clear(); profile = nil }
        func authorityChanges() async -> AsyncStream<CredentialAuthorityChange>? {
            guard expiryFixture else { return nil }
            return await authority.changes()
        }
        func authorityState() async -> CredentialAuthorityChange? {
            guard expiryFixture else { return nil }
            return await authority.currentState()
        }
        func completePendingCleanup() async throws { try await authority.completePendingCleanup() }
        func rejectRefresh() async throws {
            guard expiryFixture else { throw ResearchAPIError.invalidArgument }
            let expected = try await authority.snapshot()
            guard expected.principalId != nil else { throw ResearchAPIError.invalidArgument }
            // Inject only the terminal authentication decision. The real SDK
            // authority publishes revocation; the shipping SessionController
            // observer owns retirement and the visible signed-out transition.
            try await authority.clear(expected: expected)
            revocations += 1
        }

        func requestPasswordReset(email: String, hashedSecret: String, expected: CredentialSnapshot) async throws {
            try await check(expected); resetHash = hashedSecret; resetEmail = email; resetRequests += 1
        }
        func confirmPasswordReset(
            email: String, code: String, plaintextSecret: String, newPassword: String, expected: CredentialSnapshot
        ) async throws {
            try await check(expected)
            guard code == "ABCD1234", email == resetEmail, resetHash == hashSecret(plaintextSecret) else {
                throw ResearchAPIError.invalidArgument
            }
            password = newPassword; resetConfirmations += 1; resetHash = nil; resetEmail = nil
        }
        func verifyEmail(code: String, expected: CredentialSnapshot) async throws {
            try await check(expected)
            guard UUID(uuidString: code) != nil else { throw ResearchAPIError.invalidArgument }
            profile?.emailVerified = true
        }
        func resendVerification(expected: CredentialSnapshot) async throws { try await check(expected) }
        func changePassword(currentPassword: String, newPassword: String, expected: CredentialSnapshot) async throws {
            try await check(expected)
            guard currentPassword == password else { throw ResearchAPIError.http(status: 401, reason: .invalidLogin) }
            password = newPassword
        }
        func requestAccountDeletion(hashedSecret: String, expected: CredentialSnapshot) async throws {
            try await check(expected); deletionHash = hashedSecret
        }
        func confirmAccountDeletion(code: String, plaintextSecret: String, expected: CredentialSnapshot) async throws {
            try await check(expected)
            guard UUID(uuidString: code) != nil, deletionHash == hashSecret(plaintextSecret) else {
                throw ResearchAPIError.invalidArgument
            }
            try await authority.clear(expected: expected); profile = nil; deletionHash = nil
        }
        func signInWithApple(_ credential: AppleSignInCredential, rawNonce: String, expected: CredentialSnapshot)
            async throws
        { throw ResearchAPIError.invalidArgument }

        func prepareBrowser(id: UUID, expected: CredentialSnapshot) async throws -> BrowserHandoffReservation {
            try await check(expected)
            return BrowserHandoffReservation(
                id: id, sessionID: String(repeating: "a", count: 32), requestID: String(repeating: "b", count: 32),
                expiresAt: Date().addingTimeInterval(600))
        }
        func redeemBrowser(
            _ reservation: BrowserHandoffReservation, grant: NativeOAuthHandoffGrant, expected: CredentialSnapshot
        ) async throws -> BrowserAccountConfirmation {
            try await check(expected); redemptions += 1
            await withCheckedContinuation { redemption = $0 }
            try await check(expected)
            return BrowserAccountConfirmation(
                id: reservation.id, principalID: "browser-account", email: "browser@example.test")
        }
        func confirmBrowser(_ confirmation: BrowserAccountConfirmation, expected: CredentialSnapshot) async throws {
            try await check(expected)
            try await install(
                principal: confirmation.principalID, email: confirmation.email, method: "google", expected: expected)
            installs += 1
        }
        func cancelBrowser(id: UUID) async { releaseRedemption() }
        func releaseRedemption() { let pending = redemption; redemption = nil; pending?.resume() }

        private func install(principal: String, email: String?, method: String, expected: CredentialSnapshot? = nil)
            async throws
        {
            let captured: CredentialSnapshot
            if let expected { captured = expected } else { captured = try await snapshot() }
            _ = try await authority.install(
                accessToken: token("access", principal: principal),
                refreshToken: token("refresh", principal: principal),
                expected: captured)
            profile = MapleUser(
                id: principal, email: email, emailVerified: false, loginMethod: method, createdAt: "", updatedAt: "")
        }
        private func token(_ kind: String, principal: String) throws -> String {
            func encode(_ data: Data) -> String {
                data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
                    .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
            }
            let body = try JSONSerialization.data(withJSONObject: [
                "sub": principal, "exp": 4_000_000_000, "tf": 2,
                "aud": "urn:opensecret:internal:transport-v2:user:\(kind)-token",
            ])
            return encode(Data("{}".utf8)) + "." + encode(body) + "." + encode(Data("fixture".utf8))
        }
    }

    @MainActor @Observable private final class AuthWorkflowFixtureBrowser {
        private(set) var presentations: [AuthWorkflowFixturePresentation] = []
        private(set) var validRequest = false
        func make(url: URL, scheme: String, completion: @escaping BrowserSignInCompletion)
            -> any BrowserSignInPresentation
        {
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false)
            let query = parts?.queryItems ?? []
            validRequest =
                url.host == "127.0.0.1" && url.path == "/desktop-auth"
                && scheme == "cloud.opensecret.maple.dev"
                && query.contains(where: { $0.name == "transport" && $0.value == "v2" })
                && query.contains(where: { $0.name == "native_app_variant" && $0.value == "dev" })
            let result = AuthWorkflowFixturePresentation(completion: completion)
            presentations.append(result)
            if presentations.count > 4 { presentations.removeFirst() }
            return result
        }
        func deliver(duplicate: Bool) {
            presentations.last?.deliver(callback)
            if duplicate { presentations.last?.deliver(callback) }
        }
        func deliverWrong() {
            presentations.last?.deliver(URL(string: "cloud.opensecret.maple://auth?handoff_grant=e30.e30.e30")!)
        }
        func deliverPrevious() {
            guard presentations.count > 1 else { return }
            presentations[presentations.count - 2].deliver(callback)
        }
        func cancelFromProvider() { presentations.last?.completion(nil, .cancelled) }
        private var callback: URL { URL(string: "cloud.opensecret.maple.dev://auth?handoff_grant=e30.e30.e30")! }
    }
    @MainActor private final class AuthWorkflowFixturePresentation: BrowserSignInPresentation {
        let completion: BrowserSignInCompletion
        init(completion: @escaping BrowserSignInCompletion) { self.completion = completion }
        func start() -> Bool { true }
        func cancel() {}  // A retained callback can still arrive; ownership must reject it.
        func deliver(_ url: URL) { completion(url, nil) }
    }
#endif
