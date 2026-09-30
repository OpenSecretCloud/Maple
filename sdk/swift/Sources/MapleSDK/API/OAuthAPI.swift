import Foundation

public enum OAuthProvider: String, Codable, Sendable { case google, github, apple }

/// Only the transport can create handles; the retained session and anonymous
/// authority stay in the transport. A copied handle never permits a second use.
public struct OAuthSessionBinding: Sendable, Hashable {
    public let id: UUID
    internal init(id: UUID = UUID()) { self.id = id }
}

public protocol OAuthSessionTransport: MapleTransport {
    func beginOAuth(_ request: MapleRequest) async throws -> (MapleResponse, OAuthSessionBinding)
    func completeOAuth(_ request: MapleRequest, binding: OAuthSessionBinding) async throws -> MapleResponse
    func cancelOAuth(binding: OAuthSessionBinding) async
}

/// One session-bound OAuth attempt. Completion consumes it before awaiting any
/// network operation. State/provider/expiry validation happens before dispatch.
public actor OAuthAttempt {
    public nonisolated let authorizationURL: URL
    public nonisolated let provider: OAuthProvider
    private let state: String
    private let binding: OAuthSessionBinding
    private let startedAt: Date
    private let transport: any OAuthSessionTransport
    private enum State { case pending, completing, cancelled, finished }
    private var stateOfAttempt = State.pending

    internal init(
        url: URL, provider: OAuthProvider, state: String, binding: OAuthSessionBinding,
        startedAt: Date, transport: any OAuthSessionTransport
    ) {
        authorizationURL = url; self.provider = provider; self.state = state
        self.binding = binding; self.startedAt = startedAt; self.transport = transport
    }

    public func cancel() async {
        stateOfAttempt = .cancelled
        await transport.cancelOAuth(binding: binding)
    }

    public func complete(
        provider callbackProvider: OAuthProvider, code: String, state callbackState: String,
        inviteCode: String = "", now: Date = Date()
    ) async throws -> LoginResponse {
        guard stateOfAttempt == .pending else { throw ResearchAPIError.invalidCallback }
        stateOfAttempt = .completing
        do {
            guard now >= startedAt, now.timeIntervalSince(startedAt) <= 15 * 60 else {
                throw ResearchAPIError.expiredAuthenticationAttempt
            }
            guard callbackProvider == provider, callbackState == state, !code.isEmpty else {
                throw ResearchAPIError.invalidCallback
            }
            let body = try APIJSON.encode(["code": code, "state": state, "invite_code": inviteCode])
            let response = try await transport.completeOAuth(
                MapleRequest(
                    method: .post,
                    target: "/auth/\(provider.rawValue)/callback", headers: ["content-type": "application/json"],
                    body: body, auth: .none, allowSessionRecovery: false), binding: binding)
            let api = ResearchAPI(transport: transport)
            let checked = try await api.checked(response)
            let data = try await api.collect(checked.body)
            let login: LoginResponse
            do { login = try APIJSON.decoder().decode(LoginResponse.self, from: data) } catch {
                throw ResearchAPIError.invalidResponse
            }
            guard !login.id.isEmpty, !login.accessToken.isEmpty, !login.refreshToken.isEmpty else {
                throw ResearchAPIError.invalidResponse
            }
            try Task.checkCancellation()
            guard stateOfAttempt == .completing else { throw ResearchAPIError.invalidCallback }
            stateOfAttempt = .finished
            return login
        } catch {
            stateOfAttempt = .finished
            await transport.cancelOAuth(binding: binding)
            throw error
        }
    }
}

