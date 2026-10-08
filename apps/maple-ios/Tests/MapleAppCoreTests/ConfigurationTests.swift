import Foundation
import Testing
@testable import MapleAppCore

@Suite struct ConfigurationTests {
    private func configuration(_ origin: String, profile: AppConfiguration.Profile = .local) throws -> AppConfiguration
    {
        try AppConfiguration(
            profile: profile, apiURL: origin, billingURL: origin, flagsURL: origin,
            authURL: origin, clientID: "00000000-0000-0000-0000-000000000001",
            callbackScheme: "cloud.opensecret.maple.native.dev")
    }

    @Test(arguments: ["http://127.0.0.1:3000/", "http://localhost:3000", "http://[::1]:3000"])
    func localOriginsAreExplicitLoopback(_ origin: String) throws {
        let value = try configuration(origin)
        #expect(value.apiURL.path.isEmpty)
        #expect(value.profile == .local)
    }

    @Test(arguments: [
        "http://127.0.0.1", "http://0.0.0.0:3000", "http://127.0.0.1.evil.test:3000",
        "https://127.0.0.1:3000", "http://user:password@127.0.0.1:3000",
        "http://127.0.0.1:3000/path", "http://127.0.0.1:3000?key=value",
        "http://127.0.0.1:3000#fragment", "http://127.0.0.1:0", "http://127.0.0.1:65536",
        " http://127.0.0.1:3000", "http://%31%32%37.0.0.1:3000",
    ])
    func rejectAmbiguousOrRemoteLocalOrigins(_ origin: String) {
        #expect(throws: ConfigurationError.invalidOrigin) { try configuration(origin) }
    }

    @Test func hostedProfilesCannotUseCleartext() throws {
        #expect(throws: ConfigurationError.invalidOrigin) {
            try configuration("http://127.0.0.1:3000", profile: .production)
        }
        #expect(try configuration("https://api.example.test", profile: .development).apiURL.scheme == "https")
    }

    @Test func sameAccountReloginCannotReuseAppLease() {
        let first = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let second = AccountScope(apiOrigin: first.apiOrigin, principalID: first.principalID)
        #expect(first != second)
        #expect(first.generation != second.generation)
    }
}
