import Foundation
import MapleAppCore
import Testing
@testable import MapleAppUI

@Suite struct TeamSeatMismatchPresentationTests {
    private let english = Locale(identifier: "en")

    @Test(arguments: [
        ("admin", false, true), ("member", true, true), ("admin", true, true),
        ("member", false, false), ("ADMIN", false, false), ("owner", false, false),
    ])
    func eitherExplicitAdminSignalSelectsAdminCopy(_ role: String, _ isAdmin: Bool, _ expectedAdmin: Bool) throws {
        let status = try status(role: role, isAdmin: isAdmin)
        let presentation = try #require(TeamSeatMismatchPresentation(status: status))
        #expect(presentation.audience == (expectedAdmin ? .admin : .member))
        let resolution =
            expectedAdmin
            ? "Remove members to match the paid seat count and resume team usage."
            : "Contact your team admin to resolve the seat count."
        #expect(presentation.message(locale: english) == "This team has 3 members but only 2 paid seats. \(resolution)")
    }

    @Test func missingRoleUsesMemberCopyAndFallbackCounts() throws {
        let status = try decode([
            "has_team_subscription": true, "team_created": true, "members_count": 5, "seats_purchased": 3,
        ])
        let presentation = try #require(TeamSeatMismatchPresentation(status: status))
        #expect(presentation.audience == .member)
        #expect(presentation.exactCounts == .init(members: 5, paidSeats: 3))
        #expect(presentation.summary(locale: english) == "5 members, 3 paid seats")
    }

    @Test(arguments: [(1, 0), (2, 1), (3, 2)])
    func exactCountsUseIndependentMemberAndSeatPlurals(_ members: Int, _ paidSeats: Int) throws {
        let memberLabel = members == 1 ? "member" : "members"
        let seatLabel = paidSeats == 1 ? "seat" : "seats"
        for role in ["admin", "member"] {
            let presentation = try #require(
                TeamSeatMismatchPresentation(status: status(role: role, members: members, paidSeats: paidSeats)))
            #expect(
                presentation.summary(locale: english) == "\(members) \(memberLabel), \(paidSeats) paid \(seatLabel)")
            let resolution =
                role == "admin"
                ? "Remove members to match the paid seat count and resume team usage."
                : "Contact your team admin to resolve the seat count."
            #expect(
                presentation.message(locale: english)
                    == "This team has \(members) \(memberLabel) but only \(paidSeats) paid \(seatLabel). \(resolution)")
        }
    }

    @Test(arguments: [(0, 0), (2, 2), (2, 3)])
    func serverFlagWithoutGreaterExactCountsUsesGenericCopy(_ members: Int, _ paidSeats: Int) throws {
        let presentation = try #require(
            TeamSeatMismatchPresentation(
                status: status(role: "admin", members: members, paidSeats: paidSeats, exceeded: true)))
        #expect(presentation.exactCounts == nil)
        #expect(presentation.summary(locale: english) == "Seat count mismatch")
        #expect(
            presentation.message(locale: english)
                == "This team has more members than paid seats. Remove members to match the paid seat count and resume team usage."
        )
    }

    @Test func unknownCountsUseGenericMobileCopyForBothAudiences() throws {
        for role in ["admin", "member"] {
            let status = try decode([
                "has_team_subscription": true, "team_created": true, "role": role, "seat_limit_exceeded": true,
            ])
            let presentation = try #require(TeamSeatMismatchPresentation(status: status))
            #expect(presentation.exactCounts == nil)
            #expect(presentation.summary(locale: english) == "Seat count mismatch")
            let resolution =
                role == "admin"
                ? "Remove members to match the paid seat count and resume team usage."
                : "Contact your team admin to resolve the seat count."
            #expect(
                presentation.message(locale: english) == "This team has more members than paid seats. \(resolution)")
        }
    }

    @Test func absentTeamUncreatedTeamAndBalancedTeamDoNotShowNotice() throws {
        #expect(TeamSeatMismatchPresentation(status: nil) == nil)
        let uncreated = try decode([
            "has_team_subscription": true, "team_created": false, "seat_limit_exceeded": true,
            "team_member_count": 3, "billed_seat_count": 2,
        ])
        #expect(TeamSeatMismatchPresentation(status: uncreated) == nil)
        #expect(TeamSeatMismatchPresentation(status: try status(role: "admin", members: 2, paidSeats: 2)) == nil)
    }

    private func status(
        role: String, isAdmin: Bool = false, members: Int = 3, paidSeats: Int = 2, exceeded: Bool = false
    ) throws -> TeamStatus {
        try decode([
            "has_team_subscription": true, "team_created": true, "role": role, "is_team_admin": isAdmin,
            "team_member_count": members, "billed_seat_count": paidSeats, "seat_limit_exceeded": exceeded,
        ])
    }

    private func decode(_ values: [String: Any]) throws -> TeamStatus {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(TeamStatus.self, from: JSONSerialization.data(withJSONObject: values))
    }
}
