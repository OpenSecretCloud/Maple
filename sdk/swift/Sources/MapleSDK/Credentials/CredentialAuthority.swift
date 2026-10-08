import Foundation
import Security

/// Storage is synchronous so a durable commit and publication cannot interleave.
public protocol SecureStore: Sendable {
    func read(key: String) throws -> Data?
    func write(_ data: Data, key: String) throws
    func remove(key: String) throws
}

/// Device-local, non-synchronizing Keychain storage. Never writes tokens to preferences.
public struct KeychainSecureStore: SecureStore {
    let service: String
    public init(service: String = "ai.maple.sdk.transport-v2") { self.service = service }
    private func query(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
            kSecAttrAccount as String: key, kSecAttrSynchronizable as String: false,
        ]
    }
    public func read(key: String) throws -> Data? {
        var query = query(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else { throw TransportError.persistence }
        return data
    }
    public func write(_ data: Data, key: String) throws {
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let status = SecItemUpdate(query(key) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            let added = SecItemAdd(query(key).merging(attributes) { _, new in new } as CFDictionary, nil)
            guard added == errSecSuccess else { throw TransportError.persistence }
        } else if status != errSecSuccess {
            throw TransportError.persistence
        }
    }
    public func remove(key: String) throws {
        let status = SecItemDelete(query(key) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw TransportError.persistence }
    }
}

public struct CredentialSnapshot: Sendable, Equatable {
    public let apiOrigin: String
    public let principalId: String?
    public let revision: UInt64
    /// Changes on authentication replacement and sign-out, including same-account re-login.
    public let generation: UUID

    func ownsSameAccount(as other: Self) -> Bool {
        apiOrigin == other.apiOrigin && principalId == other.principalId && generation == other.generation
    }
}

/// Token-free authority state. Revisions from routine refresh are deliberately
/// absent: only authentication replacement, revocation and cleanup alter this state.
public struct CredentialAuthorityChange: Sendable, Equatable {
    public let apiOrigin: String
    public let generation: UUID
    public let requiresCleanup: Bool

    public init(apiOrigin: String, generation: UUID, requiresCleanup: Bool) {
        self.apiOrigin = apiOrigin; self.generation = generation; self.requiresCleanup = requiresCleanup
    }
}

struct CredentialPair: Sendable, Codable {
    let accessToken: String
    let refreshToken: String
    let principal: String
    let accessExpiry: Int
    let refreshExpiry: Int

    init(access: String, refresh: String) throws {
        let accessHints = try Self.hints(access, audience: "access")
        let refreshHints = try Self.hints(refresh, audience: "refresh")
        guard accessHints.0 == refreshHints.0 else { throw TransportError.invalidCredentials }
        accessToken = access; refreshToken = refresh
        principal = accessHints.0; accessExpiry = accessHints.1; refreshExpiry = refreshHints.1
    }

