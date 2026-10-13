import Foundation
import Testing
@testable import MapleSDK

actor APIRecorder: OAuthSessionTransport {
    struct Reply: Sendable {
        var status = 200
        var body: String
        var headers: [String: String] = [:]
        var sends = 1
    }
    var requests: [MapleRequest] = []
    var replies: [Reply]
    var bindings: [OAuthSessionBinding] = []
    var cancelledBindings: [OAuthSessionBinding] = []
    init(_ replies: [Reply]) { self.replies = replies }
    init(_ bodies: [String]) { self.replies = bodies.map { Reply(body: $0) } }
    func send(_ request: MapleRequest) throws -> MapleResponse {
        requests.append(request)
        guard !replies.isEmpty else { throw ResearchAPIError.invalidResponse }
        let reply = replies.removeFirst()
        return MapleResponse(
            status: reply.status, headers: reply.headers,
            body: AsyncThrowingStream {
                $0.yield(Data(reply.body.utf8)); $0.finish()
            }, inferenceSendCount: reply.sends)
    }
    func beginOAuth(_ request: MapleRequest) throws -> (MapleResponse, OAuthSessionBinding) {
        (try send(request), OAuthSessionBinding())
    }
    func completeOAuth(_ request: MapleRequest, binding: OAuthSessionBinding) throws -> MapleResponse {
        bindings.append(binding)
        return try send(request)
    }
    func cancelOAuth(binding: OAuthSessionBinding) { cancelledBindings.append(binding) }
    func snapshot() -> [MapleRequest] { requests }
}

private let login =
    #"{"id":"user-1","email":"fixture@example.test","access_token":"fake-access-canary","refresh_token":"fake-refresh-canary"}"#
private let conversation =
    #"{"id":"conversation-1","object":"conversation","created_at":1,"metadata":{"title":"Title"},"project_id":null,"pinned":false,"last_activity_at":2}"#
private let project =
    #"{"id":"project-1","object":"conversation.project","name":"Project","instructions":null,"created_at":1,"updated_at":2}"#
private let instruction =
    #"{"id":"instruction-1","object":"instruction","name":"Default","prompt":"Help","prompt_tokens":1,"is_default":true,"created_at":1,"updated_at":2}"#
private let deleted = #"{"id":"object-1","object":"conversation.deleted","deleted":true}"#
private let page = #"{"object":"list","data":[],"first_id":null,"last_id":null,"has_more":false}"#

private func object(_ request: MapleRequest) throws -> [String: JSONValue] {
    try JSONDecoder().decode([String: JSONValue].self, from: #require(request.body))
}

@Test func authLoginAndRegistrationWireParity() async throws {
    let transport = APIRecorder([login, login, login, login])
    let api = ResearchAPI(transport: transport)
    #expect(try await api.signIn(email: "fixture@example.test", password: "canary", clientId: "client").id == "user-1")
    _ = try await api.signInGuest(id: "guest", password: "canary", clientId: "client")
    _ = try await api.signUp(
        email: "fixture@example.test", password: "canary", inviteCode: "INVITE", clientId: "client", name: "Fixture")
    _ = try await api.signUpGuest(password: "canary", inviteCode: "INVITE", clientId: "client")
    let requests = await transport.snapshot()
    #expect(requests.map(\.target) == ["/login", "/login", "/register", "/register"])
    #expect(requests.allSatisfy { $0.auth == .none && $0.method == .post })
    #expect(
        try object(requests[0]) == [
            "email": .string("fixture@example.test"), "password": .string("canary"), "client_id": .string("client"),
        ])
    #expect(try object(requests[1])["id"] == .string("guest"))
    #expect(try object(requests[2])["inviteCode"] == .string("invite"))
    #expect(try object(requests[2])["name"] == .string("Fixture"))
    #expect(try object(requests[3])["email"] == nil)
}

