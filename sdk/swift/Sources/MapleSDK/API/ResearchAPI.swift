import Foundation

/// Research's API surface over an injected attested transport. This layer never
/// retries. An account-scoped instance carries the principal into every request.
/// Authentication responses must be installed by the credential owner using its
/// pre-request compare-and-swap snapshot, before publishing a signed-in account.
public struct ResearchAPI: Sendable {
    public let transport: any MapleTransport
    public let expectedPrincipalId: String?
    public let maximumResponseBytes: Int

    public init(
        transport: any MapleTransport, expectedPrincipalId: String? = nil,
        maximumResponseBytes: Int = 64 * 1_024 * 1_024
    ) {
        self.transport = transport
        self.expectedPrincipalId = expectedPrincipalId
        self.maximumResponseBytes = maximumResponseBytes
    }

    public func signIn(email: String, password: String, clientId: String) async throws -> LoginResponse {
        try await login(
            "/login", ["email": .string(email), "password": .string(password), "client_id": .string(clientId)])
    }

    public func signInGuest(id: String, password: String, clientId: String) async throws -> LoginResponse {
        try await login("/login", ["id": .string(id), "password": .string(password), "client_id": .string(clientId)])
    }

    public func signUp(
        email: String, password: String, inviteCode: String, clientId: String,
        name: String? = nil
    ) async throws -> LoginResponse {
        var fields: [String: JSONValue] = [
            "email": .string(email), "password": .string(password),
            "inviteCode": .string(inviteCode.lowercased()), "client_id": .string(clientId),
        ]
        if let name { fields["name"] = .string(name) }
        return try await login("/register", fields)
    }

    /// SDK parity only. Applications decide whether guest creation is offered.
    public func signUpGuest(password: String, inviteCode: String, clientId: String) async throws -> LoginResponse {
        try await login(
            "/register",
            [
                "password": .string(password), "inviteCode": .string(inviteCode.lowercased()),
                "client_id": .string(clientId),
            ])
    }

    public func getUser() async throws -> MapleUser {
        let result: UserResponse = try await json(.get, "/protected/user")
        guard expectedPrincipalId == nil || result.user.id == expectedPrincipalId else {
            throw ResearchAPIError.invalidResponse
        }
        return result.user
    }

    /// Remote revocation only. The account owner invalidates local work and clears
    /// credentials even when this best-effort network operation fails.
    public func signOut(refreshToken: String) async throws {
        try await empty(.post, "/logout", fields: ["refresh_token": .string(refreshToken)], auth: .none)
    }

    public func verifyEmail(code: String) async throws {
        try await empty(.get, "/verify-email/\(try Self.segment(code))", auth: .none)
    }

    public func requestNewVerificationEmail() async throws {
        try await empty(.post, "/protected/request_verification")
    }

    public func requestPasswordReset(email: String, hashedSecret: String, clientId: String) async throws {
        try await empty(
            .post, "/password-reset/request",
            fields: [
                "email": .string(email),
                "hashed_secret": .string(hashedSecret), "client_id": .string(clientId),
            ], auth: .none)
    }

    public func confirmPasswordReset(
        email: String, code: String, plaintextSecret: String,
        newPassword: String, clientId: String
    ) async throws {
        try await empty(
            .post, "/password-reset/confirm",
            fields: [
                "email": .string(email),
                "alphanumeric_code": .string(code), "plaintext_secret": .string(plaintextSecret),
                "new_password": .string(newPassword), "client_id": .string(clientId),
            ], auth: .none)
    }

    /// A returned token pair must be installed atomically against the authority
    /// which started this request. Never install a partial pair.
    public func changePassword(currentPassword: String, newPassword: String) async throws -> CredentialUpdateResponse {
        let result: CredentialUpdateResponse = try await json(
            .post, "/protected/change_password",
            fields: [
                "current_password": .string(currentPassword), "new_password": .string(newPassword),
            ])
        guard (result.accessToken == nil) == (result.refreshToken == nil),
            result.accessToken != "", result.refreshToken != ""
        else { throw ResearchAPIError.invalidResponse }
        return result
    }

    public func requestAccountDeletion(hashedSecret: String) async throws {
        try await empty(.post, "/protected/delete-account/request", fields: ["hashed_secret": .string(hashedSecret)])
    }

    public func confirmAccountDeletion(code: String, plaintextSecret: String) async throws {
        try await empty(
            .post, "/protected/delete-account/confirm",
            fields: [
                "confirmation_code": .string(code), "plaintext_secret": .string(plaintextSecret),
            ])
    }

