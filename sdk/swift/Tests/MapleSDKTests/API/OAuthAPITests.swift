import Foundation
import Testing
@testable import MapleSDK

private let oauthLogin = #"{"id":"user-1","access_token":"fake-access-canary","refresh_token":"fake-refresh-canary"}"#

@Test(arguments: [OAuthProvider.google, .github, .apple])
func oauthInitiationAndOneUseCallbacks(provider: OAuthProvider) async throws {
    let stateKey = provider == .apple ? "state" : "csrf_token"
    let transport = APIRecorder([
        "{\"auth_url\":\"https://provider.example.test/authorize\",\"\(stateKey)\":\"state-canary\"}", oauthLogin,
    ])
    let api = ResearchAPI(transport: transport)
    let now = Date(timeIntervalSince1970: 1_000)
    let redirect = try #require(URL(string: "https://AUTH.example.test:443/auth/%63allback?b=2&a=1"))
    let attempt = try await api.initiateOAuth(
        provider: provider, clientId: "client", inviteCode: "INVITE", redirectURL: redirect, now: now)
    #expect(attempt.authorizationURL.host == "provider.example.test")
    #expect(
        try await attempt.complete(provider: provider, code: "code-canary", state: "state-canary", now: now).id
            == "user-1")
    await #expect(throws: ResearchAPIError.invalidCallback) {
        try await attempt.complete(provider: provider, code: "code-canary", state: "state-canary", now: now)
    }
    let requests = await transport.snapshot()
    #expect(requests.count == 2)
    #expect(requests[0].target == "/auth/\(provider.rawValue)")
    #expect(requests[1].target == "/auth/\(provider.rawValue)/callback")
    #expect(!requests[1].allowSessionRecovery && requests[1].auth == .none)
    let fields = try JSONDecoder().decode([String: String].self, from: #require(requests[0].body))
    #expect(fields["redirect_url"] == redirect.absoluteString)
    #expect(fields["invite_code"] == "INVITE")
}

