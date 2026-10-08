#if os(iOS)
    import MapleAppCore
    import Observation
    import SwiftUI

    enum AccountScreen: Hashable { case verification, reset, changePassword, deletion }

    @MainActor @Observable final class AccountFlow: Identifiable {
        let id = UUID()
        let screen: AccountScreen
        let ceremonies: AuthCeremonies
        var code: String?
        init(screen: AccountScreen, ceremonies: AuthCeremonies, code: String?) {
            self.screen = screen; self.ceremonies = ceremonies; self.code = code
        }
    }

    struct AccountFlowView: View {
        let flow: AccountFlow
        let session: SessionController
        var body: some View { NavigationStack { AccountFlowContent(flow: flow, session: session) } }
    }

    struct AccountFlowContent: View {
        @Bindable var flow: AccountFlow
        let session: SessionController
        @Environment(\.dismiss) private var dismiss
        var body: some View {
            Group {
                switch flow.screen {
                case .verification: EmailVerificationView(ceremonies: flow.ceremonies, code: flow.code)
                case .reset: PasswordResetView(ceremonies: flow.ceremonies, code: flow.code)
                case .changePassword: ChangePasswordView(ceremonies: flow.ceremonies)
                case .deletion: DeleteAccountView(ceremonies: flow.ceremonies)
                }
            }
            .navigationBarBackButtonHidden()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(MapleStrings.text("Done")) { dismiss() }.disabled(flow.ceremonies.isBusy)
                }
            }
            .interactiveDismissDisabled(flow.ceremonies.isBusy)
            .onChange(of: flow.ceremonies.state) { _, state in
                if state == .completed(.emailVerified) { Task { await session.refreshUser() } }
            }
            .onDisappear { flow.ceremonies.invalidate() }
        }
    }
#endif
