#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct AccountAccessNotice: View {
        @Bindable var account: AccountEntitlements
        var body: some View {
            if !awaitingDetails && account.chatAccess != .allowed {
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
            case .seatMismatch: return MapleStrings.text("Your team’s seat allocation needs attention.")
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
#endif