@Test func accountFlowParityAndOwnership() async throws {
    let user =
        #"{"user":{"id":"user-1","name":null,"email":"fixture@example.test","email_verified":true,"login_method":"email","created_at":"now","updated_at":"now"}}"#
    let transport = APIRecorder([
        user, "", "", "", "", "", #"{"message":"changed","access_token":"new-a","refresh_token":"new-r"}"#, "", "",
        #"{"token":"billing-canary"}"#,
    ])
    let api = ResearchAPI(transport: transport, expectedPrincipalId: "user-1")
    #expect(try await api.getUser().emailVerified)
    try await api.signOut(refreshToken: "refresh-canary")
    try await api.verifyEmail(code: "a/b?c")
    try await api.requestNewVerificationEmail()
    try await api.requestPasswordReset(email: "fixture@example.test", hashedSecret: "hash", clientId: "client")
    try await api.confirmPasswordReset(
        email: "fixture@example.test", code: "code", plaintextSecret: "plain", newPassword: "new", clientId: "client")
    #expect(try await api.changePassword(currentPassword: "old", newPassword: "new").accessToken == "new-a")
    try await api.requestAccountDeletion(hashedSecret: "hash")
    try await api.confirmAccountDeletion(code: "code", plaintextSecret: "plain")
    _ = try await api.generateThirdPartyToken(audience: "https://billing.example.test")
    let requests = await transport.snapshot()
    #expect(
        requests.map(\.target) == [
            "/protected/user", "/logout", "/verify-email/a%2Fb%3Fc", "/protected/request_verification",
            "/password-reset/request", "/password-reset/confirm", "/protected/change_password",
            "/protected/delete-account/request", "/protected/delete-account/confirm", "/protected/third_party_token",
        ])
    #expect(requests.allSatisfy { $0.expectedPrincipalId == "user-1" })
    #expect(requests[0].body == nil)
    #expect(try object(requests[5])["alphanumeric_code"] == .string("code"))
    #expect(try object(requests[8])["plaintext_secret"] == .string("plain"))
}

@Test func credentialPartialUpdateAndWrongUserAreRejected() async throws {
    let transport = APIRecorder([
        #"{"message":"changed","access_token":"incomplete"}"#,
        #"{"user":{"id":"wrong","name":null,"email_verified":true,"login_method":"email","created_at":"now","updated_at":"now"}}"#,
    ])
    let api = ResearchAPI(transport: transport, expectedPrincipalId: "expected")
    await #expect(throws: ResearchAPIError.invalidResponse) {
        try await api.changePassword(currentPassword: "old", newPassword: "new")
    }
    await #expect(throws: ResearchAPIError.invalidResponse) { try await api.getUser() }
}

@Test func conversationCRUDPreservesNullFalseAndPagination() async throws {
    let transport = APIRecorder([
        conversation, conversation, conversation, page, deleted,
        #"{"object":"list.deleted","deleted":true}"#,
        #"{"object":"list","data":[{"id":"a","object":"conversation.deleted","deleted":false,"error":"not_found"}]}"#,
        #"{"success":true}"#, page,
        #"{"id":"item-1","response_id":"response-1","type":"reasoning","status":"completed","content":[{"type":"reasoning_text","text":"Thought"}]}"#,
    ])
    let api = ResearchAPI(transport: transport, expectedPrincipalId: "owner")
    _ = try await api.createConversation(metadata: ["title": .string("Title")], projectId: "p", pinned: false)
    _ = try await api.getConversation(id: "a/b")
    _ = try await api.updateConversation(id: "a", projectId: .clear, pinned: false)
    _ = try await api.listConversations(
        .init(page: .init(limit: 20, after: "a+b&c", order: .desc), unassignedProject: true, pinned: false))
    _ = try await api.deleteConversation(id: "a")
    _ = try await api.deleteAllConversations()
    #expect(try await api.batchDeleteConversations(ids: ["a"]).data.first?.error == "not_found")
    _ = try await api.batchUpdateConversationProject(ids: ["a"], projectId: nil)
    _ = try await api.listConversationItems(
        conversationId: "a", parameters: .init(limit: 20, after: "cursor", order: .desc))
    #expect(try await api.getConversationItem(conversationId: "a", itemId: "item").type == "reasoning")
    let requests = await transport.snapshot()
    #expect(requests[1].target == "/v1/conversations/a%2Fb")
    #expect(try object(requests[2]) == ["project_id": .null, "pinned": .bool(false)])
    #expect(
        requests[3].target
            == "/v1/conversations?limit=20&after=a%2Bb%26c&order=desc&unassigned_project=true&pinned=false")
    #expect(requests[4].method == .delete && requests[5].method == .delete)
    #expect(try object(requests[7])["project_id"] == .null)
    #expect(requests[8].target == "/v1/conversations/a/items?limit=20&after=cursor&order=desc")
    #expect(requests[9].target == "/v1/conversations/a/items/item")
    #expect(requests.allSatisfy { $0.auth == .required && $0.expectedPrincipalId == "owner" })
}

