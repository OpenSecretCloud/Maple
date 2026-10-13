import Foundation
import Testing
@testable import MapleAppCore

struct BillingServiceTests {
    @Test func fixedRoutesAndAudienceOnlyUseThirdPartyToken() async throws {
        let source = AccountTokenStub()
        let http = AccountHTTPStub { url, _, _ in
            let body =
                url.path.hasSuffix("team/status")
                ? Data(#"{"has_team_subscription":false,"team_created":false}"#.utf8) : try billingBody()
            return BillingHTTPResponse(status: 200, body: body)
        }
        let service = BillingService(
            scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
        async let subscription = service.subscriptionStatus()
        async let team = service.teamStatus()
        _ = try await (subscription, team)
        #expect(await source.audiences == ["http://127.0.0.1:11002"])
        let calls = await http.calls
        #expect(Set(calls.map(\.url.path)) == ["/v1/maple/subscription/status", "/v1/maple/team/status"])
        #expect(
            calls.allSatisfy { $0.url.host == "127.0.0.1" && $0.url.port == 11002 && $0.token == "fixture-token-1" })
    }

    @Test func concurrentMintIsDeduplicated() async throws {
        let gate = AccountGate(); let source = AccountTokenStub(gate: gate)
        let http = AccountHTTPStub { _, _, _ in BillingHTTPResponse(status: 200, body: try billingBody()) }
        let service = BillingService(
            scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
        let first = Task { try await service.subscriptionStatus() }
        let second = Task { try await service.subscriptionStatus() }
        await gate.waitUntilEntered()
        #expect(await gate.entries == 1)
        await gate.release()
        _ = try await (first.value, second.value)
        #expect(await source.audiences.count == 1)
    }

    @Test func unauthorizedRemintsOnceAndNeverRetriesOtherStatus() async throws {
        for status in [401, 403, 500] {
            let source = AccountTokenStub()
            let http = AccountHTTPStub { _, _, _ in BillingHTTPResponse(status: status, body: Data()) }
            let service = BillingService(
                scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
            await #expect(throws: status == 401 ? AccountServiceError.unauthorized : .unavailable) {
                try await service.subscriptionStatus()
            }
            #expect(await source.audiences.count == (status == 401 ? 2 : 1))
            #expect(await http.calls.count == (status == 401 ? 2 : 1))
        }
    }

    @Test func lateUnauthorizedCannotEvictReplacementToken() async throws {
        let gate = AccountGate(); let source = AccountTokenStub()
        let http = AccountHTTPStub { _, token, index in
            if index == 1 { await gate.wait() }
            return BillingHTTPResponse(status: token == "fixture-token-1" ? 401 : 200, body: try billingBody())
        }
        let service = BillingService(
            scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
        let first = Task { try await service.subscriptionStatus() }
        await gate.waitUntilEntered()
        _ = try await service.subscriptionStatus()  // replaces token while the first 401 is held
        await gate.release()
        _ = try await first.value
        #expect(await source.audiences.count == 2)
        #expect(
            await http.calls.map(\.token) == [
                "fixture-token-1", "fixture-token-1", "fixture-token-2", "fixture-token-2",
            ])
    }

    @Test func retirementDuringMintCannotSendHTTP() async throws {
        let gate = AccountGate(); let source = AccountTokenStub(gate: gate)
        let http = AccountHTTPStub { _, _, _ in BillingHTTPResponse(status: 200, body: try billingBody()) }
        let service = BillingService(
            scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
        let work = Task { try await service.subscriptionStatus() }
        await gate.waitUntilEntered()
        await service.retire(); await gate.release()
        await #expect(throws: AccountServiceError.sessionChanged) { try await work.value }
        #expect(await http.calls.isEmpty)
    }

    @Test func retirementDuringHTTPRejectsResultAndFurtherMint() async throws {
        let gate = AccountGate(); let source = AccountTokenStub()
        let http = AccountHTTPStub { _, _, _ in
            await gate.wait()
            return BillingHTTPResponse(status: 401, body: Data())
        }
        let service = BillingService(
            scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
        let work = Task { try await service.subscriptionStatus() }
        await gate.waitUntilEntered()
        await service.retire(); await gate.release()
        await #expect(throws: AccountServiceError.sessionChanged) { try await work.value }
        await #expect(throws: AccountServiceError.sessionChanged) { try await service.teamStatus() }
        #expect(await source.audiences.count == 1)
    }

    @Test func invalidAndOversizedBodiesRemainExplicitFailures() async throws {
        for data in [
            Data(#"{"product_name":"Free"}"#.utf8), Data("not json".utf8), Data(repeating: 32, count: 1_048_577),
        ] {
            let source = AccountTokenStub()
            let http = AccountHTTPStub { _, _, _ in BillingHTTPResponse(status: 200, body: data) }
            let service = BillingService(
                scope: accountScope(), configuration: try accountConfiguration(), tokenSource: source, http: http)
            await #expect(throws: AccountServiceError.invalidResponse) { try await service.subscriptionStatus() }
            #expect(await source.audiences.count == 1)
        }
    }

    @Test func malformedCountsAndUnknownPlanAreNotFree() throws {
        #expect(try billingStatus(plan: nil).plan == .unknown)
        #expect(try billingStatus(plan: "Future Premium").plan == .unknown)
        #expect(try billingStatus(plan: "Free plan").plan == .unknown)
        #expect(try billingStatus(plan: "").plan == .unknown)
        #expect(try billingStatus(plan: "  FREE\n").plan == .free)
        #expect(try billingStatus(plan: "  Pro annual\n").plan == .paid(.pro))
        #expect(try billingStatus(plan: "MAX monthly").plan == .paid(.max))
        #expect(try billingStatus(plan: "Team Annual").plan == .paid(.team))
        #expect(throws: AccountServiceError.invalidResponse) { try billingStatus(remaining: -1).validate() }
        #expect(throws: (any Error).self) {
            let _: TeamStatus = try accountDecode([
                "has_team_subscription": true, "team_created": true, "seats_used": 1.5,
            ])
        }
    }

    @Test func fullBillingStatusFixtureDecodesOptionalContractFields() throws {
        let status: BillingStatus = try accountDecode([
            "is_subscribed": true, "stripe_customer_id": "fixture-customer", "product_id": "fixture-product",
            "product_name": "Max", "subscription_status": "active", "current_period_end": 1_735_689_600,
            "can_chat": true, "chats_remaining": 42, "payment_provider": "stripe",
            "total_tokens": 1_000_000, "used_tokens": 123_456, "usage_reset_date": "2025-01-01",
            "api_credit_balance": 12_345.5, "ios_iap_enabled": true, "ios_us_external_link_enabled": false,
            "subscriptions": [
                [
                    "provider": "stripe", "plan": "Max", "product_id": "fixture-product", "state": "active",
                    "renews_at": 1_735_689_600, "manage": "portal", "selected": true,
                    "environment": "fixture", "auto_renew_enabled": true,
                ]
            ],
            "conflict": ["other_provider": "apple", "action": "future_action"],
            "pending_plan_change": [
                "id": "fixture-change", "type": "downgrade", "target_plan_name": "Pro", "status": "pending",
                "expires_at": "2025-01-01T00:00:00Z",
            ],
        ])
        try status.validate()
        #expect(status.isSubscribed && status.stripeCustomerId == "fixture-customer")
        #expect(status.productId == "fixture-product" && status.productName == "Max")
        #expect(status.subscriptionStatus == "active" && status.currentPeriodEnd == .seconds(1_735_689_600))
        #expect(status.canChat && status.chatsRemaining == 42 && status.paymentProvider == "stripe")
        #expect(status.totalTokens == 1_000_000 && status.usedTokens == 123_456)
        #expect(status.usageResetDate == "2025-01-01")
        #expect(status.apiCreditBalance == 12_345.5)
        #expect(status.iosIapEnabled == true && status.iosUsExternalLinkEnabled == false)
        let subscription = try #require(status.subscriptions?.first)
        #expect(subscription.provider == "stripe" && subscription.plan == "Max")
        #expect(subscription.productId == "fixture-product" && subscription.state == "active")
        #expect(subscription.renewsAt == 1_735_689_600 && subscription.manage == "portal")
        #expect(subscription.selected == true && subscription.environment == "fixture")
        #expect(subscription.autoRenewEnabled == true)
        let conflict = try #require(status.conflict)
        #expect(conflict.otherProvider == "apple" && conflict.action == "future_action")
        let pending = try #require(status.pendingPlanChange)
        #expect(pending.id == "fixture-change" && pending.type == "downgrade")
        #expect(pending.targetPlanName == "Pro" && pending.status == "pending")
        #expect(pending.expiresAt == "2025-01-01T00:00:00Z")
    }

    @Test func optionalBillingFieldsMayBeAbsentOrNull() throws {
        var payload = try #require(JSONSerialization.jsonObject(with: billingBody()) as? [String: Any])
        let optional = [
            "stripe_customer_id", "api_credit_balance", "ios_iap_enabled", "ios_us_external_link_enabled",
            "conflict", "pending_plan_change",
        ]
        for nulls in [false, true] {
            for key in optional {
                if nulls { payload[key] = NSNull() } else { payload.removeValue(forKey: key) }
            }
            let status: BillingStatus = try accountDecode(payload)
            #expect(status.stripeCustomerId == nil && status.apiCreditBalance == nil)
            #expect(status.iosIapEnabled == nil && status.iosUsExternalLinkEnabled == nil)
            #expect(status.conflict == nil && status.pendingPlanChange == nil)
        }
    }

    @Test(arguments: ["other_provider", "action", "id", "type", "target_plan_name", "status", "expires_at"])
    func missingOrNullBillingHintSubfieldsDoNotInvalidateStatus(_ field: String) throws {
        for nulls in [false, true] {
            var payload = try #require(JSONSerialization.jsonObject(with: billingBody()) as? [String: Any])
            var conflict: [String: Any] = ["other_provider": "apple", "action": "future_action"]
            var pending: [String: Any] = [
                "id": "fixture-change", "type": "downgrade", "target_plan_name": "Max", "status": "pending",
                "expires_at": "2025-01-01T00:00:00Z",
            ]
            if conflict[field] != nil {
                if nulls { conflict[field] = NSNull() } else { conflict.removeValue(forKey: field) }
            } else {
                if nulls { pending[field] = NSNull() } else { pending.removeValue(forKey: field) }
            }
            payload["conflict"] = conflict; payload["pending_plan_change"] = pending
            let status: BillingStatus = try accountDecode(payload)
            try status.validate()
            #expect(status.plan == .paid(.pro) && status.canChat)
            let decodedConflict = try #require(status.conflict), decodedPending = try #require(status.pendingPlanChange)
            #expect(decodedConflict.otherProvider == (field == "other_provider" ? nil : "apple"))
            #expect(decodedConflict.action == (field == "action" ? nil : "future_action"))
            #expect(decodedPending.id == (field == "id" ? nil : "fixture-change"))
            #expect(decodedPending.type == (field == "type" ? nil : "downgrade"))
            #expect(decodedPending.targetPlanName == (field == "target_plan_name" ? nil : "Max"))
            #expect(decodedPending.status == (field == "status" ? nil : "pending"))
            #expect(decodedPending.expiresAt == (field == "expires_at" ? nil : "2025-01-01T00:00:00Z"))
        }
    }

    @Test func emptyBillingHintObjectsDoNotInvalidateStatus() throws {
        var payload = try #require(JSONSerialization.jsonObject(with: billingBody()) as? [String: Any])
        payload["conflict"] = [String: String](); payload["pending_plan_change"] = [String: String]()
        let status: BillingStatus = try accountDecode(payload)
        try status.validate()
        #expect(status.plan == .paid(.pro) && status.canChat)
        let conflict = try #require(status.conflict), pending = try #require(status.pendingPlanChange)
        #expect(conflict.otherProvider == nil && conflict.action == nil)
        #expect(pending.id == nil && pending.type == nil && pending.targetPlanName == nil)
        #expect(pending.status == nil && pending.expiresAt == nil)
    }

    @Test func mixedDatesAndFutureManagementValuesStayInert() throws {
        let seconds: BillingDate = try JSONDecoder().decode(BillingDate.self, from: Data("1735689600".utf8))
        let iso: BillingDate = try JSONDecoder().decode(BillingDate.self, from: Data(#""2025-01-01T00:00:00Z""#.utf8))
        #expect(seconds.date == iso.date)
        #expect(BillingDate.text("1735689600").date == nil)
        #expect(BillingDate.text("01/02/2025").date == nil)
        #expect(BillingDate.seconds(-1).date == nil)
        let subscription: BillingSubscription = try accountDecode([
            "provider": "future", "plan": "Pro", "state": "active", "renews_at": NSNull(),
            "manage": "https://untrusted.example",
        ])
        #expect(subscription.manage == "https://untrusted.example")
        #expect(subscription.provider == "future")
    }
}
