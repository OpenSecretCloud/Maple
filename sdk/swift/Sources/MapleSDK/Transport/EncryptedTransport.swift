import CryptoKit
import Foundation

struct AttestedSession: Sendable, Codable {
    let version: Int
    let apiURL: String
    let policyIdentity: String
    let keys: SessionKeys
    let routingKey: String
    let expiresAt: Date
    var identity: String { keys.id.lowercaseHex }

    func validate(endpoint: APIEndpoint, policy: PCRTrustPolicy, now: Date) throws {
        guard version == 2, apiURL == endpoint.url.absoluteString, policyIdentity == policy.cacheIdentity,
            keys.id.count == 16, keys.request.count == 32, keys.response.count == 32,
            Data(canonicalBase64: routingKey, count: 32) != nil,
            expiresAt.timeIntervalSince1970.isFinite, now < expiresAt
        else { throw TransportError.invalidSession }
    }
}

/// Transport V2 only. Requests and credentials never leave as plaintext logical HTTP.
public actor EncryptedTransport: OAuthSessionTransport {
    public nonisolated let credentials: CredentialAuthority
    private let endpoint: APIEndpoint
    private let policy: PCRTrustPolicy
    private let network: any HTTPNetworking
    private let store: any SecureStore
    private let now: @Sendable () -> Date
    private var session: AttestedSession?
    private var restoreSessionCache = true
    private var establishing: (id: UUID, task: Task<AttestedSession, Error>)?
    private var oauthBindings: [UUID: (AttestedSession, CredentialSnapshot, String)] = [:]
    private var nativeBindings: [UUID: (AttestedSession, CredentialSnapshot, Data, Date)] = [:]
    private var confirmations: [UUID: (CredentialSnapshot, LoginResponse, Date)] = [:]

    public init(
        apiURL: String, policy: PCRTrustPolicy, credentials: CredentialAuthority,
        store: any SecureStore = KeychainSecureStore()
    ) throws {
        try self.init(
            apiURL: apiURL, policy: policy, credentials: credentials,
            store: store, network: URLSessionNetworking(), now: Date.init)
    }

    init(
        apiURL: String, policy: PCRTrustPolicy, credentials: CredentialAuthority,
        store: any SecureStore, network: any HTTPNetworking, now: @escaping @Sendable () -> Date
    ) throws {
        endpoint = try APIEndpoint(apiURL)
        guard credentials.apiOrigin == endpoint.origin else { throw TransportError.invalidEndpoint }
        self.policy = policy; self.credentials = credentials; self.store = store; self.network = network; self.now = now
    }

    private var sessionStorageKey: String {
        "session:" + hashSecret(endpoint.url.absoluteString + "\n" + policy.cacheIdentity)
    }

    /// Persisted keys are isolated by exact endpoint and PCR policy. Corrupt/expired
    /// sessions are discarded; malformed authenticated credentials are never anonymous.
    private func ensureSession() async throws -> AttestedSession {
        if let session, (try? session.validate(endpoint: endpoint, policy: policy, now: now())) != nil {
            return session
        }
        if let pending = establishing {
            let value = try await pending.task.value
            guard establishing?.id == pending.id || session?.identity == value.identity else {
                throw TransportError.expiredSession
            }
            try Task.checkCancellation()
            return value
        }
        let cached = restoreSessionCache ? try? store.read(key: sessionStorageKey) : nil
        restoreSessionCache = false
        if let data = cached {
            if data.count <= 16 * 1024, let restored = try? JSONDecoder().decode(AttestedSession.self, from: data),
                (try? restored.validate(endpoint: endpoint, policy: policy, now: now())) != nil
            {
                session = restored; return restored
            }
            try? store.remove(key: sessionStorageKey)
        }
        let id = UUID()
        let task = Task { [endpoint, policy, network, now] in
            try await withThrowingTaskGroup(of: AttestedSession.self) { group in
                group.addTask {
                    try await Self.establish(endpoint: endpoint, policy: policy, network: network, now: now)
                }
                group.addTask {
                    try await Task.sleep(for: .seconds(30))
                    throw TransportError.establishmentTimeout
                }
                defer { group.cancelAll() }
                return try await group.next()!
            }
        }
        establishing = (id, task)
        do {
            let established = try await task.value
            guard establishing?.id == id else { throw TransportError.expiredSession }
            // Attestation remains authoritative; a cache failure cannot discard
            // a verified session that is usable in this process.
            try? store.write(JSONEncoder().encode(established), key: sessionStorageKey)
            session = established; establishing = nil
            try Task.checkCancellation()
            return established
        } catch {
            if establishing?.id == id { establishing = nil }
            throw error
        }
    }

    private static func establish(
        endpoint: APIEndpoint, policy: PCRTrustPolicy,
        network: any HTTPNetworking, now: @Sendable () -> Date
    ) async throws -> AttestedSession {
        let started = now()
        let challenge = try TransportCrypto.random(count: 32)
        let privateKey = Curve25519.KeyAgreement.PrivateKey()
        let client = privateKey.publicKey.rawRepresentation
        var request = URLRequest(url: endpoint.outerURL("v2/session"))
        request.httpMethod = "POST"; request.timeoutInterval = 30
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue(challenge.base64EncodedString(), forHTTPHeaderField: "x-opensecret-routing-key")
        request.httpBody = try JSONSerialization.data(withJSONObject: [
            "version": 2,
            "challenge": challenge.base64EncodedString(), "client_public_key": client.base64EncodedString(),
        ])
        let connection = try await network.open(request)
        guard connection.status == 200,
            connection.headers["content-type"]?.split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                .lowercased() == "application/json"
        else {
            connection.cancel(); throw TransportError.unauthenticatedResponse
        }
        let body = try await connection.collect(maximum: 64 * 1024)
        let object = try WireProtocol.exactObject(
            body, keys: ["version", "session_id", "attestation_document", "expires_in_seconds"])
        guard WireProtocol.integer(object["version"]) == 2, WireProtocol.integer(object["expires_in_seconds"]) == 3600,
            let id = object["session_id"] as? String, id.utf8.count == 32,
            id.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
            let encoded = object["attestation_document"] as? String
        else { throw TransportError.invalidSession }
        let server: Data
        let userData: Data?
        let nonce: Data?
        if endpoint.isLoopback {
            let document = try AttestationVerifier.parseLoopbackDocument(base64: encoded)
            server = document.publicKey ?? Data(); userData = document.userData; nonce = document.nonce
        } else {
            let document = try AttestationVerifier().verify(base64: encoded, nonce: challenge, deviceTime: now())
            _ = try await policy.requireTrustedPCR0(document.pcrs[0])
            server = document.publicKey ?? Data(); userData = document.userData; nonce = document.nonce
        }
        guard server.count == 32, nonce?.constantTimeEquals(challenge) == true,
            userData?.constantTimeEquals(TransportCrypto.domain("session/v1/client-public-key") + client) == true
        else {
            throw TransportError.authentication
        }
        let shared: SharedSecret
        do {
            shared = try privateKey.sharedSecretFromKeyAgreement(
                with: Curve25519.KeyAgreement.PublicKey(rawRepresentation: server))
        } catch { throw TransportError.authentication }
        let keys = try TransportCrypto.derive(
            shared: shared.withUnsafeBytes { Data($0) }, challenge: challenge, client: client, server: server)
        guard keys.id.lowercaseHex == id else { throw TransportError.invalidSession }
        let result = AttestedSession(
            version: 2, apiURL: endpoint.url.absoluteString, policyIdentity: policy.cacheIdentity,
            keys: keys, routingKey: challenge.base64EncodedString(), expiresAt: started.addingTimeInterval(3570))
        try result.validate(endpoint: endpoint, policy: policy, now: now())
        return result
    }

    public func invalidateSession() {
        establishing?.task.cancel(); establishing = nil; session = nil
        // Never restore a retired session in this process, even if deleting its
        // best-effort durable cache fails.
        restoreSessionCache = false
        oauthBindings.removeAll(); nativeBindings.removeAll()
        try? store.remove(key: sessionStorageKey)
    }

    private func retire(_ old: AttestedSession) {
        if session?.identity == old.identity { invalidateSession() }
    }

    public func send(_ request: MapleRequest) async throws -> MapleResponse {
        try await send(request, expectedAccount: nil)
    }

    func send(_ request: MapleRequest, expectedAccount: CredentialSnapshot?) async throws -> MapleResponse {
        try Task.checkCancellation()
        var snapshot = try await credentials.snapshot()
        let originalAccount = snapshot
        if let expectedAccount, !snapshot.ownsSameAccount(as: expectedAccount) {
            throw TransportError.authorityChanged
        }
        if request.auth == .required {
            guard let pair = try await credentials.pair() else { throw TransportError.signedOut }
            if let expected = request.expectedPrincipalId, pair.principal != expected {
                throw TransportError.authorityChanged
            }
            if pair.accessExpiry <= Int(now().timeIntervalSince1970) + 30 {
                try await refreshCredentials(expected: snapshot)
                snapshot = try await credentials.snapshot()
                guard snapshot.ownsSameAccount(as: originalAccount) else { throw TransportError.authorityChanged }
            }
        }
        if let expected = request.expectedPrincipalId, snapshot.principalId != expected {
            throw TransportError.authorityChanged
        }
        let pair = try await credentials.pair()
        try await credentials.assertCurrent(snapshot)
        let credential: WireCredential? =
            request.auth == .required ? pair.map { WireCredential(kind: .bearer, value: $0.accessToken) } : nil
        let result = try await managed(request, credential: credential, snapshot: snapshot)
        // Authenticated expiry can refresh future requests; never replay this operation.
        if request.auth == .required, result.status == 401,
            result.header("x-opensecret-error-contract") == "1",
            result.header("x-opensecret-error-code") == "access_token_expired"
        {
            var bytes = Data()
            for try await chunk in result.body {
                guard bytes.count + chunk.count <= 64 * 1024 else { throw TransportError.resourceLimit }
                bytes.append(chunk)
            }
            let authenticatedBody = bytes
            try? await refreshCredentials(expected: snapshot)
            return MapleResponse(
                status: result.status, headers: result.headers,
                body: AsyncThrowingStream {
                    $0.yield(authenticatedBody); $0.finish()
                },
                inferenceSendCount: result.inferenceSendCount)
        }
        return result
    }

    func prepareAuthenticatedSnapshot() async throws -> CredentialSnapshot {
        let snapshot = try await credentials.snapshot()
        guard let pair = try await credentials.pair() else { throw TransportError.signedOut }
        if pair.accessExpiry <= Int(now().timeIntervalSince1970) + 30 {
            try await refreshCredentials(expected: snapshot)
        }
        let current = try await credentials.snapshot()
        guard current.ownsSameAccount(as: snapshot) else { throw TransportError.authorityChanged }
        return current
    }

    public func beginOAuth(_ request: MapleRequest) async throws -> (MapleResponse, OAuthSessionBinding) {
        guard request.auth == .none, request.method == .post,
            ["/auth/google", "/auth/github", "/auth/apple"].contains(request.target)
        else { throw TransportError.invalidEnvelope }
        let snapshot = try await credentials.snapshot()
        guard snapshot.principalId == nil else { throw TransportError.authorityChanged }
        let current = try await ensureSession()
        let response = try await dispatch(
            request, credential: nil, cacheRoot: nil, snapshot: snapshot,
            session: current, requestID: TransportCrypto.random(count: 16), inferenceSendCount: 0)
        try await credentials.assertCurrent(snapshot)
        guard session?.identity == current.identity else { throw TransportError.expiredSession }
        let binding = OAuthSessionBinding()
        oauthBindings = oauthBindings.filter { $0.value.0.expiresAt > now() }
        guard oauthBindings.count < 8 else { throw TransportError.resourceLimit }
        oauthBindings[binding.id] = (current, snapshot, request.target + "/callback")
        return (response, binding)
    }

    public func cancelOAuth(binding: OAuthSessionBinding) {
        oauthBindings.removeValue(forKey: binding.id)
    }

    public func completeOAuth(_ request: MapleRequest, binding: OAuthSessionBinding) async throws -> MapleResponse {
        guard let (retained, snapshot, callbackTarget) = oauthBindings.removeValue(forKey: binding.id),
            session?.identity == retained.identity, request.auth == .none, request.method == .post,
            request.target == callbackTarget
        else {
            throw TransportError.invalidSession
        }
        do {
            let response = try await dispatch(
                request, credential: nil, cacheRoot: nil, snapshot: snapshot,
                session: retained, requestID: TransportCrypto.random(count: 16), inferenceSendCount: 0)
            var data = Data()
            for try await chunk in response.body {
                guard data.count + chunk.count <= 64 * 1024 else { throw TransportError.resourceLimit }
                data.append(chunk)
            }
            try await credentials.assertCurrent(snapshot)
            guard session?.identity == retained.identity else { throw TransportError.expiredSession }
            guard (200...299).contains(response.status) else {
                throw TransportError.requestRejected(status: response.status)
            }
            return MapleResponse(
                status: response.status, headers: response.headers,
                body: AsyncThrowingStream {
                    $0.yield(data); $0.finish()
                }, inferenceSendCount: 0)
        } catch { retire(retained); throw error }
    }

    public func prepareNativeOAuthHandoff() async throws -> PreparedNativeOAuthHandoff {
        let snapshot = try await credentials.snapshot()
        guard snapshot.principalId == nil else { throw TransportError.authorityChanged }
        let current = try await ensureSession()
        try await credentials.assertCurrent(snapshot)
        guard session?.identity == current.identity, current.expiresAt > now() else {
            throw TransportError.expiredSession
        }
        nativeBindings = nativeBindings.filter { $0.value.3 > now() }
        guard nativeBindings.count < 8 else { throw TransportError.resourceLimit }
        let id = UUID(), requestID = try TransportCrypto.random(count: 16)
        let deadline = min(current.expiresAt, now().addingTimeInterval(15 * 60))
        nativeBindings[id] = (current, snapshot, requestID, deadline)
        return PreparedNativeOAuthHandoff(
            id: id, sessionID: current.identity, requestID: requestID.lowercaseHex,
            expiresAt: deadline)
    }

    /// Redemption is one-use and pinned to the reserved session/request. The
    /// returned confirmation exposes identity only; credentials stay private until confirmation.
    public func redeemNativeOAuthHandoff(
        _ prepared: PreparedNativeOAuthHandoff,
        grant: NativeOAuthHandoffGrant
    ) async throws -> NativeSignInConfirmation {
        guard let (retained, snapshot, requestID, deadline) = nativeBindings.removeValue(forKey: prepared.id),
            session?.identity == retained.identity, deadline > now()
        else { throw TransportError.invalidSession }
        do {
            let request = MapleRequest(
                method: .post, target: "/auth/native-handoff/redeem",
                headers: ["content-type": "application/json"], body: try APIJSON.encode(["grant": grant.rawValue]),
                auth: .none, allowSessionRecovery: false)
            let response = try await dispatch(
                request, credential: nil, cacheRoot: nil, snapshot: snapshot,
                session: retained, requestID: requestID, inferenceSendCount: 0)
            var data = Data()
            for try await chunk in response.body {
                guard data.count + chunk.count <= 64 * 1024 else { throw TransportError.resourceLimit }
                data.append(chunk)
            }
            guard (200...299).contains(response.status),
                let login = try? APIJSON.decoder().decode(LoginResponse.self, from: data)
            else { throw TransportError.invalidCredentials }
            let pair = try CredentialPair(access: login.accessToken, refresh: login.refreshToken)
            guard pair.principal == login.id, session?.identity == retained.identity else {
                throw TransportError.authorityChanged
            }
            try await credentials.assertCurrent(snapshot)
            retire(retained)
            let id = UUID()
            confirmations.removeAll()
            confirmations[id] = (snapshot, login, deadline)
            return NativeSignInConfirmation(id: id, principalID: login.id, email: login.email)
        } catch { retire(retained); throw error }
    }

    public func confirmNativeOAuthHandoff(_ confirmation: NativeSignInConfirmation) async throws -> CredentialSnapshot {
        guard let (snapshot, login, deadline) = confirmations.removeValue(forKey: confirmation.id),
            deadline > now()
        else { throw TransportError.authorityChanged }
        return try await credentials.install(
            accessToken: login.accessToken, refreshToken: login.refreshToken, expected: snapshot)
    }

    public func cancelNativeOAuthHandoff() {
        confirmations.removeAll()
        invalidateSession()
    }

    private func refreshCredentials(expected: CredentialSnapshot) async throws {
        try await credentials.coalescedRefresh(expected: expected) { [self] in
            try await performRefresh(expected: expected)
        }
    }

    @discardableResult
    public func refreshCredentials() async throws -> CredentialSnapshot {
        let expected = try await credentials.snapshot()
        try await refreshCredentials(expected: expected)
        return try await credentials.snapshot()
    }

    private func performRefresh(expected: CredentialSnapshot) async throws {
        try await credentials.assertCurrent(expected)
        guard let pair = try await credentials.pair() else { throw TransportError.signedOut }
        let request = MapleRequest(method: .post, target: "/refresh", auth: .none)
        let response = try await managed(
            request, credential: WireCredential(kind: .resumption, value: pair.refreshToken), snapshot: expected)
        var bytes = Data()
        for try await chunk in response.body {
            guard bytes.count + chunk.count <= 64 * 1024 else { throw TransportError.resourceLimit }
            bytes.append(chunk)
        }
        guard (200...299).contains(response.status) else {
            if response.status == 401 || response.status == 403 {
                try await credentials.clear(expected: expected)
                throw TransportError.signedOut
            }
            throw TransportError.requestRejected(status: response.status)
        }
        struct Tokens: Decodable { let access_token: String; let refresh_token: String }
        guard let tokens = try? JSONDecoder().decode(Tokens.self, from: bytes) else {
            throw TransportError.invalidCredentials
        }
        try await credentials.install(
            accessToken: tokens.access_token, refreshToken: tokens.refresh_token, expected: expected, refreshing: true)
    }

    private func managed(_ request: MapleRequest, credential: WireCredential?, snapshot: CredentialSnapshot)
        async throws -> MapleResponse
    {
        let inference =
            request.target == "/v1/responses" && request.method == .post
            || request.target.hasPrefix("/v1/audio/") && request.method == .post
        var limit = request.maximumInferenceSends
        let budgetHeaders = request.headers.filter {
            $0.key.caseInsensitiveCompare(openSecretInferenceSendLimitHeader) == .orderedSame
        }
        guard budgetHeaders.count <= 1 else { throw TransportError.invalidEnvelope }
        if let value = budgetHeaders.first?.value {
            guard let headerLimit = Int(value), (1...2).contains(headerLimit), String(headerLimit) == value else {
                throw TransportError.invalidEnvelope
            }
            limit = min(limit, headerLimit)
        }
        guard !inference || (1...2).contains(limit) else { throw TransportError.invalidEnvelope }
        let root = credential == nil ? nil : try await credentials.cacheRoot()
        var current = try await ensureSession()
        for attempt in 1...2 {
            do {
                return try await dispatch(
                    request, credential: credential, cacheRoot: root, snapshot: snapshot,
                    session: current, requestID: TransportCrypto.random(count: 16),
                    inferenceSendCount: inference ? attempt : 0)
            } catch TransportError.untrustedRecoveryHint(let code) {
                retire(current)
                guard attempt == 1, request.allowSessionRecovery,
                    ![
                        "/auth/google/callback", "/auth/github/callback", "/auth/apple/callback",
                        "/auth/native-handoff/redeem",
                    ].contains(request.target)
                else {
                    throw TransportError.untrustedRecoveryHint(code)
                }
                current = try await ensureSession()
                guard !inference || attempt < limit else { throw TransportError.untrustedRecoveryHint(code) }
            }
        }
        throw TransportError.invalidSession
    }

    private func dispatch(
        _ request: MapleRequest, credential: WireCredential?, cacheRoot: Data?,
        snapshot: CredentialSnapshot, session: AttestedSession, requestID: Data,
        inferenceSendCount: Int
    ) async throws -> MapleResponse {
        var headers: [String: String] = [:]
        for (name, value) in request.headers {
            let canonical = name.lowercased()
            if canonical == openSecretInferenceSendLimitHeader { continue }
            guard headers[canonical] == nil else { throw TransportError.invalidEnvelope }
            headers[canonical] = value
        }
        let envelope = try WireProtocol.envelope(
            method: request.method.rawValue, target: request.target,
            headers: headers.map { WireHeader(name: $0.key, value: $0.value) }, body: request.body,
            credential: credential, cacheRoot: cacheRoot)
        let ciphertext = try TransportCrypto.sealRequest(envelope, keys: session.keys, request: requestID)
        try await credentials.assertCurrent(snapshot)
        try Task.checkCancellation()
        guard now() < session.expiresAt else { throw TransportError.expiredSession }
        var outer = URLRequest(url: endpoint.outerURL("v2/request"))
        outer.httpMethod = "POST"; outer.httpBody = ciphertext
        outer.setValue("application/octet-stream", forHTTPHeaderField: "content-type")
        outer.setValue(session.routingKey, forHTTPHeaderField: "x-opensecret-routing-key")
        outer.setValue(session.identity, forHTTPHeaderField: "x-session-id")
        let response = try await network.open(outer)
        guard response.status == 200, response.headers["content-type"] == "application/octet-stream" else {
            response.cancel()
            if response.status == 400, response.headers["x-opensecret-error-contract"] == "1",
                let code = response.headers["x-opensecret-error-code"],
                ["session_not_found", "request_decryption_failed"].contains(code)
            {
                throw TransportError.untrustedRecoveryHint(code)
            }
            throw TransportError.unauthenticatedResponse
        }
        let reader = ResponseRecordReader(connection: response, keys: session.keys, requestID: requestID)
        do {
            try await credentials.assertCurrent(snapshot, exactRevision: false)
            guard case let .start(status, fields) = try await reader.next() else { throw TransportError.invalidFrame }
            var logicalHeaders: [String: String] = [:]
            for field in fields {
                if let prior = logicalHeaders[field.name] {
                    logicalHeaders[field.name] = prior + ", " + field.value
                } else {
                    logicalHeaders[field.name] = field.value
                }
            }
            let body = AsyncThrowingStream<Data, Error>(unfolding: { [credentials] in
                do {
                    try Task.checkCancellation()
                    try await credentials.assertCurrent(snapshot, exactRevision: false)
                    let record = try await reader.next()
                    try await credentials.assertCurrent(snapshot, exactRevision: false)
                    switch record {
                    case let .chunk(bytes): return bytes
                    case .end: return nil
                    case let .error(code): throw TransportError.remote(code)
                    case .start: throw TransportError.invalidFrame
                    }
                } catch { reader.cancel(); throw error }
            })
            return MapleResponse(
                status: status, headers: logicalHeaders, body: body, inferenceSendCount: inferenceSendCount)
        } catch { reader.cancel(); throw error }
    }
}
