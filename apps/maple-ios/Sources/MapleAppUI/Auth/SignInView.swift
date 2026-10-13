#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct SignInView: View {
        private enum Mode: String, CaseIterable { case email, signup, guest }
        @Bindable var session: SessionController
        let coordinator: NativeSignInCoordinator
        let onPasswordReset: () -> Void
        @State private var mode = Mode.email
        @State private var email = ""
        @State private var password = ""
        @State private var externalLink: URL?
        @FocusState private var focused: Field?
        private enum Field { case email, password }

        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        MapleWordmark()
                            .frame(width: 164, height: 32)
                            .accessibilityLabel(MapleStrings.text("Maple"))
                            .accessibilityAddTraits(.isHeader)
                        Text(
                            mode == .signup
                                ? MapleStrings.text("Create your account") : MapleStrings.text("Welcome back")
                        )
                        .font(MapleTypography.title)
                        VStack(spacing: 16) {
                            TextField(
                                mode == .guest ? MapleStrings.text("Guest account ID") : MapleStrings.text("Email"),
                                text: $email,
                                prompt: Text(
                                    mode == .guest ? MapleStrings.text("Guest account ID") : MapleStrings.text("Email")
                                ).foregroundStyle(MapleFieldPrompt())
                            )
                            .textContentType(mode == .guest ? .username : .emailAddress)
                            .keyboardType(mode == .guest ? .asciiCapable : .emailAddress)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                            .focused($focused, equals: .email).submitLabel(.next)
                            .onSubmit { focused = .password }.accessibilityIdentifier("auth.identifier")
                            SecureField(
                                MapleStrings.text("Password"), text: $password,
                                prompt: Text(MapleStrings.text("Password")).foregroundStyle(MapleFieldPrompt())
                            )
                            .textContentType(mode == .signup ? .newPassword : .password)
                            .focused($focused, equals: .password).submitLabel(.go)
                            .onSubmit { submit() }.accessibilityIdentifier("auth.password")
                        }
                        .textFieldStyle(.roundedBorder)
                        if mode == .signup {
                            Text("Use at least eight characters.", tableName: "Auth", bundle: .module)
                                .font(.footnote).foregroundStyle(.secondary)
                        }
                        if let issue = session.issue {
                            Text(message(for: issue)).foregroundStyle(MapleRedText()).accessibilityIdentifier(
                                "auth.error")
                        }
                        Button(action: submit) {
                            HStack {
                                Text(
                                    mode == .signup ? MapleStrings.text("Create account") : MapleStrings.text("Sign in")
                                )
                            }.frame(maxWidth: .infinity).padding(.vertical, 6)
                        }
                        .buttonStyle(MaplePrimaryButtonStyle())
                        .disabled(!canSubmit).accessibilityIdentifier("auth.submit")
                        if mode != .guest {
                            Button(MapleStrings.text("Forgot password?"), action: onPasswordReset)
                                .disabled(session.phase != .signedOut).accessibilityIdentifier("auth.forgot")
                            NativeSignInButtons(coordinator: coordinator)
                                .disabled(session.phase != .signedOut && !coordinator.isRunning)
                        }
                        Button(
                            mode == .signup
                                ? MapleStrings.text("Already have an account? Sign in")
                                : MapleStrings.text("Create an account")
                        ) {
                            mode = mode == .signup ? .email : .signup; password = ""
                        }.disabled(session.phase == .authenticating)
                        Button(
                            mode == .guest
                                ? MapleStrings.text("Sign in with email")
                                : MapleStrings.text("Use an existing guest account")
                        ) {
                            mode = mode == .guest ? .email : .guest; email = ""; password = ""
                        }.disabled(session.phase == .authenticating)
                        VStack(spacing: 8) {
                            Text(
                                String(
                                    localized: "auth.copyright",
                                    defaultValue:
                                        "© \(String(Calendar(identifier: .gregorian).component(.year, from: Date()))) Maple Privacy Labs Inc.",
                                    table: "Auth", bundle: .module)
                            )
                            .foregroundStyle(.secondary)
                            HStack(spacing: 24) {
                                Button {
                                    externalLink = URL(string: "https://trymaple.ai/terms")
                                } label: {
                                    Text("Terms", tableName: "Auth", bundle: .module)
                                        .frame(minWidth: 44, minHeight: 44)
                                }.accessibilityIdentifier("auth.terms")
                                Button {
                                    externalLink = URL(string: "https://trymaple.ai/privacy")
                                } label: {
                                    Text("Privacy", tableName: "Auth", bundle: .module)
                                        .frame(minWidth: 44, minHeight: 44)
                                }.accessibilityIdentifier("auth.privacy")
                            }
                        }
                        .font(.footnote).frame(maxWidth: .infinity)
                    }
                    .padding(28).frame(maxWidth: 480).frame(maxWidth: .infinity)
                }
                .scrollDismissesKeyboard(.interactively)
            }
            .externalLinks($externalLink)
        }

        private var canSubmit: Bool {
            session.phase == .signedOut && !email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                && !password.isEmpty && (mode != .signup || AuthCeremonies.validPassword(password))
        }

        private func submit() {
            guard canSubmit else { return }
            focused = nil
            let submittedID = email.trimmingCharacters(in: .whitespacesAndNewlines)
            let submittedPassword = password
            Task {
                switch mode {
                case .email: await session.signIn(email: submittedID, password: submittedPassword)
                case .guest: await session.signInGuest(id: submittedID, password: submittedPassword)
                case .signup:
                    await session.signUp(email: submittedID, password: submittedPassword)
                }
                password = ""
            }
        }

        private func message(for issue: SessionIssue) -> String {
            switch issue {
            case .clock(let clock): AttestationClockText.message(clock)
            case .credentials: MapleStrings.text("The account details were not accepted. Try again.")
            case .emailAlreadyRegistered: MapleStrings.text("An account already uses this email address.")
            case .invalidInput: MapleStrings.text("Check the account details and try again.")
            case .connection: MapleStrings.text("We couldn’t connect. Check your connection and try again.")
            case .storage, .cleanup:
                MapleStrings.text("Protected account storage is unavailable. Try again after unlocking.")
            }
        }
    }
#endif
