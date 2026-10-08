import Foundation
import MapleSDK

/// Safe presentation categories from a typed SDK clock diagnosis.
/// No dates, signed documents, underlying descriptions or server content escape.
public enum AttestationClockIssue: Sendable, Equatable {
    case deviceTime, certificateTiming

    static func classify(_ error: any Error) -> Self? {
        var current: any Error = error
        // Foundation may wrap SDK errors. Follow only its typed underlying-error
        // contract, with a strict bound even if an NSError chain is cyclic.
        for _ in 0..<8 {
            if let clock = current as? AttestationClockSkewError {
                return clock.deviceClockLikelyWrong ? .deviceTime : .certificateTiming
            }
            guard let underlying = (current as NSError).userInfo[NSUnderlyingErrorKey] as? any Error else {
                return nil
            }
            current = underlying
        }
        return nil
    }
}