@Test func projectAndInstructionParity() async throws {
    let transport = APIRecorder([
        project, page, project, project, deleted, instruction, page, instruction, instruction, deleted, instruction,
    ])
    let api = ResearchAPI(transport: transport)
    _ = try await api.createProject(name: "Project")
    _ = try await api.listProjects(.init(limit: 10, order: .asc))
    _ = try await api.getProject(id: "p")
    _ = try await api.updateProject(id: "p", instructions: .clear)
    _ = try await api.deleteProject(id: "p")
    _ = try await api.createInstruction(name: "Default", prompt: "Help", isDefault: false)
    _ = try await api.listInstructions(.init(after: "cursor"))
    _ = try await api.getInstruction(id: "i")
    _ = try await api.updateInstruction(id: "i", prompt: "Changed", isDefault: false)
    _ = try await api.deleteInstruction(id: "i")
    _ = try await api.setDefaultInstruction(id: "i")
    let requests = await transport.snapshot()
    #expect(
        requests.map(\.target) == [
            "/v1/conversation-projects", "/v1/conversation-projects?limit=10&order=asc", "/v1/conversation-projects/p",
            "/v1/conversation-projects/p", "/v1/conversation-projects/p", "/v1/instructions",
            "/v1/instructions?after=cursor", "/v1/instructions/i", "/v1/instructions/i", "/v1/instructions/i",
            "/v1/instructions/i/set-default",
        ])
    #expect(try object(requests[3]) == ["instructions": .null])
    #expect(try object(requests[5])["is_default"] == .bool(false))
    #expect(try object(requests[8])["is_default"] == .bool(false))
    #expect(requests[10].body == nil)
    await #expect(throws: ResearchAPIError.invalidArgument) { try await api.updateProject(id: "p") }
    await #expect(throws: ResearchAPIError.invalidArgument) { try await api.updateConversation(id: "c") }
    await #expect(throws: ResearchAPIError.invalidArgument) {
        try await api.listConversations(.init(page: .init(limit: 101)))
    }
}

@Test func modelAndAudioContracts() async throws {
    let models = #"{"object":"list","data":[{"id":"model","object":"model","created":0,"owned_by":"opensecret"}]}"#
    let catalog =
        #"{"object":"list","data":[],"aliases":[],"defaults":{"quick":"auto:quick","powerful":"auto:powerful"},"audio":{"speech":{"available":true,"model":"voxtral-tts"}}}"#
    let transport = APIRecorder([models, models, catalog, #"{"text":"Transcript"}"#, "wav"])
    let api = ResearchAPI(transport: transport)
    #expect(try await api.fetchModels(authenticated: false).first?.ownedBy == "opensecret")
    _ = try await api.fetchModels()
    #expect(try await api.fetchModelCatalog().defaults.powerful == .powerful)
    #expect(
        try await api.transcribeAudio(
            data: Data([0, 1, 2]), filename: "fixture.wav", contentType: "audio/wav", language: "en", temperature: 0
        ).text == "Transcript")
    let speech = try await api.synthesizeSpeech(input: "Hello", voice: "voice", speed: 1.5)
    #expect(String(data: try await api.collect(speech.body), encoding: .utf8) == "wav")
    let requests = await transport.snapshot()
    #expect(requests[0].auth == .none && requests[1].auth == .required)
    #expect(
        requests.map(\.target) == [
            "/v1/models", "/v1/models", "/v1/models/catalog", "/v1/audio/transcriptions", "/v1/audio/speech",
        ])
    #expect(try object(requests[3])["file"] == .string("AAEC"))
    #expect(try object(requests[3])["model"] == .string("whisper-large-v3"))
    #expect(try object(requests[4])["response_format"] == .string("wav"))
}

