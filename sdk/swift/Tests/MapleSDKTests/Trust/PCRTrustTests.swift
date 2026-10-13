import Foundation
import Testing
@testable import MapleSDK

struct PCRTrustTests {
    @Test func embeddedRootsMatchCurrentAndRollbackApprovals() {
        #expect(
            PCRTrustPolicy.productionRoots == [
                "21d27bd19cdd02632485a6e8874753e42cf7f098f1335bb62a9c23e7f64da9100c02dba582da17ac68052fa6be16b7a9",
                "643b3c293a4eac11ec14ab405d2405e46cb714263a12da6c62f485a0fa395cb3c3d5ef07b18d39a1f9715f25e7943b66",
            ])
        #expect(
            PCRTrustPolicy.developmentRoots == [
                "a95956c3fb9457e509be6d614ffefcddf98fe594f77bb6a56891fc1f5bdec41da2add13db22e0908363c8d0929c04def",
                "e573a3ddc8a27f387e24cfb02a8fa594fe782674de6ee1fa9385126b9f8ec38f4a236e91c371bdd2b9444623c1c1cc34",
            ])
    }

    static let signedPCR0 =
        "3637534c33a8bafc5034d5763e441a481f161bbbe888e375ce14b016c7497dc4e550afe866bd8e65969b409d54766481"
    static let signature =
        "GZTXC0Xt0+yAaAatmMUd37pUJpF0nRAOj3Df9qxDOvDvRkiTF8UbGlzlL4kIOi/nd7dXAaEqYnY7OlpyngHBED2CSTpRRwV0xGo109epfqUKWWudrFaXpMsJ+GRKJLFO"

    static func bytes(_ hex: String) -> Data {
        let chars = Array(hex)
        return Data(stride(from: 0, to: chars.count, by: 2).map { UInt8(String(chars[$0...($0 + 1)]), radix: 16)! })
    }

    static func history(signature: String = signature, hash: String = signedPCR0) throws -> Data {
        try JSONSerialization.data(withJSONObject: [
            [
                "PCR0": hash, "PCR1": String(repeating: "a", count: 96),
                "PCR2": String(repeating: "b", count: 96), "timestamp": 1_743_710_235, "signature": signature,
                "futureMetadata": ["ignored": true],
            ]
        ])
    }

    @Test func environmentsNeverFallBackToEachOther() async throws {
        let production = try PCRTrustPolicy(remoteHistoryEnabled: false)
        let development = try PCRTrustPolicy(environment: .development, remoteHistoryEnabled: false)
        for (policy, roots, wrong) in [
            (production, PCRTrustPolicy.productionRoots, PCRTrustPolicy.developmentRoots),
            (development, PCRTrustPolicy.developmentRoots, PCRTrustPolicy.productionRoots),
        ] {
            for root in roots { #expect(try await policy.requireTrustedPCR0(Self.bytes(root)) == root) }
            for root in wrong {
                await #expect(throws: TrustError.untrustedPCR0) {
                    try await policy.requireTrustedPCR0(Self.bytes(root))
                }
            }
        }
    }

    @Test func signedHistoryAuthenticatesOnlyMatchingPCR0() throws {
        #expect(try SignedPCRHistory.verify(hash: Self.signedPCR0, data: Self.history()))
        #expect(try !SignedPCRHistory.verify(hash: String(repeating: "1", count: 96), data: Self.history()))
        #expect(
            try !SignedPCRHistory.verify(
                hash: Self.signedPCR0,
                data: Self.history(signature: Data(repeating: 0, count: 96).base64EncodedString())))
        #expect(
            try !SignedPCRHistory.verify(
                hash: String(repeating: "1", count: 96), data: Self.history(hash: String(repeating: "1", count: 96))))
    }

    @Test func remoteHistoryErrorsAndUnknownMeasurementsFailClosed() async throws {
        let policy = try PCRTrustPolicy(environment: .development)
        #expect(policy.remoteHistoryURL?.lastPathComponent == "pcrDevHistory.json")
        let accepted = try await policy.requireTrustedPCR0(Self.bytes(Self.signedPCR0)) { url in
            #expect(url.lastPathComponent == "pcrDevHistory.json")
            return try Self.history()
        }
        #expect(accepted == Self.signedPCR0)
        await #expect(throws: TrustError.untrustedPCR0) {
            try await policy.requireTrustedPCR0(Self.bytes(Self.signedPCR0)) { _ in throw TrustError.historyUnavailable
            }
        }
        await #expect(throws: TrustError.untrustedPCR0) {
            try await policy.requireTrustedPCR0(Self.bytes(Self.signedPCR0)) { _ in Data("[]".utf8) }
        }
        await #expect(throws: CancellationError.self) {
            try await policy.requireTrustedPCR0(Self.bytes(Self.signedPCR0)) { _ in throw CancellationError() }
        }
    }

    @Test func malformedPCR0CannotReachHistory() async throws {
        let policy = try PCRTrustPolicy()
        for invalid in [
            nil, Data(repeating: 1, count: 47), Data(repeating: 1, count: 49), Data(repeating: 0, count: 48),
        ] {
            await #expect(throws: TrustError.invalidPCR0) {
                try await policy.requireTrustedPCR0(invalid) { _ in
                    Issue.record("Invalid PCR0 reached network"); return Data()
                }
            }
        }
        let local = PCRTrustPolicy.productionRoots[0]
        _ = try await policy.requireTrustedPCR0(Self.bytes(local)) { _ in
            Issue.record("Local root reached network"); return Data()
        }
    }

    @Test func historySchemaAndSizeAreBounded() throws {
        for invalid in [
            Data(), Data("[]".utf8), Data("{}".utf8), Data(repeating: 32, count: 1_048_577),
            try Self.history(signature: "bad"), try Self.history(hash: "x"),
            try Self.history(hash: Self.signedPCR0.uppercased()),
        ] {
            #expect(throws: TrustError.historyUnavailable) {
                try SignedPCRHistory.verify(hash: Self.signedPCR0, data: invalid)
            }
        }
        let entry = try #require((JSONSerialization.jsonObject(with: Self.history()) as? [[String: Any]])?.first)
        let tooMany = try JSONSerialization.data(withJSONObject: Array(repeating: entry, count: 2_049))
        #expect(throws: TrustError.historyUnavailable) {
            try SignedPCRHistory.verify(hash: Self.signedPCR0, data: tooMany)
        }
    }

    @Test func configurationAndCacheBindingAreImmutableAndComplete() throws {
        let normalized = try PCRTrustPolicy(additionalPCR0s: [
            "  " + Self.signedPCR0.uppercased() + " ", Self.signedPCR0,
        ])
        #expect(normalized.cacheIdentity == (try PCRTrustPolicy(additionalPCR0s: [Self.signedPCR0])).cacheIdentity)
        #expect(normalized.cacheIdentity != (try PCRTrustPolicy()).cacheIdentity)
        #expect(
            normalized.cacheIdentity
                != (try PCRTrustPolicy(environment: .development, additionalPCR0s: [Self.signedPCR0])).cacheIdentity)
        #expect(
            normalized.cacheIdentity
                != (try PCRTrustPolicy(additionalPCR0s: [Self.signedPCR0], remoteHistoryEnabled: false)).cacheIdentity)
        for value in ["", "hello", String(repeating: "0", count: 96)] {
            #expect(throws: TrustError.invalidPolicy) { try PCRTrustPolicy(additionalPCR0s: [value]) }
        }
        for value in [
            "http://example.com/history", "https://user:password@example.com/history",
            "https://example.com/history#fragment",
            "http://localhost.evil.test/history", "http://127.0.0.1.evil.test/history", "file:///tmp/history",
        ] {
            #expect(throws: TrustError.invalidPolicy) { try PCRTrustPolicy(remoteHistoryURL: URL(string: value)!) }
        }
        for value in [
            "https://example.com/history", "http://127.0.0.1/history", "http://localhost/history",
            "http://[::1]/history",
        ] {
            _ = try PCRTrustPolicy(remoteHistoryURL: URL(string: value)!)
        }
    }
}