    public func generateThirdPartyToken(audience: String? = nil) async throws -> ThirdPartyTokenResponse {
        try await json(.post, "/protected/third_party_token", fields: audience.map { ["audience": .string($0)] } ?? [:])
    }

    public func createConversation(
        metadata: [String: JSONValue]? = nil, projectId: String? = nil,
        pinned: Bool? = nil
    ) async throws -> Conversation {
        var fields: [String: JSONValue] = [:]
        if let metadata { fields["metadata"] = .object(metadata) }
        if let projectId { fields["project_id"] = .string(projectId) }
        if let pinned { fields["pinned"] = .bool(pinned) }
        return try await json(.post, "/v1/conversations", fields: fields)
    }

    public func getConversation(id: String) async throws -> Conversation {
        try await json(.get, "/v1/conversations/\(try Self.segment(id))")
    }

    public func listConversations(_ parameters: ConversationsListParameters = .init()) async throws -> APIPage<
        Conversation
    > {
        var query = try parameters.page.query()
        if let id = parameters.projectId { query.append(("project_id", id)) }
        if let value = parameters.unassignedProject { query.append(("unassigned_project", String(value))) }
        if let value = parameters.pinned { query.append(("pinned", String(value))) }
        return try await json(.get, Self.target("/v1/conversations", query))
    }

    public func updateConversation(
        id: String, metadata: [String: JSONValue]? = nil,
        projectId: NullableUpdate<String> = .unchanged, pinned: Bool? = nil
    ) async throws -> Conversation {
        var fields: [String: JSONValue] = [:]
        if let metadata { fields["metadata"] = .object(metadata) }
        switch projectId {
        case .unchanged: break
        case .clear: fields["project_id"] = .null
        case let .set(value): fields["project_id"] = .string(value)
        }
        if let pinned { fields["pinned"] = .bool(pinned) }
        guard !fields.isEmpty else { throw ResearchAPIError.invalidArgument }
        return try await json(.post, "/v1/conversations/\(try Self.segment(id))", fields: fields)
    }

    public func deleteConversation(id: String) async throws -> DeletedObject {
        try await json(.delete, "/v1/conversations/\(try Self.segment(id))")
    }

    public func deleteAllConversations() async throws -> DeletedObject {
        try await json(.delete, "/v1/conversations")
    }

    public func batchDeleteConversations(ids: [String]) async throws -> BatchDeleteResponse {
        try await json(.post, "/v1/conversations/batch-delete", fields: ["ids": .array(ids.map(JSONValue.string))])
    }

    public func batchUpdateConversationProject(ids: [String], projectId: String?) async throws
        -> BatchUpdateProjectResponse
    {
        try await json(
            .post, "/v1/conversations/batch-update-project",
            fields: [
                "ids": .array(ids.map(JSONValue.string)), "project_id": projectId.map(JSONValue.string) ?? .null,
            ])
    }

    public func listConversationItems(conversationId: String, parameters: ListParameters = .init()) async throws
        -> APIPage<ConversationItem>
    {
        try await json(
            .get, Self.target("/v1/conversations/\(try Self.segment(conversationId))/items", parameters.query()))
    }

