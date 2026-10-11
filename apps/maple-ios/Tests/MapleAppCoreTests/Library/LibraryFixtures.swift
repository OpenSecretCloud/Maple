import Foundation
import MapleSDK
@testable import MapleAppCore

func libraryConversation(_ id: String, project: String? = nil, pinned: Bool = false) -> Conversation {
    Conversation(
        id: id, object: "conversation", createdAt: 0, metadata: ["title": .string("Title \(id)")], projectId: project,
        pinned: pinned, lastActivityAt: 0)
}
func libraryInstruction(_ id: String, prompt: String = "original", isDefault: Bool = true) -> Instruction {
    Instruction(
        id: id, object: "instruction", name: "User Preferences", prompt: prompt, promptTokens: 2, isDefault: isDefault,
        createdAt: 0, updatedAt: 0)
}
actor LibraryFixture: LibraryServing {
    var conversations: [Conversation] = []
    var instructions: [Instruction] = []
    var projectInstructions: String? = "project original"
    var deleteCalls: [[String]] = []
    var moveCalls: [[String]] = []
    var moveGate: ChatTestGate?
    var failedMoveCall: Int?
    var projectDeletes: [String] = []
    var deleteAllCalls = 0
    var instructionChanges: [String] = []
    var updateMetadata: [String: JSONValue]?
    var conversationUpdateIDs: [String] = []
    var malformedConversationUpdate = false
    var failedDeletes: Set<String> = []
    var failList = false
    var failDelete = false
    var listGate: ChatTestGate?
    var projectDiscoveryGate: ChatTestGate?
    var repeatedCursor = false
    var updateGate: ChatTestGate?
    var instructionSaveGate: ChatTestGate?
    var instructionSaveFailure = false
    var instructionUpdateIDs: [String] = []
    var conversationListCalls: [ConversationsListParameters] = []
    var projects = [
        ConversationProjectListItem(id: "project", object: "project", name: "Project", createdAt: 0, updatedAt: 0)
    ]
    var projectsGate: ChatTestGate?
    var projectListCalls: [ListParameters] = []
    func configureProjects(_ values: [ConversationProjectListItem], gate: ChatTestGate? = nil) {
        projects = values; projectsGate = gate
    }
    func holdUpdate(_ gate: ChatTestGate?) { updateGate = gate }
    func returnMalformedConversationUpdate() { malformedConversationUpdate = true }
    func configureMove(gate: ChatTestGate? = nil, failedCall: Int? = nil) {
        moveGate = gate; failedMoveCall = failedCall
    }
    func holdInstructionSave(_ gate: ChatTestGate?, fail: Bool = false) {
        instructionSaveGate = gate; instructionSaveFailure = fail
    }
    func configure(
        conversations: [Conversation]? = nil, instructions: [Instruction]? = nil,
        failedDeletes: Set<String> = [], failList: Bool = false, failDelete: Bool = false,
        listGate: ChatTestGate? = nil, projectDiscoveryGate: ChatTestGate? = nil, repeatedCursor: Bool = false
    ) {
        if let conversations { self.conversations = conversations }
        if let instructions { self.instructions = instructions }
        self.failedDeletes = failedDeletes; self.failList = failList; self.failDelete = failDelete
        self.listGate = listGate; self.projectDiscoveryGate = projectDiscoveryGate; self.repeatedCursor = repeatedCursor
    }
    private func page<T: Codable & Sendable & Equatable>(_ values: [T], ids: [String], parameters: ListParameters)
        -> APIPage<T>
    {
        let start = parameters.after.flatMap { ids.firstIndex(of: $0).map { $0 + 1 } } ?? 0
        let end = min(values.count, start + (parameters.limit ?? 100))
        let data = Array(values[start..<end])
        return APIPage(
            data: data, firstId: start < ids.count ? ids[start] : nil, lastId: end > start ? ids[end - 1] : nil,
            hasMore: end < values.count)
    }
    func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
        conversationListCalls.append(parameters)
        if failList { throw URLError(.notConnectedToInternet) }
        let rows = conversations.filter {
            (parameters.projectId == nil || $0.projectId == parameters.projectId)
                && (parameters.unassignedProject != true || $0.projectId == nil)
                && (parameters.pinned == nil || $0.pinned == parameters.pinned)
        }
        let result = page(rows, ids: rows.map(\.id), parameters: parameters.page)
        if let listGate { await listGate.hold() }
        if parameters.projectId != nil, let projectDiscoveryGate { await projectDiscoveryGate.hold() }
        if repeatedCursor { return APIPage(data: Array(rows.prefix(1)), lastId: "same", hasMore: true) }
        return result
    }
    func getConversation(id: String) throws -> Conversation { try require(conversations.first { $0.id == id }) }
    func updateConversation(
        id: String, metadata: [String: JSONValue]?, projectId: NullableUpdate<String>, pinned: Bool?
    ) async throws -> Conversation {
        conversationUpdateIDs.append(id)
        if let updateGate { await updateGate.hold() }
        let index = try require(conversations.firstIndex { $0.id == id })
        updateMetadata = metadata
        if let metadata { conversations[index].metadata = metadata }
        if let pinned { conversations[index].pinned = pinned }
        switch projectId {
        case .clear: conversations[index].projectId = nil;
        case .set(let id): conversations[index].projectId = id;
        case .unchanged: break
        }
        if malformedConversationUpdate { return libraryConversation("wrong-id") }
        return conversations[index]
    }
    func deleteConversation(id: String) throws -> DeletedObject {
        if failDelete { throw URLError(.cannotConnectToHost) }
        conversations.removeAll { $0.id == id }; return DeletedObject(id: id, object: "conversation", deleted: true)
    }
    func deleteAllConversations() throws -> DeletedObject {
        deleteAllCalls += 1
        if failDelete { throw URLError(.cannotConnectToHost) }
        conversations = []; return DeletedObject(object: "conversation", deleted: true)
    }
    func batchDeleteConversations(ids: [String]) throws -> BatchDeleteResponse {
        guard ids.count <= 20 else { throw ResearchAPIError.invalidArgument }
        deleteCalls.append(ids)
        if failDelete { throw URLError(.cannotConnectToHost) }
        conversations.removeAll { ids.contains($0.id) && !failedDeletes.contains($0.id) }
        return BatchDeleteResponse(
            object: "list",
            data: ids.map { BatchDeleteItem(id: $0, object: "conversation", deleted: !failedDeletes.contains($0)) })
    }
    func batchUpdateConversationProject(ids: [String], projectId: String?) async throws -> BatchUpdateProjectResponse {
        guard ids.count <= 20 else { throw ResearchAPIError.invalidArgument }
        moveCalls.append(ids)
        if let moveGate { await moveGate.hold() }
        if failedMoveCall == moveCalls.count { throw URLError(.cannotConnectToHost) }
        for index in conversations.indices where ids.contains(conversations[index].id) {
            conversations[index].projectId = projectId
        }
        return BatchUpdateProjectResponse(success: true)
    }
    func listProjects(_ parameters: ListParameters) async -> APIPage<ConversationProjectListItem> {
        projectListCalls.append(parameters)
        let result = page(projects, ids: projects.map(\.id), parameters: parameters)
        if let projectsGate { await projectsGate.hold() }
        return result
    }
    func getProject(id: String) -> ConversationProject {
        ConversationProject(
            id: id, object: "project", name: "Project", instructions: projectInstructions, createdAt: 0, updatedAt: 0)
    }
    func createProject(name: String) -> ConversationProject {
        ConversationProject(id: "created-project", object: "project", name: name, createdAt: 0, updatedAt: 0)
    }
    func updateProject(id: String, name: String?, instructions: NullableUpdate<String>) -> ConversationProject {
        switch instructions {
        case .clear: projectInstructions = nil;
        case .set(let value): projectInstructions = value;
        case .unchanged: break
        }
        return ConversationProject(
            id: id, object: "project", name: name ?? "Project", instructions: projectInstructions, createdAt: 0,
            updatedAt: 0)
    }
    func deleteProject(id: String) throws -> DeletedObject {
        projectDeletes.append(id)
        if failDelete { throw URLError(.cannotConnectToHost) }
        conversations.removeAll { $0.projectId == id }; return DeletedObject(id: id, object: "project", deleted: true)
    }
    func listInstructions(_ parameters: ListParameters) async throws -> APIPage<Instruction> {
        if failList { throw URLError(.cannotConnectToHost) }
        if let listGate { await listGate.hold() }
        return page(instructions, ids: instructions.map(\.id), parameters: parameters)
    }
    func createInstruction(name: String, prompt: String, isDefault: Bool?) -> Instruction {
        instructionChanges.append("create");
        let value = libraryInstruction("created", prompt: prompt, isDefault: isDefault ?? false);
        instructions.append(value); return value
    }
    func updateInstruction(id: String, name: String?, prompt: String?, isDefault: Bool?) async throws -> Instruction {
        instructionUpdateIDs.append(id)
        if let instructionSaveGate { await instructionSaveGate.hold() }
        if instructionSaveFailure { throw URLError(.cannotConnectToHost) }
        instructionChanges.append("update");
        return libraryInstruction(id, prompt: prompt ?? "", isDefault: isDefault ?? true)
    }
    func deleteInstruction(id: String) -> DeletedObject {
        instructionChanges.append("delete"); instructions.removeAll { $0.id == id };
        return DeletedObject(id: id, object: "instruction", deleted: true)
    }
    private func require<T>(_ value: T?) throws -> T {
        guard let value else { throw ResearchAPIError.invalidResponse }; return value
    }
}
