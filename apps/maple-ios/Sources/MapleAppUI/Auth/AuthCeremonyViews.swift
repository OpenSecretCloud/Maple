#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct AuthIssueView: View {
        let issue: AuthCeremonyIssue
        var body: some View {
            Text(message).foregroundStyle(MapleRedText())
                .accessibilityIdentifier("auth.ceremony-error")
        }
        private var message: String {
            switch issue {
            case .clock(let clock): AttestationClockText.message(clock)
            case .invalidInput:
                String(
                    localized: "Check the code and make sure your new passwords match. Use at least eight characters.",
                    table: "Auth", bundle: .module)
            case .expired:
                String(
                    localized: "This request expired. Start again to get a new code or sign-in request.", table: "Auth",
                    bundle: .module)
            case .changedAccount:
                String(
                    localized: "Your sign-in changed. Reopen this screen to continue.", table: "Auth", bundle: .module)
            case .browserSignInUnavailable:
                String(
                    localized:
                        "Browser sign-in isn’t available in this build. Sign in with email or an existing guest account.",
                    table: "Auth", bundle: .module)
            case .invalidCallback:
                String(
                    localized: "This sign-in response could not be verified. Start sign-in again.", table: "Auth",
                    bundle: .module)
            case .connection:
                String(
                    localized: "The request could not be completed. Check your connection and try again.",
                    table: "Auth", bundle: .module)
            case .invalidCredentials:
                String(localized: "The current password is incorrect.", table: "Auth", bundle: .module)
            case .billingCleanupUnavailable:
                String(
                    localized:
                        "Your account was not deleted because billing cleanup could not finish. Try again later or contact support.",
                    table: "Auth", bundle: .module)
            case .cleanupRequired:
                String(
                    localized: "Maple could not safely stop account activity. Your account was not deleted. Try again.",
                    table: "Auth", bundle: .module)
            case .deletionUnconfirmed:
                String(
                    localized: "Account deletion was not confirmed. Check your account before trying again.",
                    table: "Auth", bundle: .module)
            }
        }
    }

    public struct PasswordResetView: View {
        @Bindable private var ceremonies: AuthCeremonies
        @Environment(\.dismiss) private var dismiss
        @Environment(\.scenePhase) private var scenePhase
        @State private var email = ""
        @State private var code: String
        @State private var password = ""
        @State private var confirmation = ""
        @State private var showWarning = false
        private let incomingCode: String?
        public init(ceremonies: AuthCeremonies, code: String? = nil) {
            self.ceremonies = ceremonies; incomingCode = code; _code = State(initialValue: code ?? "")
            _email = State(initialValue: ceremonies.accountEmail ?? "")
        }
        public var body: some View {
            List {
                if ceremonies.state == .completed(.passwordReset) {
                    Section {
                        Text(
                            "Your password was reset. Sign in with your new password.", tableName: "Auth",
                            bundle: .module)
                        Button {
                            dismiss()
                        } label: {
                            Text("Return to sign-in", tableName: "Auth", bundle: .module)
                        }
                    }
                } else if case .resetCode(let address) = ceremonies.state {
                    resetConfirmation(address)
                } else if ceremonies.isBusy, !password.isEmpty {
                    resetConfirmation(email)
                } else {
                    Section {
                        TextField(
                            text: $email,
                            prompt: Text("Email", tableName: "Auth", bundle: .module).foregroundStyle(
                                MapleFieldPrompt())
                        ) { Text("Email", tableName: "Auth", bundle: .module) }
                        .textContentType(.emailAddress).keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier(
                            "auth.reset-email"
                        )
                        .disabled(ceremonies.scope != nil)
                        Text(
                            "Password reset creates a new private key and removes private encrypted content, including chats and saved data. If you can still sign in, change your password in settings instead.",
                            tableName: "Auth", bundle: .module
                        )
                        .font(.callout)
                        if !code.isEmpty {
                            Text(
                                "Start the reset in this app to use your code. A code alone cannot restore the private confirmation secret.",
                                tableName: "Auth", bundle: .module
                            ).font(.callout)
                        }
                        Button {
                            showWarning = true
                        } label: {
                            Text("Request password reset", tableName: "Auth", bundle: .module)
                        }.disabled(email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || ceremonies.isBusy)
                            .accessibilityIdentifier("auth.request-reset")
                    }
                }
                if let issue = ceremonies.issue { Section { AuthIssueView(issue: issue) } }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(Text("Reset password", tableName: "Auth", bundle: .module))
            .confirmationDialog(
                Text("Resetting your password deletes private data", tableName: "Auth", bundle: .module),
                isPresented: $showWarning, titleVisibility: .visible
            ) {
                Button(role: .destructive) {
                    Task { await ceremonies.requestPasswordReset(email: email) }
                } label: {
                    Text("Continue with reset", tableName: "Auth", bundle: .module)
                }
            } message: {
                Text(
                    "Try your current password or Apple, Google, or GitHub first. Resetting removes private encrypted chats and saved data.",
                    tableName: "Auth", bundle: .module)
            }
            .onChange(of: ceremonies.state) { _, value in
                if value == .completed(.passwordReset) { password = ""; confirmation = ""; code = "" }
            }
            .onChange(of: incomingCode) { _, value in if let value { code = value } }
            .onChange(of: scenePhase) { _, phase in if phase == .active { ceremonies.expireIfNeeded() } }
            .onDisappear {
                password = ""; confirmation = ""; Task { await ceremonies.cancel() }
            }
        }
        @ViewBuilder private func resetConfirmation(_ address: String) -> some View {
            Section {
                Text(address).font(.callout).textSelection(.enabled)
                TextField(
                    text: $code,
                    prompt: Text("Eight-character reset code", tableName: "Auth", bundle: .module).foregroundStyle(
                        MapleFieldPrompt())
                ) { Text("Eight-character reset code", tableName: "Auth", bundle: .module) }
                .textContentType(.oneTimeCode).textInputAutocapitalization(.characters).autocorrectionDisabled()
                .accessibilityIdentifier("auth.reset-code")
                SecureField(
                    text: $password,
                    prompt: Text("New password", tableName: "Auth", bundle: .module).foregroundStyle(MapleFieldPrompt())
                ) { Text("New password", tableName: "Auth", bundle: .module) }
                .textContentType(.newPassword).accessibilityIdentifier("auth.reset-password")
                SecureField(
                    text: $confirmation,
                    prompt: Text("Confirm new password", tableName: "Auth", bundle: .module).foregroundStyle(
                        MapleFieldPrompt())
                ) { Text("Confirm new password", tableName: "Auth", bundle: .module) }
                .textContentType(.newPassword).accessibilityIdentifier("auth.reset-confirm-password")
                Text(
                    "If a code does not arrive, check spam or try the provider you used to create your account.",
                    tableName: "Auth", bundle: .module
                ).font(.callout)
                Button {
                    Task {
                        await ceremonies.confirmPasswordReset(
                            code: code, newPassword: password, confirmation: confirmation)
                    }
                } label: {
                    Text("Reset password", tableName: "Auth", bundle: .module)
                }.disabled(ceremonies.isBusy || code.isEmpty || password.isEmpty || confirmation.isEmpty)
                    .accessibilityIdentifier("auth.confirm-reset")
            }.disabled(ceremonies.isBusy)
        }
    }

    public struct EmailVerificationView: View {
        @Bindable private var ceremonies: AuthCeremonies
        @State private var code: String
        private let initialCode: String?
        public init(ceremonies: AuthCeremonies, code: String? = nil) {
            self.ceremonies = ceremonies; initialCode = code; _code = State(initialValue: code ?? "")
        }
        public var body: some View {
            List {
                if ceremonies.state == .completed(.emailVerified) {
                    Text("Your email is verified.", tableName: "Auth", bundle: .module).accessibilityIdentifier(
                        "auth.verified")
                } else {
                    Section {
                        TextField(
                            text: $code,
                            prompt: Text("Verification code", tableName: "Auth", bundle: .module).foregroundStyle(
                                MapleFieldPrompt())
                        ) { Text("Verification code", tableName: "Auth", bundle: .module) }
                        .textContentType(.oneTimeCode).textInputAutocapitalization(.never).autocorrectionDisabled()
                        .accessibilityIdentifier("auth.verify-code")
                        Button {
                            Task { await ceremonies.verifyEmail(code: code) }
                        } label: {
                            Text("Verify email", tableName: "Auth", bundle: .module)
                        }.disabled(ceremonies.isBusy || code.isEmpty).accessibilityIdentifier("auth.verify-submit")
                        if ceremonies.scope != nil {
                            Button {
                                Task { await ceremonies.resendVerification() }
                            } label: {
                                Text("Send another verification email", tableName: "Auth", bundle: .module)
                            }.disabled(ceremonies.isBusy).accessibilityIdentifier("auth.verify-resend")
                        }
                        if ceremonies.state == .completed(.verificationSent) {
                            Text("Check your inbox for a verification email.", tableName: "Auth", bundle: .module)
                        }
                    }
                }
                if let issue = ceremonies.issue { AuthIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(Text("Verify email", tableName: "Auth", bundle: .module))
            .task { if let initialCode { await ceremonies.verifyEmail(code: initialCode) } }
            .onChange(of: initialCode) { _, value in
                guard let value else { return }
                code = value
                Task { await ceremonies.verifyEmail(code: value) }
            }
            .onDisappear { Task { await ceremonies.cancel() } }
        }
    }

    public struct ChangePasswordView: View {
        @Bindable private var ceremonies: AuthCeremonies
        @State private var current = ""
        @State private var password = ""
        @State private var confirmation = ""
        public init(ceremonies: AuthCeremonies) { self.ceremonies = ceremonies }
        public var body: some View {
            List {
                if ceremonies.state == .completed(.passwordChanged) {
                    Text("Your password was changed.", tableName: "Auth", bundle: .module).accessibilityIdentifier(
                        "auth.password-changed")
                } else {
                    Section {
                        SecureField(
                            text: $current,
                            prompt: Text("Current password", tableName: "Auth", bundle: .module).foregroundStyle(
                                MapleFieldPrompt())
                        ) { Text("Current password", tableName: "Auth", bundle: .module) }
                        .textContentType(.password).accessibilityIdentifier("auth.current-password")
                        SecureField(
                            text: $password,
                            prompt: Text("New password", tableName: "Auth", bundle: .module).foregroundStyle(
                                MapleFieldPrompt())
                        ) { Text("New password", tableName: "Auth", bundle: .module) }
                        .textContentType(.newPassword).accessibilityIdentifier("auth.new-password")
                        SecureField(
                            text: $confirmation,
                            prompt: Text("Confirm new password", tableName: "Auth", bundle: .module).foregroundStyle(
                                MapleFieldPrompt())
                        ) { Text("Confirm new password", tableName: "Auth", bundle: .module) }
                        .textContentType(.newPassword).accessibilityIdentifier("auth.confirm-new-password")
                        Text("Use at least eight characters.", tableName: "Auth", bundle: .module).font(.callout)
                        Button {
                            Task {
                                await ceremonies.changePassword(
                                    currentPassword: current, newPassword: password, confirmation: confirmation)
                            }
                        } label: {
                            Text("Change password", tableName: "Auth", bundle: .module)
                        }.disabled(ceremonies.isBusy || current.isEmpty || password.isEmpty || confirmation.isEmpty)
                            .accessibilityIdentifier("auth.change-password")
                    }.disabled(ceremonies.isBusy)
                }
                if let issue = ceremonies.issue { AuthIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(Text("Change password", tableName: "Auth", bundle: .module))
            .interactiveDismissDisabled(ceremonies.isBusy).navigationBarBackButtonHidden(ceremonies.isBusy)
            .onChange(of: ceremonies.state) { _, value in
                if value == .completed(.passwordChanged) { current = ""; password = ""; confirmation = "" }
            }
            .onDisappear {
                current = ""; password = ""; confirmation = ""; Task { await ceremonies.cancel() }
            }
        }
    }

    public struct DeleteAccountView: View {
        @Bindable private var ceremonies: AuthCeremonies
        @Environment(\.scenePhase) private var scenePhase
        @State private var confirmation = ""
        @State private var code = ""
        @State private var confirmingCode = false
        public init(ceremonies: AuthCeremonies) { self.ceremonies = ceremonies }
        public var body: some View {
            List {
                if ceremonies.state == .completed(.accountDeleted) {
                    Text(
                        "Your account was deleted. Maple is clearing this account’s local data.", tableName: "Auth",
                        bundle: .module
                    )
                    .accessibilityIdentifier("auth.account-deleted")
                } else {
                    Section {
                        Text(
                            "This permanently removes your account and its saved data. You will lose access to chats and paid features. This cannot be undone.",
                            tableName: "Auth", bundle: .module)
                        Text(
                            "Any required billing cleanup must finish before the account can be deleted.",
                            tableName: "Auth", bundle: .module
                        ).font(.callout)
                        if confirmingCode {
                            TextField(
                                text: $code,
                                prompt: Text("Account deletion code", tableName: "Auth", bundle: .module)
                                    .foregroundStyle(MapleFieldPrompt())
                            ) { Text("Account deletion code", tableName: "Auth", bundle: .module) }
                            .textContentType(.oneTimeCode).textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("auth.delete-code")
                            Button(role: .destructive) {
                                Task { await ceremonies.confirmAccountDeletion(code: code) }
                            } label: {
                                Text("Confirm account deletion", tableName: "Auth", bundle: .module)
                            }.disabled(ceremonies.isBusy || code.isEmpty).accessibilityIdentifier("auth.confirm-delete")
                        } else {
                            TextField(
                                text: $confirmation,
                                prompt: Text("Type DELETE to confirm", tableName: "Auth", bundle: .module)
                                    .foregroundStyle(MapleFieldPrompt())
                            ) { Text("Type DELETE to confirm", tableName: "Auth", bundle: .module) }
                            .textInputAutocapitalization(.characters).autocorrectionDisabled().accessibilityIdentifier(
                                "auth.delete-confirmation")
                            Button(role: .destructive) {
                                Task { await ceremonies.requestAccountDeletion(confirmation: confirmation) }
                            } label: {
                                Text("Request deletion code", tableName: "Auth", bundle: .module)
                            }.disabled(ceremonies.isBusy || confirmation != "DELETE").accessibilityIdentifier(
                                "auth.request-delete")
                        }
                    }.disabled(ceremonies.isBusy)
                }
                if let issue = ceremonies.issue { AuthIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(Text("Delete account", tableName: "Auth", bundle: .module))
            .interactiveDismissDisabled(ceremonies.isBusy).navigationBarBackButtonHidden(ceremonies.isBusy)
            .onChange(of: ceremonies.state) { _, value in
                if value == .deletionCode { confirmingCode = true }
                if value == .idle { confirmingCode = false; code = "" }
            }
            .onChange(of: scenePhase) { _, phase in if phase == .active { ceremonies.expireIfNeeded() } }
            .onDisappear {
                code = ""; confirmation = ""; Task { await ceremonies.cancel() }
            }
        }
    }
#endif
