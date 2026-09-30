import Foundation
import Testing
@testable import MapleSDK

private enum LocalFixtureError: Error {
    case missingConfiguration, unsafeEndpoint, mailHelperUnavailable, mailHelperFailed
}

private struct LocalConfiguration {
    let url: String
    let email: String
    let password: String
    let clientId: String
    let inviteCode: String

    init() throws {
        let env = ProcessInfo.processInfo.environment
        guard let url = env["MAPLE_SWIFT_INTEGRATION_URL"],
            let parts = URLComponents(string: url), parts.scheme == "http",
            ["127.0.0.1", "localhost", "::1", "[::1]"].contains(parts.host ?? ""),
            parts.user == nil, parts.password == nil, parts.query == nil, parts.fragment == nil,
            parts.path.isEmpty || parts.path == "/"
        else { throw LocalFixtureError.unsafeEndpoint }
        guard let email = env["MAPLE_SWIFT_TEST_EMAIL"], !email.isEmpty,
            let password = env["MAPLE_SWIFT_TEST_PASSWORD"], !password.isEmpty,
            let clientId = env["MAPLE_SWIFT_TEST_CLIENT_ID"], UUID(uuidString: clientId) != nil
        else {
            throw LocalFixtureError.missingConfiguration
        }
        self.url = url; self.email = email; self.password = password; self.clientId = clientId
        inviteCode = env["MAPLE_SWIFT_TEST_INVITE_CODE"] ?? ""
    }

    func client() throws -> MapleClient {
        try MapleClient(
            apiURL: url, clientID: clientId,
            policy: PCRTrustPolicy(environment: .development, remoteHistoryEnabled: false),
            store: TestSecureStore())
    }

    func signedInEmailClient() async throws -> MapleClient {
        let client = try client()
        do { _ = try await client.signIn(email: email, password: password) } catch ResearchAPIError.http(
            status: 401, reason: _)
        {
            _ = try await client.signUp(email: email, password: password, inviteCode: inviteCode, name: "SDK fixture")
        }
        return client
    }

    func newGuest() async throws -> (MapleClient, String) {
        let client = try client()
        let before = try await client.credentials.snapshot()
        let login = try await client.anonymousAPI.signUpGuest(
            password: password, inviteCode: inviteCode, clientId: clientId)
        _ = try await client.install(login, expected: before)
        return (client, login.id)
    }
}

@Suite(.serialized, .enabled(if: ProcessInfo.processInfo.environment["MAPLE_SWIFT_INTEGRATION_URL"] != nil))
struct LocalIntegrationTests {
    @Test func localEmailAuthenticationRefreshModelsAndBillingBearer() async throws {
        let configuration = try LocalConfiguration()
        let client = try await configuration.signedInEmailClient()
        let api = try await client.accountAPI()
        let user = try await api.getUser()
        #expect(user.email == configuration.email)
        let before = try await client.credentials.snapshot()
        _ = try await client.refreshCredentials()
        let after = try await client.credentials.snapshot()
        #expect(after.principalId == before.principalId)
        #expect(after.revision > before.revision)
        #expect(try await api.getUser().id == user.id)
        #expect(!(try await api.fetchModels()).isEmpty)
        let catalog = try await api.fetchModelCatalog()
        #expect(catalog.object == "list")
        let token = try await api.generateThirdPartyToken(audience: "https://billing.example.test")
        #expect(!token.token.isEmpty)
        try await client.signOut()
        #expect(try await client.credentials.snapshot().principalId == nil)
        await #expect(throws: TransportError.authorityChanged) { try await api.getUser() }
        let anonymous = try configuration.client()
        #expect(!(try await anonymous.anonymousAPI.fetchModels(authenticated: false)).isEmpty)
    }

