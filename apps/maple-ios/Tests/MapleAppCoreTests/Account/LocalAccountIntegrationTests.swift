import Foundation
import MapleSDK
import Synchronization
import Testing
@testable import MapleAppCore

private final class AccountLiveMemoryStore: SecureStore, Sendable {
    private let values = Mutex<[String: Data]>([:])
    func read(key: String) -> Data? { values.withLock { $0[key] } }
    func write(_ data: Data, key: String) { values.withLock { $0[key] = data } }
    func remove(key: String) { values.withLock { _ = $0.removeValue(forKey: key) } }
}

/// Explicit opt-in, disposable loopback fixtures only. Provide credentials in the
/// child environment; never pass them as test arguments or use a personal account.
@Suite(.serialized, .enabled(if: ProcessInfo.processInfo.environment["MAPLE_IOS_ACCOUNT_LIVE"] == "1"))
@MainActor struct LocalAccountIntegrationTests {
    @Test(arguments: ["PRO", "FREE"])
    func localBillingAndCatalogUseOwnedThirdPartyBearer(_ role: String) async throws {
        let environment = ProcessInfo.processInfo.environment
        func required(_ name: String) throws -> String {
            guard let value = environment[name], !value.isEmpty else { throw AccountServiceError.unavailable }
            return value
        }
        let config = try AppConfiguration(
            profile: .local, apiURL: required("MAPLE_IOS_ACCOUNT_API"),
            billingURL: required("MAPLE_IOS_ACCOUNT_BILLING"),
            flagsURL: required("MAPLE_IOS_ACCOUNT_FLAGS"), authURL: required("MAPLE_IOS_ACCOUNT_AUTH"),
            clientID: required("MAPLE_IOS_ACCOUNT_CLIENT_ID"), callbackScheme: "cloud.opensecret.maple.native.dev")
        let client = try MapleClient(
            apiURL: config.apiURL.absoluteString, clientID: config.clientID,
            policy: config.trustPolicy, store: AccountLiveMemoryStore())
        do {
            _ = try await client.signIn(
                email: required("MAPLE_IOS_ACCOUNT_\(role)_EMAIL"),
                password: required("MAPLE_IOS_ACCOUNT_\(role)_PASSWORD"))
            let api = try await client.accountAPI()
            let user = try await api.getUser()
            let scope = AccountScope(apiOrigin: config.apiURL.absoluteString, principalID: user.id)
            let billing = BillingService(
                scope: scope, configuration: config, tokenSource: SDKBillingTokenSource(api: api))
            let account = AccountEntitlements(
                scope: scope, isGuest: false, billing: billing, catalog: SDKModelCatalogSource(api: api))
            await account.refresh()
            #expect(account.billing.value != nil)
            #expect(account.team.value != nil)
            #expect(account.catalog.value != nil)
            #expect(!account.primaryModels.isEmpty)
            let originalSelection = account.selectedModelID
            let gemma = try #require(account.advancedModels.first { $0.id == "gemma4-31b" })
            #expect(gemma.accessTier != "free")
            let expectedGemmaAccess: AccountAccess = role == "PRO" ? .allowed : .paidPlanRequired
            #expect(account.modelAccess(gemma) == expectedGemmaAccess)
            #expect(account.selectModel(gemma.id) == expectedGemmaAccess)
            #expect(account.selectedModelID == (role == "PRO" ? gemma.id : originalSelection))
            #expect(account.selectModel(originalSelection) == .allowed)
            #expect(account.selectedModelID == originalSelection)
            if role == "PRO" {
                #expect(account.plan.isPaid)
                #expect(account.chatAccess == .allowed)
                #expect(account.effectiveWebSearch)
            } else {
                #expect(account.plan == .free)
                #expect(account.effectiveWebSearch)
            }
            // SDK credential refresh preserves the captured account handle and
            // is independent of the third-party token cache.
            _ = try await client.refreshCredentials()
            let refreshed = try await billing.subscriptionStatus()
            #expect(refreshed.plan == account.plan)
            await account.retire()
            await #expect(throws: AccountServiceError.sessionChanged) { try await billing.subscriptionStatus() }
            try await client.signOut()
        } catch {
            try? await client.signOut()
            // Never surface fixture values or raw service diagnostics in test output.
            throw BillingService.safe(error)
        }
    }
}
