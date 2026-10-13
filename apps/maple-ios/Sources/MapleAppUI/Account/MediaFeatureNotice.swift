#if os(iOS)
    import MapleAppCore
    import SwiftUI

    enum MediaFeature: String, Identifiable {
        case images, dictation, speech, webSearch, model
        var id: String { rawValue }

        var title: String {
            switch self {
            case .images: MapleStrings.text("Images")
            case .dictation: MapleStrings.text("Dictation")
            case .speech: MapleStrings.text("Read aloud")
            case .webSearch: MapleStrings.text("Web search")
            case .model: String(localized: "Powerful AI Models", table: "Settings", bundle: .module)
            }
        }

        @MainActor func access(in account: AccountEntitlements) -> AccountAccess {
            switch self {
            case .images: account.imageAccess
            case .dictation: account.transcriptionAccess
            case .speech: account.speechAccess
            case .webSearch: account.webSearchAccess
            case .model: .paidPlanRequired
            }
        }
    }

    /// Explains current read-only entitlement and catalog state. It does not
    /// change preferences, initiate purchases, or claim backend authorization.
    struct MediaFeatureNotice: View {
        let feature: MediaFeature
        @Bindable var account: AccountEntitlements
        var modelName: String? = nil
        @Environment(\.dismiss) private var dismiss

        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Label(title, systemImage: "info.circle")
                            .font(.title2).accessibilityAddTraits(.isHeader)
                        Text(message).accessibilityIdentifier("media.access.message")
                        if feature == .model {
                            VStack(alignment: .leading, spacing: 12) {
                                Text(
                                    "All models run in secure, encrypted environments", tableName: "Settings",
                                    bundle: .module)
                                Text(
                                    "Access to GLM 5.3, Kimi K2.6, and the full model lineup", tableName: "Settings",
                                    bundle: .module)
                                Text(
                                    "OpenAI GPT-OSS, Gemma, and other advanced models", tableName: "Settings",
                                    bundle: .module)
                                Text(
                                    "Higher token limits for longer conversations", tableName: "Settings",
                                    bundle: .module)
                                Text(
                                    "Priority access to new models as they launch", tableName: "Settings",
                                    bundle: .module)
                            }.font(.subheadline)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(24)
                }
                .navigationTitle(title).navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button(MapleStrings.text("Done")) { dismiss() }
                            .accessibilityIdentifier("media.access.close")
                    }
                }
            }
            .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        }

        private var title: String {
            guard feature == .model, let modelName else { return feature.title }
            return String(
                localized: "settings.model-access-title", defaultValue: "Access \(modelName)",
                table: "Settings", bundle: .module)
        }

        private var message: String {
            if feature == .model {
                return String(
                    localized: "Get access to our most advanced AI models for superior performance",
                    table: "Settings", bundle: .module)
            }
            return switch feature.access(in: account) {
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
            case .model:
                String(
                    localized: "Get access to our most advanced AI models for superior performance",
                    table: "Settings", bundle: .module)
            }
        }

        private var capabilityMessage: String {
            switch feature {
            case .images: MapleStrings.text("Choose a model that supports images.")
            case .webSearch: MapleStrings.text("Choose a model that supports web search.")
            case .dictation: MapleStrings.text("Dictation is currently unavailable. Try again later.")
            case .speech: MapleStrings.text("Read aloud is currently unavailable. Try again later.")
            case .model:
                String(
                    localized: "Get access to our most advanced AI models for superior performance",
                    table: "Settings", bundle: .module)
            }
        }
    }
#endif