@Test func errorsAreSanitizedBoundedAndNeverRetried() async throws {
    let transport = APIRecorder([
        .init(
            status: 403, body: #"{"error":{"code":"usage_limit_reached","message":"private-canary"}}"#,
            headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "usage_limit_reached"])
    ])
    let api = ResearchAPI(transport: transport)
    do { _ = try await api.getConversation(id: "c"); Issue.record("Expected error") } catch {
        #expect(error as? ResearchAPIError == .http(status: 403, reason: .usageLimit))
        #expect(!String(describing: error).contains("private-canary"))
    }
    #expect(await transport.snapshot().count == 1)
    let oversized = ResearchAPI(transport: APIRecorder([conversation]), maximumResponseBytes: 5)
    await #expect(throws: ResearchAPIError.responseTooLarge) { try await oversized.getConversation(id: "c") }
}

@Test func chatErrorCodesUseEverySuppliedSourceAndTheHTTPStatus() async throws {
    let cases: [(Int, String, ResearchAPIError.RejectionReason, ResearchAPIError.RejectionReason)] = [
        (403, "usage_limit_reached", .usageLimit, .forbidden),
        (403, "free_tier_token_limit_exceeded", .freeTierTokenLimit, .forbidden),
        (413, "message_exceeds_context_limit", .contextLimit, .payloadTooLarge),
        (403, "model_not_available_on_plan", .modelUnavailable, .forbidden),
        (400, "unsupported_value", .unsupportedValue, .unspecified),
    ]
    for (status, code, reason, fallback) in cases {
        let headers = ["x-opensecret-error-contract": "1", "x-opensecret-error-code": code]
        let body = String(
            data: try JSONEncoder().encode([
                "code": JSONValue.string(code),
                "error": .object(["code": .string(code), "message": .string("Changed public wording")]),
            ]), encoding: .utf8)!
        for reply in [
            APIRecorder.Reply(status: status, body: body, headers: headers),
            APIRecorder.Reply(status: status, body: "", headers: headers),
            APIRecorder.Reply(status: status, body: body),
        ] {
            let api = ResearchAPI(transport: APIRecorder([reply]))
            await #expect(throws: ResearchAPIError.http(status: status, reason: reason)) {
                try await api.getConversation(id: "c")
            }
        }
        // The same code under a different HTTP status cannot establish the issue.
        let wrongStatus = status == 403 ? 413 : 403
        let wrongReason: ResearchAPIError.RejectionReason = wrongStatus == 413 ? .payloadTooLarge : .forbidden
        let wrongAPI = ResearchAPI(
            transport: APIRecorder([.init(status: wrongStatus, body: body, headers: headers)]))
        await #expect(throws: ResearchAPIError.http(status: wrongStatus, reason: wrongReason)) {
            try await wrongAPI.getConversation(id: "c")
        }
        let unversionedAPI = ResearchAPI(
            transport: APIRecorder([
                .init(status: status, body: body, headers: ["x-opensecret-error-code": code])
            ]))
        await #expect(throws: ResearchAPIError.http(status: status, reason: fallback)) {
            try await unversionedAPI.getConversation(id: "c")
        }
        // A nested body code alone is sufficient, with or without contract 1.
        let nestedBody = String(
            data: try JSONEncoder().encode(["error": ["code": code]]), encoding: .utf8)!
        for nestedHeaders in [[:], ["x-opensecret-error-contract": "1"]] {
            let nestedAPI = ResearchAPI(
                transport: APIRecorder([.init(status: status, body: nestedBody, headers: nestedHeaders)]))
            await #expect(throws: ResearchAPIError.http(status: status, reason: reason)) {
                try await nestedAPI.getConversation(id: "c")
            }
        }
    }
}