    @Test func localGuestAuthPasswordAndCompleteCRUD() async throws {
        let configuration = try LocalConfiguration()
        let (client, userId) = try await configuration.newGuest()
        let api = try await client.accountAPI()
        #expect(try await api.getUser().id == userId)
        let replacementPassword = "SwiftFixture-" + UUID().uuidString
        try await client.changePassword(currentPassword: configuration.password, newPassword: replacementPassword)
        #expect(try await api.getUser().id == userId)
        try await client.signOut()
        _ = try await client.signInGuest(id: userId, password: replacementPassword)
        let current = try await client.accountAPI()

        let firstProject = try await current.createProject(name: "Swift fixture A")
        let secondProject = try await current.createProject(name: "Swift fixture B")
        #expect(try await current.getProject(id: firstProject.id).name == "Swift fixture A")
        #expect(
            try await current.updateProject(
                id: firstProject.id, name: "Renamed", instructions: .set("Synthetic instructions")
            ).instructions == "Synthetic instructions")
        #expect(try await current.updateProject(id: firstProject.id, instructions: .clear).instructions == nil)
        #expect(try await current.listProjects(.init(limit: 1, order: .asc)).hasMore)
        let projectPage = try await current.listProjects(.init(limit: 1, order: .asc))
        #expect(try await current.listProjects(.init(limit: 1, after: projectPage.lastId, order: .asc)).data.count == 1)

