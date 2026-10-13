#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct AccountAccessNotice: View {
        @Bindable var account: AccountEntitlements
        var body: some View {
            if !awaitingDetails && account.chatAccess != .allowed && account.chatAccess != .seatMismatch {
                HStack(alignment: .top) {
                    Text(message).font(.footnote).foregroundStyle(.secondary)
                    if account.chatAccess == .unavailable || account.chatAccess == .unknownPlan
                        || (account.chatAccess == .modelUnavailable && account.catalog.value == nil)
                    {
                        Button(MapleStrings.text("Retry")) { Task { await account.refresh() } }.font(.footnote)
                    }
                }
            }
        }
        private var awaitingDetails: Bool {
            switch account.chatAccess {
            case .unavailable: account.billing == .unknown || account.billing == .loading
            case .modelUnavailable: account.catalog == .unknown || account.catalog == .loading
            default: false
            }
        }
        private var message: String {
            switch account.chatAccess {
            case .allowed: return ""
            case .usageLimit: return MapleStrings.text("You’ve reached your current chat allowance.")
            case .freeGuest:
                return MapleStrings.text("This guest account can’t start a chat. Sign in with an eligible account.")
            case .seatMismatch: return ""
            case .paidPlanRequired:
                return MapleStrings.text("This model requires a paid plan. Choose an available model.")
            case .modelUnavailable, .modelIncompatible:
                return MapleStrings.text("The selected model isn’t currently available.")
            case .retired: return MapleStrings.text("Your session has ended.")
            case .unknownPlan, .unavailable:
                return MapleStrings.text("Account access is unavailable. Your draft is preserved.")
            }
        }
    }

    /// Read-only status shared by the chat routes. Team management and purchases
    /// remain outside this presentation; the server owns the usage restriction.
    struct TeamSeatMismatchNotice: View {
        @Bindable var account: AccountEntitlements
        @Environment(\.locale) private var locale

        var body: some View {
            if let presentation = TeamSeatMismatchPresentation(status: account.team.value) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(.red).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(MapleStrings.text("Team usage paused", locale: locale))
                            .font(.subheadline.weight(.medium)).accessibilityAddTraits(.isHeader)
                        Text(presentation.summary(locale: locale))
                            .font(.caption.weight(.medium)).foregroundStyle(.red)
                        Text(presentation.message(locale: locale))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(16)
                .background(MapleSurface.card, in: RoundedRectangle(cornerRadius: 8))
                .overlay {
                    RoundedRectangle(cornerRadius: 8).strokeBorder(.red.opacity(0.5), lineWidth: 1)
                }
                .padding(.horizontal, 12).padding(.vertical, 8)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("chat.team-seat-mismatch")
            }
        }
    }
#endif