@Test func conflictingUnknownAndMalformedChatCodesNeverUseLegacyMessages() async throws {
    let legacy = #"{"status":403,"message":"Usage limit reached"}"#
    let cases: [APIRecorder.Reply] = [
        .init(
            status: 403, body: legacy,
            headers: ["x-opensecret-error-contract": "2", "x-opensecret-error-code": "usage_limit_reached"]),
        .init(status: 403, body: legacy, headers: ["x-opensecret-error-contract": "2"]),
        .init(status: 403, body: legacy, headers: ["x-opensecret-error-code": "usage_limit_reached"]),
        .init(
            status: 403, body: legacy,
            headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "unknown"]),
        .init(status: 403, body: #"{"status":403,"message":"Usage limit reached","code":"unknown"}"#),
        .init(status: 403, body: #"{"status":403,"message":"Usage limit reached","code":403}"#),
        .init(status: 403, body: #"{"status":403,"message":"Usage limit reached","error":{"code":false}}"#),
        .init(
            status: 403,
            body:
                #"{"status":403,"message":"Usage limit reached","code":"usage_limit_reached","error":{"code":"free_tier_token_limit_exceeded"}}"#
        ),
        .init(
            status: 403,
            body: #"{"status":403,"message":"Usage limit reached","error":{"code":"model_not_available_on_plan"}}"#,
            headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "usage_limit_reached"]),
        .init(
            status: 403, body: #"{"error":{"code":"model_not_available_on_plan"},"extra":1e400}"#,
            headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "usage_limit_reached"]),
        .init(
            status: 403, body: #"{"error":{"code":1e400}}"#,
            headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "usage_limit_reached"]),
    ]
    for reply in cases {
        let api = ResearchAPI(transport: APIRecorder([reply]))
        await #expect(throws: ResearchAPIError.http(status: 403, reason: .forbidden)) {
            try await api.getConversation(id: "c")
        }
    }
}

@Test func legacyChatLimitsRequireExactBoundedMessagesAndMatchingStatuses() async throws {
    let cases: [(Int, String, ResearchAPIError.RejectionReason)] = [
        (403, "Usage limit reached", .usageLimit),
        (403, "Free tier token limit exceeded", .freeTierTokenLimit),
        (413, "Message exceeds context limit", .contextLimit),
    ]
    for (status, message, reason) in cases {
        let body = String(
            data: try JSONEncoder().encode(["status": JSONValue.integer(Int64(status)), "message": .string(message)]),
            encoding: .utf8)!
        let api = ResearchAPI(transport: APIRecorder([.init(status: status, body: body)]))
        await #expect(throws: ResearchAPIError.http(status: status, reason: reason)) {
            try await api.getConversation(id: "c")
        }
    }
    for body in [
        "Usage limit reached",
        #"{"error":"Usage limit reached"}"#,
        #"{"message":"Usage limit reached"}"#,
        #"{"status":"403","message":"Usage limit reached"}"#,
        #"{"status":413,"message":"Usage limit reached"}"#,
        #"{"status":403,"message":"Usage limit reached soon"}"#,
        #"{"status":403,"message":"Model not available on current plan"}"#,
        #"{"status":403,"message":"Usage limit reached""#,
        #"{"status":403,"message":"Usage limit reached"} trailing"#,
        #"{"status":403,"message":"Usage limit reached","padding":""# + String(repeating: "x", count: 4_096) + #""}"#,
    ] {
        let api = ResearchAPI(transport: APIRecorder([.init(status: 403, body: body)]))
        await #expect(throws: ResearchAPIError.http(status: 403, reason: .forbidden)) {
            try await api.getConversation(id: "c")
        }
    }
}

