import Foundation
import MapleSDK

public protocol ChatServing: Sendable {
    func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
        -> Conversation
    func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation>
    func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
        -> AsyncThrowingStream<ResponseStreamEvent, Error>
    func retrieveResponse(id: String) async throws -> ResearchResponse
    func cancelResponse(id: String) async throws -> ResearchResponse
    func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem
    func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
        ConversationItem
    >
}

/// Implementations serialize saves by revision and reject writes from retired
/// account generations, including writes already waiting when retirement begins.
public protocol ChatPersistence: Sendable {
    func load(chatID: String, scope: AccountScope) async throws -> ChatSnapshot?
    func loadAll(scope: AccountScope) async throws -> [ChatSnapshot]
    func save(_ snapshot: ChatSnapshot, scope: AccountScope) async throws
    func remove(chatID: String, scope: AccountScope) async throws
    func rekey(from: String, to: ChatSnapshot, scope: AccountScope) async throws
}

public struct ChatImageAttachment: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID
    public var data: Data
    public var contentType: String

    public init(id: UUID = UUID(), data: Data, contentType: String) {
        self.id = id; self.data = data; self.contentType = contentType
    }
}

public struct ChatDraft: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID
    public var text: String
    public var images: [ChatImageAttachment]

    public init(id: UUID = UUID(), text: String = "", images: [ChatImageAttachment] = []) {
        self.id = id; self.text = text; self.images = images
    }

    public var isEmpty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && images.isEmpty }
}

public struct ChatSendOptions: Codable, Sendable, Equatable {
    public var model: String
    public var webSearch: Bool
    public var projectID: String?

    public init(model: String = "auto:quick", webSearch: Bool = false, projectID: String? = nil) {
        self.model = model; self.webSearch = webSearch; self.projectID = projectID
    }
}

public enum ChatRunPhase: String, Codable, Sendable { case preparing, sending, streaming, recovering, stopping }

public struct ChatPendingRun: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID
    public var messageID: String
    public var outgoingDraft: ChatDraft
    public var responseID: String?
    public var phase: ChatRunPhase
    public var stopRequested: Bool
    public var dispatched: Bool
    public var options: ChatSendOptions
    public var queuedTurn: ChatQueuedTurn?
    /// A distinct, durable create request owner, separate from the Responses UUID.
    public var creationID: String?
    public var creationDispatched: Bool?

    public init(id: UUID, messageID: String, outgoingDraft: ChatDraft, options: ChatSendOptions) {
        self.id = id; self.messageID = messageID; self.outgoingDraft = outgoingDraft
        responseID = nil; phase = .preparing; stopRequested = false; dispatched = false; self.options = options
    }
}

public struct ChatSnapshot: Codable, Sendable, Equatable {
    public static let newChatKey = "new-chat"
    public var storageKey: String
    /// Stable across composer changes, editing and run settlement. Older
    /// snapshots adopt their original draft ID before the first mutation.
    public var storageOwnerID: UUID?
    public var storageOwner: UUID { storageOwnerID ?? draft.id }
    public var conversationID: String?
    public var draft: ChatDraft
    public var pendingRun: ChatPendingRun?
    /// A provably rejected turn remains available if the user has already typed
    /// a different composer draft. Restoring it must never overwrite that draft.
    public var unsentTurn: ChatDraft?
    public var queue: ChatQueueState?
    public var revision: UInt64

    public init(
        storageKey: String = Self.newChatKey, storageOwnerID: UUID? = nil, conversationID: String? = nil,
        draft: ChatDraft = ChatDraft(), pendingRun: ChatPendingRun? = nil,
        unsentTurn: ChatDraft? = nil, queue: ChatQueueState? = nil, revision: UInt64 = 0
    ) {
        self.storageKey = storageKey; self.conversationID = conversationID; self.draft = draft
        self.storageOwnerID = storageOwnerID ?? draft.id
        self.pendingRun = pendingRun; self.unsentTurn = unsentTurn; self.queue = queue; self.revision = revision
    }
}

/// One successful durable settlement, retaining its original turn ownership.
public struct ChatCompletion: Sendable {
    public let scope: AccountScope
    public let runID: UUID
    public let conversationID: String
    public let messageID: String
    public let responseID: String?
    public let assistantItems: [ConversationItem]
    public let recovered: Bool
    public let wasStopped: Bool
    public let wasVisible: Bool
    public let wasForeground: Bool
    public var assistantText: String {
        var text: [String] = []
        for item in assistantItems {
            for part in item.content ?? [] {
                if part.type == "output_text" || part.type == "text", let value = part.text {
                    text.append(value)
                }
            }
        }
        return text.joined(separator: "\n")
    }
}

public enum ChatOutcome: String, Sendable { case completed, cancelled, failed, rejected }

/// An explicit Send or Enqueue accepted into protected local state. It is never
/// recreated by queue promotion, reconciliation or process restoration.
public struct ChatSendAcceptance: Sendable, Equatable {
    public let scope: AccountScope
    public let storageOwnerID: UUID
    public let draftID: UUID
    public let messageID: String
    public let conversationID: String?
}

/// Presentation maps these categories to localized strings. No provider body,
/// prompt, token, callback or underlying error description crosses this boundary.
public enum ChatIssue: String, Error, Sendable {
    case offline, busy, emptyDraft, invalidImages, draftTooLarge, storageUnavailable
    case conversationUnavailable, uncertainResponse, responseUnavailable, cancellationUnavailable
    case historyUnavailable, requestRejected, contextLimit, freeTierTokenLimit, usageLimit, modelUnavailable, signedOut
    case unsupportedValue, imageDescriptionUnavailable, inferenceCapacityUnavailable, generationFailed
    case stoppedBeforeSending, stoppingWhenResponseReady, responseFinishedBeforeStop, recoveredCompletion, recoveredStop
    case queueLimit, unfinishedQueueEdit, draftInUse
}

public struct ChatRuntimeHooks: Sendable {
    public var uuid: @Sendable () -> UUID
    public var sleep: @Sendable (Duration) async throws -> Void

    public init(
        uuid: @escaping @Sendable () -> UUID = UUID.init,
        sleep: (@Sendable (Duration) async throws -> Void)? = nil
    ) {
        self.uuid = uuid; self.sleep = sleep ?? Self.delay
    }

    private static func delay(_ duration: Duration) async throws {
        try Task.checkCancellation()
        guard duration > .zero else { return }
        let parts = duration.components
        let (seconds, overflow) = UInt64(clamping: parts.seconds).multipliedReportingOverflow(by: 1_000_000_000)
        let (nanoseconds, additionOverflow) = seconds.addingReportingOverflow(
            UInt64(clamping: parts.attoseconds / 1_000_000_000))
        // Use the non-generic cancellable primitive to avoid cross-module
        // Clock specialization failures in the pinned Apple toolchain.
        try await Task<Never, Never>.sleep(nanoseconds: overflow || additionOverflow ? UInt64.max : nanoseconds)
    }
}
