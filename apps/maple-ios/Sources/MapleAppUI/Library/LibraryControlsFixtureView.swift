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
        @Environment(\.horizontalSizeClass) private var sizeClass
        var body: some View {
            VStack(spacing: 0) {
                Group {
                    if model.optionsProof {
                        VStack(spacing: 16) {
                            Button {
                                model.showOptions = true
                            } label: {
                                Image(systemName: "slider.horizontal.3").frame(minWidth: 44, minHeight: 44)
                            }
                            .accessibilityLabel("Chat options")
                            .accessibilityIdentifier("chat-options.fixture.open")
                            Button("Manage projects") { model.showProjects = true }
                                .accessibilityIdentifier("chat-options.fixture.manage")
                            Text(model.selectedProjectID ?? "none")
                                .accessibilityIdentifier("chat-options.fixture.project")
                            Text(model.account.selectedModelID)
                                .accessibilityIdentifier("chat-options.fixture.model")
                            Text(model.preferences.value.webSearchPreferred ? "web on" : "web off")
                                .accessibilityIdentifier("chat-options.fixture.web")
                        }
                        .disabled(!model.ready)
                        .sheet(isPresented: $model.showOptions) {
                            ChatOptionsSheet(
                                entitlements: model.account, preferences: model.preferences, library: model.library,
                                regularWidth: sizeClass == .regular,
                                selectedProjectID: model.selectedProjectID,
                                makeInstructionEditor: model.makeInstructions,
                                isCurrent: { model.ready && model.showOptions },
                                onSelectProject: { id in
                                    guard model.ready, model.showOptions else { return false }
                                    model.selectedProjectID = id; return true
                                },
                                onSelectModel: { id in
                                    model.preferences.setSelectedModelID(id); model.showOptions = false
                                })
                        }
                        .sheet(isPresented: $model.showProjects) {
                            ProjectManagementSheet(
                                library: model.library, makeInstructionEditor: model.makeInstructions,
                                isCurrent: { model.ready && model.showProjects })
                        }
                    } else if model.dragProof || model.menuProof {
                        LibraryView(
                            library: model.library, makeLibrary: model.makeLibrary,
                            makeInstructionEditor: model.makeInstructions,
                            onSelectChat: model.selectChat, onNewChat: { _ in },
                            settings: {
                                AnyView(
                                    Text("Synthetic settings").accessibilityIdentifier("library.fixture.settings")
                                        .navigationTitle("Settings"))
                            },
                            account: model.accountProof ? model.account : nil,
                            conversationActivity: { model.parityProof ? model.activities[$0] ?? .idle : .idle },
                            projectActivity: { id in
                                model.parityProof && id == "fixture-project"
                                    ? model.activities["active"] ?? .idle : .idle
                            }, drawerNavigation: model.accountProof)
                    } else {
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
                    }
                }
                .frame(maxHeight: .infinity)
                Group {
                    if model.dragProof {
                        dragDiagnostics
                    } else if model.menuProof {
                        menuDeletionDiagnostics
                    } else {
                        diagnostics
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
            }
            .task { await model.prepare() }
        }

        private var menuDeletionDiagnostics: some View {
            VStack(alignment: .leading, spacing: 0) {
                Text(model.ready ? "ready" : "preparing").accessibilityIdentifier("library.fixture.ready")
                Text("deletions \(model.service.events.filter { $0.hasPrefix("delete") }.count)")
                    .accessibilityIdentifier("library.fixture.deletion-count")
                Text("Remaining " + model.service.conversations.map(\.id).joined(separator: ","))
                    .accessibilityIdentifier("library.fixture.remaining")
                Text(model.service.lastDelete.sorted().joined(separator: ","))
                    .accessibilityIdentifier("library.fixture.deleted")
                Text(model.service.events.joined(separator: "|"))
                    .accessibilityIdentifier("library.fixture.events")
            }
            .font(.caption2).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 6).padding(.vertical, 3)
            .frame(height: 76).background(.regularMaterial)
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
                Text("deletions \(model.service.events.filter { $0.hasPrefix("delete") }.count)")
                    .accessibilityIdentifier("library.fixture.deletion-count")
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
        private var dragDiagnostics: some View {
            let alpha = model.service.conversations.first { $0.id == "alpha" }
            let state =
                alpha.map {
                    "alpha:" + ($0.projectId ?? "recent") + ":" + ($0.pinned ? "pinned" : "unpinned")
                } ?? "alpha:missing"
            let trace = LibraryChatDragTrace.shared.summary
            return VStack(alignment: .leading, spacing: 0) {
                Text(model.ready ? "ready" : "preparing")
                    .accessibilityIdentifier("library.fixture.ready")
                    .frame(height: 14)
                Text(state).accessibilityIdentifier("library.fixture.alpha-state")
                    .accessibilityLabel(Text(verbatim: state)).frame(height: 14)
                Text(trace).accessibilityIdentifier("library.fixture.drag-trace")
                    .accessibilityLabel(Text(verbatim: trace)).frame(height: 14)
            }
            // The fixture must not resize the shipping List as event counts
            // change, or shift a native drag's pre-resolved release target.
            .font(.caption2).lineLimit(1)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 6).padding(.vertical, 3)
            .frame(height: 48).background(.regularMaterial)
        }
    }

    @MainActor @Observable private final class LibraryControlsFixtureModel {
        let scope: AccountScope
        let service: LibraryControlsFixtureService
        let runtime: ChatRuntime
        let library: LibraryModel
        let dragProof: Bool
        let menuProof: Bool
        let optionsProof: Bool
        let parityProof: Bool
        let accountProof: Bool
        let account: AccountEntitlements
        let preferences: PreferencesController
        var selectedProjectID: String?
        var showOptions = false
        var showProjects = false
        private let deletions: ChatDeletionCoordinator
        @ObservationIgnored private var menuLibraries: [LibraryModel] = []
        var selected = ""
        var ready = false
        var activities: [String: ConversationActivity] = ["alpha": .running, "beta": .unread, "active": .unread]
        var removed = false
        private var preparing = false
        let project = ConversationProjectListItem(
            id: "fixture-project", object: "project", name: "Synthetic project", createdAt: 0, updatedAt: 0)

        init() {
            let arguments = ProcessInfo.processInfo.arguments
            let dragProof = arguments.contains("--maple-library-drag-proof")
            let optionsProof = arguments.contains("--maple-chat-options-proof")
            let parityProof = arguments.contains("--maple-library-parity-proof")
            let accountProof = arguments.contains("--maple-library-account-proof")
            self.dragProof = dragProof
            menuProof = arguments.contains("--maple-library-menu-proof") || parityProof || accountProof
            self.optionsProof = optionsProof
            self.parityProof = parityProof
            self.accountProof = accountProof
            let scope = AccountScope(apiOrigin: "http://127.0.0.1:1", principalID: "synthetic-library-controls")
            self.scope = scope
            let service = LibraryControlsFixtureService(
                failPage: arguments.contains("--maple-library-controls-page-failure"),
                failDelete: arguments.contains("--maple-library-controls-delete-failure"), dragProof: dragProof,
                optionsProof: optionsProof || parityProof)
            let store = LibraryControlsFixtureStore()
            let runtime = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "active")
            let deletions = ChatDeletionCoordinator(scope: scope, retained: [], service: service, persistence: store)
            self.service = service; self.runtime = runtime; self.deletions = deletions
            library = LibraryModel(scope: scope, service: service, deletions: deletions)
            let accountService = ChatOptionsFixtureAccountService(accountProof: accountProof)
            account = AccountEntitlements(
                scope: scope, isGuest: false, billing: accountService, catalog: accountService)
            preferences = PreferencesController(scope: scope, store: ChatOptionsFixturePreferences())
            deletions.register(runtime, projectID: "fixture-project")
            deletions.onRemoved = { [weak self] owner in
                if owner === self?.runtime { self?.removed = true }
            }
            attach(library)
        }
        func prepare() async {
            guard !ready, !preparing else { return }; preparing = true
            defer { preparing = false }
            if optionsProof {
                await preferences.load(); await account.refresh(); await library.loadProjects()
            } else if accountProof {
                await account.refresh()
            }
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
        func selectChat(_ id: String) {
            selected = id
            if activities[id] == .unread { activities[id] = .idle }
        }
        func makeLibrary() -> LibraryModel {
            let value = LibraryModel(scope: scope, service: service, deletions: deletions)
            attach(value); return value
        }
        private func attach(_ value: LibraryModel) {
            menuLibraries.append(value)
            value.onConversationUpdated = { [weak self, weak value] updated in
                guard let self else { return }
                for other in menuLibraries where other !== value {
                    other.receiveConversationUpdate(updated, from: scope)
                }
            }
            value.onProjectDeleted = { [weak self] id in
                if self?.selectedProjectID == id { self?.selectedProjectID = nil }
            }
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

    @MainActor @Observable final class LibraryControlsFixtureService: LibraryServing, ChatServing {
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
        private let dragProof: Bool
        private let optionsProof: Bool
        private var optionProjects: [String: ConversationProject] = [
            "fixture-project": ConversationProject(
                id: "fixture-project", object: "project", name: "Synthetic project", createdAt: 0, updatedAt: 0)
        ]
        private var projectSequence = 0

        init(failPage: Bool, failDelete: Bool, dragProof: Bool, optionsProof: Bool) {
            self.failPage = failPage; self.failDelete = failDelete; self.dragProof = dragProof
            self.optionsProof = optionsProof
        }
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
            if optionsProof {
                return APIPage(
                    data: optionProjects.values.sorted { $0.id < $1.id }.map {
                        ConversationProjectListItem(
                            id: $0.id, object: "project", name: $0.name, createdAt: 0, updatedAt: 0)
                    })
            }
            var values: [ConversationProjectListItem] = []
            if projectExists {
                values.append(
                    ConversationProjectListItem(
                        id: "fixture-project", object: "project", name: "Synthetic project", createdAt: 0,
                        updatedAt: 0))
            }
            if dragProof {
                values.append(
                    ConversationProjectListItem(
                        id: "fixture-project-2", object: "project", name: "Second project", createdAt: 0, updatedAt: 0))
            }
            return APIPage(data: values)
        }
        func getProject(id: String) async throws -> ConversationProject {
            if optionsProof, let project = optionProjects[id] { return project }
            guard projectExists, id == "fixture-project" else { throw ResearchAPIError.invalidArgument }
            return ConversationProject(id: id, object: "project", name: "Synthetic project", createdAt: 0, updatedAt: 0)
        }
        func createProject(name: String) async throws -> ConversationProject {
            guard optionsProof else { throw ResearchAPIError.invalidArgument }
            projectSequence += 1
            let value = ConversationProject(
                id: "created-project-\(projectSequence)", object: "project", name: name, createdAt: 0, updatedAt: 0)
            optionProjects[value.id] = value; events.append("create-project"); return value
        }
        func updateProject(id: String, name: String?, instructions: NullableUpdate<String>) async throws
            -> ConversationProject
        {
            guard optionsProof, let old = optionProjects[id] else { throw ResearchAPIError.invalidArgument }
            let text: String?
            switch instructions {
            case .unchanged: text = old.instructions
            case .set(let value): text = value
            case .clear: text = nil
            }
            let value = ConversationProject(
                id: id, object: "project", name: name ?? old.name, instructions: text, createdAt: 0, updatedAt: 0)
            optionProjects[id] = value; events.append(name == nil ? "save-instructions" : "rename-project")
            return value
        }
        func deleteProject(id: String) async throws -> DeletedObject {
            if optionsProof {
                guard optionProjects.removeValue(forKey: id) != nil else { throw ResearchAPIError.invalidArgument }
                conversations.removeAll { $0.projectId == id }; events.append("delete-project")
                return DeletedObject(id: id, object: "project", deleted: true)
            }
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
    private actor ChatOptionsFixtureAccountService: BillingServing, ModelCatalogServing {
        private let accountProof: Bool
        init(accountProof: Bool = false) { self.accountProof = accountProof }
        func subscriptionStatus() throws -> BillingStatus {
            var value: [String: Any] = [
                "is_subscribed": true, "product_id": "synthetic", "product_name": "Pro",
                "subscription_status": "active", "can_chat": true, "chats_remaining": 50,
            ]
            if accountProof {
                value["total_tokens"] = 100; value["used_tokens"] = 25
                value["api_credit_balance"] = 2.5; value["usage_reset_date"] = NSNull()
            }
            return try decode(value)
        }
        func teamStatus() throws -> TeamStatus {
            if accountProof {
                return try decode([
                    "has_team_subscription": true, "team_created": true, "role": "admin", "is_team_admin": true,
                    "team_member_count": 1, "billed_seat_count": 0, "seat_limit_exceeded": true,
                ])
            }
            return try decode(["has_team_subscription": false, "team_created": false])
        }
        func fetchCatalog() -> ModelCatalog {
            let capabilities = ModelCapabilities(chat: true, vision: true, reasoning: true, toolUse: true)
            func item(_ id: String, _ label: String) -> ModelCatalogItem {
                ModelCatalogItem(
                    id: id, object: "model", created: 0, ownedBy: "synthetic", displayName: label,
                    shortName: label, contextWindow: 32768, maxContextTokens: 32768, access: "pro",
                    capabilities: capabilities, enabled: true, deprecated: false)
            }
            return ModelCatalog(
                object: "list",
                data: [item("options-quick", "Synthetic Quick"), item("options-power", "Synthetic Powerful")],
                aliases: [
                    ModelAlias(
                        id: .quick, label: "Quick", shortName: "Quick", description: "Fast, everyday responses",
                        targetModel: "options-quick", access: "pro", capabilities: capabilities),
                    ModelAlias(
                        id: .powerful, label: "Powerful", shortName: "Powerful",
                        description: "Deeper thinking & analysis",
                        targetModel: "options-power", access: "pro", capabilities: capabilities),
                ], defaults: ModelDefaults(quick: .quick, powerful: .powerful))
        }
        func retire() {}
        private func decode<T: Decodable>(_ value: [String: Any]) throws -> T {
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(T.self, from: JSONSerialization.data(withJSONObject: value))
        }
    }

    private actor ChatOptionsFixturePreferences: PreferencesStoring {
        private var value = LocalPreferences()
        func activate(_ scope: AccountScope) -> LocalPreferences { value }
        func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) { self.value = value }
        func retire(_ scope: AccountScope, removePersisted: Bool) {}
    }
#endif
