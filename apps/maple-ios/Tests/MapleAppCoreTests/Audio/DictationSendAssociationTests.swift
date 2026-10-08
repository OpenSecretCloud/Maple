import Foundation
import Testing
@testable import MapleAppCore

struct DictationSendAssociationTests {
    private func owner(scope: AccountScope = accountScope(), storage: UUID = UUID(), draft: UUID = UUID())
        -> DictationDraftOwner
    {
        DictationDraftOwner(scope: scope, storageOwnerID: storage, draftID: draft)
    }
    private func accepted(_ owner: DictationDraftOwner, chat: String? = "chat", message: String = "turn")
        -> ChatSendAcceptance
    {
        ChatSendAcceptance(
            scope: owner.scope, storageOwnerID: owner.storageOwnerID, draftID: owner.draftID,
            messageID: message, conversationID: chat)
    }

    @Test func anotherChatOrHeldComposerCannotAdoptReview() throws {
        var gate = DictationSendAssociation()
        let dictated = owner(), otherChat = owner(scope: dictated.scope),
            held = owner(scope: dictated.scope, storage: dictated.storageOwnerID)
        gate.captureAdmitted(dictated)
        #expect(gate.ticket(for: otherChat, reviewActive: true) == nil)
        #expect(gate.ticket(for: held, reviewActive: true) == nil)
        #expect(gate.ticket(for: dictated, reviewActive: false) == nil)
        let ticket = try #require(gate.ticket(for: dictated, reviewActive: true))
        gate.didSubmit(ticket)
        let unrelated = gate.accepted(accepted(otherChat), reviewActive: true)
        #expect(unrelated == nil)
        let own = gate.accepted(accepted(dictated), reviewActive: true)
        #expect(own?.chatID == "chat" && own?.messageID == "turn")
        let replay = gate.accepted(accepted(dictated), reviewActive: true)
        #expect(replay == nil)
    }

    @Test func onlyDurablyAcceptedExplicitSendCanArmTheTurn() throws {
        var gate = DictationSendAssociation()
        let draft = owner(); gate.captureAdmitted(draft)
        let ticket = try #require(gate.ticket(for: draft, reviewActive: true))
        let unsent = gate.accepted(accepted(draft), reviewActive: true)
        #expect(unsent == nil)
        gate.didSubmit(ticket)
        let noLongerReviewing = gate.accepted(accepted(draft), reviewActive: false)
        #expect(noLongerReviewing == nil)
        gate.captureAdmitted(draft)
        gate.didSubmit(ticket)  // A new permission request invalidated this lease.
        let stale = gate.accepted(accepted(draft), reviewActive: true)
        #expect(stale == nil)
    }

    @Test func newChatCreationKeepsStableOwnerAndRejectsLateAssignment() throws {
        var gate = DictationSendAssociation()
        let draft = owner(); gate.captureAdmitted(draft)
        let ticket = try #require(gate.ticket(for: draft, reviewActive: true)); gate.didSubmit(ticket)
        let waiting = gate.accepted(accepted(draft, chat: nil), reviewActive: true)
        #expect(waiting == nil)
        let other = gate.conversationAssigned(
            scope: draft.scope, storageOwnerID: UUID(), chatID: "unrelated", reviewActive: true)
        #expect(other == nil)
        let assigned = gate.conversationAssigned(
            scope: draft.scope, storageOwnerID: draft.storageOwnerID, chatID: "created", reviewActive: true)
        #expect(assigned?.chatID == "created" && assigned?.messageID == "turn")

        gate.captureAdmitted(draft)
        let next = try #require(gate.ticket(for: draft, reviewActive: true)); gate.didSubmit(next)
        _ = gate.accepted(accepted(draft, chat: nil), reviewActive: true)
        gate.captureAdmitted(owner(scope: draft.scope))
        let late = gate.conversationAssigned(
            scope: draft.scope, storageOwnerID: draft.storageOwnerID, chatID: "old", reviewActive: true)
        #expect(late == nil)
    }

    @Test func retirementAndSamePrincipalReplacementCannotAdoptOldReview() throws {
        var gate = DictationSendAssociation()
        let draft = owner(); gate.captureAdmitted(draft)
        let ticket = try #require(gate.ticket(for: draft, reviewActive: true)); gate.didSubmit(ticket)
        let replacementScope = AccountScope(apiOrigin: draft.scope.apiOrigin, principalID: draft.scope.principalID)
        let replacement = owner(scope: replacementScope, storage: draft.storageOwnerID, draft: draft.draftID)
        let otherGeneration = gate.accepted(accepted(replacement), reviewActive: true)
        #expect(otherGeneration == nil)
        gate.retire(draft)
        let retired = gate.accepted(accepted(draft), reviewActive: true)
        #expect(retired == nil)
        gate.captureAdmitted(draft); gate.reset()
        #expect(gate.ticket(for: draft, reviewActive: true) == nil)
    }
}
