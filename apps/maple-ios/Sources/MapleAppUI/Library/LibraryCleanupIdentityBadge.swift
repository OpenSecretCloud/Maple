#if os(iOS) && DEBUG
    import CryptoKit
    import Foundation
    import MapleAppCore
    import SwiftUI

    /// Local test runner ownership marker. No credentials or raw account ID are
    /// exposed, and the root admits this only with its explicit DEBUG launch flag.
    struct LibraryCleanupIdentityBadge: View {
        let scope: AccountScope
        var body: some View {
            let encoded = try? JSONEncoder().encode([scope.apiOrigin, scope.principalID])
            let fingerprint =
                encoded.map { data in
                    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
                } ?? "unavailable"
            Text(verbatim: "Account diagnostics").font(.system(size: 8)).lineLimit(1)
                .accessibilityLabel(fingerprint)
                .accessibilityIdentifier("library.cleanup-account")
        }
    }
#endif