    public func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
        try await json(.get, "/v1/conversations/\(try Self.segment(conversationId))/items/\(try Self.segment(itemId))")
    }

    public func createProject(name: String) async throws -> ConversationProject {
        try await json(.post, "/v1/conversation-projects", fields: ["name": .string(name)])
    }

    public func listProjects(_ parameters: ListParameters = .init()) async throws -> APIPage<
        ConversationProjectListItem
    > {
        guard parameters.before == nil else { throw ResearchAPIError.invalidArgument }
        return try await json(.get, Self.target("/v1/conversation-projects", parameters.query()))
    }

    public func getProject(id: String) async throws -> ConversationProject {
        try await json(.get, "/v1/conversation-projects/\(try Self.segment(id))")
    }

    public func updateProject(
        id: String, name: String? = nil,
        instructions: NullableUpdate<String> = .unchanged
    ) async throws -> ConversationProject {
        var fields: [String: JSONValue] = [:]
        if let name { fields["name"] = .string(name) }
        switch instructions {
        case .unchanged: break
        case .clear: fields["instructions"] = .null
        case let .set(value): fields["instructions"] = .string(value)
        }
        guard !fields.isEmpty else { throw ResearchAPIError.invalidArgument }
        return try await json(.post, "/v1/conversation-projects/\(try Self.segment(id))", fields: fields)
    }

    public func deleteProject(id: String) async throws -> DeletedObject {
        try await json(.delete, "/v1/conversation-projects/\(try Self.segment(id))")
    }

    public func createInstruction(name: String, prompt: String, isDefault: Bool? = nil) async throws -> Instruction {
        var fields: [String: JSONValue] = ["name": .string(name), "prompt": .string(prompt)]
        if let isDefault { fields["is_default"] = .bool(isDefault) }
        return try await json(.post, "/v1/instructions", fields: fields)
    }

    public func listInstructions(_ parameters: ListParameters = .init()) async throws -> APIPage<Instruction> {
        guard parameters.before == nil else { throw ResearchAPIError.invalidArgument }
        return try await json(.get, Self.target("/v1/instructions", parameters.query()))
    }

    public func getInstruction(id: String) async throws -> Instruction {
        try await json(.get, "/v1/instructions/\(try Self.segment(id))")
    }

    public func updateInstruction(
        id: String, name: String? = nil, prompt: String? = nil,
        isDefault: Bool? = nil
    ) async throws -> Instruction {
        var fields: [String: JSONValue] = [:]
        if let name { fields["name"] = .string(name) }
        if let prompt { fields["prompt"] = .string(prompt) }
        if let isDefault { fields["is_default"] = .bool(isDefault) }
        guard !fields.isEmpty else { throw ResearchAPIError.invalidArgument }
        return try await json(.post, "/v1/instructions/\(try Self.segment(id))", fields: fields)
    }

    public func deleteInstruction(id: String) async throws -> DeletedObject {
        try await json(.delete, "/v1/instructions/\(try Self.segment(id))")
    }

    public func setDefaultInstruction(id: String) async throws -> Instruction {
        try await json(.post, "/v1/instructions/\(try Self.segment(id))/set-default")
    }

    public func fetchModels(authenticated: Bool = true) async throws -> [Model] {
        struct Models: Decodable { let object: String; let data: [Model] }
        let result: Models = try await json(.get, "/v1/models", auth: authenticated ? .required : .none)
        guard result.object == "list" else { throw ResearchAPIError.invalidResponse }
        return result.data
    }

    public func fetchModelCatalog() async throws -> ModelCatalog {
        let result: ModelCatalog = try await json(.get, "/v1/models/catalog")
        guard result.object == "list" else { throw ResearchAPIError.invalidResponse }
        return result
    }

    public func transcribeAudio(
        data: Data, filename: String = "audio", contentType: String = "audio/mpeg",
        model: String = "whisper-large-v3", language: String? = nil,
        prompt: String? = nil, temperature: Double? = nil
    ) async throws -> TranscriptionResponse {
        var fields: [String: JSONValue] = [
            "file": .string(data.base64EncodedString()), "filename": .string(filename),
            "content_type": .string(contentType), "model": .string(model),
        ]
        if let language, !language.isEmpty { fields["language"] = .string(language) }
        if let prompt, !prompt.isEmpty { fields["prompt"] = .string(prompt) }
        if let temperature { fields["temperature"] = .number(temperature) }
        return try await json(.post, "/v1/audio/transcriptions", fields: fields)
    }

    /// Returns decrypted audio bytes without eagerly buffering an entire reply.
    public func synthesizeSpeech(
        input: String, voice: String, speed: Double = 1,
        model: String = "voxtral-tts", format: String = "wav"
    ) async throws -> MapleResponse {
        try await request(
            .post, "/v1/audio/speech",
            fields: [
                "input": .string(input), "voice": .string(voice),
                "speed": .number(speed), "model": .string(model), "response_format": .string(format),
            ])
    }

    public func retrieveResponse(id: String) async throws -> ResearchResponse {
        try await json(.get, "/v1/responses/\(try Self.segment(id))")
    }

    public func cancelResponse(id: String) async throws -> ResearchResponse {
        try await json(.post, "/v1/responses/\(try Self.segment(id))/cancel")
    }

    internal func login(_ target: String, _ fields: [String: JSONValue]) async throws -> LoginResponse {
        let result: LoginResponse = try await json(.post, target, fields: fields, auth: .none)
        guard !result.id.isEmpty, !result.accessToken.isEmpty, !result.refreshToken.isEmpty else {
            throw ResearchAPIError.invalidResponse
        }
        return result
    }

    internal func json<T: Decodable>(
        _ method: MapleHTTPMethod, _ target: String,
        fields: [String: JSONValue]? = nil,
        auth: MapleRequestAuthentication = .required
    ) async throws -> T {
        let response = try await request(method, target, fields: fields, auth: auth)
        let data = try await collect(response.body)
        do { return try APIJSON.decoder().decode(T.self, from: data) } catch { throw ResearchAPIError.invalidResponse }
    }

    internal func empty(
        _ method: MapleHTTPMethod, _ target: String,
        fields: [String: JSONValue]? = nil,
        auth: MapleRequestAuthentication = .required
    ) async throws {
        let response = try await request(method, target, fields: fields, auth: auth)
        _ = try await collect(response.body)
    }

    internal func request(
        _ method: MapleHTTPMethod, _ target: String,
        fields: [String: JSONValue]? = nil,
        auth: MapleRequestAuthentication = .required
    ) async throws -> MapleResponse {
        let body = try fields.map { try APIJSON.encode($0) }
        return try await checked(
            try await transport.send(
                MapleRequest(
                    method: method, target: target,
                    headers: fields == nil ? [:] : ["content-type": "application/json"], body: body,
                    auth: auth, expectedPrincipalId: expectedPrincipalId)))
    }

    internal func checked(_ response: MapleResponse) async throws -> MapleResponse {
        guard !(200...299).contains(response.status) else { return response }
        // Consume the authenticated tunnel to EOF before trusting error headers.
        let body = try await collect(response.body, limit: 1_024 * 1_024)
        if let capacity = OpenSecretInferenceCapacityError(response: response) { throw capacity }
        var reason: ResearchAPIError.RejectionReason = .unspecified
        // Recognize only stable, bounded public contracts. Never expose the body.
        let text = String(data: body, encoding: .utf8) ?? ""
        if response.status == 413 && text.contains("Message exceeds context limit") {
            reason = .contextLimit
        } else if response.status == 403 && text.contains("Free tier token limit exceeded") {
            reason = .freeTierTokenLimit
        } else if response.status == 403 && text.contains("Usage limit reached") {
            reason = .usageLimit
        } else if response.status == 503 && response.header("x-opensecret-error-contract") == "1"
            && response.header("x-opensecret-error-code") == "image_description_unavailable"
        {
            reason = .imageDescriptionUnavailable
        } else if response.status == 503 && response.header("x-opensecret-error-contract") == "1"
            && response.header("x-opensecret-error-code") == "billing_account_deletion_failed"
        {
            reason = .billingCleanupUnavailable
        } else if response.status == 401 && text.contains("Invalid email, password, or login method") {
            reason = .invalidLogin
        } else if response.status == 401 && text.contains("Invalid invite code") {
            reason = .invalidInviteCode
        } else if response.status == 409 && text.contains("Email already registered") {
            reason = .emailAlreadyRegistered
        } else if response.status == 409 && text.contains("User exists but Oauth not linked") {
            reason = .oauthNotLinked
        } else if response.status == 404 && text.contains("User not found") {
            reason = .userNotFound
        } else if response.status == 400 && text.contains("No valid email found for the Oauth account") {
            reason = .oauthEmailUnavailable
        } else if response.status == 400 && text.contains("User is already verified") {
            reason = .alreadyVerified
        } else if response.status == 403 && text.contains("Model not available on current plan") {
            reason = .modelUnavailable
        } else if response.status == 413 {
            reason = .payloadTooLarge
        } else if response.status == 429 {
            reason = .rateLimited
        } else if response.status == 401 {
            reason = .authenticationRequired
        } else if response.status == 403 {
            reason = .forbidden
        } else if response.status == 404 {
            reason = .notFound
        }
        throw ResearchAPIError.http(status: response.status, reason: reason)
    }

    internal func collect(_ stream: AsyncThrowingStream<Data, Error>, limit: Int? = nil) async throws -> Data {
        var data = Data()
        let limit = limit ?? maximumResponseBytes
        for try await chunk in stream {
            try Task.checkCancellation()
            guard limit >= 0, chunk.count <= limit - data.count else { throw ResearchAPIError.responseTooLarge }
            data.append(chunk)
        }
        try Task.checkCancellation()
        return data
    }

    internal static func segment(_ value: String) throws -> String {
        guard !value.isEmpty, value != ".", value != "..",
            !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else {
            throw ResearchAPIError.invalidArgument
        }
        return value.addingPercentEncoding(withAllowedCharacters: unreserved) ?? ""
    }

    internal static let unreserved = CharacterSet(
        charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")

    internal static func target(_ path: String, _ query: [(String, String)]) -> String {
        guard !query.isEmpty else { return path }
        return path + "?"
            + query.map { name, value in
                name + "=" + (value.addingPercentEncoding(withAllowedCharacters: unreserved) ?? "")
            }.joined(separator: "&")
    }
}
