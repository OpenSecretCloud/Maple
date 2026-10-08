#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI

    /// Only remote services and storage are synthetic. Navigation destinations,
    /// selection, typed confirmation and deletion ownership use shipping code.
    /// This fixture constructs no SDK client and can never reach a real account.
    struct LibraryControlsFixtureView: View {
        @State private var model = LibraryControlsFixtureModel()
        var body: some View {
            VStack(spacing: 0) {
                NavigationStack {
                    List {
                        NavigationLink("History fixture") {
                            HistoryView(library: model.library, onSelectChat: { model.selected = $0 })
                        }.accessibilityIdentifier("library.fixture.history")
                        NavigationLink("Project fixture") {
                            ProjectView(
                                project: model.project, library: model.library,
                                makeInstructionEditor: model.makeInstructions,
                                onSelectChat: { model.selected = $0 }, onNewChat: { _ in })
                        }.accessibilityIdentifier("library.fixture.project")
                    }
                    .navigationTitle("Synthetic library controls")
                    .disabled(!model.ready)
                }
                .frame(maxHeight: .infinity)
                diagnostics.fixedSize(horizontal: false, vertical: true)
            }
            .task { await model.prepare() }
        }

        private var diagnostics: some View {
            VStack(alignment: .leading, spacing: 2) {
                Text(model.ready ? "ready" : "preparing").accessibilityIdentifier("library.fixture.ready")
                Text(
                    "Remaining "
                        + (model.service.conversations.isEmpty
                            ? "none" : model.service.conversations.map(\.id).joined(separator: ","))
                )
                .accessibilityIdentifier("library.fixture.remaining")
                Text(model.service.projectExists ? "project retained" : "project deleted")
                    .accessibilityIdentifier("library.fixture.project-state")
                Text(model.service.events.joined(separator: "|"))
                    .accessibilityIdentifier("library.fixture.events")
                Text(
                    model.service.lastMove.sorted().joined(separator: ",") + " to "
                        + (model.service.lastMoveProject ?? "none")
                )
                .accessibilityIdentifier("library.fixture.moved")
                Text(model.service.lastDelete.sorted().joined(separator: ","))
                    .accessibilityIdentifier("library.fixture.deleted")
                Text("pages \(model.service.pageRequests)")
                    .accessibilityIdentifier("library.fixture.pages")
                Text(model.runtime.queuedTurns.map(\.draft.text).joined(separator: "|"))
                    .accessibilityIdentifier("library.fixture.queue-items")
                Text(model.runtime.isQueuePaused ? "paused" : "running")
                    .accessibilityIdentifier("library.fixture.queue")
                Text("Draft " + (model.runtime.draftText.isEmpty ? "empty" : model.runtime.draftText))
                    .accessibilityIdentifier("library.fixture.draft")
                Text(model.removed ? "removed" : "retained").accessibilityIdentifier("library.fixture.owner")
            }.font(.caption2).frame(maxWidth: .infinity, alignment: .leading).padding(6)
                .background(.regularMaterial)
        }
    }

    @MainActor @Observable private final class LibraryControlsFixtureModel {
        let scope: AccountScope
        let service: LibraryControlsFixtureService
        let runtime: ChatRuntime
        let library: LibraryModel
        private let deletions: ChatDeletionCoordinator
        var selected = ""
        var ready = false
        var removed = false
        private var preparing = false
        let project = ConversationProjectListItem(
            id: "fixture-project", object: "project", name: "Synthetic project", createdAt: 0, updatedAt: 0)

        init() {
            let arguments = ProcessInfo.processInfo.arguments
            let scope = AccountScope(apiOrigin: "http://127.0.0.1:1", principalID: "synthetic-library-controls")
            self.scope = scope
            let service = LibraryControlsFixtureService(
                failPage: arguments.contains("--maple-library-controls-page-failure"),
                failDelete: arguments.contains("--maple-library-controls-delete-failure"))
            let store = LibraryControlsFixtureStore()
            let runtime = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "active")
            let deletions = ChatDeletionCoordinator(scope: scope, retained: [], service: service, persistence: store)
            self.service = service; self.runtime = runtime; self.deletions = deletions
            library = LibraryModel(scope: scope, service: service, deletions: deletions)
            deletions.register(runtime, projectID: "fixture-project")
            deletions.onRemoved = { [weak self] owner in
                if owner === self?.runtime { self?.removed = true }
            }
        }
        func prepare() async {
            guard !ready, !preparing else { return }; preparing = true
            defer { preparing = false }
            if ProcessInfo.processInfo.arguments.contains("--maple-library-controls-active") {
                runtime.draftText = "Synthetic active request"
                guard runtime.send() else { return }
                // A real runtime must consume created before the UI admits deletion.
                for _ in 0..<200 {
                    if runtime.snapshot.pendingRun?.responseID == "fixture-response" { break }
                    try? await Task<Never, Never>.sleep(nanoseconds: 10_000_000)
                }
                guard runtime.snapshot.pendingRun?.responseID == "fixture-response" else { return }
                runtime.draftText = "Synthetic queued request"
                guard runtime.enqueueDraft() else { return }
                runtime.draftText = "Unsent draft survives a failed deletion"
                await runtime.saveDraft()
            }
            ready = true
        }
        func makeInstructions(_ destination: InstructionDestination) -> InstructionEditor {
            InstructionEditor(scope: scope, destination: destination, service: service)
        }
    }

    private actor LibraryControlsFixtureStore: ChatPersistence {
        var values: [String: ChatSnapshot] = [:]
        func load(chatID: String, scope: AccountScope) -> ChatSnapshot? { values[chatID] }
        func loadAll(scope: AccountScope) -> [ChatSnapshot] { Array(values.values) }
        func save(_ snapshot: ChatSnapshot, scope: AccountScope) {
            if (values[snapshot.storageKey]?.revision ?? 0) <= snapshot.revision {
                values[snapshot.storageKey] = snapshot
            }
        }
        func remove(chatID: String, scope: AccountScope) { values.removeValue(forKey: chatID) }
        func rekey(from: String, to: ChatSnapshot, scope: AccountScope) {
            values.removeValue(forKey: from); values[to.storageKey] = to
        }
    }

    @MainActor @Observable private final class LibraryControlsFixtureService: LibraryServing, ChatServing {
        var conversations: [Conversation] = [
            LibraryControlsFixtureService.conversation("alpha", title: "Alpha chat"),
            LibraryControlsFixtureService.conversation("beta", title: "Beta chat"),
            LibraryControlsFixtureService.conversation(
                "active", title: "Active project chat", project: "fixture-project"),
            LibraryControlsFixtureService.conversation("outside", title: "Outside chat"),
        ]
        var projectExists = true
        var events: [String] = []
        var lastMove: [String] = []
        var lastMoveProject: String?
        var lastDelete: [String] = []
        var pageRequests = 0
        private var failPage: Bool
        private var failDelete: Bool
        private var responseStatus: ResponseStatus = .inProgress
        private var continuation: AsyncThrowingStream<ResponseStreamEvent, Error>.Continuation?
        private var messageID: String?

        init(failPage: Bool, failDelete: Bool) { self.failPage = failPage; self.failDelete = failDelete }
        private static func conversation(_ id: String, title: String, project: String? = nil) -> Conversation {
            Conversation(
                id: id, object: "conversation", createdAt: 0, metadata: ["title": .string(title)],
                projectId: project, pinned: false, lastActivityAt: 0)
        }
        func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
            pageRequests += 1
            if parameters.page.after != nil, failPage {
                failPage = false; throw URLError(.notConnectedToInternet)
            }
            let rows = conversations.filter {
                (parameters.projectId == nil || $0.projectId == parameters.projectId)
                    && (parameters.unassignedProject != true || $0.projectId == nil)
                    && (parameters.pinned == nil || $0.pinned == parameters.pinned)
            }
            let start =
                parameters.page.after.flatMap { after in rows.firstIndex { $0.id == after }.map { $0 + 1 } } ?? 0
            let end = min(rows.count, start + 2)
            let values = Array(rows[start..<end])
            return APIPage(data: values, firstId: values.first?.id, lastId: values.last?.id, hasMore: end < rows.count)
        }
        func getConversation(id: String) async throws -> Conversation {
            guard let result = conversations.first(where: { $0.id == id }) else {
                throw ResearchAPIError.invalidArgument
            }
            return result
        }
        func updateConversation(
            id: String, metadata: [String: JSONValue]?, projectId: NullableUpdate<String>, pinned: Bool?
        ) async throws -> Conversation {
            guard let index = conversations.firstIndex(where: { $0.id == id }) else {
                throw ResearchAPIError.invalidArgument
            }
            if let metadata { conversations[index].metadata = metadata }
            if let pinned { conversations[index].pinned = pinned }
            switch projectId {
            case .clear: conversations[index].projectId = nil
            case .set(let id): conversations[index].projectId = id
            case .unchanged: break
            }
            return conversations[index]
        }
        func deleteConversation(id: String) async throws -> DeletedObject {
            throw ResearchAPIError.invalidArgument
        }
        func deleteAllConversations() async throws -> DeletedObject {
            events.append("delete-all"); conversations = []
            return DeletedObject(object: "conversation", deleted: true)
        }
        func batchDeleteConversations(ids: [String]) async throws -> BatchDeleteResponse {
            guard !ids.isEmpty, ids.count <= 20 else { throw ResearchAPIError.invalidArgument }
            lastDelete = ids; events.append("delete-selected")
            conversations.removeAll { ids.contains($0.id) }
            return BatchDeleteResponse(
                object: "list",
                data: ids.map {
                    BatchDeleteItem(id: $0, object: "conversation", deleted: true)
                })
        }
        func batchUpdateConversationProject(ids: [String], projectId: String?) async throws
            -> BatchUpdateProjectResponse
        {
            guard !ids.isEmpty, ids.count <= 20 else { throw ResearchAPIError.invalidArgument }
            lastMove = ids; lastMoveProject = projectId; events.append("move-selected")
            for index in conversations.indices where ids.contains(conversations[index].id) {
                conversations[index].projectId = projectId
            }
            return BatchUpdateProjectResponse(success: true)
        }
        func listProjects(_ parameters: ListParameters) async throws -> APIPage<ConversationProjectListItem> {
            APIPage(
                data: projectExists
                    ? [
                        ConversationProjectListItem(
                            id: "fixture-project", object: "project", name: "Synthetic project", createdAt: 0,
                            updatedAt: 0)
                    ] : [])
        }
        func getProject(id: String) async throws -> ConversationProject {
            guard projectExists, id == "fixture-project" else { throw ResearchAPIError.invalidArgument }
            return ConversationProject(id: id, object: "project", name: "Synthetic project", createdAt: 0, updatedAt: 0)
        }
        func createProject(name: String) async throws -> ConversationProject { throw ResearchAPIError.invalidArgument }
        func updateProject(id: String, name: String?, instructions: NullableUpdate<String>) async throws
            -> ConversationProject
        { throw ResearchAPIError.invalidArgument }
        func deleteProject(id: String) async throws -> DeletedObject {
            guard id == "fixture-project", responseStatus == .cancelled else { throw ResearchAPIError.invalidArgument }
            if failDelete { failDelete = false; events.append("delete-failed"); throw URLError(.cannotConnectToHost) }
            events.append("delete-project"); projectExists = false
            conversations.removeAll { $0.projectId == id }
            return DeletedObject(id: id, object: "project", deleted: true)
        }
        func listInstructions(_ parameters: ListParameters) async throws -> APIPage<Instruction> { APIPage(data: []) }
        func createInstruction(name: String, prompt: String, isDefault: Bool?) async throws -> Instruction {
            throw ResearchAPIError.invalidArgument
        }
        func updateInstruction(id: String, name: String?, prompt: String?, isDefault: Bool?) async throws -> Instruction
        { throw ResearchAPIError.invalidArgument }
        func deleteInstruction(id: String) async throws -> DeletedObject { throw ResearchAPIError.invalidArgument }
        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
            -> Conversation
        { throw ResearchAPIError.invalidArgument }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        {
            guard request.conversation == "active",
                case .some(.string(let id)) = request.metadata?["internal_message_id"], messageID == nil
            else { throw ResearchAPIError.invalidArgument }
            messageID = id; events.append("post")
            let stream = AsyncThrowingStream<ResponseStreamEvent, Error>.makeStream()
            continuation = stream.continuation
            struct Wrapper: Encodable { let response: ResearchResponse }
            let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            let event = try decoder.decode(
                ResponseLifecycleEvent.self, from: encoder.encode(Wrapper(response: response())))
            stream.continuation.yield(.created(event)); return stream.stream
        }
        func retrieveResponse(id: String) async throws -> ResearchResponse {
            guard id == "fixture-response" else { throw ResearchAPIError.invalidArgument }
            return response()
        }
        func cancelResponse(id: String) async throws -> ResearchResponse {
            guard id == "fixture-response" else { throw ResearchAPIError.invalidArgument }
            events.append("cancel-request")
            try await Task<Never, Never>.sleep(nanoseconds: 300_000_000)
            responseStatus = .cancelled; events.append("cancel-confirm")
            return response()
        }
        func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
            guard conversationId == "active", itemId == messageID else {
                throw ResearchAPIError.http(status: 404, reason: .notFound)
            }
            return ConversationItem(id: itemId, responseId: "fixture-response", type: "message", role: "user")
        }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
            ConversationItem
        > { APIPage(data: []) }
        private func response() -> ResearchResponse {
            ResearchResponse(
                id: "fixture-response", object: "response", createdAt: 0, status: responseStatus, model: "auto:quick")
        }
    }
#endif
