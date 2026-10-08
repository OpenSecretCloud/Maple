import Foundation
import Testing
@testable import MapleSDK

struct ClockDiagnosticTests {
    private let signedAt = Date(timeIntervalSince1970: 1_800_000_000)

    @Test func validatedPublicDiagnosticPreservesDirectionAndThreshold() throws {
        for seconds in [-60.0, -59.999, 0, 59.999, 60] {
            let error = try #require(
                AttestationClockSkewError(
                    validatingDeviceTime: signedAt.addingTimeInterval(seconds), enclaveTime: signedAt,
                    certificateIndex: 0, notBefore: signedAt.addingTimeInterval(-600),
                    notAfter: signedAt.addingTimeInterval(30)))
            #expect(error.deviceClockLikelyWrong == (abs(seconds) >= 60))
            let description = try #require(error.errorDescription)
            #expect(description.contains("date, time and time zone settings"))
            if seconds <= -60 { #expect(description.contains("behind the secure enclave")) }
            if seconds >= 60 { #expect(description.contains("ahead of the secure enclave")) }
        }
    }

    @Test func publicConstructionRejectsNonfiniteExtremeOrMalformedMetadata() {
        for time in [Double.nan, .infinity, -.infinity, .greatestFiniteMagnitude, -62_135_596_801, 253_402_300_800] {
            for index in 0..<4 {
                var dates = Array(repeating: signedAt, count: 4)
                dates[index] = Date(timeIntervalSince1970: time)
                #expect(
                    AttestationClockSkewError(
                        validatingDeviceTime: dates[0], enclaveTime: dates[1], certificateIndex: 0,
                        notBefore: dates[2], notAfter: dates[3]) == nil)
            }
        }
        #expect(
            AttestationClockSkewError(
                validatingDeviceTime: signedAt, enclaveTime: signedAt, certificateIndex: -1,
                notBefore: signedAt, notAfter: signedAt) == nil)
        #expect(
            AttestationClockSkewError(
                validatingDeviceTime: signedAt, enclaveTime: signedAt, certificateIndex: 0,
                notBefore: signedAt, notAfter: signedAt.addingTimeInterval(-1)) == nil)
    }

    @Test func widestSupportedDatesProduceBoundedDescriptionWithoutIntegerOverflow() throws {
        let earliest = Date(timeIntervalSince1970: -62_135_596_800)
        let latest = Date(timeIntervalSince1970: 253_402_300_799)
        for pair in [(earliest, latest), (latest, earliest)] {
            let error = try #require(
                AttestationClockSkewError(
                    validatingDeviceTime: pair.0, enclaveTime: pair.1, certificateIndex: Int.max,
                    notBefore: earliest, notAfter: latest))
            let description = try #require(error.errorDescription)
            #expect(description.utf8.count < 1_024)
            #expect(description.contains("date, time and time zone settings"))
            #expect(!description.contains("unrepresentable"))
        }
    }

    @Test func internalVerifierDiagnosticFormattingCannotTrapOnExtremeDeviceClock() throws {
        for seconds in [Double.infinity, -.infinity, .greatestFiniteMagnitude] {
            let error = AttestationClockSkewError(
                deviceTime: Date(timeIntervalSince1970: seconds), enclaveTime: signedAt,
                certificateIndex: 0, notBefore: signedAt, notAfter: signedAt)
            let description = try #require(error.errorDescription)
            #expect(description.contains("a very large amount of time"))
            #expect(description.contains("an unrepresentable date"))
        }
    }
}
