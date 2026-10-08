#if DEBUG && os(iOS)
    import SwiftUI
    import UIKit

    /// Read-only, data-free proof of the actual environment received by the normal
    /// application. It never changes traits, settings, focus, layout or account state.
    struct LocalLocalizationProof: UIViewRepresentable {
        @Environment(\.dynamicTypeSize) private var typeSize
        @Environment(\.legibilityWeight) private var weight
        @Environment(\.layoutDirection) private var direction

        func makeUIView(context: Context) -> ProofLabel {
            let view = ProofLabel()
            view.font = .systemFont(ofSize: 8)
            view.text = "Localization diagnostics"
            view.isAccessibilityElement = true
            view.accessibilityIdentifier = "localization.observed-traits"
            view.isUserInteractionEnabled = false
            return view
        }

        func updateUIView(_ view: ProofLabel, context: Context) {
            view.swiftAccessibility = typeSize.isAccessibilitySize
            view.swiftBold = weight == .bold
            view.swiftRTL = direction == .rightToLeft
            view.publish()
        }

        @MainActor final class ProofLabel: UILabel {
            var swiftAccessibility = false
            var swiftBold = false
            var swiftRTL = false

            override func didMoveToWindow() {
                super.didMoveToWindow()
                publish()
            }

            func publish() {
                let values: [String: Bool] = [
                    "swiftAccessibility": swiftAccessibility,
                    "uiAccessibility": traitCollection.preferredContentSizeCategory.isAccessibilityCategory,
                    "swiftBold": swiftBold,
                    "uiBold": traitCollection.legibilityWeight == .bold,
                    "systemBold": UIAccessibility.isBoldTextEnabled,
                    "swiftRTL": swiftRTL,
                    "uiRTL": effectiveUserInterfaceLayoutDirection == .rightToLeft,
                ]
                if let bytes = try? JSONEncoder().encode(values), let value = String(data: bytes, encoding: .utf8) {
                    accessibilityValue = value
                }
            }
        }
    }
#endif
