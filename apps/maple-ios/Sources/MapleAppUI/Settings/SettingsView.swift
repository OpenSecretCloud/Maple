#if os(iOS)
    import Foundation
    import MapleAppCore
    import SwiftUI

    public struct SettingsAccount: Equatable {
        public let email: String?
        public let isVerified: Bool
        public let isGuest: Bool
        public let canChangePassword: Bool

        public init(email: String?, isVerified: Bool, isGuest: Bool, canChangePassword: Bool) {
            self.email = email; self.isVerified = isVerified
            self.isGuest = isGuest; self.canChangePassword = canChangePassword
        }
    }

    /// Navigation and destructive workflows stay in the account-session owner.
    /// External URLs must pass its confirmation/browser policy before opening.
    public enum SettingsAction {
        case accountInstructions, verifyEmail, changePassword, resetPassword
        case deleteAllHistory, deleteAccount, signOut, externalURL(URL)
    }

    public struct SettingsView: View {
        @Bindable private var preferences: PreferencesController
        @Bindable private var entitlements: AccountEntitlements
        private let account: SettingsAccount
        private let version: String
        private let onAction: (SettingsAction) -> Void

        public init(
            preferences: PreferencesController, entitlements: AccountEntitlements,
            account: SettingsAccount, version: String, onAction: @escaping (SettingsAction) -> Void
        ) {
            self.preferences = preferences; self.entitlements = entitlements
            self.account = account; self.version = version; self.onAction = onAction
        }

        public var body: some View {
            List {
                accountSection
                Section {
                    planRow
                    Button {
                        Task {
                            await entitlements.refreshBilling(); await entitlements.refreshTeam()
                        }
                    } label: {
                        settingsText("Refresh account status")
                    }
                } header: {
                    settingsText("Plan")
                }
                preferencesSection
                Section {
                    Button {
                        onAction(.accountInstructions)
                    } label: {
                        settingsText("Default instructions")
                    }
                    if account.canChangePassword {
                        Button {
                            onAction(.changePassword)
                        } label: {
                            settingsText("Change password")
                        }
                    }
                    if account.email != nil {
                        Button {
                            onAction(.resetPassword)
                        } label: {
                            settingsText("Reset password")
                        }
                    }
                    Button(role: .destructive) {
                        onAction(.deleteAllHistory)
                    } label: {
                        settingsText("Delete all chat history")
                            .foregroundStyle(MapleRedText())
                    }.accessibilityIdentifier("settings.delete-history")
                    Button(role: .destructive) {
                        onAction(.deleteAccount)
                    } label: {
                        settingsText("Delete account")
                            .foregroundStyle(MapleRedText())
                    }.accessibilityIdentifier("settings.delete-account")
                    Button {
                        onAction(.signOut)
                    } label: {
                        settingsText("Sign out")
                    }
                    .accessibilityIdentifier("settings.sign-out")
                } header: {
                    settingsText("Account management")
                }
                Section {
                    LabeledContent {
                        Text(verbatim: version)
                    } label: {
                        settingsText("Version")
                    }
                    externalRow("Privacy policy", "https://trymaple.ai/privacy")
                    externalRow("Terms of service", "https://trymaple.ai/terms")
                    externalRow("Contact support", "mailto:support@trymaple.ai")
                } header: {
                    settingsText("About Maple")
                }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(settingsText("Settings"))
            .task { await preferences.load() }
            .accessibilityIdentifier("settings.screen")
        }

        private var accountSection: some View {
            Section {
                if let email = account.email {
                    Text(verbatim: email).textSelection(.enabled)
                    Label {
                        settingsText(account.isVerified ? "Email verified" : "Email not verified")
                    } icon: {
                        Image(systemName: account.isVerified ? "checkmark.seal" : "envelope.badge")
                    }
                    if !account.isVerified {
                        Button {
                            onAction(.verifyEmail)
                        } label: {
                            settingsText("Verify email")
                        }
                        .accessibilityIdentifier("settings.verify-email")
                    }
                } else {
                    settingsText(account.isGuest ? "Guest account" : "Signed in")
                }

            } header: {
                settingsText("Account")
            }
        }

        @ViewBuilder private var planRow: some View {
            switch entitlements.billing {
            case .unknown, .loading:
                LabeledContent {
                    Text(verbatim: "—").foregroundStyle(.secondary)
                } label: {
                    settingsText("Current plan")
                }
            case .failed:
                settingsText("Your plan could not be loaded. Try refreshing account status.")
            case let .loaded(status):
                LabeledContent {
                    planName(status.plan)
                } label: {
                    settingsText("Current plan")
                }
                if let date = status.currentPeriodEnd?.date {
                    LabeledContent {
                        Text(date, format: .dateTime.year().month().day())
                    } label: {
                        settingsText("Current period ends")
                    }
                }
                if status.plan == .unknown {
                    settingsText(
                        "Plan details are unavailable. Paid features remain unavailable until your plan is known.")
                } else if !status.canChat || status.chatsRemaining == 0 {
                    settingsText("Your account has reached its current chat allowance.")
                }
                if entitlements.team.value?.seatMismatch != nil {
                    settingsText("Your team has more members than paid seats. Contact your team administrator.")
                }
            }
        }

        private var preferencesSection: some View {
            Section {
                switch preferences.state {
                case .idle, .loading:
                    preferenceControls.disabled(true)
                case .failed:
                    settingsText("Saved preferences could not be loaded.")
                    Button {
                        Task { await preferences.load() }
                    } label: {
                        settingsText("Try again")
                    }
                case .retired: settingsText("Sign in again to change preferences.")
                case .ready: preferenceControls
                }
            } header: {
                settingsText("Preferences")
            }
        }

        @ViewBuilder private var preferenceControls: some View {
            Group {
                Picker(selection: Binding(get: { preferences.value.theme }, set: { preferences.setTheme($0) })) {
                    ForEach(MapleTheme.allCases, id: \.self) { settingsText($0.settingsLabel).tag($0) }
                } label: {
                    settingsText("Appearance")
                }
                .accessibilityIdentifier("settings.theme")
                Picker(
                    selection: Binding(get: { preferences.value.chatFont }, set: { preferences.setChatFont($0) })
                ) {
                    ForEach(MapleChatFont.allCases, id: \.self) { settingsText($0.settingsLabel).tag($0) }
                } label: {
                    settingsText("Chat font")
                }
                .accessibilityIdentifier("settings.font")
                NavigationLink {
                    SettingsModelPicker(entitlements: entitlements) { id in preferences.setSelectedModelID(id) }
                } label: {
                    settingsText("Default model")
                }
                Toggle(
                    isOn: Binding(
                        get: { preferences.value.webSearchPreferred },
                        set: {
                            preferences.setWebSearchPreferred($0); entitlements.webSearchPreferred = $0
                        }
                    )
                ) { settingsText("Web search") }
                .disabled(entitlements.webSearchAccess != .allowed)
                .accessibilityIdentifier("settings.web-search")
                if entitlements.webSearchAccess != .allowed {
                    settingsText(
                        "Your web search preference is saved. Web search requires an eligible paid plan and a compatible model."
                    )
                    .font(.footnote).foregroundStyle(.secondary)
                }
                Picker(selection: Binding(get: { preferences.value.voice }, set: { preferences.setVoice($0) })) {
                    ForEach(MapleVoice.allCases, id: \.self) { settingsText($0.settingsLabel).tag($0) }
                } label: {
                    settingsText("Read aloud voice")
                }
                .accessibilityIdentifier("settings.voice")
                Picker(
                    selection: Binding(
                        get: { preferences.value.playbackSpeed }, set: { preferences.setPlaybackSpeed($0) }
                    )
                ) {
                    ForEach(MaplePlaybackSpeed.allCases, id: \.self) { speed in
                        Text(
                            verbatim: "\(speed.rawValue.formatted(.number.precision(.fractionLength(0...2))))×"
                        ).tag(speed)
                    }
                } label: {
                    settingsText("Playback speed")
                }
                .accessibilityIdentifier("settings.speed")
                settingsText("Text size follows your system settings. Autoplay is off each time Maple launches.")
                    .font(.footnote).foregroundStyle(.secondary)
                if preferences.saveIssue != nil {
                    settingsText("This change could not be saved on this device.")
                    Button {
                        preferences.retrySave()
                    } label: {
                        settingsText("Retry saving preferences")
                    }
                }
            }
        }

        private func externalRow(_ title: String, _ address: String) -> some View {
            Button {
                if let url = URL(string: address) { onAction(.externalURL(url)) }
            } label: {
                Label {
                    settingsText(title)
                } icon: {
                    Image(systemName: "arrow.up.right.square")
                }
            }
        }

        private func planName(_ plan: BillingPlan) -> Text {
            switch plan {
            case .unknown: settingsText("Unknown")
            case .free: settingsText("Free")
            case .paid(.pro): settingsText("Pro")
            case .paid(.max): settingsText("Max")
            case .paid(.team): settingsText("Team")
            }
        }
    }

    /// Fixed app-owned keys are maintained in Settings.xcstrings. Constructing
    /// the key here keeps extraction from assigning helper literals to Localizable.
    private func settingsText(_ key: String) -> Text {
        Text(LocalizedStringKey(key), tableName: "Settings", bundle: .module)
    }

    public struct SettingsModelPicker: View {
        @Bindable private var entitlements: AccountEntitlements
        private let webSearchPreference: Binding<Bool>?
        private let webSearchPreferences: PreferencesController?
        private let onSelection: (String) -> Void
        @State private var webSearchNoticePresented = false

        public init(
            entitlements: AccountEntitlements, webSearchPreference: Binding<Bool>? = nil,
            webSearchPreferences: PreferencesController? = nil, onSelection: @escaping (String) -> Void
        ) {
            self.entitlements = entitlements; self.webSearchPreference = webSearchPreference
            self.webSearchPreferences = webSearchPreferences; self.onSelection = onSelection
        }

        public var body: some View {
            List {
                if let webSearchPreference {
                    Section {
                        Toggle(isOn: webSearchPreference) { settingsText("Web search") }
                            .disabled(
                                (webSearchPreferences?.state ?? .ready) != .ready
                                    || entitlements.webSearchAccess != .allowed
                            )
                            .accessibilityIdentifier("model.web-search")
                        if let preferences = webSearchPreferences {
                            switch preferences.state {
                            case .idle, .loading: EmptyView()
                            case .failed:
                                settingsText("Saved preferences could not be loaded.")
                                Button {
                                    Task { [weak preferences] in await preferences?.load() }
                                } label: {
                                    settingsText("Try again")
                                }
                                .accessibilityIdentifier("model.preferences-retry")
                            case .retired: settingsText("Sign in again to change preferences.")
                            case .ready: EmptyView()
                            }
                        }
                        if entitlements.webSearchAccess != .allowed {
                            Button {
                                webSearchNoticePresented = true
                            } label: {
                                Image(systemName: "info.circle")
                            }
                            .frame(minWidth: 44, minHeight: 44)
                            .accessibilityLabel(settingsText("Why is web search unavailable?"))
                            .accessibilityValue(MapleStrings.text("Unavailable"))
                            .accessibilityHint(MapleStrings.text("Learn why this feature is unavailable."))
                            .accessibilityIdentifier("model.web-search-unavailable")
                        }
                    }
                }
                switch entitlements.catalog {
                case .unknown, .loading: EmptyView()
                case .failed:
                    settingsText("Models could not be loaded.")
                    Button {
                        Task { await entitlements.refreshCatalog() }
                    } label: {
                        settingsText("Try again")
                    }
                case .loaded:
                    Section { ForEach(entitlements.primaryModels) { modelRow($0) } }
                    Section {
                        ForEach(entitlements.advancedModels) { modelRow($0) }
                    } header: {
                        settingsText("Advanced models")
                    }
                    if entitlements.primaryModels.isEmpty && entitlements.advancedModels.isEmpty {
                        settingsText("No compatible models are currently available.")
                    }
                }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("settings.models")
            .sheet(isPresented: $webSearchNoticePresented) {
                MediaFeatureNotice(feature: .webSearch, account: entitlements)
            }
        }

        private func modelRow(_ model: AccountModelChoice) -> some View {
            Button {
                if entitlements.selectModel(model.id) == .allowed { onSelection(model.id) }
            } label: {
                HStack {
                    Text(verbatim: model.label).foregroundStyle(.primary)
                    Spacer()
                    if entitlements.modelAccess(model) != .allowed {
                        Image(systemName: "lock").foregroundStyle(.secondary)
                    }
                    if entitlements.selectedModelID == model.id {
                        Image(systemName: "checkmark").foregroundStyle(MapleAccent.foreground)
                    }
                }
                .frame(minHeight: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(entitlements.modelAccess(model) != .allowed)
            .accessibilityAddTraits(entitlements.selectedModelID == model.id ? .isSelected : [])
            .accessibilityValue(
                entitlements.modelAccess(model) == .allowed
                    ? Text(verbatim: "") : settingsText("Unavailable for your current account")
            )
            .accessibilityIdentifier("settings.model.\(model.id)")
        }
    }
#endif
