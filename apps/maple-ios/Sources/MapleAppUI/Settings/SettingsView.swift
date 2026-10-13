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
                    settingsText("A private AI workspace built for secure research and collaboration.")
                        .font(.subheadline).foregroundStyle(.secondary)
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
                    if let name = status.productName {
                        Text(
                            String(
                                localized: "settings.plan-name", defaultValue: "\(name) Plan",
                                table: "Settings", bundle: .module)
                        )
                        .accessibilityIdentifier("settings.plan-name")
                    } else {
                        Text(verbatim: "—").foregroundStyle(.secondary)
                    }
                } label: {
                    settingsText("Current plan")
                }
                if let date = status.currentPeriodEnd?.date {
                    LabeledContent {
                        Text(date, format: .dateTime.year().month().day())
                    } label: {
                        settingsText(
                            status.paymentProvider == "subscription_pass" || status.paymentProvider == "zaprite"
                                ? "Expires on" : "Renews on")
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
                settingsText(preferences.value.chatFont.settingsDescription)
                    .font(.footnote).foregroundStyle(.secondary)
                    .accessibilityIdentifier("settings.font-description")
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
                        "Your web search preference is saved. Web search requires an eligible paid plan."
                    )
                    .font(.footnote).foregroundStyle(.secondary)
                }
                Picker(selection: Binding(get: { preferences.value.voice }, set: { preferences.setVoice($0) })) {
                    ForEach(["Default voices", "Reference accents"], id: \.self) { group in
                        Section {
                            ForEach(MapleVoice.allCases.filter { $0.settingsGroup == group }, id: \.self) {
                                settingsText($0.settingsLabel).tag($0)
                            }
                        } header: {
                            settingsText(group)
                        }
                    }
                } label: {
                    settingsText("Read aloud voice")
                }
                .accessibilityIdentifier("settings.voice")
                settingsText(
                    "Any voice can read any supported language; your text determines the spoken language. Matching the voice accent to the text generally sounds most natural."
                )
                .font(.footnote).foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.voice-description")
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
        private let projectRow: AnyView?
        private let topContentMargin: CGFloat
        private let canInteract: @MainActor () -> Bool
        private let onSelection: (String) -> Void
        @State private var webSearchNoticePresented = false
        @State private var modelNotice: ModelNotice?

        private struct ModelNotice: Identifiable {
            let id = UUID()
            let name: String
        }

        public init(
            entitlements: AccountEntitlements, webSearchPreference: Binding<Bool>? = nil,
            webSearchPreferences: PreferencesController? = nil, projectRow: AnyView? = nil,
            topContentMargin: CGFloat = 24,
            canInteract: @escaping @MainActor () -> Bool = { true }, onSelection: @escaping (String) -> Void
        ) {
            self.entitlements = entitlements; self.webSearchPreference = webSearchPreference
            self.webSearchPreferences = webSearchPreferences; self.projectRow = projectRow
            self.topContentMargin = topContentMargin
            self.canInteract = canInteract; self.onSelection = onSelection
        }

        public var body: some View {
            List {
                if hasOptionsGroup {
                    Section {
                        if let projectRow {
                            projectRow.padding(.top, 8)
                                .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
                                .listRowSeparator(.hidden)
                        }
                        if let webSearchPreference {
                            Toggle(isOn: webSearchPreference) {
                                HStack(spacing: 12) {
                                    Image(systemName: "globe")
                                        .foregroundStyle(MapleAccent.foreground)
                                        .accessibilityHidden(true)
                                    settingsText("Web search")
                                }
                                .font(MapleTypography.button)
                            }
                            .modifier(OptionRowSpacing(pairedWithProject: projectRow != nil))
                            .listRowSeparator(.hidden)
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
                    .listRowSeparator(.hidden)
                    .listSectionSeparator(.hidden)
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
                    if !entitlements.primaryModels.isEmpty {
                        Section {
                            ForEach(entitlements.primaryModels) { modelRow($0) }
                        } header: {
                            modelSectionHeader("Recommended", separated: hasOptionsGroup)
                        }
                        .listSectionSeparator(.hidden)
                    }
                    if !entitlements.advancedModels.isEmpty {
                        Section {
                            ForEach(entitlements.advancedModels) { modelRow($0) }
                        } header: {
                            modelSectionHeader(
                                "All models", separated: hasOptionsGroup || !entitlements.primaryModels.isEmpty)
                        }
                        .listSectionSeparator(.hidden)
                    }
                    if entitlements.primaryModels.isEmpty && entitlements.advancedModels.isEmpty {
                        settingsText("No compatible models are currently available.")
                    }
                }
            }
            .listStyle(.insetGrouped)
            .listRowSeparator(.hidden)
            .listSectionSpacing(16)
            .contentMargins(.top, topContentMargin, for: .scrollContent)
            .contentMargins(.bottom, 12, for: .scrollContent)
            .listRowBackground(MapleSurface.card)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .disabled(!canInteract())
            .accessibilityIdentifier("settings.models")
            .sheet(isPresented: $webSearchNoticePresented) {
                MediaFeatureNotice(feature: .webSearch, account: entitlements)
            }
            .sheet(item: $modelNotice) { notice in
                MediaFeatureNotice(feature: .model, account: entitlements, modelName: notice.name)
            }
        }

        private var hasOptionsGroup: Bool { projectRow != nil || webSearchPreference != nil }

        private struct OptionRowSpacing: ViewModifier {
            let pairedWithProject: Bool
            func body(content: Content) -> some View {
                if pairedWithProject {
                    content.frame(minHeight: 44).padding(.bottom, 8)
                        .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
                } else {
                    content.padding(.vertical, 8)
                }
            }
        }

        private func modelSectionHeader(_ title: String, separated: Bool) -> some View {
            VStack(alignment: .leading, spacing: 12) {
                if separated {
                    Divider().overlay(MapleSurface.border).accessibilityHidden(true)
                }
                settingsText(title)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                    .textCase(nil)
            }
            .padding(.top, 12)
            .padding(.bottom, 8)
        }

        private func modelRow(_ model: AccountModelChoice) -> some View {
            Button {
                guard canInteract() else { return }
                if entitlements.selectModel(model.id) == .allowed {
                    onSelection(model.id)
                } else {
                    let name = entitlements.advancedModels.first { $0.id == model.targetModel }?.label ?? model.label
                    modelNotice = ModelNotice(name: name)
                }
            } label: {
                HStack(spacing: 16) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(verbatim: model.label)
                            .font(model.isAlias ? .title3.weight(.semibold) : MapleTypography.button)
                            .foregroundStyle(.primary)
                        let badges = model.badges.filter {
                            $0 != "Pro" && $0.lowercased() != model.accessTier.lowercased()
                        }
                        if !badges.isEmpty {
                            HStack(spacing: 6) {
                                ForEach(Array(badges.enumerated()), id: \.offset) { _, badge in
                                    Text(verbatim: badge)
                                        .font(.caption2.weight(.medium))
                                        .foregroundStyle(.secondary)
                                        .padding(.horizontal, 8).padding(.vertical, 4)
                                        .background(MapleSurface.canvas, in: Capsule())
                                }
                            }
                        }
                        if let description = model.description, !description.isEmpty {
                            Text(verbatim: description)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    Spacer(minLength: 8)
                    if entitlements.modelAccess(model) != .allowed {
                        Image(systemName: "lock").foregroundStyle(.secondary)
                    }
                    if entitlements.selectedModelID == model.id {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.title3)
                            .symbolRenderingMode(.hierarchical)
                            .foregroundStyle(MapleAccent.foreground)
                    }
                }
                .frame(minHeight: 48).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 12, leading: 24, bottom: 12, trailing: 24))
            .accessibilityAddTraits(entitlements.selectedModelID == model.id ? .isSelected : [])
            .accessibilityValue(
                entitlements.modelAccess(model) == .allowed
                    ? Text(verbatim: "") : settingsText("Unavailable for your current account")
            )
            .accessibilityIdentifier("settings.model.\(model.id)")
        }
    }
#endif
