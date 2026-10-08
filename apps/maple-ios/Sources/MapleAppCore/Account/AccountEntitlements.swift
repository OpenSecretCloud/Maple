import Foundation
import MapleSDK
import Observation

public enum AccountAccess: Equatable, Sendable {
    case allowed
    case unavailable, unknownPlan, freeGuest, usageLimit, seatMismatch
    case paidPlanRequired, modelUnavailable, modelIncompatible, retired
}

public struct AccountModelChoice: Equatable, Sendable, Identifiable {
    /// Alias IDs remain aliases in requests; targetModel is presentation metadata.
    public let id: String
    public let targetModel: String
    public let label: String
    public let shortName: String
    public let description: String?
    public let contextWindow: Int
    public let accessTier: String
    public let capabilities: ModelCapabilities
    public let badges: [String]
    public let isAlias: Bool
    public let sortOrder: Int
}

/// Read-only entitlement state for exactly one app session. It never changes SDK
/// authentication and does not execute purchase, management or feature-flag actions.
@Observable @MainActor public final class AccountEntitlements {
    public let scope: AccountScope
    public let isGuest: Bool
    public private(set) var billing: AccountLoadState<BillingStatus> = .unknown
    public private(set) var team: AccountLoadState<TeamStatus> = .unknown
    public private(set) var catalog: AccountLoadState<ModelCatalog> = .unknown
    public private(set) var selectedModelID: String
    /// A gated preference is retained; effectiveWebSearch is the dispatch value.
    public var webSearchPreferred: Bool
    private var active = true
    private var billingRequest = UUID()
    private var teamRequest = UUID()
    private var catalogRequest = UUID()
    private let billingService: any BillingServing
    private let catalogService: any ModelCatalogServing

    public init(
        scope: AccountScope, isGuest: Bool, billing: any BillingServing,
        catalog: any ModelCatalogServing, selectedModelID: String = ModelAliasId.quick.rawValue,
        webSearchPreferred: Bool = true
    ) {
        self.scope = scope
        self.isGuest = isGuest
        self.billingService = billing
        self.catalogService = catalog
        self.selectedModelID = selectedModelID
        self.webSearchPreferred = webSearchPreferred
    }

    public var plan: BillingPlan { billing.value?.plan ?? .unknown }

    public func refresh() async {
        guard active else { return }
        async let status: Void = refreshBilling()
        async let seats: Void = refreshTeam()
        async let models: Void = refreshCatalog()
        _ = await (status, seats, models)
    }

    public func refreshBilling() async {
        guard active else { return }
        let request = UUID()
        billingRequest = request
        // Keep only this generation's currently verified value while refreshing.
        // A failure replaces it; a later retry cannot recover a discarded value.
        if billing.value == nil { billing = .loading }
        do {
            let status = try await billingService.subscriptionStatus()
            try status.validate()
            guard active, billingRequest == request else { return }
            billing = .loaded(status)
        } catch {
            guard active, billingRequest == request else { return }
            billing = .failed(BillingService.safe(error))
        }
    }

    public func refreshTeam() async {
        guard active else { return }
        let request = UUID()
        teamRequest = request
        if team.value == nil { team = .loading }
        do {
            let status = try await billingService.teamStatus()
            try status.validate()
            guard active, teamRequest == request else { return }
            team = .loaded(status)
        } catch {
            guard active, teamRequest == request else { return }
            team = .failed(BillingService.safe(error))
        }
    }

    public func refreshCatalog() async {
        guard active else { return }
        let request = UUID()
        catalogRequest = request
        if catalog.value == nil { catalog = .loading }
        do {
            let result = try await catalogService.fetchCatalog()
            guard result.object == "list", result.data.count <= 1_024,
                result.aliases.count <= 16
            else { throw AccountServiceError.invalidResponse }
            guard active, catalogRequest == request else { return }
            catalog = .loaded(result)
            // A successful catalog may remove or disable a concrete model. Keep
            // explicit aliases stable so the server can change their targets.
            if ModelAliasId(rawValue: selectedModelID) == nil,
                !advancedModels.contains(where: { $0.id == selectedModelID })
            {
                selectedModelID = ModelAliasId.quick.rawValue
            }
        } catch {
            guard active, catalogRequest == request else { return }
            catalog = .failed(BillingService.safe(error))
        }
    }

    /// Call before retiring the captured SDK account. Late mint, HTTP and catalog
    /// completions cannot publish into this model or a replacement generation.
    public func retire() async {
        active = false
        billingRequest = UUID(); teamRequest = UUID(); catalogRequest = UUID()
        billing = .unknown; team = .unknown; catalog = .unknown
        await billingService.retire()
    }

    public var primaryModels: [AccountModelChoice] {
        guard let catalog = catalog.value else { return [] }
        return [ModelAliasId.quick, .powerful].compactMap { id in
            guard let alias = catalog.aliases.first(where: { $0.id == id }),
                let target = eligibleCatalogModels.first(where: { $0.id == alias.targetModel })
            else { return nil }
            return AccountModelChoice(
                id: alias.id.rawValue, targetModel: target.id, label: alias.label,
                shortName: alias.shortName, description: alias.description,
                contextWindow: target.contextWindow, accessTier: target.access,
                capabilities: target.capabilities, badges: target.badges ?? [],
                isAlias: true, sortOrder: id == .quick ? 0 : 1)
        }
    }

