import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ThinkingLevelTests {
    @Test func absentReasoningHidesTheControlAndNeverSendsTheStoredEffort() {
        let resolved = AccountEntitlements.resolveThinkingLevel(.high, reasoning: nil)
        #expect(resolved.options.isEmpty)
        #expect(resolved.effective == .default && resolved.effort == nil)
    }

    @Test func optionalReasoningOffersOffAndOrdersDeduplicatedKnownEffortsLowToHigh() {
        let reasoning = ModelReasoning(
            mandatory: false, defaultEnabled: true,
            supportedEfforts: ["max", "high", "low", "none", "minimal", "xhigh", "medium", "low", "future"],
            defaultEffort: "high")
        let options = AccountEntitlements.thinkingLevelOptions(reasoning)
        #expect(options.map(\.value) == [.default, .none, .minimal, .low, .medium, .high, .xhigh, .max])
        #expect(options.first?.defaultEffort == .high)
        let off = AccountEntitlements.resolveThinkingLevel(.none, reasoning: reasoning)
        #expect(off.effective == .none && off.effort == ReasoningEffort.none)
    }

    @Test func mandatoryReasoningNeverOffersOffEvenWhenThePublishedListContainsNone() {
        let reasoning = ModelReasoning(mandatory: true, supportedEfforts: ["high", "none", "low"])
        #expect(AccountEntitlements.thinkingLevelOptions(reasoning).map(\.value) == [.default, .low, .high])
        let off = AccountEntitlements.resolveThinkingLevel(.none, reasoning: reasoning)
        #expect(off.effective == .default && off.effort == nil)
    }

    @Test func untieredReasoningOffersOnlyDefaultAndOptionalOffRegardlessOfDefaultEnabled() {
        for defaultEnabled in [false, true] {
            let reasoning = ModelReasoning(mandatory: false, defaultEnabled: defaultEnabled)
            #expect(AccountEntitlements.thinkingLevelOptions(reasoning).map(\.value) == [.default, .none])
            #expect(AccountEntitlements.resolveThinkingLevel(.low, reasoning: reasoning).effort == nil)
        }
    }

    @Test func unsupportedStoredChoiceFallsBackWithoutChangingPreferenceWhenModelsSwitch() async throws {
        var low = catalogItem("low-model"), high = catalogItem("high-model")
        low.reasoning = .init(mandatory: true, supportedEfforts: ["low"], defaultEffort: "low")
        high.reasoning = .init(mandatory: true, supportedEfforts: ["high"], defaultEffort: "high")
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(accountCatalog(items: [low, high]))])
        let account = AccountEntitlements(
            scope: accountScope(), isGuest: false, billing: service, catalog: service,
            selectedModelID: "high-model", thinkingLevel: .high)
        #expect(account.resolvedThinkingLevel.effort == nil)
        await account.refreshCatalog()
        #expect(account.resolvedThinkingLevel.effort == .high)
        #expect(account.selectModel("low-model") == .allowed)
        #expect(account.resolvedThinkingLevel.effective == .default && account.resolvedThinkingLevel.effort == nil)
        #expect(account.thinkingLevel == .high)
        #expect(account.selectModel("high-model") == .allowed)
        #expect(account.resolvedThinkingLevel.effort == .high)
        await account.retire()
        #expect(account.resolvedThinkingLevel.options.isEmpty && account.resolvedThinkingLevel.effort == nil)
    }

    @Test func aliasReasoningIsAuthoritativeAndMissingAliasMetadataDoesNotInheritTargetEfforts() async throws {
        var target = catalogItem()
        target.reasoning = .init(mandatory: false, supportedEfforts: ["max"], defaultEffort: "max")
        var catalog = accountCatalog(items: [target])
        catalog.aliases[0].reasoning = .init(mandatory: true, supportedEfforts: ["low"], defaultEffort: "low")
        let withoutAliasReasoning = accountCatalog(items: [target])
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(catalog), .success(withoutAliasReasoning), .failure(.unavailable)])
        let account = AccountEntitlements(
            scope: accountScope(), isGuest: false, billing: service, catalog: service, thinkingLevel: .max)
        await account.refreshCatalog()
        #expect(account.resolvedThinkingLevel.options.map(\.value) == [.default, .low])
        #expect(account.resolvedThinkingLevel.effort == nil && account.thinkingLevel == .max)
        account.thinkingLevel = .low
        #expect(account.resolvedThinkingLevel.effort == .low)
        await account.refreshCatalog()
        #expect(account.resolvedThinkingLevel.options.isEmpty && account.resolvedThinkingLevel.effort == nil)
        await account.refreshCatalog()
        #expect(account.catalog == .failed(.unavailable))
        #expect(account.resolvedThinkingLevel.effort == nil && account.chatAccess == .allowed)
    }

    @Test func queuedAndPendingTurnsRetainTheirOwnEffortsAcrossSnapshotRestart() throws {
        let waiting = ChatQueuedTurn(draft: .init(text: "Queued synthetic"), options: .init(reasoningEffort: .high))
        let pending = ChatPendingRun(
            id: UUID(), messageID: "message", outgoingDraft: .init(text: "Active synthetic"),
            options: .init(reasoningEffort: .low))
        let snapshot = ChatSnapshot(pendingRun: pending, queue: .init(items: [waiting]))
        let decoded = try JSONDecoder().decode(ChatSnapshot.self, from: JSONEncoder().encode(snapshot))
        #expect(decoded == snapshot)
        #expect(decoded.pendingRun?.options.reasoningEffort == .low)
        #expect(decoded.queue?.items.first?.options.reasoningEffort == .high)
        let legacy = try JSONDecoder().decode(
            ChatSendOptions.self, from: Data(#"{"model":"auto:quick","webSearch":true}"#.utf8))
        #expect(legacy.webSearch && legacy.reasoningEffort == nil)
    }
}