        let first = try await current.createConversation(
            metadata: ["title": .string("Swift A")], projectId: firstProject.id, pinned: true)
        let second = try await current.createConversation(metadata: ["title": .string("Swift B")])
        let third = try await current.createConversation(metadata: ["title": .string("Swift C")])
        #expect(try await current.getConversation(id: first.id).pinned)
        #expect(
            try await current.listConversations(.init(projectId: firstProject.id, pinned: true)).data.map(\.id) == [
                first.id
            ])
        #expect(try await current.listConversations(.init(unassignedProject: true)).data.count == 2)
        let conversationPage = try await current.listConversations(.init(page: .init(limit: 1, order: .desc)))
        #expect(conversationPage.hasMore)
        #expect(
            try await current.listConversations(
                .init(page: .init(limit: 1, after: conversationPage.lastId, order: .desc))
            ).data.count == 1)
        #expect(
            !(try await current.updateConversation(
                id: first.id, metadata: ["title": .string("Renamed")], projectId: .clear, pinned: false)).pinned)
        #expect(
            try await current.batchUpdateConversationProject(ids: [first.id, second.id], projectId: secondProject.id)
                .success)
        #expect(try await current.getConversation(id: first.id).projectId == secondProject.id)
        #expect(
            try await current.listConversationItems(conversationId: first.id, parameters: .init(order: .desc)).data
                .isEmpty)
        await #expect(throws: ResearchAPIError.http(status: 404, reason: .notFound)) {
            try await current.getConversationItem(conversationId: first.id, itemId: UUID().uuidString)
        }

        let defaultInstruction = try await current.createInstruction(
            name: "First", prompt: "Be concise.", isDefault: true)
        let otherInstruction = try await current.createInstruction(name: "Second", prompt: "Use prose.")
        #expect(try await current.getInstruction(id: defaultInstruction.id).isDefault)
        #expect(
            try await current.updateInstruction(id: otherInstruction.id, name: "Updated", prompt: "Use plain prose.")
                .name == "Updated")
        #expect(try await current.setDefaultInstruction(id: otherInstruction.id).isDefault)
        #expect(!(try await current.getInstruction(id: defaultInstruction.id)).isDefault)
        let instructions = try await current.listInstructions(.init(limit: 1, order: .asc))
        #expect(instructions.hasMore)
        #expect(
            try await current.listInstructions(.init(limit: 1, after: instructions.lastId, order: .asc)).data.count == 1
        )
        #expect(try await current.deleteInstruction(id: defaultInstruction.id).deleted)
        #expect(try await current.deleteInstruction(id: otherInstruction.id).deleted)

        let (foreignClient, _) = try await configuration.newGuest()
        let foreign = try await foreignClient.accountAPI()
        await #expect(throws: ResearchAPIError.http(status: 404, reason: .notFound)) {
            try await foreign.getConversation(id: first.id)
        }
        await #expect(throws: ResearchAPIError.http(status: 404, reason: .notFound)) {
            try await foreign.getProject(id: firstProject.id)
        }
        let batch = try await current.batchDeleteConversations(ids: [first.id, second.id, UUID().uuidString])
        #expect(batch.data.filter(\.deleted).count == 2)
        #expect(batch.data.contains { !$0.deleted && $0.error == "not_found" })
        #expect(try await current.deleteConversation(id: third.id).deleted)
        _ = try await current.createConversation()
        #expect(try await current.deleteAllConversations().deleted)
        #expect(try await current.listConversations().data.isEmpty)
        #expect(try await current.deleteProject(id: firstProject.id).deleted)
        #expect(try await current.deleteProject(id: secondProject.id).deleted)
        try await foreignClient.signOut()
        try await client.signOut()
    }

    @Test func localAudioResponseAndEmailRejectionContractsWithoutProviderSpending() async throws {
        let configuration = try LocalConfiguration()
        let client = try await configuration.signedInEmailClient()
        let api = try await client.accountAPI()
        // Empty audio is rejected before provider dispatch, and blank speech is
        // rejected by the existing server input validation.
        await #expect(throws: ResearchAPIError.self) { try await api.transcribeAudio(data: Data()) }
        await #expect(throws: ResearchAPIError.self) {
            try await api.synthesizeSpeech(input: "", voice: "neutral_female")
        }
        await #expect(throws: ResearchAPIError.http(status: 404, reason: .notFound)) {
            try await api.retrieveResponse(id: UUID().uuidString)
        }
        await #expect(throws: ResearchAPIError.http(status: 404, reason: .notFound)) {
            try await api.cancelResponse(id: UUID().uuidString)
        }
        // Invalid conversation UUID cannot dispatch inference.
        await #expect(throws: ResearchAPIError.self) {
            try await api.createResponse(
                .init(input: .text("Synthetic rejection fixture"), conversation: "invalid-uuid"))
        }
        await #expect(throws: ResearchAPIError.self) { try await api.verifyEmail(code: UUID().uuidString) }
        try await client.signOut()
    }

    @Test func localNativeHandoffUsesExactSessionRequestAndAnonymousAuthority() async throws {
        let configuration = try LocalConfiguration()
        let browser = try await configuration.signedInEmailClient()
        let browserAPI = try await browser.accountAPI()
        let browserUser = try await browserAPI.getUser()
        let app = try configuration.client()
        let prepared = try await app.transport.prepareNativeOAuthHandoff()
        let grant = try await mintFixtureGrant(browser: browserAPI, prepared: prepared)
        let confirmation = try await app.transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        #expect(confirmation.principalID == browserUser.id)
        #expect(try await app.credentials.snapshot().principalId == nil)
        _ = try await app.transport.confirmNativeOAuthHandoff(confirmation)
        let accountAPI = try await app.accountAPI()
        #expect(try await accountAPI.getUser().id == browserUser.id)
        await #expect(throws: TransportError.authorityChanged) {
            try await app.transport.confirmNativeOAuthHandoff(confirmation)
        }
        await #expect(throws: TransportError.self) {
            try await app.transport.redeemNativeOAuthHandoff(prepared, grant: grant)
        }
        await #expect(throws: TransportError.authorityChanged) {
            try await app.transport.prepareNativeOAuthHandoff()
        }
        try await app.signOut()
        _ = try await app.signIn(email: configuration.email, password: configuration.password)
        await #expect(throws: TransportError.authorityChanged) { try await accountAPI.getUser() }

        let intended = try configuration.client()
        let unintended = try configuration.client()
        let intendedPreparation = try await intended.transport.prepareNativeOAuthHandoff()
        let unintendedPreparation = try await unintended.transport.prepareNativeOAuthHandoff()
        let boundGrant = try await mintFixtureGrant(browser: browserAPI, prepared: intendedPreparation)
        await #expect(throws: TransportError.self) {
            try await unintended.transport.redeemNativeOAuthHandoff(unintendedPreparation, grant: boundGrant)
        }
        #expect(try await unintended.credentials.snapshot().principalId == nil)
        _ = try await intended.signIn(email: configuration.email, password: configuration.password)
        await #expect(throws: TransportError.self) {
            try await intended.transport.redeemNativeOAuthHandoff(intendedPreparation, grant: boundGrant)
        }

        let delayed = try configuration.client()
        let delayedPreparation = try await delayed.transport.prepareNativeOAuthHandoff()
        let delayedGrant = try await mintFixtureGrant(browser: browserAPI, prepared: delayedPreparation)
        let delayedConfirmation = try await delayed.transport.redeemNativeOAuthHandoff(
            delayedPreparation, grant: delayedGrant)
        _ = try await delayed.signIn(email: configuration.email, password: configuration.password)
        await #expect(throws: TransportError.authorityChanged) {
            try await delayed.transport.confirmNativeOAuthHandoff(delayedConfirmation)
        }
        try await delayed.signOut()
        try await intended.signOut()
        try await app.signOut()
        try await browser.signOut()
    }

    #if os(macOS)
        @Test(.enabled(if: ProcessInfo.processInfo.environment["MAPLE_SWIFT_MAIL_FIXTURE_HELPER"] != nil))
        func localEmailVerificationPasswordResetAndDeletionCeremonies() async throws {
            let configuration = try LocalConfiguration()
            let client = try configuration.client()
            let email = "swift-" + UUID().uuidString.lowercased() + "@example.test"
            _ = try await client.signUp(
                email: email, password: configuration.password, inviteCode: configuration.inviteCode,
                name: "Disposable SDK fixture")
            let api = try await client.accountAPI()
            try await api.requestNewVerificationEmail()
            let verification = try mailCode(kind: "verification", email: email)
            try await api.verifyEmail(code: verification)
            #expect(try await api.getUser().emailVerified)
            let secret = try generateSecureSecret()
            try await api.requestPasswordReset(
                email: email, hashedSecret: hashSecret(secret), clientId: configuration.clientId)
            let reset = try mailCode(kind: "password-reset", email: email)
            let replacement = "SwiftReset-" + UUID().uuidString
            try await client.signOut()
            try await client.anonymousAPI.confirmPasswordReset(
                email: email, code: reset, plaintextSecret: secret, newPassword: replacement,
                clientId: configuration.clientId)
            _ = try await client.signIn(email: email, password: replacement)
            let current = try await client.accountAPI()
            let deletionSecret = try generateSecureSecret()
            try await current.requestAccountDeletion(hashedSecret: hashSecret(deletionSecret))
            let deletion = try mailCode(kind: "account-deletion", email: email)
            try await client.confirmAccountDeletion(code: deletion, plaintextSecret: deletionSecret)
            #expect(try await client.credentials.snapshot().principalId == nil)
            await #expect(throws: ResearchAPIError.self) {
                try await client.signIn(email: email, password: replacement)
            }
        }
    #endif

    @Test(.enabled(if: ProcessInfo.processInfo.environment["RUN_LIVE_AI"] == "1"))
    func localLiveResponsesAudioAndCancellation() async throws {
        let configuration = try LocalConfiguration()
        let client = try await configuration.signedInEmailClient()
        let api = try await client.accountAPI()
        let conversation = try await api.createConversation(metadata: [
            "title": .string("Disposable SDK provider fixture")
        ])
        let correlation = UUID().uuidString.lowercased()
        let events = try await api.createResponse(
            .init(
                input: .text("Reply with the word Maple."), conversation: conversation.id,
                metadata: ["internal_message_id": .string(correlation)]))
        var responseId: String?
        var completed = false
        for try await event in events {
            if case let .created(value) = event { responseId = value.response.id }
            if case .completed = event { completed = true }
        }
        #expect(completed)
        guard let responseId else { throw ResearchAPIError.invalidResponse }
        #expect(try await api.retrieveResponse(id: responseId).status == .completed)
        let items = try await api.listConversationItems(conversationId: conversation.id, parameters: .init(order: .asc))
        #expect(items.data.contains { $0.id == correlation && $0.role == "user" && $0.responseId == responseId })
        #expect(items.data.contains { $0.role == "assistant" })
        let correlatedItem = try await api.getConversationItem(conversationId: conversation.id, itemId: correlation)
        #expect(correlatedItem.id == correlation)
        #expect(correlatedItem.type == "message")
        #expect(correlatedItem.role == "user")
        #expect(correlatedItem.responseId == responseId)

        let speech = try await api.synthesizeSpeech(input: "Maple test.", voice: "neutral_female")
        let bytes = try await api.collect(speech.body, limit: 10 * 1_024 * 1_024)
        #expect(!bytes.isEmpty)
        let transcript = try await api.transcribeAudio(
            data: bytes, filename: "fixture.wav", contentType: "audio/wav", language: "en")
        #expect(!transcript.text.isEmpty)

        let cancellable = try await api.createResponse(
            .init(
                input: .text("Write a numbered list of the integers from one to one thousand."),
                conversation: conversation.id))
        var cancelId: String?
        var terminal = false
        for try await event in cancellable {
            if case let .created(value) = event {
                cancelId = value.response.id
                do { _ = try await api.cancelResponse(id: value.response.id) } catch ResearchAPIError.http(
                    status: 400, reason: _)
                { /* Finished before cancellation: reconcile below. */  }
            }
            if event.isTerminal { terminal = true }
        }
        #expect(terminal)
        guard let cancelId else { throw ResearchAPIError.invalidResponse }
        let status = try await api.retrieveResponse(id: cancelId).status
        #expect(status == .cancelled || status == .completed)
        #expect(try await api.deleteConversation(id: conversation.id).deleted)
        try await client.signOut()
    }
}

