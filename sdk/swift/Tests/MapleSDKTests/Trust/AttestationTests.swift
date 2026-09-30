import CryptoKit
import Foundation
import Testing
@testable import MapleSDK

struct AttestationTests {
    static let signedAt = Date(timeIntervalSince1970: 1_730_141_656.206)
    static let notBefore = Date(timeIntervalSince1970: 1_730_141_295)
    static let notAfter = notBefore.addingTimeInterval(3 * 3_600 + 3)
    static let nonce = Data("cc6b95ef-a0d7-477d-90f2-36cc088d2449".utf8)

    static func fixture() throws -> String {
        let url = try #require(
            Bundle.module.url(forResource: "nitro_attestation_document_2024-10-28", withExtension: "b64"))
        return try String(contentsOf: url, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func verify(at time: Date = signedAt) throws -> AttestationDocument {
        try AttestationVerifier().verify(base64: Self.fixture(), nonce: Self.nonce, deviceTime: time)
    }

    @Test func realNitroDocumentAndPinnedRootVerify() throws {
        let document = try verify()
        #expect(document.moduleID == "i-06c79bf817127030a-enc0192d3d4945e0432")
        #expect(document.timestamp == 1_730_141_656_206)
        #expect(document.digest == "SHA384")
        #expect(document.nonce == Self.nonce)
        let validity = try CertificateValidity(der: document.certificate)
        #expect(abs(validity.notBefore.timeIntervalSince(Self.notBefore)) < 0.001)
        #expect(abs(validity.notAfter.timeIntervalSince(Self.notAfter)) < 0.001)
        let fingerprint = SHA256.hash(data: document.caBundle[0]).map { String(format: "%02x", $0) }.joined()
        #expect(fingerprint == "641a0321a3e244efe456463195d606317ed7cdcc3c1756e09893f3c68f79bb5b")
    }

    @Test func notBeforeLeewayIsFiveMinutesOnly() throws {
        for seconds in [1.0, 60, 300] { _ = try verify(at: Self.notBefore.addingTimeInterval(-seconds)) }
        do {
            _ = try verify(at: Self.notBefore.addingTimeInterval(-301))
            Issue.record("Accepted certificate before clock leeway")
        } catch let error as AttestationClockSkewError {
            #expect(error.certificateIndex == 0)
            #expect(error.deviceClockLikelyWrong)
            #expect(error.errorDescription?.contains("about 11 minutes behind the secure enclave") == true)
            #expect(error.errorDescription?.contains("looks not yet valid") == true)
            #expect(error.errorDescription?.contains("date, time and time zone settings") == true)
        }
    }

    @Test func notAfterIsStrictAndClockErrorsMatchReference() throws {
        _ = try verify(at: Self.notAfter)
        for days in [-3.0, 3] {
            do {
                _ = try verify(at: Self.signedAt.addingTimeInterval(days * 86_400))
                Issue.record("Accepted expired or future certificate")
            } catch let error as AttestationClockSkewError {
                #expect(
                    error.errorDescription?.contains(
                        "about 3 days \(days > 0 ? "ahead of" : "behind") the secure enclave") == true)
                #expect(error.enclaveTime == Self.signedAt)
            }
        }
        #expect(throws: AttestationClockSkewError.self) { try verify(at: Self.notAfter.addingTimeInterval(0.001)) }
        #expect(throws: AttestationClockSkewError.self) { try verify(at: Date()) }
    }

