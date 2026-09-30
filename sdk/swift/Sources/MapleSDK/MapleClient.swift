import Foundation

/// One account authority and transport per configured API origin. Keep this client
/// for the application's lifetime; navigation must not create competing authorities.
public final class MapleClient: Sendable {
    public let transport: EncryptedTransport
    public let credentials: CredentialAuthority
    public let clientID: String

    public init(
        apiURL: String, clientID: String, policy: PCRTrustPolicy,
        store: any SecureStore = KeychainSecureStore()
    ) throws {
        guard UUID(uuidString: clientID) != nil else { throw ResearchAPIError.invalidArgument }
        self.clientID = clientID
        if let keychain = store as? KeychainSecureStore {
            (credentials, transport) = try ClientRegistry.shared.components(
                apiURL: apiURL,
                policy: policy, store: keychain, namespace: keychain.service)
        } else {
            credentials = try CredentialAuthority(apiURL: apiURL, store: store)
            transport = try EncryptedTransport(apiURL: apiURL, policy: policy, credentials: credentials, store: store)
        }
    }

    public var anonymousAPI: ResearchAPI { ResearchAPI(transport: transport) }

    /// Capture once per account session. An old API cannot silently use a new account.
    public func accountAPI() async throws -> ResearchAPI {
        let snapshot = try await credentials.snapshot()
        guard let principal = snapshot.principalId else { throw TransportError.signedOut }
        return ResearchAPI(
            transport: AccountScopedTransport(transport: transport, snapshot: snapshot), expectedPrincipalId: principal)
    }

    public func signIn(email: String, password: String) async throws -> CredentialSnapshot {
        let snapshot = try await credentials.snapshot()
        let result = try await anonymousAPI.signIn(email: email, password: password, clientId: clientID)
        return try await install(result, expected: snapshot)
    }

    public func signInGuest(id: String, password: String) async throws -> CredentialSnapshot {
        let snapshot = try await credentials.snapshot()
        let result = try await anonymousAPI.signInGuest(id: id, password: password, clientId: clientID)
        return try await install(result, expected: snapshot)
    }

    public func signUp(email: String, password: String, inviteCode: String = "", name: String? = nil) async throws
        -> CredentialSnapshot
    {
        let snapshot = try await credentials.snapshot()
        let result = try await anonymousAPI.signUp(
            email: email, password: password, inviteCode: inviteCode, clientId: clientID, name: name)
        return try await install(result, expected: snapshot)
    }

    /// Use for an owned native Apple or browser OAuth completion. Capture expected before starting sign-in.
    @discardableResult
    public func install(_ login: LoginResponse, expected: CredentialSnapshot) async throws -> CredentialSnapshot {
        let pair = try CredentialPair(access: login.accessToken, refresh: login.refreshToken)
        guard pair.principal == login.id else { throw TransportError.invalidCredentials }
        return try await credentials.install(
            accessToken: login.accessToken, refreshToken: login.refreshToken, expected: expected)
    }

    public func signOut() async throws {
        let snapshot = try await credentials.snapshot()
        let refresh = try await credentials.pair()?.refreshToken
        try await credentials.clear(expected: snapshot)
        try await transport.cancelNativeOAuthHandoff()
        if let refresh { try? await anonymousAPI.signOut(refreshToken: refresh) }
    }

    public func changePassword(currentPassword: String, newPassword: String) async throws {
        let snapshot = try await transport.prepareAuthenticatedSnapshot()
        let api = try await accountAPI()
        let update = try await api.changePassword(currentPassword: currentPassword, newPassword: newPassword)
        if let access = update.accessToken, let refresh = update.refreshToken {
            _ = try await credentials.install(
                accessToken: access, refreshToken: refresh, expected: snapshot, refreshing: true)
        } else {
            try await credentials.assertCurrent(snapshot)
        }
    }

    @discardableResult
    public func refreshCredentials() async throws -> CredentialSnapshot { try await transport.refreshCredentials() }

    public func confirmAccountDeletion(code: String, plaintextSecret: String) async throws {
        let snapshot = try await credentials.snapshot()
        try await accountAPI().confirmAccountDeletion(code: code, plaintextSecret: plaintextSecret)
        try await credentials.clearAccount(expected: snapshot)
        try await transport.cancelNativeOAuthHandoff()
    }
}

/// Retains the original generation even when the same account later signs in.
/// Routine refresh changes revisions but preserves this account lease.
private struct AccountScopedTransport: MapleTransport {
    let transport: EncryptedTransport
    let snapshot: CredentialSnapshot
    func send(_ request: MapleRequest) async throws -> MapleResponse {
        try await transport.credentials.assertCurrent(snapshot, exactRevision: false)
        let response = try await transport.send(request, expectedAccount: snapshot)
        try await transport.credentials.assertCurrent(snapshot, exactRevision: false)
        let reader = AccountScopedBody(body: response.body, credentials: transport.credentials, snapshot: snapshot)
        return MapleResponse(
            status: response.status, headers: response.headers,
            body: AsyncThrowingStream(unfolding: { try await reader.next() }),
            inferenceSendCount: response.inferenceSendCount)
    }
}

private actor AccountScopedBody {
    var iterator: AsyncThrowingStream<Data, Error>.Iterator
    let credentials: CredentialAuthority
    let snapshot: CredentialSnapshot
    var reading = false
    init(body: AsyncThrowingStream<Data, Error>, credentials: CredentialAuthority, snapshot: CredentialSnapshot) {
        iterator = body.makeAsyncIterator(); self.credentials = credentials; self.snapshot = snapshot
    }
    func next() async throws -> Data? {
        guard !reading else { throw TransportError.invalidFrame }
        reading = true
        defer { reading = false }
        try await credentials.assertCurrent(snapshot, exactRevision: false)
        var local = iterator
        let chunk = try await local.next(isolation: self)
        iterator = local
        try await credentials.assertCurrent(snapshot, exactRevision: false)
        return chunk
    }
}
