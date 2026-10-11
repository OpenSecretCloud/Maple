import Foundation
import MapleSDK

public protocol LibraryServing: Sendable {
    func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation>
    func getConversation(id: String) async throws -> Conversation
    func updateConversation(
        id: String, metadata: [String: JSONValue]?, projectId: NullableUpdate<String>, pinned: Bool?
    ) async throws -> Conversation
    func deleteConversation(id: String) async throws -> DeletedObject
    func deleteAllConversations() async throws -> DeletedObject
    func batchDeleteConversations(ids: [String]) async throws -> BatchDeleteResponse
    func batchUpdateConversationProject(ids: [String], projectId: String?) async throws -> BatchUpdateProjectResponse
    func listProjects(_ parameters: ListParameters) async throws -> APIPage<ConversationProjectListItem>
    func getProject(id: String) async throws -> ConversationProject
    func createProject(name: String) async throws -> ConversationProject
    func updateProject(id: String, name: String?, instructions: NullableUpdate<String>) async throws
        -> ConversationProject
    func deleteProject(id: String) async throws -> DeletedObject
    func listInstructions(_ parameters: ListParameters) async throws -> APIPage<Instruction>
    func createInstruction(name: String, prompt: String, isDefault: Bool?) async throws -> Instruction
    func updateInstruction(id: String, name: String?, prompt: String?, isDefault: Bool?) async throws -> Instruction
    func deleteInstruction(id: String) async throws -> DeletedObject
}

public struct SDKLibraryService: LibraryServing {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
        try await api.listConversations(parameters)
    }
    public func getConversation(id: String) async throws -> Conversation { try await api.getConversation(id: id) }
    public func updateConversation(
        id: String, metadata: [String: JSONValue]?, projectId: NullableUpdate<String>, pinned: Bool?
    ) async throws -> Conversation {
        try await api.updateConversation(id: id, metadata: metadata, projectId: projectId, pinned: pinned)
    }
    public func deleteConversation(id: String) async throws -> DeletedObject {
        try await api.deleteConversation(id: id)
    }
    public func deleteAllConversations() async throws -> DeletedObject { try await api.deleteAllConversations() }
    public func batchDeleteConversations(ids: [String]) async throws -> BatchDeleteResponse {
        try await api.batchDeleteConversations(ids: ids)
    }
    public func batchUpdateConversationProject(ids: [String], projectId: String?) async throws
        -> BatchUpdateProjectResponse
    { try await api.batchUpdateConversationProject(ids: ids, projectId: projectId) }
    public func listProjects(_ parameters: ListParameters) async throws -> APIPage<ConversationProjectListItem> {
        try await api.listProjects(parameters)
    }
    public func getProject(id: String) async throws -> ConversationProject { try await api.getProject(id: id) }
    public func createProject(name: String) async throws -> ConversationProject {
        try await api.createProject(name: name)
    }
    public func updateProject(id: String, name: String?, instructions: NullableUpdate<String>) async throws
        -> ConversationProject
    { try await api.updateProject(id: id, name: name, instructions: instructions) }
    public func deleteProject(id: String) async throws -> DeletedObject { try await api.deleteProject(id: id) }
    public func listInstructions(_ parameters: ListParameters) async throws -> APIPage<Instruction> {
        try await api.listInstructions(parameters)
    }
    public func createInstruction(name: String, prompt: String, isDefault: Bool?) async throws -> Instruction {
        try await api.createInstruction(name: name, prompt: prompt, isDefault: isDefault)
    }
    public func updateInstruction(id: String, name: String?, prompt: String?, isDefault: Bool?) async throws
        -> Instruction
    { try await api.updateInstruction(id: id, name: name, prompt: prompt, isDefault: isDefault) }
    public func deleteInstruction(id: String) async throws -> DeletedObject { try await api.deleteInstruction(id: id) }
}

public enum LibraryFilter: Sendable, Equatable { case all, pinned, recent, unassigned, project(String) }
/// A menu drop changes project membership and pinning together. Pinning keeps
/// the existing project; the other destinations are mutually exclusive rows.
public enum LibraryConversationDestination: Sendable, Equatable, Hashable {
    case project(String), pinned, recent
}
public enum LibraryIssue: Error, Sendable, Equatable {
    case connection, invalidInput, invalidResponse, busy, retired, quiescence, cleanupRequired, partialMutation
    static func present(_ error: Error) -> LibraryIssue {
        if let issue = error as? LibraryIssue { return issue }
        if let deletion = error as? ChatDeletionError {
            switch deletion {
            case .cleanupRequired: return .cleanupRequired
            case .retired: return .retired
            default: return .quiescence
            }
        }
        return .connection
    }
}
