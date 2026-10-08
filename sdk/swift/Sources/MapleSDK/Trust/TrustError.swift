import Foundation

/// Bounded categories suitable for crossing application boundaries. No error contains
/// certificates, URLs, key material, or untrusted response content.
public enum TrustError: Error, Equatable, Sendable, LocalizedError {
    case malformedDocument
    case nonceMismatch
    case unsupportedAlgorithm
    case certificateChain
    case documentSignature
    case invalidPCR0
    case untrustedPCR0
    case invalidPolicy
    case historyUnavailable

    public var errorDescription: String? {
        switch self {
        case .malformedDocument: "The enclave attestation document is invalid."
        case .nonceMismatch: "The enclave attestation challenge does not match."
        case .unsupportedAlgorithm: "The enclave attestation algorithm is unsupported."
        case .certificateChain: "The enclave attestation certificate chain is invalid."
        case .documentSignature: "The enclave attestation signature is invalid."
        case .invalidPCR0: "The enclave attestation contains an invalid PCR0."
        case .untrustedPCR0: "Attestation PCR0 is not in the configured trust roots."
        case .invalidPolicy: "The enclave trust policy is invalid."
        case .historyUnavailable: "Signed enclave trust history is unavailable."
        }
    }
}

/// Matches the TypeScript SDK's certificate-clock diagnosis after authenticating
/// the document. A signed timestamp explains a failure; it never replaces the clock.
public struct AttestationClockSkewError: Error, Sendable, LocalizedError {
    public let deviceTime: Date
    public let enclaveTime: Date
    public let certificateIndex: Int
    public let notBefore: Date
    public let notAfter: Date
    public var skewMs: Double { deviceTime.timeIntervalSince(enclaveTime) * 1_000 }
    public var deviceClockLikelyWrong: Bool { abs(skewMs) >= 60_000 }

    public var errorDescription: String? {
        let state = deviceTime > notAfter ? "expired" : "not yet valid"
        if deviceClockLikelyWrong {
            return "This device's clock is about \(Self.duration(abs(skewMs))) "
                + "\(skewMs >= 0 ? "ahead of" : "behind") the secure enclave "
                + "(device: \(Self.iso(deviceTime)), enclave: \(Self.iso(enclaveTime))), "
                + "so the enclave's certificate looks \(state). "
                + "Check the device's date, time and time zone settings, then try again."
        }
        return "The secure enclave's certificate is \(state) on this device's clock "
            + "(valid \(Self.iso(notBefore)) to \(Self.iso(notAfter)), "
            + "device: \(Self.iso(deviceTime))). Try again in a moment; if it keeps happening, "
            + "check the device's date, time and time zone settings."
    }

    private static func duration(_ milliseconds: Double) -> String {
        // A diagnostic must never trap on an extreme injected system clock.
        guard milliseconds.isFinite, milliseconds >= 0,
            milliseconds < Double(Int.max) / 2
        else { return "a very large amount of time" }
        for (name, size) in [("day", 86_400_000.0), ("hour", 3_600_000), ("minute", 60_000)] {
            if milliseconds >= size {
                let count = Int((milliseconds / size).rounded())
                return "\(count) \(name)\(count == 1 ? "" : "s")"
            }
        }
        return "\(Int((milliseconds / 1_000).rounded())) seconds"
    }

    private static func iso(_ date: Date) -> String {
        guard diagnosticDateRange.contains(date.timeIntervalSince1970) else { return "an unrepresentable date" }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }
}

extension AttestationClockSkewError {
    private static var diagnosticDateRange: ClosedRange<Double> { -62_135_596_800...253_402_300_799 }

    /// Constructs a bounded diagnostic for presentation or error propagation.
    /// Constructing this value does not verify an attestation or grant trust.
    /// Dates must fit the ISO-8601 year range 0001...9999, and certificate
    /// validity must be ordered. The verifier creates its error independently.
    public init?(
        validatingDeviceTime deviceTime: Date, enclaveTime: Date, certificateIndex: Int,
        notBefore: Date, notAfter: Date
    ) {
        let dates = [deviceTime, enclaveTime, notBefore, notAfter]
        let delta = deviceTime.timeIntervalSince(enclaveTime) * 1_000
        guard certificateIndex >= 0, notBefore <= notAfter,
            dates.allSatisfy({ Self.diagnosticDateRange.contains($0.timeIntervalSince1970) }),
            delta.isFinite, abs(delta) < Double(Int.max) / 2
        else { return nil }
        self.init(
            deviceTime: deviceTime, enclaveTime: enclaveTime, certificateIndex: certificateIndex,
            notBefore: notBefore, notAfter: notAfter)
    }
}

func trustEqual(_ left: Data, _ right: Data) -> Bool {
    guard left.count == right.count else { return false }
    var difference: UInt8 = 0
    for (a, b) in zip(left, right) { difference |= a ^ b }
    return difference == 0
}
