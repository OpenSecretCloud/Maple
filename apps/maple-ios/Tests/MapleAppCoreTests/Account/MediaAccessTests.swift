import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor struct MediaAccessTests {
    private func visionCatalog() -> ModelCatalog {
        var model = catalogItem(); model.capabilities.vision = true
        // Alias capability metadata is deliberately stale; the actual target wins.
        return accountCatalog(items: [model, catalogItem("power-target", access: "pro")])
    }

    private func account(
        plan: String? = "Pro", guest: Bool = false, canChat: Bool = true, remaining: Int? = nil,
        catalog: ModelCatalog? = nil, team: TeamStatus? = nil
    ) throws -> AccountEntitlements {
        let service = AccountServingStub(
            statuses: [.success(try billingStatus(plan: plan, canChat: canChat, remaining: remaining))],
            teams: [.success(try team ?? basicTeam())], catalogs: [.success(catalog ?? visionCatalog())])
        return AccountEntitlements(scope: accountScope(), isGuest: guest, billing: service, catalog: service)
    }

    @Test func unknownLoadingAndBillingFailureDoNotClaimAFreePlan() async throws {
        let gate = AccountGate()
        let service = AccountServingStub(
            statuses: [.failure(.unavailable)], teams: [.success(try basicTeam())],
            catalogs: [.success(visionCatalog())], billingGates: [1: gate])
        let value = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        #expect(value.imageAccess == .unavailable && value.transcriptionAccess == .unavailable)
        let refresh = Task { await value.refresh() }
        await gate.waitUntilEntered()
        #expect(value.billing == .loading)
        #expect(value.imageAccess == .unavailable && value.transcriptionAccess == .unavailable)
        await gate.release(); await refresh.value
        #expect(value.billing == .failed(.unavailable))
        #expect(value.imageAccess == .unavailable && value.transcriptionAccess == .unavailable)
        #expect(!value.transcriptionAvailable)
    }

    @Test func confirmedPlanGatesMediaWhileQuotaAndSeatsRemainServerOwned() async throws {
        let cases: [(AccountEntitlements, AccountAccess)] = [
            (try account(plan: "Free"), .paidPlanRequired),
            (try account(plan: "Free", guest: true), .freeGuest),
            (try account(plan: "Future Premium"), .unknownPlan),
            (try account(canChat: false), .allowed),
            (try account(remaining: 0), .allowed),
            (
                try account(
                    team: accountDecode([
                        "has_team_subscription": true, "team_created": true, "seat_limit_exceeded": true,
                    ])), .allowed
            ),
        ]
        for (value, reason) in cases {
            await value.refresh()
            #expect(value.imageAccess == reason)
            #expect(value.transcriptionAccess == reason)
            #expect(value.transcriptionAvailable == (reason == .allowed))
        }
    }

    @Test func imagesAndWebSearchDoNotDependOnSelectedModelCapabilitiesOrCatalog() async throws {
        let value = try account(); await value.refresh()
        #expect(value.imageAccess == .allowed)
        #expect(value.selectedModel?.capabilities.vision == true)
        #expect(value.selectModel("auto:powerful") == .allowed)
        #expect(value.imageAccess == .allowed)
        // Audio does not depend on the text model's vision capability.
        #expect(value.transcriptionAccess == .allowed && value.transcriptionAvailable)

        var unknownTier = catalogItem(access: "future"); unknownTier.capabilities.vision = true
        let unsupported = try account(catalog: accountCatalog(items: [unknownTier]))
        await unsupported.refresh()
        #expect(unsupported.chatAccess == .modelUnavailable)
        #expect(unsupported.imageAccess == .allowed && unsupported.webSearchAccess == .allowed)
        #expect(unsupported.transcriptionAccess == .allowed)

        let missing = try account(catalog: accountCatalog(items: [])); await missing.refresh()
        #expect(missing.chatAccess == .modelUnavailable)
        #expect(missing.imageAccess == .allowed && missing.webSearchAccess == .allowed)
    }

    @Test func absentAndConfirmedUnavailableTranscriptionHaveDifferentReasons() async throws {
        var noAudio = visionCatalog(); noAudio.audio = nil
        var noTranscription = visionCatalog(); noTranscription.audio?.transcription = nil
        var disabled = visionCatalog()
        disabled.audio?.transcription = AudioModelAvailability(available: false, model: "whisper-large-v3")
        for catalog in [noAudio, noTranscription] {
            let value = try account(catalog: catalog); await value.refresh()
            #expect(value.imageAccess == .allowed)
            #expect(value.transcriptionAccess == .unavailable && !value.transcriptionAvailable)
        }
        let value = try account(catalog: disabled); await value.refresh()
        #expect(value.transcriptionAccess == .modelIncompatible && !value.transcriptionAvailable)
    }

    @Test func catalogFailureOnlyRevokesCatalogOwnedAudioAvailability() async throws {
        let failureGate = AccountGate(), retryGate = AccountGate()
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(visionCatalog()), .failure(.unavailable), .success(visionCatalog())],
            catalogGates: [2: failureGate, 3: retryGate])
        let value = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        await value.refresh()
        let failure = Task { await value.refreshCatalog() }
        await failureGate.waitUntilEntered()
        #expect(value.imageAccess == .allowed && value.transcriptionAccess == .allowed)
        await failureGate.release(); await failure.value
        #expect(value.webSearchAccess == .allowed)
        #expect(value.imageAccess == .allowed && value.transcriptionAccess == .unavailable)
        let retry = Task { await value.refreshCatalog() }
        await retryGate.waitUntilEntered()
        #expect(value.catalog == .loading)
        #expect(value.webSearchAccess == .allowed)
        #expect(value.imageAccess == .allowed && value.transcriptionAccess == .unavailable)
        await retryGate.release(); await retry.value
        #expect(value.imageAccess == .allowed && value.transcriptionAccess == .allowed)
    }

    @Test func retiredGenerationCannotRegainMediaAccessFromLateCatalog() async throws {
        let gate = AccountGate()
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(visionCatalog())], catalogGates: [1: gate])
        let value = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        let refresh = Task { await value.refresh() }
        await gate.waitUntilEntered()
        await value.retire(); await gate.release(); await refresh.value
        #expect(value.imageAccess == .retired && value.transcriptionAccess == .retired)
        #expect(!value.transcriptionAvailable && value.catalog == .unknown)
    }
}
