import Foundation
import MapleSDK

/// The captured SDK API holds its original credential generation throughout the
/// chat's lifetime. It cannot acquire a replacement account's authorization.
public struct SDKChatService: ChatServing {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
        -> Conversation
    {
        try await api.createConversation(metadata: metadata, projectId: projectId, pinned: pinned)
    }
    public func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
        -> AsyncThrowingStream<ResponseStreamEvent, Error>
    {
        try await api.createResponse(request, maximumInferenceSends: maximumInferenceSends)
    }
    public func retrieveResponse(id: String) async throws -> ResearchResponse { try await api.retrieveResponse(id: id) }
    public func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
        try await api.listConversations(parameters)
    }
    public func cancelResponse(id: String) async throws -> ResearchResponse { try await api.cancelResponse(id: id) }
    public func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
        try await api.getConversationItem(conversationId: conversationId, itemId: itemId)
    }
    public func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
        ConversationItem
    > {
        try await api.listConversationItems(conversationId: conversationId, parameters: parameters)
    }
}
