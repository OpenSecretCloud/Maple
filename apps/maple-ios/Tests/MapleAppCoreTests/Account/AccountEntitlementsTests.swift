import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor struct AccountEntitlementsTests {
    private func fixture(
        plan: String? = "Pro", guest: Bool = false, canChat: Bool = true, remaining: Int? = nil,
        catalog: ModelCatalog = accountCatalog(), selected: String = "auto:quick"
    ) throws -> AccountEntitlements {
        let service = AccountServingStub(
            statuses: [.success(try billingStatus(plan: plan, canChat: canChat, remaining: remaining))],
            teams: [.success(try basicTeam())], catalogs: [.success(catalog)])
        return AccountEntitlements(
            scope: accountScope(), isGuest: guest, billing: service, catalog: service, selectedModelID: selected)
    }

    @Test func unknownLoadingAndFailureNeverBecomeFree() async throws {
        let gate = AccountGate()
        let service = AccountServingStub(
            statuses: [.failure(.unavailable)], teams: [.success(try basicTeam())], billingGates: [1: gate])
        let account = AccountEntitlements(
            scope: accountScope(), isGuest: true, billing: service, catalog: service,
            selectedModelID: "auto:powerful")
        #expect(account.billing == .unknown)
        #expect(account.plan == .unknown)
        #expect(account.chatAccess == .modelUnavailable)
        await account.refreshCatalog()
        #expect(account.chatAccess == .allowed)
        let load = Task { await account.refresh() }
        await gate.waitUntilEntered()
        #expect(account.billing == .loading)
        #expect(account.plan == .unknown)
        #expect(account.webSearchPreferred)
        #expect(!account.effectiveWebSearch)
        #expect(account.chatAccess == .allowed)
        await gate.release(); await load.value
        #expect(account.billing == .failed(.unavailable))
        #expect(account.plan == .unknown)
        #expect(account.chatAccess == .allowed)
    }

    @Test func explicitFreeEmailAndGuestDifferWithoutPurchaseActions() async throws {
        let email = try fixture(plan: "Free")
        let guest = try fixture(plan: "Free", guest: true)
        await email.refresh(); await guest.refresh()
        #expect(email.chatAccess == .allowed)
        #expect(email.webSearchAccess == .paidPlanRequired)
        #expect(email.webSearchPreferred && !email.effectiveWebSearch)
        #expect(!email.transcriptionAvailable && !email.speechAvailable)
        #expect(email.selectModel("auto:powerful") == .paidPlanRequired)
        #expect(email.selectedModelID == "auto:quick")
        #expect(guest.chatAccess == .freeGuest)
    }

    @Test(arguments: ["Pro", "Max monthly", "Team Annual"])
    func paidPlansAndPaidGuestUseKnownCatalogCapabilities(_ plan: String) async throws {
        let account = try fixture(plan: plan, guest: true)
        await account.refresh()
        #expect(account.plan.isPaid)
        #expect(account.chatAccess == .allowed)
        #expect(account.effectiveWebSearch)
        #expect(account.transcriptionAvailable && account.speechAvailable)
        #expect(account.selectModel("auto:powerful") == .allowed)
        account.webSearchPreferred = false
        #expect(!account.effectiveWebSearch)
    }

    @Test func unknownPlanAndServerQuotaDoNotBlockFreeModelChat() async throws {
        for account in [try fixture(plan: nil), try fixture(plan: "Future Premium", guest: true)] {
            await account.refresh()
            #expect(account.plan == .unknown)
            #expect(account.chatAccess == .allowed)
            #expect(!account.effectiveWebSearch)
        }
        for account in [try fixture(canChat: false), try fixture(remaining: 0)] {
            await account.refresh()
            #expect(account.chatAccess == .allowed)
            #expect(account.effectiveWebSearch)
        }
    }

    @Test func onlyLoadedKnownFreeGuestIsBlocked() async throws {
        let service = AccountServingStub(
            statuses: [.success(try billingStatus(plan: "  FREE\n")), .failure(.unavailable)],
            teams: [.success(try basicTeam())])
        let guest = AccountEntitlements(scope: accountScope(), isGuest: true, billing: service, catalog: service)
        await guest.refreshCatalog()
        #expect(guest.chatAccess == .allowed)
        await guest.refreshBilling()
        #expect(guest.chatAccess == .freeGuest)
        await guest.refreshBilling()
        #expect(guest.billing == .failed(.unavailable))
        #expect(guest.chatAccess == .allowed)
    }

    @Test func selectedPaidModelMayChatWithUnknownPlanWhilePickerRemainsPaidGated() async throws {
        for plan: String? in [nil, "", "Future Premium"] {
            let account = try fixture(plan: plan, selected: "auto:powerful")
            await account.refresh()
            #expect(account.plan == .unknown)
            #expect(account.chatAccess == .allowed)
            #expect(account.selectModel("auto:powerful") == .paidPlanRequired)
            #expect(account.selectedModelID == "auto:powerful")
            #expect(!account.effectiveWebSearch)
        }
    }

    @Test func selectedPaidModelReachesServerAfterDowngradeAndBillingFailure() async throws {
        for guest in [false, true] {
            let service = AccountServingStub(
                statuses: [
                    .success(try billingStatus()), .success(try billingStatus(plan: "Free")),
                    .failure(.unavailable),
                ], teams: [.success(try basicTeam())])
            let account = AccountEntitlements(
                scope: accountScope(), isGuest: guest, billing: service, catalog: service,
                selectedModelID: "auto:powerful")
            await account.refresh()
            #expect(account.chatAccess == .allowed)
            await account.refreshBilling()
            #expect(account.plan == .free)
            #expect(account.chatAccess == (guest ? .freeGuest : .allowed))
            #expect(account.selectModel("auto:powerful") == .paidPlanRequired)
            #expect(account.webSearchAccess == (guest ? .freeGuest : .paidPlanRequired))
            await account.refreshBilling()
            #expect(account.billing == .failed(.unavailable))
            #expect(account.chatAccess == .allowed)
            #expect(account.selectedModelID == "auto:powerful")
        }
    }

    @Test func teamCountsUseFallbacksAndMissingCountsStayUnknown() async throws {
        let fallback: TeamStatus = try accountDecode([
            "has_team_subscription": true, "team_created": true, "members_count": 5,
            "seats_purchased": 3, "seats_available": -2,
        ])
        try fallback.validate()
        #expect(fallback.memberCount == 5 && fallback.paidSeatCount == 3)
        #expect(fallback.availableSeatCount == 0)
        #expect(fallback.seatMismatch?.hasExactCounts == true)
        let missing: TeamStatus = try accountDecode([
            "has_team_subscription": true, "team_created": true, "seat_limit_exceeded": true,
        ])
        #expect(missing.seatMismatch?.hasExactCounts == false)
        #expect(missing.seatMismatch?.memberCount == nil)
        let precedence: TeamStatus = try accountDecode([
            "has_team_subscription": true, "team_created": true, "team_member_count": 2,
            "members_count": 100, "billed_seat_count": 2, "seats_purchased": 0,
        ])
        #expect(precedence.seatMismatch == nil)
        let service = AccountServingStub(
            statuses: [.success(try billingStatus(plan: "Team"))], teams: [.success(missing), .failure(.unavailable)])
        let account = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        await account.refresh()
        #expect(account.chatAccess == .allowed)
        await account.refreshTeam()
        #expect(account.team == .failed(.unavailable))
        #expect(account.team.value?.seatMismatch == nil)
        #expect(account.billing.value?.plan == .paid(.team))
    }

    @Test func catalogFiltersDisabledDeprecatedNonChatAndUnknownTier() async throws {
        let catalog = accountCatalog(items: [
            catalogItem(), catalogItem("power-target", access: "pro"), catalogItem("off", enabled: false),
            catalogItem("old", deprecated: true), catalogItem("audio", chat: false),
            catalogItem("future", access: "future"),
            catalogItem(),  // duplicate server entry cannot duplicate menus
        ])
        let account = try fixture(catalog: catalog)
        await account.refresh()
        #expect(Set(account.advancedModels.map(\.id)) == ["quick-target", "power-target", "future"])
        #expect(account.advancedModels.count == 3)
        #expect(account.primaryModels.map(\.id) == ["auto:quick", "auto:powerful"])
        #expect(account.selectModel("future") == .modelUnavailable)
    }

    @Test func catalogFailurePreservesSelectionAndDoesNotAdoptFallback() async throws {
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(accountCatalog()), .failure(.unavailable)])
        let account = AccountEntitlements(
            scope: accountScope(), isGuest: false, billing: service, catalog: service,
            selectedModelID: "power-target")
        await account.refresh()
        #expect(account.selectedModelID == "power-target")
        await account.refreshCatalog()
        #expect(account.selectedModelID == "power-target")
        #expect(account.catalog == .failed(.unavailable))
        #expect(account.chatAccess == .modelUnavailable)
        #expect(account.plan == .paid(.pro))
        #expect(account.primaryModels.isEmpty)
    }

    @Test func successfulCatalogReconcilesRemovedConcreteModelAndKeepsAliases() async throws {
        let first = accountCatalog()
        let second = accountCatalog(
            items: [catalogItem("new-target"), catalogItem("power-target", access: "pro")], quickTarget: "new-target")
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(first), .success(second)])
        let account = AccountEntitlements(
            scope: accountScope(), isGuest: false, billing: service, catalog: service,
            selectedModelID: "quick-target")
        await account.refresh(); await account.refreshCatalog()
        #expect(account.selectedModelID == "auto:quick")
        #expect(account.selectedModel?.targetModel == "new-target")
        await account.refreshCatalog()
        #expect(account.selectedModelID == "auto:quick")
    }

    @Test func actualAliasTargetOverridesAccessWithoutGatingWebSearchOnTools() async throws {
        let catalog = accountCatalog(items: [
            catalogItem(access: "pro", tools: false), catalogItem("power-target", access: "pro"),
        ])
        let free = try fixture(plan: "Free", catalog: catalog)
        let paid = try fixture(catalog: catalog)
        await free.refresh(); await paid.refresh()
        #expect(free.chatAccess == .allowed)
        #expect(free.selectModel("auto:quick") == .paidPlanRequired)
        #expect(paid.chatAccess == .allowed)
        #expect(paid.webSearchAccess == .allowed)
        #expect(paid.webSearchPreferred && paid.effectiveWebSearch)
    }

    @Test func newerRefreshWinsOverLateBillingAndCatalogResults() async throws {
        let billingGate = AccountGate(); let catalogGate = AccountGate()
        let service = AccountServingStub(
            statuses: [.success(try billingStatus(plan: "Free")), .success(try billingStatus())],
            teams: [.success(try basicTeam())],
            catalogs: [.success(accountCatalog()), .failure(.unavailable)],
            billingGates: [1: billingGate], catalogGates: [1: catalogGate])
        let account = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        let first = Task { await account.refresh() }
        await billingGate.waitUntilEntered(); await catalogGate.waitUntilEntered()
        await account.refresh()
        #expect(account.plan == .paid(.pro))
        #expect(account.catalog == .failed(.unavailable))
        await billingGate.release(); await catalogGate.release(); await first.value
        #expect(account.plan == .paid(.pro))
        #expect(account.catalog == .failed(.unavailable))
    }

    @Test func samePrincipalReloginCannotReceiveRetiredGenerationResults() async throws {
        let gate = AccountGate()
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())], billingGates: [1: gate])
        let old = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        let load = Task { await old.refresh() }
        await gate.waitUntilEntered()
        await old.retire()
        let replacement = try fixture(plan: "Free")
        #expect(old.scope.principalID == replacement.scope.principalID)
        #expect(old.scope.generation != replacement.scope.generation)
        await replacement.refresh(); await gate.release(); await load.value
        #expect(old.billing == .unknown && old.catalog == .unknown)
        #expect(old.chatAccess == .retired)
        #expect(replacement.plan == .free)
        #expect(await service.retired)
        let calls = await service.statusCalls
        await old.refresh()
        #expect(await service.statusCalls == calls)
    }

    @Test func billingRefreshRetainsVerifiedAccessUntilDenialAndNeverRevivesFailedValue() async throws {
        let denied = AccountGate(), failed = AccountGate(), retried = AccountGate()
        let service = AccountServingStub(
            statuses: [
                .success(try billingStatus()), .success(try billingStatus(plan: "Free")),
                .failure(.unavailable), .success(try billingStatus()),
            ], teams: [.success(try basicTeam())], billingGates: [2: denied, 3: failed, 4: retried])
        let account = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        await account.refresh()
        let refresh = Task { await account.refreshBilling() }
        await denied.waitUntilEntered()
        #expect(account.speechAccess == .allowed && account.speechAvailable && account.plan.isPaid)
        await denied.release(); await refresh.value
        #expect(account.speechAccess == .paidPlanRequired && account.plan == .free)
        let failure = Task { await account.refreshBilling() }
        await failed.waitUntilEntered()
        #expect(account.speechAccess == .paidPlanRequired)
        await failed.release(); await failure.value
        #expect(account.speechAccess == .unavailable && account.billing == .failed(.unavailable))
        let retry = Task { await account.refreshBilling() }
        await retried.waitUntilEntered()
        #expect(account.billing == .loading && account.plan == .unknown && account.speechAccess == .unavailable)
        await retried.release(); await retry.value
        #expect(account.speechAccess == .allowed)
        await account.retire()
        #expect(account.speechAccess == .retired && account.billing == .unknown && account.catalog == .unknown)
    }

    @Test func catalogRefreshRetainsKnownCapabilityUntilFailureThenRetryStartsUnknown() async throws {
        let failed = AccountGate(), retried = AccountGate()
        var unavailable = accountCatalog()
        unavailable.audio?.speech = AudioModelAvailability(available: false, model: "voxtral-tts")
        let service = AccountServingStub(
            statuses: [.success(try billingStatus())], teams: [.success(try basicTeam())],
            catalogs: [.success(accountCatalog()), .failure(.unavailable), .success(unavailable)],
            catalogGates: [2: failed, 3: retried])
        let account = AccountEntitlements(scope: accountScope(), isGuest: false, billing: service, catalog: service)
        await account.refresh()
        let failure = Task { await account.refreshCatalog() }
        await failed.waitUntilEntered()
        #expect(account.speechAccess == .allowed && !account.primaryModels.isEmpty)
        await failed.release(); await failure.value
        #expect(account.speechAccess == .unavailable && account.catalog == .failed(.unavailable))
        let retry = Task { await account.refreshCatalog() }
        await retried.waitUntilEntered()
        #expect(account.catalog == .loading && account.speechAccess == .unavailable)
        await retried.release(); await retry.value
        #expect(account.speechAccess == .modelIncompatible && !account.speechAvailable)
    }

    @Test func speechAdmissionDistinguishesUnknownFreeAndMissingCapabilityWithoutQuotaGates() async throws {
        let unknown = try fixture(plan: "Future Premium"), free = try fixture(plan: "Free", guest: true)
        let exhausted = try fixture(remaining: 0)
        var missing = accountCatalog(); missing.audio = nil
        let absent = try fixture(catalog: missing)
        #expect(unknown.speechAccess == .unavailable)
        await unknown.refresh(); await free.refresh(); await exhausted.refresh(); await absent.refresh()
        #expect(unknown.speechAccess == .unavailable)
        #expect(free.speechAccess == .paidPlanRequired)
        #expect(exhausted.speechAccess == .allowed)
        #expect(absent.speechAccess == .unavailable)
    }
}