extension ResearchAPI {
    public func initiateOAuth(
        provider: OAuthProvider, clientId: String, inviteCode: String? = nil,
        redirectURL: URL? = nil, now: Date = Date()
    ) async throws -> OAuthAttempt {
        guard let transport = transport as? any OAuthSessionTransport else { throw ResearchAPIError.invalidArgument }
        var fields: [String: JSONValue] = ["client_id": .string(clientId)]
        if let inviteCode, !inviteCode.isEmpty { fields["invite_code"] = .string(inviteCode) }
        if let redirectURL { fields["redirect_url"] = .string(redirectURL.absoluteString) }
        let (response, binding) = try await transport.beginOAuth(
            MapleRequest(
                method: .post,
                target: "/auth/\(provider.rawValue)", headers: ["content-type": "application/json"],
                body: try APIJSON.encode(fields), auth: .none))
        do {
            let checked = try await checked(response)
            let data = try await collect(checked.body)
            let result: OAuthInitiationResponse
            do { result = try APIJSON.decoder().decode(OAuthInitiationResponse.self, from: data) } catch {
                throw ResearchAPIError.invalidResponse
            }
            guard let state = provider == .apple ? result.state : result.csrfToken, !state.isEmpty,
                let url = URL(string: result.authUrl), url.scheme == "https", url.host != nil,
                url.user == nil, url.password == nil
            else { throw ResearchAPIError.invalidResponse }
            try Task.checkCancellation()
            return OAuthAttempt(
                url: url, provider: provider, state: state, binding: binding,
                startedAt: now, transport: transport)
        } catch {
            await transport.cancelOAuth(binding: binding)
            throw error
        }
    }

    /// Apple receives SHA-256(rawNonce); the enclave receives rawNonce and checks
    /// the identity-token nonce. This API requires it even though older SDKs made
    /// the field optional. The caller retains attempt ownership during acquisition.
    public func handleAppleNativeSignIn(
        identityToken: String, userIdentifier: String,
        rawNonce: String, clientId: String, email: String? = nil,
        givenName: String? = nil, familyName: String? = nil,
        inviteCode: String? = nil
    ) async throws -> LoginResponse {
        guard !identityToken.isEmpty, !userIdentifier.isEmpty, !rawNonce.isEmpty else {
            throw ResearchAPIError.invalidArgument
        }
        var fields: [String: JSONValue] = [
            "identity_token": .string(identityToken),
            "user_identifier": .string(userIdentifier), "nonce": .string(rawNonce), "client_id": .string(clientId),
        ]
        if let email { fields["email"] = .string(email) }
        if let givenName { fields["given_name"] = .string(givenName) }
        if let familyName { fields["family_name"] = .string(familyName) }
        if let inviteCode, !inviteCode.isEmpty { fields["invite_code"] = .string(inviteCode) }
        return try await login("/auth/apple/native", fields)
    }
}

public struct NativeOAuthHandoffGrant: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    public let rawValue: String
    public var description: String { "NativeOAuthHandoffGrant(<redacted>)" }
    public var debugDescription: String { description }

    public init(validating value: String) throws {
        let segments = value.split(separator: ".", omittingEmptySubsequences: false)
        guard value.utf8.count <= 4_096, segments.count == 3,
            segments.allSatisfy({ Self.isCanonicalBase64URL(String($0)) })
        else {
            throw ResearchAPIError.invalidCallback
        }
        rawValue = value
    }

    /// Validates the envelope only. The enclave verifies the grant's signature,
    /// expiry, account, and exact native session/request binding during redemption.
    public init(callbackURL: URL, expectedScheme: String) throws {
        guard let parts = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false),
            parts.scheme == expectedScheme, parts.host == "auth", parts.user == nil,
            parts.password == nil, parts.port == nil, parts.fragment == nil,
            parts.path.isEmpty || parts.path == "/",
            let query = parts.queryItems, query.count == 1,
            query[0].name == "handoff_grant", let grant = query[0].value
        else {
            throw ResearchAPIError.invalidCallback
        }
        try self.init(validating: grant)
    }

    private static func isCanonicalBase64URL(_ value: String) -> Bool {
        guard !value.isEmpty, value.utf8.count % 4 != 1,
            value.utf8.allSatisfy({
                (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 45 || $0 == 95
            })
        else { return false }
        let standard = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        guard let decoded = Data(base64Encoded: standard + String(repeating: "=", count: (4 - standard.count % 4) % 4))
        else { return false }
        return decoded.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "") == value
    }
}
