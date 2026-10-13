import Foundation
import MapleSDK

public struct SDKSessionService: SessionServing {
    public let client: MapleClient
    public init(client: MapleClient) { self.client = client }
    public func authorityChanges() async -> AsyncStream<CredentialAuthorityChange>? {
        await client.credentials.changes()
    }
    public func authorityState() async -> CredentialAuthorityChange? { await client.credentials.currentState() }
    public func completePendingCleanup() async throws { try await client.credentials.completePendingCleanup() }
    public func identity() async throws -> SessionIdentity? {
        let snapshot = try await client.credentials.snapshot()
        guard let principal = snapshot.principalId else { return nil }
        return SessionIdentity(origin: snapshot.apiOrigin, principal: principal, generation: snapshot.generation)
    }
    public func user() async throws -> MapleUser { try await client.accountAPI().getUser() }
    public func signIn(email: String, password: String) async throws {
        _ = try await client.signIn(email: email, password: password)
    }
    public func signInGuest(id: String, password: String) async throws {
        _ = try await client.signInGuest(id: id, password: password)
    }
    public func signUp(email: String, password: String, name: String?) async throws {
        _ = try await client.signUp(email: email, password: password, inviteCode: "", name: name)
    }
    public func signOut() async throws { try await client.signOut() }
    public func signOut(ifOwnedBy identity: SessionIdentity?) async throws {
        guard let identity else { return }
        let current = try await client.credentials.snapshot()
        guard current.apiOrigin == identity.origin, current.principalId == identity.principal,
            current.generation == identity.generation
        else { return }
        do { try await client.signOut(expected: current) } catch TransportError.authorityChanged
        { /* The replacement owns its credentials. */  }
    }
}
