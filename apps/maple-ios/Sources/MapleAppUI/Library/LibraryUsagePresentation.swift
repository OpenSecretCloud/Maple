import Foundation
import MapleAppCore

/// Keeps the read-only status copy separate from the account's access policy.
enum LibraryUsagePresentation {
    static func plan(_ label: BillingUsageSummary.PlanLabel, locale: Locale = .current) -> String {
        switch label {
        case .loading:
            String(localized: "Loading...", table: "Library", bundle: .module, locale: locale)
        case let .product(name): name
        case let .productPlan(name):
            String(
                localized: "library.usage.product-plan", defaultValue: "\(name) Plan",
                table: "Library", bundle: .module, locale: locale)
        }
    }

    static func percent(_ value: Int, locale: Locale = .current) -> String {
        let number = value.formatted(.number.grouping(.never).locale(locale))
        return String(
            localized: "library.usage.percent-used", defaultValue: "\(number)% used",
            table: "Library", bundle: .module, locale: locale)
    }

    static func credits(_ value: Double, locale: Locale = .current) -> String {
        let number = value.formatted(.number.locale(locale))
        return String(
            localized: "library.usage.credits", defaultValue: "+\(number) credits",
            table: "Library", bundle: .module, locale: locale)
    }

    static func reset(
        _ value: BillingUsageReset, locale: Locale = .current, calendar: Calendar = .current
    ) -> String {
        switch value {
        case .monthly:
            return String(localized: "Resets Monthly", table: "Library", bundle: .module, locale: locale)
        case .lessThanOneHour:
            return String(localized: "Resets in less than 1 hour", table: "Library", bundle: .module, locale: locale)
        case let .hours(count):
            return String(
                localized: "library.usage.reset-hours",
                defaultValue: count == 1 ? "Resets in \(count) hour" : "Resets in \(count) hours",
                table: "Library", bundle: .module, locale: locale)
        case .tomorrow:
            return String(localized: "Resets Tomorrow", table: "Library", bundle: .module, locale: locale)
        case let .days(count):
            return String(
                localized: "library.usage.reset-days",
                defaultValue: count == 1 ? "Resets in \(count) day" : "Resets in \(count) days",
                table: "Library", bundle: .module, locale: locale)
        case let .date(date):
            let day = date.formatted(
                Date.FormatStyle(
                    date: .omitted, time: .omitted, locale: locale, calendar: calendar,
                    timeZone: calendar.timeZone
                ).month(.abbreviated).day())
            return String(
                localized: "library.usage.reset-date", defaultValue: "Resets \(day)",
                table: "Library", bundle: .module, locale: locale)
        }
    }
}

public enum LibraryAccountAttention: Equatable, Sendable {
    case usagePaused, setupRequired

    public init?(team: TeamStatus?) {
        if team?.seatMismatch != nil {
            self = .usagePaused
        } else if let team, team.hasTeamSubscription && !team.teamCreated {
            self = .setupRequired
        } else {
            return nil
        }
    }

    public var accessibilityLabel: String {
        switch self {
        case .usagePaused: String(localized: "Team usage paused", table: "Library", bundle: .module)
        case .setupRequired: String(localized: "Team setup required", table: "Library", bundle: .module)
        }
    }
}
