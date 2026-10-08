import Foundation
import MapleSDK
@testable import MapleAppCore

func accountConfiguration() throws -> AppConfiguration {
    try AppConfiguration(
        profile: .local, apiURL: "http://127.0.0.1:11001", billingURL: "http://127.0.0.1:11002",
        flagsURL: "http://127.0.0.1:11003", authURL: "http://127.0.0.1:11004",
        clientID: "11111111-1111-4111-8111-111111111111", callbackScheme: "cloud.opensecret.maple.native.dev")
}

func accountScope() -> AccountScope { AccountScope(apiOrigin: "http://127.0.0.1:11001", principalID: "fixture") }

func accountDecode<Value: Decodable>(_ value: [String: Any], as: Value.Type = Value.self) throws -> Value {
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(Value.self, from: JSONSerialization.data(withJSONObject: value))
}

func billingBody(plan: String? = "Pro", canChat: Bool = true, remaining: Int? = nil) throws -> Data {
    try JSONSerialization.data(withJSONObject: [
        "is_subscribed": plan != "Free", "product_id": "fixture-plan", "product_name": plan as Any? ?? NSNull(),
        "subscription_status": "active", "current_period_end": NSNull(), "can_chat": canChat,
        "chats_remaining": remaining as Any? ?? NSNull(), "payment_provider": NSNull(),
        "total_tokens": NSNull(), "used_tokens": NSNull(), "usage_reset_date": NSNull(),
        "ios_iap_enabled": true, "ios_us_external_link_enabled": true,
    ])
}

func billingStatus(plan: String? = "Pro", canChat: Bool = true, remaining: Int? = nil) throws -> BillingStatus {
    let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(BillingStatus.self, from: billingBody(plan: plan, canChat: canChat, remaining: remaining))
}

func basicTeam() throws -> TeamStatus {
    try accountDecode(["has_team_subscription": false, "team_created": false])
}

func catalogItem(
    _ id: String = "quick-target", access: String = "free", tools: Bool = true,
    enabled: Bool = true, deprecated: Bool = false, chat: Bool = true, order: Int = 0
) -> ModelCatalogItem {
    ModelCatalogItem(
        id: id, object: "model", created: 0, ownedBy: "fixture", displayName: "Model \(id)",
        shortName: id, contextWindow: 131_072, maxContextTokens: 131_072, access: access,
        capabilities: ModelCapabilities(chat: chat, vision: false, reasoning: true, toolUse: tools),
        enabled: enabled, deprecated: deprecated, sortOrder: order)
}

func accountCatalog(items: [ModelCatalogItem]? = nil, quickTarget: String = "quick-target") -> ModelCatalog {
    let capabilities = ModelCapabilities(chat: true, vision: false, reasoning: true, toolUse: true)
    return ModelCatalog(
        object: "list", data: items ?? [catalogItem(), catalogItem("power-target", access: "pro")],
        aliases: [
            ModelAlias(
                id: .quick, label: "Quick", shortName: "Quick", description: "Fast",
                targetModel: quickTarget, access: "free", capabilities: capabilities),
            ModelAlias(
                id: .powerful, label: "Powerful", shortName: "Powerful", description: "Deep",
                targetModel: "power-target", access: "pro", capabilities: capabilities),
        ],
        defaults: ModelDefaults(quick: .quick, powerful: .powerful),
        audio: AudioModelCatalog(
            transcription: AudioModelAvailability(available: true, model: "whisper-large-v3"),
            speech: AudioModelAvailability(available: true, model: "voxtral-tts")))
}

actor AccountGate {
    private var open = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var entryWaiters: [CheckedContinuation<Void, Never>] = []
    private(set) var entries = 0
    func wait() async {
        entries += 1
        let entered = entryWaiters; entryWaiters.removeAll()
        entered.forEach { $0.resume() }
        if !open { await withCheckedContinuation { waiters.append($0) } }
    }
    /// Synchronize with service entry rather than assuming a yield schedules it.
    func waitUntilEntered() async {
        if entries > 0 { return }
        await withCheckedContinuation { entryWaiters.append($0) }
    }
    func release() {
        open = true
        let pending = waiters; waiters.removeAll()
        pending.forEach { $0.resume() }
    }
}

actor AccountTokenStub: BillingTokenSource {
    private(set) var audiences: [String] = []
    let gate: AccountGate?
    let failure: AccountServiceError?
    init(gate: AccountGate? = nil, failure: AccountServiceError? = nil) { self.gate = gate; self.failure = failure }
    func mintToken(audience: String) async throws -> String {
        audiences.append(audience)
        let index = audiences.count
        if let gate { await gate.wait() }
        if let failure { throw failure }
        return "fixture-token-\(index)"
    }
}

actor AccountHTTPStub: BillingHTTP {
    struct Call: Sendable { let url: URL; let token: String }
    private(set) var calls: [Call] = []
    let handler: @Sendable (URL, String, Int) async throws -> BillingHTTPResponse
    init(handler: @escaping @Sendable (URL, String, Int) async throws -> BillingHTTPResponse) { self.handler = handler }
    func get(url: URL, bearerToken: String) async throws -> BillingHTTPResponse {
        calls.append(Call(url: url, token: bearerToken))
        return try await handler(url, bearerToken, calls.count)
    }
}

actor AccountServingStub: BillingServing, ModelCatalogServing {
    var statuses: [Result<BillingStatus, AccountServiceError>]
    var teams: [Result<TeamStatus, AccountServiceError>]
    var catalogs: [Result<ModelCatalog, AccountServiceError>]
    let billingGates: [Int: AccountGate]
    let catalogGates: [Int: AccountGate]
    private(set) var statusCalls = 0
    private(set) var teamCalls = 0
    private(set) var catalogCalls = 0
    private(set) var retired = false
    init(
        statuses: [Result<BillingStatus, AccountServiceError>],
        teams: [Result<TeamStatus, AccountServiceError>],
        catalogs: [Result<ModelCatalog, AccountServiceError>] = [.success(accountCatalog())],
        billingGates: [Int: AccountGate] = [:], catalogGates: [Int: AccountGate] = [:]
    ) {
        self.statuses = statuses; self.teams = teams; self.catalogs = catalogs
        self.billingGates = billingGates; self.catalogGates = catalogGates
    }
    func subscriptionStatus() async throws -> BillingStatus {
        statusCalls += 1
        let value = statuses[min(statusCalls - 1, statuses.count - 1)]
        if let gate = billingGates[statusCalls] { await gate.wait() }
        return try value.get()
    }
    func teamStatus() throws -> TeamStatus {
        teamCalls += 1
        return try teams[min(teamCalls - 1, teams.count - 1)].get()
    }
    func fetchCatalog() async throws -> ModelCatalog {
        catalogCalls += 1
        let value = catalogs[min(catalogCalls - 1, catalogs.count - 1)]
        if let gate = catalogGates[catalogCalls] { await gate.wait() }
        return try value.get()
    }
    func retire() { retired = true }
}
