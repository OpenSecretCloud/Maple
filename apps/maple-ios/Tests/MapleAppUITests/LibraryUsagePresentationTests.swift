import Foundation
import MapleAppCore
import Testing
@testable import MapleAppUI

struct LibraryUsagePresentationTests {
    private let english = Locale(identifier: "en_US")

    @Test func planAndUsageCopyKeepsServerNamesVerbatimAndFormatsCreditNumbersForTheLocale() {
        #expect(LibraryUsagePresentation.plan(.loading, locale: english) == "Loading...")
        #expect(LibraryUsagePresentation.plan(.productPlan("Future Premium"), locale: english) == "Future Premium Plan")
        #expect(LibraryUsagePresentation.plan(.product("Family PLAN"), locale: english) == "Family PLAN")
        #expect(LibraryUsagePresentation.percent(25, locale: english) == "25% used")
        #expect(LibraryUsagePresentation.credits(12_345.5, locale: english) == "+12,345.5 credits")
        #expect(LibraryUsagePresentation.credits(12_345.5, locale: Locale(identifier: "de_DE")) == "+12.345,5 credits")
    }

    @Test func resetCopyUsesTheFeatureTableAndPluralRules() {
        #expect(LibraryUsagePresentation.reset(.monthly, locale: english) == "Resets Monthly")
        #expect(LibraryUsagePresentation.reset(.lessThanOneHour, locale: english) == "Resets in less than 1 hour")
        #expect(LibraryUsagePresentation.reset(.hours(1), locale: english) == "Resets in 1 hour")
        #expect(LibraryUsagePresentation.reset(.hours(2), locale: english) == "Resets in 2 hours")
        #expect(LibraryUsagePresentation.reset(.tomorrow, locale: english) == "Resets Tomorrow")
        #expect(LibraryUsagePresentation.reset(.days(1), locale: english) == "Resets in 1 day")
        #expect(LibraryUsagePresentation.reset(.days(7), locale: english) == "Resets in 7 days")
    }

    @Test func resetDateUsesTheInjectedCalendarTimeZoneAndLocale() throws {
        let date = try #require(ISO8601DateFormatter().date(from: "2026-10-21T00:30:00Z"))
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = try #require(TimeZone(identifier: "America/Los_Angeles"))
        #expect(LibraryUsagePresentation.reset(.date(date), locale: english, calendar: calendar) == "Resets Oct 20")
        calendar.timeZone = try #require(TimeZone(secondsFromGMT: 0))
        #expect(LibraryUsagePresentation.reset(.date(date), locale: english, calendar: calendar) == "Resets Oct 21")
        #expect(
            LibraryUsagePresentation.reset(.date(date), locale: Locale(identifier: "en_GB"), calendar: calendar)
                == "Resets 21 Oct")
    }

    @Test func attentionUsesTheExistingSeatMismatchAndKnownTeamSetupState() throws {
        func team(_ fields: [String: Any]) throws -> TeamStatus {
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(TeamStatus.self, from: JSONSerialization.data(withJSONObject: fields))
        }
        #expect(LibraryAccountAttention(team: nil) == nil)
        #expect(
            try LibraryAccountAttention(team: team(["has_team_subscription": false, "team_created": false])) == nil)
        #expect(
            try LibraryAccountAttention(team: team(["has_team_subscription": true, "team_created": false]))
                == .setupRequired)
        #expect(
            try LibraryAccountAttention(
                team: team([
                    "has_team_subscription": true, "team_created": true, "team_member_count": 3, "billed_seat_count": 2,
                ])) == .usagePaused)
        #expect(
            try LibraryAccountAttention(
                team: team([
                    "has_team_subscription": true, "team_created": true, "seat_limit_exceeded": true,
                ])) == .usagePaused)
        #expect(
            try LibraryAccountAttention(
                team: team([
                    "has_team_subscription": true, "team_created": true, "team_member_count": 2, "billed_seat_count": 2,
                ])) == nil)
    }
}