    /// JWT claims are unverified hints; the server verifies signature and authorization.
    static func hints(_ token: String, audience: String) throws -> (String, Int) {
        guard token.utf8.count <= 16 * 1024 else { throw TransportError.invalidCredentials }
        let segments = token.split(separator: ".", omittingEmptySubsequences: false)
        guard segments.count == 3 else { throw TransportError.invalidCredentials }
        var payload = Data()
        for (index, segment) in segments.enumerated() {
            guard !segment.isEmpty,
                segment.utf8.allSatisfy({
                    (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 45 || $0 == 95
                }), segment.count % 4 != 1
            else { throw TransportError.invalidCredentials }
            let base = segment.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
            guard let bytes = Data(base64Encoded: base + String(repeating: "=", count: (4 - base.count % 4) % 4)),
                bytes.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(
                    of: "/", with: "_"
                ).replacingOccurrences(of: "=", with: "") == segment
            else {
                throw TransportError.invalidCredentials
            }
            if index == 1 { payload = bytes }
        }
        guard String(data: payload, encoding: .utf8) != nil,
            let claims = try? JSONSerialization.jsonObject(with: payload) as? [String: Any],
            claims["aud"] as? String == "urn:opensecret:internal:transport-v2:user:\(audience)-token",
            let principal = claims["sub"] as? String, !principal.isEmpty, principal.utf16.count <= 256,
            let expiry = WireProtocol.integer(claims["exp"]), expiry > 0,
            WireProtocol.integer(claims["tf"]) == 2
        else { throw TransportError.invalidCredentials }
        return (principal, expiry)
    }
}

/// Share one authority per API origin across clients in a process.
/// Revision checks defend async credential installation; generations fence account-owned streams.
public actor CredentialAuthority {
    public nonisolated let apiOrigin: String
    private let store: any SecureStore
    private let storageKey: String
    private var state: State
    private var cleanupFailed = false
    private var observers: [UUID: AsyncStream<CredentialAuthorityChange>.Continuation] = [:]
    private var refreshing: (snapshot: CredentialSnapshot, task: Task<Void, Error>)?

    private struct State: Codable {
        var version = 1
        var origin: String
        var revision: UInt64 = 0
        var generation = UUID()
        var access: String?
        var refresh: String?
        var cacheRoot: Data?
    }

    public init(apiURL: String, store: any SecureStore = KeychainSecureStore()) throws {
        apiOrigin = try APIEndpoint(apiURL).origin
        self.store = store
        storageKey = "authority:" + apiOrigin
        if let data = try store.read(key: storageKey) {
            guard data.count <= 96 * 1024, let saved = try? JSONDecoder().decode(State.self, from: data),
                saved.version == 1, saved.origin == apiOrigin,
                saved.cacheRoot == nil || saved.cacheRoot?.count == 32,
                (saved.access == nil) == (saved.refresh == nil)
            else { throw TransportError.invalidCredentials }
            if let access = saved.access, let refresh = saved.refresh {
                _ = try CredentialPair(access: access, refresh: refresh)
            }
            state = saved
            // A new process never adopts another process's in-flight ownership leases.
            state.generation = UUID()
        } else {
            state = State(origin: apiOrigin)
        }
    }

    /// Registration and the initial value share this actor turn, so a subscriber
    /// cannot miss a replacement between reading the current state and observing.
    /// Slow consumers receive only the newest state and must recheck currentState().
    public func changes() -> AsyncStream<CredentialAuthorityChange> {
        let id = UUID()
        let pair = AsyncStream<CredentialAuthorityChange>.makeStream(bufferingPolicy: .bufferingNewest(1))
        pair.continuation.onTermination = { [weak self] _ in
            Task { await self?.removeObserver(id) }
        }
        observers[id] = pair.continuation
        pair.continuation.yield(currentState())
        return pair.stream
    }

    public func currentState() -> CredentialAuthorityChange {
        CredentialAuthorityChange(apiOrigin: apiOrigin, generation: state.generation, requiresCleanup: cleanupFailed)
    }

    private func removeObserver(_ id: UUID) { observers.removeValue(forKey: id) }
    var observerCount: Int { observers.count }
    private func publish() {
        let value = currentState()
        for observer in observers.values { observer.yield(value) }
    }

    /// Retries only an already-revoked authority's failed durable cleanup. It
    /// cannot clear credentials installed by another caller while this awaited.
    public func completePendingCleanup() throws {
        guard cleanupFailed else { return }
        try commit(state)
        cleanupFailed = false
        publish()
    }

    public func snapshot() throws -> CredentialSnapshot {
        guard !cleanupFailed else { throw TransportError.persistence }
        return CredentialSnapshot(
            apiOrigin: apiOrigin, principalId: try pair()?.principal,
            revision: state.revision, generation: state.generation)
    }
    func pair() throws -> CredentialPair? {
        guard !cleanupFailed else { throw TransportError.persistence }
        guard let access = state.access, let refresh = state.refresh else { return nil }
        return try CredentialPair(access: access, refresh: refresh)
    }
    public func authenticatedUserId() throws -> String? { try pair()?.principal }
    func assertCurrent(_ expected: CredentialSnapshot, exactRevision: Bool = true) throws {
        let current = try snapshot()
        guard current.apiOrigin == expected.apiOrigin, current.principalId == expected.principalId,
            current.generation == expected.generation,
            !exactRevision || current.revision == expected.revision
        else { throw TransportError.authorityChanged }
    }
    public func isCurrent(_ expected: CredentialSnapshot) -> Bool { (try? snapshot()) == expected }

    private func commit(_ updated: State) throws {
        let data = try JSONEncoder().encode(updated)
        guard data.count <= 96 * 1024 else { throw TransportError.resourceLimit }
        try store.write(data, key: storageKey)
        state = updated
    }
    private func nextState() throws -> State {
        guard state.revision < UInt64.max else { throw TransportError.invalidCredentials }
        var updated = state; updated.revision += 1
        return updated
    }

    @discardableResult
    public func install(accessToken: String, refreshToken: String, expected: CredentialSnapshot) throws
        -> CredentialSnapshot
    {
        try install(accessToken: accessToken, refreshToken: refreshToken, expected: expected, refreshing: false)
    }
    @discardableResult
    func install(accessToken: String, refreshToken: String, expected: CredentialSnapshot, refreshing: Bool) throws
        -> CredentialSnapshot
    {
        try Task.checkCancellation()
        try assertCurrent(expected)
        let credentials = try CredentialPair(access: accessToken, refresh: refreshToken)
        if refreshing && credentials.principal != expected.principalId { throw TransportError.invalidCredentials }
        var updated = try nextState()
        updated.access = accessToken; updated.refresh = refreshToken
        if !refreshing { updated.generation = UUID() }
        try commit(updated)
        if !refreshing { publish() }
        return try snapshot()
    }

    /// Invalidate live authority before cleanup; a Keychain failure blocks further use until cleanup succeeds.
    public func clear(expected: CredentialSnapshot? = nil) throws {
        if let expected { try assertCurrent(expected) }
        var updated = try nextState()
        updated.access = nil; updated.refresh = nil; updated.generation = UUID()
        state = updated
        defer { publish() }
        do { try commit(updated); cleanupFailed = false } catch {
            cleanupFailed = true; throw TransportError.persistence
        }
    }

    /// Capture the refresh token and revoke the exact owning generation in one
    /// actor turn. A routine refresh cannot interleave between capture and clear.
    func revoke(expected: CredentialSnapshot?) throws -> String? {
        try completePendingCleanup()
        if let expected { try assertCurrent(expected, exactRevision: false) }
        let refresh = try pair()?.refreshToken
        try clear()
        return refresh
    }

    /// An account deletion invalidates the account lease even if a routine
    /// refresh completed while the server was processing the deletion.
    func clearAccount(expected: CredentialSnapshot) throws {
        try assertCurrent(expected, exactRevision: false)
        try clear()
    }

    /// All transports for this authority coalesce token rotation, including
    /// clients that select a different endpoint path or attestation policy.
    func coalescedRefresh(
        expected: CredentialSnapshot,
        operation: @escaping @Sendable () async throws -> Void
    ) async throws {
        try assertCurrent(expected, exactRevision: false)
        if state.revision != expected.revision { return }
        if let pending = refreshing, pending.snapshot == expected {
            try await pending.task.value
            return
        }
        let task = Task { try await operation() }
        refreshing = (expected, task)
        defer { if refreshing?.snapshot == expected { refreshing = nil } }
        try await task.value
    }

    func cacheRoot() throws -> Data {
        guard !cleanupFailed else { throw TransportError.persistence }
        if let root = state.cacheRoot { return root }
        var updated = state
        let root = try TransportCrypto.random(count: 32)
        updated.cacheRoot = root
        try commit(updated)
        return root
    }
}
