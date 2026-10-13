import MapleAppCore

/// Read-only chat feedback copied from the Research client.
enum ChatRecoveryPresentation {
    static func message(_ issue: ChatIssue, productName: String? = nil) -> String {
        switch issue {
        case .offline:
            return MapleStrings.text("You’re offline. Your draft is saved when protected storage is available.")
        case .storageUnavailable:
            return MapleStrings.text("Your draft hasn’t been saved. Unlock your device and try again.")
        case .uncertainResponse:
            return MapleStrings.text("The response is still being checked. Your message will not be sent again.")
        case .cancellationUnavailable:
            return MapleStrings.text("Stop hasn’t been confirmed. Check the response to try again.")
        case .usageLimit:
            let plan = productName?.lowercased()
            if plan == nil || plan?.isEmpty == true || plan == "free" {
                return MapleStrings.text("You've reached your daily usage limit. Upgrade to Pro for more chats.")
            } else if plan?.contains("pro") == true, plan?.contains("max") == false {
                return MapleStrings.text("You've reached your monthly Pro limit. Upgrade to Max for 10x more usage.")
            } else {
                return MapleStrings.text(
                    "You've reached your monthly usage limit. Please wait for the next billing cycle.")
            }
        case .freeTierTokenLimit:
            return MapleStrings.text(
                "This conversation is too long for the free tier. Upgrade to Pro for longer conversations.")
        case .contextLimit: return MapleStrings.text("Your message exceeds the context limit for this model.")
        case .modelUnavailable: return MapleStrings.text("This model or feature is not available on your current plan.")
        case .generationFailed: return MapleStrings.text("Failed to generate response. Please try again.")
        case .stoppedBeforeSending: return MapleStrings.text("Stopped before sending. Your message was restored.")
        case .stoppingWhenResponseReady: return MapleStrings.text("Stopping as soon as the response is ready…")
        case .responseFinishedBeforeStop: return MapleStrings.text("The response finished before it could be stopped.")
        case .recoveredCompletion:
            return MapleStrings.text(
                "The response completed after the connection was restored. Queued messages were kept.")
        case .recoveredStop: return MapleStrings.text("The interrupted response stopped. Queued messages were kept.")
        case .imageDescriptionUnavailable:
            return MapleStrings.text(
                "Image description is temporarily unavailable. Your message and images were restored; please try again."
            )
        case .inferenceCapacityUnavailable:
            return MapleStrings.text(
                "Inference capacity is temporarily unavailable. Your message was restored. Please try again shortly.")
        case .historyUnavailable: return MapleStrings.text("Couldn’t load the conversation. Try again when connected.")
        case .invalidImages: return MapleStrings.text("Check the image attachments and try again.")
        case .draftTooLarge: return MapleStrings.text("This message is too large. Shorten it and try again.")
        case .signedOut: return MapleStrings.text("Your session has ended. Sign in again.")
        default: return MapleStrings.text("The request couldn’t finish. Your unsent work is preserved.")
        }
    }

    static var contextLimitMessage: String {
        MapleStrings.text(
            "Your message exceeds the context limit for the current model.\n\nHere's what you can try:\nShorten your message - Try reducing the amount of text or content"
        )
    }
}

#if os(iOS)
    import SwiftUI

    /// Shared shipping presentation for recoverable account-owned chat work.
    struct ChatRecoveryControls: View {
        @Bindable var runtime: ChatRuntime
        var productName: String? = nil
        var body: some View {
            if let issue = runtime.lastIssue {
                Text(ChatRecoveryPresentation.message(issue, productName: productName))
                    .font(.footnote).foregroundStyle(MapleRedText())
                    .accessibilityIdentifier("chat.error")
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
                    .frame(minHeight: 44)
            }
        }
    }
#endif
