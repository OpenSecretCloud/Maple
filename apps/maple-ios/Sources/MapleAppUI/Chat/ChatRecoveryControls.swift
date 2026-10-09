#if os(iOS)
    import MapleAppCore
    import SwiftUI

    /// Shared shipping presentation for recoverable account-owned chat work.
    struct ChatRecoveryControls: View {
        @Bindable var runtime: ChatRuntime
        var body: some View {
            if let issue = runtime.lastIssue {
                Text(issueMessage(issue)).font(.footnote).foregroundStyle(MapleRedText()).accessibilityIdentifier(
                    "chat.error")
            }
            if runtime.phase == .recovering, runtime.lastIssue != nil {
                Button(MapleStrings.text("Check response"), systemImage: "arrow.clockwise") {
                    Task { await runtime.reconcile() }
                }
                .labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                .accessibilityIdentifier("chat.recover")
            }
            if runtime.snapshot.unsentTurn != nil {
                Button(MapleStrings.text("Restore unsent message")) { runtime.restoreUnsentTurn() }
            }
        }
        private func issueMessage(_ issue: ChatIssue) -> String {
            switch issue {
            case .offline: MapleStrings.text("You’re offline. Your draft is saved when protected storage is available.")
            case .storageUnavailable:
                MapleStrings.text("Your draft hasn’t been saved. Unlock your device and try again.")
            case .uncertainResponse:
                MapleStrings.text("The response is still being checked. Your message will not be sent again.")
            case .cancellationUnavailable:
                MapleStrings.text("Stop hasn’t been confirmed. Check the response to try again.")
            case .usageLimit: MapleStrings.text("You’ve reached your usage limit.")
            case .contextLimit:
                MapleStrings.text("This conversation is too long for the selected model. Start a new chat.")
            case .modelUnavailable: MapleStrings.text("This model is unavailable. Choose another model.")
            case .historyUnavailable: MapleStrings.text("Couldn’t load the conversation. Try again when connected.")
            case .invalidImages: MapleStrings.text("Check the image attachments and try again.")
            case .draftTooLarge: MapleStrings.text("This message is too large. Shorten it and try again.")
            case .signedOut: MapleStrings.text("Your session has ended. Sign in again.")
            default: MapleStrings.text("The request couldn’t finish. Your unsent work is preserved.")
            }
        }
    }
#endif
