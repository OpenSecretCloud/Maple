import Foundation

/// Stable draft ownership survives chat assignment and navigation. A queue
/// editor has its own draft ID, distinct from its held composer and original row.
public struct DictationDraftOwner: Equatable, Sendable {
    public let scope: AccountScope
    public let storageOwnerID: UUID
    public let draftID: UUID
    public init(scope: AccountScope, storageOwnerID: UUID, draftID: UUID) {
        self.scope = scope; self.storageOwnerID = storageOwnerID; self.draftID = draftID
    }
}

/// Binds the narrow Send-after-dictation transition to its original draft and
/// one durable accepted turn. It retains identifiers only, never draft text.
public struct DictationSendAssociation: Sendable {
    public struct Ticket: Equatable, Sendable {
        fileprivate let owner: DictationDraftOwner
        fileprivate let epoch: UUID
    }
    public struct Arm: Equatable, Sendable {
        public let chatID: String
        public let messageID: String
    }
    private var review: Ticket?
    private var submitted: Ticket?
    private var awaitingConversation: (ticket: Ticket, messageID: String)?
    public init() {}

    public mutating func captureAdmitted(_ owner: DictationDraftOwner) {
        reset()
        review = Ticket(owner: owner, epoch: UUID())
    }
    public func ticket(for owner: DictationDraftOwner, reviewActive: Bool) -> Ticket? {
        guard reviewActive, review?.owner == owner else { return nil }
        return review
    }
    /// Call only after explicit Send returned true. Persistence acceptance remains
    /// a separate callback, so a failed save cannot arm automatic replacement.
    public mutating func didSubmit(_ ticket: Ticket) {
        guard review == ticket else { return }
        submitted = ticket
    }
    public mutating func accepted(_ value: ChatSendAcceptance, reviewActive: Bool) -> Arm? {
        let owner = DictationDraftOwner(
            scope: value.scope, storageOwnerID: value.storageOwnerID, draftID: value.draftID)
        guard reviewActive, let ticket = submitted, review == ticket, ticket.owner == owner else { return nil }
        submitted = nil
        if let chatID = value.conversationID {
            reset()
            return Arm(chatID: chatID, messageID: value.messageID)
        }
        awaitingConversation = (ticket, value.messageID)
        return nil
    }
    public mutating func conversationAssigned(
        scope: AccountScope, storageOwnerID: UUID, chatID: String, reviewActive: Bool
    ) -> Arm? {
        guard reviewActive, let pending = awaitingConversation, review == pending.ticket,
            pending.ticket.owner.scope == scope, pending.ticket.owner.storageOwnerID == storageOwnerID
        else { return nil }
        reset()
        return Arm(chatID: chatID, messageID: pending.messageID)
    }
    public mutating func retire(_ owner: DictationDraftOwner) {
        if review?.owner == owner { reset() }
    }
    public mutating func reset() { review = nil; submitted = nil; awaitingConversation = nil }
}