#if os(macOS)
    /// This opt-in local fixture hook reads only disposable test mail codes. It never
    /// invokes a shell, prints captured output, or accepts production integration URLs.
    private func mailCode(kind: String, email: String) throws -> String {
        guard let path = ProcessInfo.processInfo.environment["MAPLE_SWIFT_MAIL_FIXTURE_HELPER"], path.hasPrefix("/")
        else {
            throw LocalFixtureError.mailHelperUnavailable
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = [kind, email]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { throw LocalFixtureError.mailHelperFailed }
        let bytes = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0, bytes.count <= 1_024,
            let code = String(data: bytes, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
            !code.isEmpty, code.count <= 128, !code.contains("\n")
        else { throw LocalFixtureError.mailHelperFailed }
        return code
    }
#endif

/// The hosted broker owns this endpoint. It is intentionally available only to
/// the integration fixture, not added to the native SDK API surface.
private func mintFixtureGrant(browser: ResearchAPI, prepared: PreparedNativeOAuthHandoff) async throws
    -> NativeOAuthHandoffGrant
{
    struct GrantResponse: Decodable { let grant: String; let expiresAt: Int64 }
    let response: GrantResponse = try await browser.json(
        .post, "/auth/native-handoff/grant",
        fields: [
            "native_session_id": .string(prepared.sessionID), "native_request_id": .string(prepared.requestID),
        ])
    guard response.expiresAt > Int64(Date().timeIntervalSince1970) else { throw ResearchAPIError.invalidResponse }
    return try NativeOAuthHandoffGrant(validating: response.grant)
}
