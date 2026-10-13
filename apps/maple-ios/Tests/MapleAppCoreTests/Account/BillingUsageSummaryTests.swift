import Foundation
import Testing
@testable import MapleAppCore

struct BillingUsageSummaryTests {
    private let now = Date(timeIntervalSince1970: 1_791_892_800)  // 2026-10-13 12:00 UTC.
    private var calendar: Calendar {
        var value = Calendar(identifier: .gregorian)
        value.timeZone = TimeZone(secondsFromGMT: 0)!
        return value
    }

    private func status(
        plan: String? = "Pro", total: Int? = 100, used: Int? = 25,
        reset: String? = nil, credits: Double? = nil
    ) throws -> BillingStatus {
        var body = try #require(JSONSerialization.jsonObject(with: billingBody(plan: plan)) as? [String: Any])
        body["total_tokens"] = total as Any? ?? NSNull()
        body["used_tokens"] = used as Any? ?? NSNull()
        body["usage_reset_date"] = reset as Any? ?? NSNull()
        body["api_credit_balance"] = credits as Any? ?? NSNull()
        return try accountDecode(body)
    }

    @Test func rawProductNameIsPreservedAndOnlyAStandalonePlanWordSuppressesTheSuffix() throws {
        let cases: [(String?, BillingUsageSummary.PlanLabel)] = [
            (nil, .loading), ("", .loading), (" \n ", .loading),
            ("  Future Premium \n", .productPlan("Future Premium")),
            ("Family PLAN", .product("Family PLAN")), ("Plan Flexible", .product("Plan Flexible")),
            ("Planet", .productPlan("Planet")), ("Planned Access", .productPlan("Planned Access")),
            ("Plan_2", .productPlan("Plan_2")), ("Plané", .product("Plané")), ("计划Plan", .product("计划Plan")),
            ("ſPlan", .product("ſPlan")), ("KPlan", .product("KPlan")),
        ]
        for (name, expected) in cases {
            #expect(try status(plan: name).usageSummary().planLabel == expected)
        }
    }

    @Test func missingUsageAndZeroTotalsHideTheEntireMeterIncludingCredits() throws {
        let cases: [(Int?, Int?)] = [(nil, 25), (100, nil), (0, 25), (-1, 25)]
        for (total, used) in cases {
            #expect(try status(total: total, used: used, credits: 200).usageSummary().meter == nil)
        }
    }

    @Test func usageClampsAtBothEndsRoundsHalfUpAndDoesNotOverflowLargeCounts() throws {
        for (total, used, percent) in [(200, 1, 0.5), (100, -1, 0), (100, 150, 100), (Int.max, Int.max, 100)] {
            let meter = try #require(status(total: total, used: used).usageSummary().meter)
            #expect(meter.percentUsed == percent)
            #expect(meter.roundedPercentUsed == Int(percent.rounded()))
        }
    }

    @Test func onlyPositiveCreditsAreShownAlongsideAValidMeter() throws {
        let balances: [Double?] = [nil, 0, -1, 0.125, 12_345]
        for balance in balances {
            let meter = try #require(status(credits: balance).usageSummary().meter)
            #expect(meter.creditBalance == balance.flatMap { $0 > 0 ? $0 : nil })
        }
    }

    @Test func resetLadderUsesTheResetDateRatherThanTheSubscriptionRenewalDate() throws {
        let formatter = ISO8601DateFormatter()
        let cases: [(TimeInterval, BillingUsageReset)] = [
            (-1, .monthly), (0, .lessThanOneHour), (3_599, .lessThanOneHour),
            (3_600, .hours(1)), (3_601, .hours(1)), (3_600 * 2, .hours(2)),
            (3_600 * 13, .tomorrow), (86_400 * 2, .days(2)), (86_400 * 7, .days(7)),
            (86_400 * 8, .date(now.addingTimeInterval(86_400 * 8))),
        ]
        for (offset, expected) in cases {
            let reset = formatter.string(from: now.addingTimeInterval(offset))
            #expect(try status(reset: reset).usageSummary(now: now, calendar: calendar).meter?.reset == expected)
        }
        let unreadable: [String?] = [nil, "", "not-a-date", "1792324800"]
        for reset in unreadable {
            #expect(try status(reset: reset).usageSummary(now: now, calendar: calendar).meter?.reset == .monthly)
        }
    }

    @Test func nextCalendarDayTakesPrecedenceEvenIfOnlyOneMinuteAway() throws {
        let formatter = ISO8601DateFormatter()
        let late = try #require(formatter.date(from: "2026-10-13T23:59:00Z"))
        let reset = "2026-10-14T00:00:00Z"
        #expect(try status(reset: reset).usageSummary(now: late, calendar: calendar).meter?.reset == .tomorrow)
    }

    @Test func isoDateOnlyResetValuesUseUTCMidnightAndKeepRenewalDateParsingStrict() throws {
        #expect(try status(reset: "2026-10-14").usageSummary(now: now, calendar: calendar).meter?.reset == .tomorrow)
        #expect(try status(reset: "2026-10-12").usageSummary(now: now, calendar: calendar).meter?.reset == .monthly)
        #expect(BillingDate.text("2026-10-14").date == nil)
        #expect(
            try status(reset: "2030-04-17bogus").usageSummary(now: now, calendar: calendar).meter?.reset == .monthly)
        var local = calendar
        local.timeZone = try #require(TimeZone(identifier: "America/Los_Angeles"))
        #expect(try status(reset: "2026-10-14").usageSummary(now: now, calendar: local).meter?.reset == .hours(12))
    }

    @Test func calendarDaysRemainCorrectAcrossBothDaylightSavingTransitions() throws {
        var local = Calendar(identifier: .gregorian)
        local.timeZone = try #require(TimeZone(identifier: "America/Los_Angeles"))
        let formatter = ISO8601DateFormatter()
        for (start, reset, expected) in [
            ("2026-03-07T23:30:00-08:00", "2026-03-08T23:00:00-07:00", BillingUsageReset.tomorrow),
            ("2026-10-31T23:30:00-07:00", "2026-11-01T23:00:00-08:00", BillingUsageReset.tomorrow),
            ("2026-03-07T12:00:00-08:00", "2026-03-14T12:00:00-07:00", BillingUsageReset.days(7)),
            ("2026-10-31T12:00:00-07:00", "2026-11-07T12:00:00-08:00", BillingUsageReset.days(7)),
        ] {
            let startDate = try #require(formatter.date(from: start))
            #expect(try status(reset: reset).usageSummary(now: startDate, calendar: local).meter?.reset == expected)
        }
    }
}
