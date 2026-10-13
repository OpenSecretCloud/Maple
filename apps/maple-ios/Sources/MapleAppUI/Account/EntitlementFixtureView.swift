#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI

    /// Synthetic, in-memory entitlement fixture. Uses shipping account, plan,
    /// model, editor and explanation views; it never constructs a network client.
    struct EntitlementFixtureView: View {
        @State private var model = EntitlementFixtureModel()
        @State private var focused = false
        @State private var media: MediaFeature?
        @State private var showModels = false
        @State private var showSettings = false

        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Menu("Synthetic account state") {
                            ForEach(EntitlementScenario.allCases) { scenario in
                                Button(scenario.rawValue) { Task { await model.apply(scenario) } }
                                    .accessibilityIdentifier("entitlement.scenario." + scenario.rawValue)
                            }
                        }.accessibilityIdentifier("entitlement.scenarios")
                        Button("Recover service without refreshing") { Task { await model.prepareRetryRecovery() } }
                            .accessibilityIdentifier("entitlement.prepare-retry")
                        Text(model.retryPrepared ? "service ready; account unchanged" : "not prepared")
                            .accessibilityIdentifier("entitlement.retry-service")
                        Text(model.billingLabel).accessibilityIdentifier("entitlement.billing")
                        Text(model.scenario.rawValue).accessibilityIdentifier("entitlement.scenario")
                        if model.account.chatAccess != .allowed {
                            AccountAccessNotice(account: model.account)
                                .accessibilityIdentifier("entitlement.notice")
                        }
                        ComposerEditor(
                            text: $model.draft.text, focused: $focused, ownerID: model.draft.id,
                            accountGeneration: model.account.scope.generation,
                            placeholder: "Synthetic unsent draft", accessibilityIdentifier: "entitlement.composer",
                            onSubmit: {}
                        )
                        .frame(minHeight: 72).padding(8).background(.secondary.opacity(0.1))
                        Button("Send") {}
                            .disabled(model.account.chatAccess != .allowed || model.draft.isEmpty)
                            .frame(minWidth: 44, minHeight: 44).accessibilityIdentifier("entitlement.send")
                        Button(model.account.selectedModel?.label ?? "Models") { showModels = true }
                            .frame(minWidth: 44, minHeight: 44).accessibilityIdentifier("entitlement.models")
                        Text(model.account.selectedModelID).accessibilityIdentifier("entitlement.selected-model")
                        ForEach([MediaFeature.images, .dictation, .speech, .webSearch]) { feature in
                            Button {
                                media = feature
                            } label: {
                                HStack {
                                    Text(feature.title)
                                    Spacer()
                                    Image(
                                        systemName: feature.access(in: model.account) == .allowed
                                            ? "checkmark" : "info.circle")
                                }
                            }
                            .frame(minHeight: 44)
                            .accessibilityValue(
                                feature.access(in: model.account) == .allowed ? "available" : "unavailable"
                            )
                            .accessibilityIdentifier("entitlement.media." + feature.rawValue)
                        }
                        Text(model.account.webSearchPreferred ? "Web preference on" : "Web preference off")
                            .accessibilityIdentifier("entitlement.web-preference")
                        Text(model.account.effectiveWebSearch ? "Effective web on" : "Effective web off")
                            .accessibilityIdentifier("entitlement.web-effective")
                        Button("Read-only account settings") { showSettings = true }
                            .frame(minHeight: 44).accessibilityIdentifier("entitlement.settings")
                    }.padding()
                }
                .navigationTitle("Synthetic account")
                .sheet(item: $media) { MediaFeatureNotice(feature: $0, account: model.account) }
                .sheet(isPresented: $showModels) {
                    NavigationStack {
                        SettingsModelPicker(entitlements: model.account) { _ in showModels = false }
                            .toolbar {
                                ToolbarItem(placement: .confirmationAction) {
                                    Button("Done") { showModels = false }.accessibilityIdentifier(
                                        "entitlement.models.close")
                                }
                            }
                    }
                }
                .sheet(isPresented: $showSettings) {
                    NavigationStack {
                        SettingsView(
                            preferences: model.preferences, entitlements: model.account,
                            account: SettingsAccount(
                                email: model.account.isGuest ? nil : "synthetic@example.test", isVerified: true,
                                isGuest: model.account.isGuest, canChangePassword: false),
                            version: "Synthetic fixture", onAction: { _ in }
                        )
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) {
                                Button("Done") { showSettings = false }.accessibilityIdentifier(
                                    "entitlement.settings.close")
                            }
                        }
                    }
                }
            }
        }
    }

    private enum EntitlementScenario: String, CaseIterable, Identifiable, Sendable {
        case loading, failed, free, pro, pass, zaprite, seat, usage, unknownPlan
        var id: String { rawValue }
    }

    @MainActor @Observable private final class EntitlementFixtureModel {
        let account: AccountEntitlements
        let preferences: PreferencesController
        let service: EntitlementFixtureService
        var scenario: EntitlementScenario = .loading
        var draft = ChatDraft(text: "Keep this unsent draft")
        var retryPrepared = false

        init() {
            let scope = AccountScope(apiOrigin: "http://127.0.0.1:1", principalID: "synthetic-entitlement")
            let service = EntitlementFixtureService()
            self.service = service
            account = AccountEntitlements(
                scope: scope, isGuest: ProcessInfo.processInfo.arguments.contains("--maple-entitlement-guest"),
                billing: service, catalog: service)
            preferences = PreferencesController(scope: scope, store: EntitlementFixturePreferences())
        }
        var billingLabel: String {
            switch account.billing {
            case .unknown: "unknown"
            case .loading: "loading"
            case .failed: "failed"
            case let .loaded(status):
                switch status.plan {
                case .unknown: "unknownPlan"
                case .free: "Free"
                case .paid: "paid"
                }
            }
        }
        func prepareRetryRecovery() async {
            // Only the synthetic backend recovers. The loaded account stays in
            // its failure state until the shipping Retry control fetches again.
            await service.configure(.pro)
            retryPrepared = true
        }
        func apply(_ scenario: EntitlementScenario) async {
            self.scenario = scenario; retryPrepared = false
            await service.configure(scenario)
            await account.refresh()
        }
    }

    private actor EntitlementFixtureService: BillingServing, ModelCatalogServing {
        private var scenario: EntitlementScenario = .loading
        private var waiters: [CheckedContinuation<Void, Never>] = []
        func configure(_ scenario: EntitlementScenario) {
            self.scenario = scenario
            if scenario != .loading {
                let pending = waiters; waiters.removeAll()
                pending.forEach { $0.resume() }
            }
        }
        func subscriptionStatus() async throws -> BillingStatus {
            if scenario == .loading { await withCheckedContinuation { waiters.append($0) } }
            if scenario == .failed { throw AccountServiceError.unavailable }
            let plan =
                scenario == .free ? "Free" : scenario == .unknownPlan ? "Unrecognized fixture plan" : "Pro Annual"
            return try decode([
                "is_subscribed": scenario != .free, "product_id": "synthetic",
                "product_name": plan, "subscription_status": "active", "can_chat": scenario != .usage,
                "chats_remaining": scenario == .usage ? 0 : 50,
                "current_period_end": 1_900_000_000,
                "payment_provider": scenario == .pass
                    ? "subscription_pass" : scenario == .zaprite ? "zaprite" : "stripe",
                // Acquisition fields are intentionally inert input to the shipping decoder.
                "ios_iap_enabled": true, "ios_us_external_link_enabled": true,
            ])
        }
        func teamStatus() throws -> TeamStatus {
            try decode([
                "has_team_subscription": scenario == .seat, "team_created": scenario == .seat,
                "members_count": 3, "billed_seat_count": scenario == .seat ? 2 : 3,
            ])
        }
        func fetchCatalog() -> ModelCatalog {
            func item(_ id: String, label: String, paid: Bool = false, media: Bool = true, enabled: Bool = true)
                -> ModelCatalogItem
            {
                ModelCatalogItem(
                    id: id, object: "model", created: 0, ownedBy: "synthetic", displayName: label,
                    shortName: label, contextWindow: 32768, maxContextTokens: 32768, access: paid ? "pro" : "free",
                    capabilities: ModelCapabilities(chat: true, vision: media, reasoning: true, toolUse: media),
                    badges: paid ? ["Pro", "Reasoning", "New"] : ["free"],
                    enabled: enabled, deprecated: false)
            }
            let capabilities = ModelCapabilities(chat: true, vision: false, reasoning: true, toolUse: false)
            return ModelCatalog(
                object: "list",
                data: [
                    item("synthetic-quick", label: "Quick target"),
                    item("synthetic-power", label: "Power target", paid: true),
                    item("synthetic-text", label: "Text only", media: false),
                    item("synthetic-disabled", label: "Disabled model", enabled: false),
                ],
                aliases: [
                    ModelAlias(
                        id: .quick, label: "Quick", shortName: "Quick", description: "Synthetic quick model",
                        targetModel: "synthetic-quick", access: "free",
                        capabilities: capabilities),
                    ModelAlias(
                        id: .powerful, label: "Powerful", shortName: "Powerful", description: "Synthetic paid model",
                        targetModel: "synthetic-power",
                        access: "pro", capabilities: capabilities),
                ], defaults: ModelDefaults(quick: .quick, powerful: .powerful),
                audio: AudioModelCatalog(
                    transcription: AudioModelAvailability(available: true, model: "synthetic-transcription"),
                    speech: AudioModelAvailability(available: true, model: "synthetic-speech")))
        }
        func retire() { configure(.failed) }
        private func decode<T: Decodable>(_ value: [String: Any]) throws -> T {
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(T.self, from: JSONSerialization.data(withJSONObject: value))
        }
    }

    private actor EntitlementFixturePreferences: PreferencesStoring {
        private var value = LocalPreferences()
        func activate(_ scope: AccountScope) -> LocalPreferences { value }
        func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) { self.value = value }
        func retire(_ scope: AccountScope, removePersisted: Bool) {}
    }
#endif