@Test(arguments: ["wrong-state", "wrong-provider", "expired", "future", "cancelled"])
func oauthRejectionsNeverDispatch(_ rejection: String) async throws {
    let transport = APIRecorder([#"{"auth_url":"https://provider.example.test/authorize","csrf_token":"state"}"#])
    let api = ResearchAPI(transport: transport)
    let now = Date(timeIntervalSince1970: 1_000)
    let attempt = try await api.initiateOAuth(provider: .google, clientId: "client", now: now)
    if rejection == "cancelled" { await attempt.cancel() }
    let callbackNow =
        rejection == "expired" ? now.addingTimeInterval(901) : rejection == "future" ? now.addingTimeInterval(-1) : now
    await #expect(throws: ResearchAPIError.self) {
        try await attempt.complete(
            provider: rejection == "wrong-provider" ? .github : .google,
            code: "code", state: rejection == "wrong-state" ? "wrong" : "state", now: callbackNow)
    }
    #expect(await transport.snapshot().count == 1)
    #expect(await transport.cancelledBindings.count == 1)
}

@Test func nativeApplePassesRawNonceAndOptionalProfileWithoutLogging() async throws {
    let transport = APIRecorder([oauthLogin])
    let api = ResearchAPI(transport: transport)
    _ = try await api.handleAppleNativeSignIn(
        identityToken: "identity-canary", userIdentifier: "apple-sub", rawNonce: "nonce-canary", clientId: "client",
        givenName: "Fixture")
    let request = try #require(await transport.snapshot().first)
    let fields = try JSONDecoder().decode([String: String].self, from: #require(request.body))
    #expect(request.target == "/auth/apple/native" && request.auth == .none)
    #expect(fields["nonce"] == "nonce-canary")
    #expect(fields["given_name"] == "Fixture")
    #expect(fields["email"] == nil && fields["invite_code"] == nil)
    await #expect(throws: ResearchAPIError.invalidArgument) {
        try await api.handleAppleNativeSignIn(
            identityToken: "token", userIdentifier: "sub", rawNonce: "", clientId: "client")
    }
    #expect(await transport.snapshot().count == 1)
}

@Test func secureSecretAndHashMatchReference() throws {
    let secret = try generateSecureSecret()
    #expect(secret.count == 64)
    #expect(secret.allSatisfy { "0123456789abcdef".contains($0) })
    #expect(secret != (try generateSecureSecret()))
    #expect(hashSecret("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
}

@Test func nativeHandoffEnforcesCanonicalGrantAndExactURL() throws {
    let grant = "eyJhbGciOiJFUzI1NksifQ.eyJzdWIiOiJ1c2VyIn0.c2lnbmF0dXJl"
    let valid = try NativeOAuthHandoffGrant(
        callbackURL: #require(URL(string: "cloud.opensecret.maple.dev://auth?handoff_grant=\(grant)")),
        expectedScheme: "cloud.opensecret.maple.dev")
    #expect(valid.rawValue == grant)
    #expect(!String(describing: valid).contains(grant))
    #expect(!String(reflecting: valid).contains(grant))
    let badURLs = [
        "cloud.opensecret.maple://auth?handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://evil?handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://auth/other?handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://user@auth?handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://auth:1?handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://auth?handoff_grant=\(grant)#fragment",
        "cloud.opensecret.maple.dev://auth?handoff_grant=\(grant)&handoff_grant=\(grant)",
        "cloud.opensecret.maple.dev://auth?handoff_grant=\(grant)&state=x",
        "cloud.opensecret.maple.dev://auth?token=\(grant)",
    ]
    for url in badURLs {
        #expect(throws: ResearchAPIError.invalidCallback) {
            try NativeOAuthHandoffGrant(
                callbackURL: #require(URL(string: url)), expectedScheme: "cloud.opensecret.maple.dev")
        }
    }
    for bad in ["", "a.b.c", "YQ.Yg", "YQ.Yg.Yw.extra", "YQ=.Yg.Yw", "YR.Yg.Yw", String(repeating: "x", count: 4097)] {
        #expect(throws: ResearchAPIError.invalidCallback) { try NativeOAuthHandoffGrant(validating: bad) }
    }
}

@Test func cancelledAndRejectedOAuthAttemptsReleaseTheirTransportBinding() async throws {
    let success = #"{"auth_url":"https://provider.example.test/authorize","csrf_token":"state"}"#
    let recorder = APIRecorder(Array(repeating: success, count: 20))
    let api = ResearchAPI(transport: recorder)
    for _ in 0..<20 {
        let attempt = try await api.initiateOAuth(provider: .google, clientId: "client")
        await attempt.cancel()
    }
    #expect(await recorder.cancelledBindings.count == 20)

    for reply in [
        APIRecorder.Reply(body: "invalid"),
        .init(body: #"{"auth_url":"http://provider.example.test","csrf_token":"state"}"#),
        .init(status: 403, body: "forbidden"),
    ] {
        let recorder = APIRecorder([reply])
        let api = ResearchAPI(transport: recorder)
        await #expect(throws: ResearchAPIError.self) {
            try await api.initiateOAuth(provider: .google, clientId: "client")
        }
        #expect(await recorder.cancelledBindings.count == 1)
    }
}

@Test func rejectedOAuthInitiationDoesNotExhaustRealTransportCapacity() async throws {
    let store = TestSecureStore()
    let credentials = try CredentialAuthority(apiURL: "http://127.0.0.1:3000", store: store)
    let network = FixtureNetwork()
    let transport = try EncryptedTransport(
        apiURL: "http://127.0.0.1:3000", policy: PCRTrustPolicy(environment: .development), credentials: credentials,
        store: store, network: network, now: Date.init)
    let api = ResearchAPI(transport: transport)
    // FixtureNetwork returns authenticated JSON without the OAuth fields. Every
    // initiation must release its retained handle after decoding fails.
    for _ in 0..<20 {
        await #expect(throws: ResearchAPIError.invalidResponse) {
            try await api.initiateOAuth(provider: .google, clientId: "client")
        }
    }
    for _ in 0..<20 {
        let (response, binding) = try await transport.beginOAuth(
            MapleRequest(method: .post, target: "/auth/google", auth: .none))
        _ = try await api.collect(response.body)
        await transport.cancelOAuth(binding: binding)
    }
    #expect(await network.observations.count == 40)
    #expect(await network.handshakes == 1)
}