@Test func chatErrorHeadersRemainUntrustedUntilTheBodyFinishes() async throws {
    let api = ResearchAPI(transport: APIRecorder([String]()))
    let response = MapleResponse(
        status: 403,
        headers: ["x-opensecret-error-contract": "1", "x-opensecret-error-code": "usage_limit_reached"],
        body: AsyncThrowingStream {
            $0.yield(Data(#"{"error":{"code":"usage_limit_reached"}}"#.utf8))
            $0.finish(throwing: ResearchAPIError.invalidStream)
        })
    await #expect(throws: ResearchAPIError.invalidStream) { try await api.checked(response) }
}

@Test(arguments: ["0", "60", "61", "999999999999999999999", "01", "not-a-delay"])
func capacityContractAndBoundedRetryAfter(_ delay: String) throws {
    let response = MapleResponse(
        status: 503,
        headers: [
            "X-OpenSecret-Error-Contract": "1", "x-opensecret-error-code": "inference_capacity",
            "x-opensecret-client-replay": "safe", "retry-after": delay,
        ], body: AsyncThrowingStream { $0.finish() }, inferenceSendCount: 2)
    let error = try #require(OpenSecretInferenceCapacityError(response: response))
    #expect(error.inferenceSendCount == 2)
    let expected: Int? = ["0": 0, "60": 60_000, "01": 1_000, "not-a-delay": 1_000][delay]
    #expect(error.retryDelayMilliseconds == expected)
    #expect(
        OpenSecretInferenceCapacityError(
            response: MapleResponse(status: 503, body: AsyncThrowingStream { $0.finish() })) == nil)
}

@Test func capacityFinderRecognizesBoundedFoundationWrappers() throws {
    let response = MapleResponse(
        status: 429,
        headers: [
            "x-opensecret-error-contract": "1", "x-opensecret-error-code": "inference_capacity",
            "x-opensecret-client-replay": "safe",
        ], body: AsyncThrowingStream { $0.finish() })
    let capacity = try #require(OpenSecretInferenceCapacityError(response: response))
    let wrapper = NSError(domain: "SyntheticWrapper", code: 0, userInfo: [NSUnderlyingErrorKey: capacity])
    #expect(findOpenSecretInferenceCapacityError(wrapper) == capacity)
    #expect(findOpenSecretInferenceCapacityError(ResearchAPIError.invalidResponse) == nil)
    var nested: any Error = capacity
    for _ in 0..<8 { nested = NSError(domain: "SyntheticWrapper", code: 0, userInfo: [NSUnderlyingErrorKey: nested]) }
    #expect(findOpenSecretInferenceCapacityError(nested) == nil)
}

@Test func accountAndPlanFailuresRetainOnlyPublicCategories() async throws {
    let cases: [(Int, String, ResearchAPIError.RejectionReason)] = [
        (401, "Invalid email, password, or login method", .invalidLogin),
        (401, "Invalid invite code", .invalidInviteCode),
        (409, "Email already registered", .emailAlreadyRegistered),
        (409, "User exists but Oauth not linked", .oauthNotLinked),
        (404, "User not found", .userNotFound),
        (400, "No valid email found for the Oauth account", .oauthEmailUnavailable),
        (400, "User is already verified", .alreadyVerified),
        (403, "Model not available on current plan", .forbidden),
        (413, "Payload too large", .payloadTooLarge),
        (429, "Too many requests", .rateLimited),
    ]
    for (status, message, expected) in cases {
        let body = String(data: try JSONEncoder().encode(["message": message]), encoding: .utf8)!
        let api = ResearchAPI(transport: APIRecorder([.init(status: status, body: body)]))
        await #expect(throws: ResearchAPIError.http(status: status, reason: expected)) { try await api.getUser() }
    }
    let api = ResearchAPI(
        transport: APIRecorder([
            .init(
                status: 503, body: "private-canary",
                headers: [
                    "x-opensecret-error-contract": "1", "x-opensecret-error-code": "billing_account_deletion_failed",
                ])
        ]))
    await #expect(throws: ResearchAPIError.http(status: 503, reason: .billingCleanupUnavailable)) {
        try await api.confirmAccountDeletion(code: "code", plaintextSecret: "secret")
    }
}
