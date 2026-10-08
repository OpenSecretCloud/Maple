#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct AccountAccessNotice: View {
        @Bindable var account: AccountEntitlements
        var body: some View {
            HStack(alignment: .top) {
                Text(message).font(.footnote).foregroundStyle(.secondary)
                if account.chatAccess == .unavailable || account.chatAccess == .unknownPlan {
                    Button(MapleStrings.text("Retry")) { Task { await account.refresh() } }.font(.footnote)
                }
            }
        }
        private var message: String {
            if account.billing == .loading || account.catalog == .loading {
                return MapleStrings.text("Checking account access…")
            }
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
