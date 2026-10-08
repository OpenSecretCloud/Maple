import Foundation
import Testing
@testable import MapleSDK

@Suite struct AccountCeremonyBoundaryTests {
    @Test(arguments: [false, true])
    func staleCeremonyCannotChangeOrDeleteReplacementAccount(samePrincipal: Bool) async throws {
        let client = try MapleClient(
            apiURL: "https://example.invalid", clientID: UUID().uuidString,
            policy: PCRTrustPolicy(environment: .development, remoteHistoryEnabled: false),
            store: TestSecureStore())
        let first = try await client.credentials.install(
            accessToken: fixtureJWT(principal: "a", kind: "access"),
            refreshToken: fixtureJWT(principal: "a", kind: "refresh"), expected: client.credentials.snapshot())
        try await client.credentials.clear(expected: first)
        let replacement = samePrincipal ? "a" : "b"
        let current = try await client.credentials.install(
            accessToken: fixtureJWT(principal: replacement, kind: "access"),
            refreshToken: fixtureJWT(principal: replacement, kind: "refresh"), expected: client.credentials.snapshot())
        // The prior screen resumes after sign-out/re-login. Neither convenience
        // API may adopt the replacement merely because it is current now.
        await #expect(throws: TransportError.authorityChanged) {
            try await client.changePassword(currentPassword: "old", newPassword: "new", expected: first)
        }
        await #expect(throws: TransportError.authorityChanged) {
            try await client.confirmAccountDeletion(code: "CODE1234", plaintextSecret: "secret", expected: first)
        }
        #expect(try await client.credentials.snapshot() == current)
    }

    @Test func anonymousDeletionNeverDispatches() async throws {
        let client = try MapleClient(
            apiURL: "https://example.invalid", clientID: UUID().uuidString,
            policy: PCRTrustPolicy(environment: .development, remoteHistoryEnabled: false),
            store: TestSecureStore())
        let anonymous = try await client.credentials.snapshot()
        await #expect(throws: TransportError.signedOut) {
            try await client.confirmAccountDeletion(code: "CODE1234", plaintextSecret: "secret", expected: anonymous)
        }
        #expect(try await client.credentials.snapshot() == anonymous)
    }
}