    public var advancedModels: [AccountModelChoice] {
        eligibleCatalogModels.map { item in
            AccountModelChoice(
                id: item.id, targetModel: item.id, label: item.displayName,
                shortName: item.shortName, description: item.description,
                contextWindow: item.contextWindow, accessTier: item.access,
                capabilities: item.capabilities, badges: item.badges ?? [],
                isAlias: false, sortOrder: item.sortOrder ?? 999)
        }.sorted { lhs, rhs in
            let leftAllowed = modelAccess(lhs) == .allowed
            let rightAllowed = modelAccess(rhs) == .allowed
            if leftAllowed != rightAllowed { return leftAllowed }
            if lhs.sortOrder != rhs.sortOrder { return lhs.sortOrder < rhs.sortOrder }
            return lhs.id < rhs.id
        }
    }

    private var eligibleCatalogModels: [ModelCatalogItem] {
        guard let catalog = catalog.value else { return [] }
        var seen = Set<String>()
        return catalog.data.filter {
            seen.insert($0.id).inserted && !$0.id.isEmpty && ModelAliasId(rawValue: $0.id) == nil
                && $0.enabled && !$0.deprecated && $0.capabilities.chat && $0.contextWindow > 0
        }
    }

    public var selectedModel: AccountModelChoice? {
        (primaryModels + advancedModels).first { $0.id == selectedModelID }
    }

    public func modelAccess(_ model: AccountModelChoice) -> AccountAccess {
        guard active else { return .retired }
        guard billing.value != nil else { return .unavailable }
        guard plan != .unknown else { return .unknownPlan }
        switch model.accessTier {
        case "free": return .allowed
        case "starter", "pro": return plan.isPaid ? .allowed : .paidPlanRequired
        default: return .modelUnavailable
        }
    }

    @discardableResult public func selectModel(_ id: String) -> AccountAccess {
        guard let choice = (primaryModels + advancedModels).first(where: { $0.id == id }) else {
            return .modelUnavailable
        }
        let access = modelAccess(choice)
        if access == .allowed { selectedModelID = id }
        return access
    }

    private var accountChatAccess: AccountAccess {
        guard active else { return .retired }
        guard let status = billing.value else { return .unavailable }
        guard status.plan != .unknown else { return .unknownPlan }
        if isGuest, status.plan == .free { return .freeGuest }
        if team.value?.seatMismatch != nil { return .seatMismatch }
        guard status.canChat, status.chatsRemaining != 0 else { return .usageLimit }
        return .allowed
    }

    public var chatAccess: AccountAccess {
        let account = accountChatAccess
        guard account == .allowed else { return account }
        guard let selectedModel else { return .modelUnavailable }
        return modelAccess(selectedModel)
    }

    public var paidFeatureAccess: AccountAccess {
        let chat = accountChatAccess
        guard chat == .allowed else { return chat }
        return plan.isPaid ? .allowed : .paidPlanRequired
    }

    public var webSearchAccess: AccountAccess {
        let paid = paidFeatureAccess
        guard paid == .allowed else { return paid }
        guard catalog.value != nil else { return .unavailable }
        guard let selectedModel else { return .modelUnavailable }
        let model = modelAccess(selectedModel)
        guard model == .allowed else { return model }
        return selectedModel.capabilities.toolUse ? .allowed : .modelIncompatible
    }

    public var effectiveWebSearch: Bool { webSearchPreferred && webSearchAccess == .allowed }
    public var imageAccess: AccountAccess {
        let paid = paidFeatureAccess
        guard paid == .allowed else { return paid }
        guard catalog.value != nil else { return .unavailable }
        guard let selectedModel else { return .modelUnavailable }
        let model = modelAccess(selectedModel)
        guard model == .allowed else { return model }
        return selectedModel.capabilities.vision ? .allowed : .modelIncompatible
    }

    public var transcriptionAccess: AccountAccess {
        let paid = paidFeatureAccess
        guard paid == .allowed else { return paid }
        guard let transcription = catalog.value?.audio?.transcription else { return .unavailable }
        return transcription.available ? .allowed : .modelIncompatible
    }

    public var transcriptionAvailable: Bool {
        transcriptionAccess == .allowed
    }
    public var speechAvailable: Bool {
        speechAccess == .allowed
    }
    public var speechAccess: AccountAccess {
        guard active else { return .retired }
        guard let status = billing.value, status.plan != .unknown else { return .unavailable }
        guard status.plan.isPaid else { return .paidPlanRequired }
        let paid = paidFeatureAccess
        guard paid == .allowed else { return paid }
        guard let speech = catalog.value?.audio?.speech else { return .unavailable }
        return speech.available ? .allowed : .modelIncompatible
    }
}
