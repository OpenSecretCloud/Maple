import Foundation
import Testing
@testable import MapleAppCore

struct IncomingAccountLinkTests {
    private func configuration() throws -> AppConfiguration {
        try AppConfiguration(
            profile: .local, apiURL: "http://127.0.0.1:10001", billingURL: "http://127.0.0.1:10002",
            flagsURL: "http://127.0.0.1:10003", authURL: "http://127.0.0.1:10004",
            clientID: "00000000-0000-4000-8000-000000000001", callbackScheme: "cloud.opensecret.maple.native.dev")
    }

    @Test func exactVerificationAndResetRoutesCarryNoExternalSecret() throws {
        let configuration = try configuration()
        let code = "00000000-0000-4000-8000-000000000002"
        #expect(
            IncomingAccountLink(
                url: URL(string: "http://127.0.0.1:10004/verify/\(code)")!, configuration: configuration)
                == .verify(code: code))
        #expect(
            IncomingAccountLink(
                url: URL(string: "http://127.0.0.1:10004/password-reset/confirm?email=synthetic&secret=untrusted")!,
                configuration: configuration) == .passwordReset(code: nil))
        #expect(
            IncomingAccountLink(
                url: URL(string: "http://127.0.0.1:10004/password-reset?code=ABCD1234")!, configuration: configuration)
                == .passwordReset(code: "ABCD1234"))
    }

    @Test(arguments: [
        "http://127.0.0.1:10005/password-reset", "https://127.0.0.1:10004/password-reset",
        "http://user@127.0.0.1:10004/password-reset", "http://127.0.0.1:10004/password-reset#x",
        "http://127.0.0.1:10004/password-reset?code=ABCD1234&code=ABCD1234",
        "http://127.0.0.1:10004/password-reset?next=/payment", "http://127.0.0.1:10004/password-reset?code=abcd1234",
        "http://127.0.0.1:10004/payment", "http://127.0.0.1:10004/verify/a/b",
        "cloud.opensecret.maple.native.dev://password-reset", "http://127.0.0.1:10004/verify/%2F",
    ]) func rejectsAmbiguousForeignAndDeferredRoutes(_ value: String) throws {
        #expect(IncomingAccountLink(url: URL(string: value)!, configuration: try configuration()) == nil)
    }
}
