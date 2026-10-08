import Foundation

public struct ChatQueuedTurn: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID
    public var messageID: String
    public var draft: ChatDraft
    public var options: ChatSendOptions

    public init(
        id: UUID = UUID(), messageID: String = UUID().uuidString.lowercased(),
        draft: ChatDraft, options: ChatSendOptions = ChatSendOptions()
    ) {
        self.id = id; self.messageID = messageID; self.draft = draft; self.options = options
    }
}

public struct ChatQueueEdit: Codable, Sendable, Equatable {
    public var original: ChatQueuedTurn
    public var originalIndex: Int
    public var heldDraft: ChatDraft

    public init(original: ChatQueuedTurn, originalIndex: Int, heldDraft: ChatDraft) {
        self.original = original; self.originalIndex = originalIndex; self.heldDraft = heldDraft
    }
}

public struct ChatQueueState: Codable, Sendable, Equatable {
    public var items: [ChatQueuedTurn]
    public var edit: ChatQueueEdit?
    public var paused: Bool
    /// A previously admitted active turn can return ahead of sixteen waiting
    /// turns. This one recovery slot never admits an additional new message.
    public var recoveredHeadID: UUID?

    public init(
        items: [ChatQueuedTurn] = [], edit: ChatQueueEdit? = nil, paused: Bool = false,
        recoveredHeadID: UUID? = nil
    ) {
        self.items = items; self.edit = edit; self.paused = paused
        self.recoveredHeadID = recoveredHeadID
    }
}

extension ChatSnapshot {
    /// Includes original, working and held drafts. Identical references may be
    /// shared, but differing payload versions are never hidden by a reused ID.
    public var ownedDrafts: [ChatDraft] {
        var drafts = [draft]
        if let outgoing = pendingRun?.outgoingDraft { drafts.append(outgoing) }
        if let queued = pendingRun?.queuedTurn { drafts.append(queued.draft) }
        if let unsentTurn { drafts.append(unsentTurn) }
        drafts += queue?.items.map(\.draft) ?? []
        if let edit = queue?.edit { drafts += [edit.original.draft, edit.heldDraft] }
        return drafts
    }
}

/// The account registry supplies every retained snapshot, substituting a
/// proposed transition for its current owner before admitting more resources.
public struct ChatResourceUsage: Sendable, Equatable {
    public private(set) var retainedTurns = 0
    public private(set) var images = 0
    public private(set) var imageBytes = 0

    public init(snapshots: [ChatSnapshot]) {
        var turns = Set<UUID>()
        var imageVersions: [UUID: [ChatImageAttachment]] = [:]
        for snapshot in snapshots {
            for item in snapshot.queue?.items ?? [] { turns.insert(item.id) }
            if let edited = snapshot.queue?.edit?.original { turns.insert(edited.id) }
            if let queued = snapshot.pendingRun?.queuedTurn { turns.insert(queued.id) }
            for draft in snapshot.ownedDrafts {
                for image in draft.images where !(imageVersions[image.id]?.contains(image) ?? false) {
                    imageVersions[image.id, default: []].append(image)
                    images += 1; imageBytes += image.data.count
                }
            }
        }
        retainedTurns = turns.count
    }

    public var fitsAccount: Bool { retainedTurns <= 64 && images <= 640 && imageBytes <= 256 * 1_024 * 1_024 }
}