    @Test func nonceIsExactAndCannotBeOmitted() throws {
        for nonce in [Data(), Data(repeating: 0, count: Self.nonce.count), Self.nonce + Data([0])] {
            #expect(throws: TrustError.nonceMismatch) {
                try AttestationVerifier().verify(base64: Self.fixture(), nonce: nonce, deviceTime: Self.signedAt)
            }
        }
    }

    private func mutate(_ field: String, value: TrustCBOR) throws -> String {
        let encoded = try #require(Data(base64Encoded: Self.fixture()))
        var envelope = try #require(TrustCBOR.decode(encoded).items)
        let payload = try #require(envelope[2].data)
        var fields = try #require(TrustCBOR.decode(payload).pairs)
        fields.removeAll { $0.key == .text(field) }
        fields.append(.init(.text(field), value))
        envelope[2] = .bytes(TrustCBOR.map(fields).encode())
        return TrustCBOR.array(envelope).encode().base64EncodedString()
    }

    @Test func tamperedTimestampFailsSignatureBeforeClockDiagnosis() throws {
        let forged = try mutate("timestamp", value: .unsigned(1_800_000_000_000))
        #expect(throws: TrustError.documentSignature) {
            try AttestationVerifier().verify(base64: forged, nonce: Self.nonce, deviceTime: Self.signedAt)
        }
    }

    @Test func wrongRootAndTamperedCertificateFailClosed() throws {
        let document = try verify()
        var root = document.caBundle[0]
        root[root.count - 1] ^= 1
        let forged = try mutate(
            "cabundle", value: .array([.bytes(root)] + document.caBundle.dropFirst().map { .bytes($0) }))
        #expect(throws: TrustError.certificateChain) {
            try AttestationVerifier().verify(base64: forged, nonce: Self.nonce, deviceTime: Self.signedAt)
        }
        let truncated = try mutate("certificate", value: .bytes(document.certificate.dropLast()))
        #expect(throws: (any Error).self) {
            try AttestationVerifier().verify(base64: truncated, nonce: Self.nonce, deviceTime: Self.signedAt)
        }
    }

    @Test func coseRejectsAlgorithmSubstitutionMalformedAndTrailingData() throws {
        let encoded = try #require(Data(base64Encoded: Self.fixture()))
        var envelope = try #require(TrustCBOR.decode(encoded).items)
        envelope[0] = .bytes(TrustCBOR.map([.init(.unsigned(1), .negative(-7))]).encode())
        #expect(throws: TrustError.unsupportedAlgorithm) {
            try COSEDocument(encoded: TrustCBOR.array(envelope).encode())
        }
        #expect(throws: TrustError.malformedDocument) { try COSEDocument(encoded: encoded + Data([0])) }
        #expect(throws: TrustError.malformedDocument) { try COSEDocument(encoded: Data(repeating: 0, count: 65_537)) }
        #expect(throws: TrustError.malformedDocument) { try COSEDocument(encoded: .init([0x9f, 0xff])) }
        #expect(throws: TrustError.malformedDocument) {
            try COSEDocument(encoded: TrustCBOR.tag(17, .array([])).encode())
        }
        _ = try COSEDocument(encoded: TrustCBOR.tag(18, TrustCBOR.decode(encoded)).encode())
    }

    @Test func cborRejectsDuplicateKeysEvenWithDifferentIntegerEncoding() throws {
        #expect(throws: TrustError.malformedDocument) { try TrustCBOR.decode(Data([0xa2, 1, 2, 0x18, 1, 3])) }
        #expect(throws: TrustError.malformedDocument) { try TrustCBOR.decode(Data([0x61, 0xff])) }
        #expect(throws: TrustError.malformedDocument) { try TrustCBOR.decode(Data([0x58, 0xff])) }
        #expect(throws: TrustError.malformedDocument) {
            try TrustCBOR.decode(Data(repeating: 0x81, count: 18) + Data([0]))
        }
    }

    @Test func currentNitroIndefiniteContainersStayBoundedAndRejectDuplicateKeys() throws {
        #expect(try TrustCBOR.decode(Data([0xbf, 1, 2, 0xff])) == .map([.init(.unsigned(1), .unsigned(2))]))
        #expect(try TrustCBOR.decode(Data([0x9f, 1, 2, 0xff])) == .array([.unsigned(1), .unsigned(2)]))
        for invalid in [
            Data([0xbf, 1, 0xff]), Data([0xbf, 1, 2]), Data([0xff]),
            Data([0xbf, 1, 2, 0x18, 1, 3, 0xff]), Data([0x9f]) + Data(repeating: 0, count: 129) + Data([0xff]),
        ] {
            #expect(throws: TrustError.malformedDocument) { try TrustCBOR.decode(invalid) }
        }
    }

    @Test func schemaRejectsWrongDigestAndOptionalFieldTypes() throws {
        for (field, value) in [
            ("digest", TrustCBOR.text("SHA256")), ("public_key", .text("not bytes")),
            ("module_id", .text("")), ("pcrs", .map([])),
        ] {
            let forged = try mutate(field, value: value)
            #expect(throws: TrustError.malformedDocument) {
                try AttestationVerifier().verify(base64: forged, nonce: Self.nonce, deviceTime: Self.signedAt)
            }
        }
    }

    @Test func publicVerifierNeverAcceptsLocalMock() throws {
        let payload = TrustCBOR.map([
            .init(.text("public_key"), .bytes(Data(repeating: 1, count: 32))),
            .init(.text("user_data"), .bytes(Data("local-test".utf8))),
            .init(.text("nonce"), .bytes(Self.nonce)),
        ])
        let encoded = TrustCBOR.array([.bytes(Data()), .map([]), .bytes(payload.encode()), .bytes(Data())]).encode()
            .base64EncodedString()
        let parsed = try AttestationVerifier.parseLoopbackDocument(base64: encoded)
        #expect(parsed.nonce == Self.nonce)
        #expect(parsed.publicKey?.count == 32)
        #expect(parsed.pcrs.isEmpty)
        #expect(throws: TrustError.malformedDocument) {
            try AttestationVerifier().verify(base64: encoded, nonce: Self.nonce, deviceTime: Self.signedAt)
        }
    }
}
