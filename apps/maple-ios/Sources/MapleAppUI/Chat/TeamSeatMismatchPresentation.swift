import Foundation
import MapleAppCore

/// Research's mobile seat-mismatch copy. Counts may be present without proving
/// the mismatch, so the server's flag still uses generic copy in that case.
struct TeamSeatMismatchPresentation: Equatable, Sendable {
    enum Audience: Equatable, Sendable { case admin, member }
    struct ExactCounts: Equatable, Sendable {
        let members: Int
        let paidSeats: Int
    }

    let audience: Audience
    let exactCounts: ExactCounts?

    init?(status: TeamStatus?) {
        guard let status, let mismatch = status.seatMismatch else { return nil }
        audience = status.role == "admin" || status.isTeamAdmin == true ? .admin : .member
        if let members = mismatch.memberCount, let paidSeats = mismatch.billedSeatCount,
            members > paidSeats
        {
            exactCounts = ExactCounts(members: members, paidSeats: paidSeats)
        } else {
            exactCounts = nil
        }
    }

    func summary(locale: Locale = .current) -> String {
        guard let exactCounts else { return MapleStrings.text("Seat count mismatch", locale: locale) }
        return String(
            localized: "team.seat-mismatch-summary",
            defaultValue: summaryDefault(exactCounts),
            table: "Localizable", bundle: .module, locale: locale)
    }

    func message(locale: Locale = .current) -> String {
        switch (audience, exactCounts) {
        case (.admin, .some(let counts)):
            String(
                localized: "team.seat-mismatch-admin-message",
                defaultValue: messageDefault(counts),
                table: "Localizable", bundle: .module, locale: locale)
        case (.member, .some(let counts)):
            String(
                localized: "team.seat-mismatch-member-message",
                defaultValue: messageDefault(counts),
                table: "Localizable", bundle: .module, locale: locale)
        case (.admin, .none):
            MapleStrings.text(
                "This team has more members than paid seats. Remove members to match the paid seat count and resume team usage.",
                locale: locale)
        case (.member, .none):
            MapleStrings.text(
                "This team has more members than paid seats. Contact your team admin to resolve the seat count.",
                locale: locale)
        }
    }

    // Package consumers can fall back before a catalog is compiled. Keep English
    // grammar correct while retaining both typed count arguments for the native
    // catalog's independent plural substitutions. Exact counts require members
    // to exceed paid seats, leaving these three possible singular/plural pairs.
    private func summaryDefault(_ counts: ExactCounts) -> String.LocalizationValue {
        if counts.members == 1 {
            return "\(counts.members) member, \(counts.paidSeats) paid seats"
        }
        if counts.paidSeats == 1 {
            return "\(counts.members) members, \(counts.paidSeats) paid seat"
        }
        return "\(counts.members) members, \(counts.paidSeats) paid seats"
    }

    private func messageDefault(_ counts: ExactCounts) -> String.LocalizationValue {
        switch (audience, counts.members == 1, counts.paidSeats == 1) {
        case (.admin, true, _):
            "This team has \(counts.members) member but only \(counts.paidSeats) paid seats. Remove members to match the paid seat count and resume team usage."
        case (.admin, false, true):
            "This team has \(counts.members) members but only \(counts.paidSeats) paid seat. Remove members to match the paid seat count and resume team usage."
        case (.admin, false, false):
            "This team has \(counts.members) members but only \(counts.paidSeats) paid seats. Remove members to match the paid seat count and resume team usage."
        case (.member, true, _):
            "This team has \(counts.members) member but only \(counts.paidSeats) paid seats. Contact your team admin to resolve the seat count."
        case (.member, false, true):
            "This team has \(counts.members) members but only \(counts.paidSeats) paid seat. Contact your team admin to resolve the seat count."
        case (.member, false, false):
            "This team has \(counts.members) members but only \(counts.paidSeats) paid seats. Contact your team admin to resolve the seat count."
        }
    }
}
