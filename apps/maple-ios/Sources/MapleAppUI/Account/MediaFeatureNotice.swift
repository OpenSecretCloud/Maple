#if os(iOS)
    import MapleAppCore
    import SwiftUI

    enum MediaFeature: String, Identifiable {
        case images, dictation, speech, webSearch
        var id: String { rawValue }

        var title: String {
            switch self {
            case .images: MapleStrings.text("Images")
            case .dictation: MapleStrings.text("Dictation")
            case .speech: MapleStrings.text("Read aloud")
            case .webSearch: MapleStrings.text("Web search")
            }
        }

        @MainActor func access(in account: AccountEntitlements) -> AccountAccess {
            switch self {
            case .images: account.imageAccess
            case .dictation: account.transcriptionAccess
            case .speech: account.speechAccess
            case .webSearch: account.webSearchAccess
            }
        }
    }

    /// Explains current read-only entitlement and catalog state. It does not
    /// change preferences, initiate purchases, or claim backend authorization.
    struct MediaFeatureNotice: View {
        let feature: MediaFeature
        @Bindable var account: AccountEntitlements
        @Environment(\.dismiss) private var dismiss

        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Label(feature.title, systemImage: "info.circle")
                            .font(.title2).accessibilityAddTraits(.isHeader)
                        Text(message).accessibilityIdentifier("media.access.message")
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(24)
                }
                .navigationTitle(feature.title).navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button(MapleStrings.text("Done")) { dismiss() }
                            .accessibilityIdentifier("media.access.close")
                    }
                }
            }
            .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        }

        private var message: String {
            switch feature.access(in: account) {
            case .allowed: MapleStrings.text("This feature is available for your account.")
            case .unavailable:
                if account.billing == .loading || account.catalog == .loading {
                    MapleStrings.text("Account details are still loading. Try again when loading finishes.")
                } else {
                    MapleStrings.text("Account details are unavailable. Refresh account status and try again.")
                }
            case .unknownPlan:
                MapleStrings.text("Your plan details are unavailable. Refresh account status and try again.")
            case .freeGuest, .paidPlanRequired: paidPlanMessage
            case .usageLimit: MapleStrings.text("Your account has reached its current usage limit.")
            case .seatMismatch:
                MapleStrings.text("Your team’s seat allocation needs attention. Contact your team administrator.")
            case .modelUnavailable:
                MapleStrings.text("The selected model is unavailable. Choose an available model.")
            case .modelIncompatible: capabilityMessage
            case .retired: MapleStrings.text("Your session has ended. Sign in again.")
            }
        }

        private var paidPlanMessage: String {
            switch feature {
            case .images: MapleStrings.text("Image attachments require an eligible paid plan.")
            case .dictation: MapleStrings.text("Dictation requires an eligible paid plan.")
            case .speech: MapleStrings.text("Read aloud requires an eligible paid plan.")
            case .webSearch: MapleStrings.text("Web search requires an eligible paid plan.")
            }
        }

        private var capabilityMessage: String {
            switch feature {
            case .images: MapleStrings.text("Choose a model that supports images.")
            case .webSearch: MapleStrings.text("Choose a model that supports web search.")
            case .dictation: MapleStrings.text("Dictation is currently unavailable. Try again later.")
            case .speech: MapleStrings.text("Read aloud is currently unavailable. Try again later.")
            }
        }
    }
#endif
