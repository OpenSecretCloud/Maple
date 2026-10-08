import Foundation
import MapleSDK
import Observation

@MainActor @Observable public final class LibraryModel {
    public let scope: AccountScope
    public private(set) var conversations: [Conversation] = []
    public private(set) var projects: [ConversationProjectListItem] = []
    public private(set) var filter: LibraryFilter = .all
    public private(set) var isLoading = false
    public private(set) var isLoadingProjects = false
    public private(set) var isMutating = false
    public private(set) var hasMore = false
    public private(set) var issue: LibraryIssue?
    public var searchText = ""
    public var selectedIDs: Set<String> = []
    public var onConversationUpdated: (@MainActor (Conversation) -> Void)?
    public var onConversationProjectUpdated: (@MainActor (Set<String>, String?) -> Void)?
    public var onConversationsDeleted: (@MainActor (Set<String>, Bool) -> Void)?
    public var onProjectDeleted: (@MainActor (String) -> Void)?
    @ObservationIgnored private let service: any LibraryServing
    @ObservationIgnored private let deletions: ChatDeletionCoordinator
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var loadID = UUID()
    @ObservationIgnored private var projectLoadID = UUID()
    @ObservationIgnored private var revision: UInt64 = 0
    @ObservationIgnored private var cursor: String?
    @ObservationIgnored private var cursors: Set<String> = []

    public init(scope: AccountScope, service: any LibraryServing, deletions: ChatDeletionCoordinator) {
        self.scope = scope; self.service = service; self.deletions = deletions
    }
    public var visibleConversations: [Conversation] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        return conversations.filter {
            accepts($0) && (query.isEmpty || Self.title($0).localizedCaseInsensitiveContains(query))
        }
    }
    public var visibleProjects: [ConversationProjectListItem] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        return projects.filter { project in
            query.isEmpty || project.name.localizedCaseInsensitiveContains(query)
                || conversations.contains {
                    $0.projectId == project.id && Self.title($0).localizedCaseInsensitiveContains(query)
                }
        }
    }
    public static func title(_ conversation: Conversation) -> String {
        if case .string(let title) = conversation.metadata?["title"],
            !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        {
            return title
        }
        return ""
    }

    public func load(_ filter: LibraryFilter = .all) async {
        guard valid else { return }
        self.filter = filter; conversations = []; cursor = nil; cursors = []; hasMore = true; selectedIDs = []
        loadID = UUID(); isLoading = false
        await loadMore()
    }
    public func loadMore() async {
        guard valid, !isLoading, hasMore else { return }
        let ticket = loadID, generation = revision, requested = filter
        isLoading = true; issue = nil
        do {
            let page = try await service.listConversations(parameters(requested, after: cursor))
            guard valid, loadID == ticket else { return }
            guard revision == generation else { isLoading = false; return }
            guard page.data.count <= 100 else { throw LibraryIssue.invalidResponse }
            if page.hasMore {
                guard let next = page.lastId, !page.data.isEmpty, cursors.insert(next).inserted else {
                    throw LibraryIssue.invalidResponse
                }
                cursor = next
            }
            var ids = Set(conversations.map(\.id))
            for value in page.data where ids.insert(value.id).inserted {
                conversations.append(value);
                deletions.updateProject(conversationID: value.id, projectID: value.projectId)
            }
            hasMore = page.hasMore
        } catch { if valid, loadID == ticket, revision == generation { issue = LibraryIssue.present(error) } }
        if valid, loadID == ticket { isLoading = false }
    }
    public func loadProjects() async {
        guard valid, !isLoadingProjects else { return }
        let ticket = UUID(), generation = revision; projectLoadID = ticket; isLoadingProjects = true
        do {
            var results: [ConversationProjectListItem] = [], after: String?, visited = Set<String>()
            for _ in 0..<100 {
                let page = try await service.listProjects(ListParameters(limit: 100, after: after, order: .desc))
                guard valid, projectLoadID == ticket else { return }
                guard generation == revision else { isLoadingProjects = false; return }
                results += page.data
                if !page.hasMore {
                    guard Set(results.map(\.id)).count == results.count else { throw LibraryIssue.invalidResponse }
                    projects = results; isLoadingProjects = false; return
                }
                guard let next = page.lastId, !page.data.isEmpty, visited.insert(next).inserted else {
                    throw LibraryIssue.invalidResponse
                }
                after = next
            }
            throw LibraryIssue.invalidResponse
        } catch {
            if valid, projectLoadID == ticket {
                if revision == generation { issue = LibraryIssue.present(error) }
                isLoadingProjects = false
            }
        }
    }

    public func renameConversation(id: String, title: String) async {
        guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { issue = .invalidInput; return }
        await mutate { [self] in
            let original = try await service.getConversation(id: id); try check()
            guard original.id == id else { throw LibraryIssue.invalidResponse }
            var metadata = original.metadata ?? [:]; metadata["title"] = .string(title)
            let updated = try await service.updateConversation(
                id: id, metadata: metadata, projectId: .unchanged, pinned: nil)
            try check(); guard updated.id == id else { throw LibraryIssue.invalidResponse }; publish(updated)
        }
    }
    public func setPinned(id: String, pinned: Bool) async {
        await mutate { [self] in
            let updated = try await service.updateConversation(
                id: id, metadata: nil, projectId: .unchanged, pinned: pinned)
            try check(); guard updated.id == id else { throw LibraryIssue.invalidResponse }; publish(updated)
        }
    }
    public func moveConversations(ids: Set<String>, projectID: String?) async {
        guard !ids.isEmpty, ids.count <= 10_000 else { issue = .invalidInput; return }
        await mutate { [self] in
            for batch in Self.batches(ids) {
                let result = try await service.batchUpdateConversationProject(ids: batch, projectId: projectID)
                try check(); guard result.success else { throw LibraryIssue.invalidResponse }
                let changed = Set(batch)
                for index in conversations.indices where changed.contains(conversations[index].id) {
                    conversations[index].projectId = projectID; onConversationUpdated?(conversations[index])
                }
                for id in batch { deletions.updateProject(conversationID: id, projectID: projectID) }
                selectedIDs.subtract(changed)
                // Bulk selection can span independently paged menu sections.
                // Publish acknowledged membership even when this owner has no row snapshot.
                onConversationProjectUpdated?(changed, projectID)
            }
        }
    }
    public func createProject(name: String) async -> ConversationProject? {
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { issue = .invalidInput; return nil }
        var result: ConversationProject?
        await mutate { [self] in
            let value = try await service.createProject(name: name); try check(); publish(value); result = value
        }
        return result
    }
    public func renameProject(id: String, name: String) async {
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { issue = .invalidInput; return }
        await mutate { [self] in
            let value = try await service.updateProject(id: id, name: name, instructions: .unchanged)
            try check(); guard value.id == id else { throw LibraryIssue.invalidResponse }; publish(value)
        }
    }

    public func deleteConversations(ids: Set<String>) async {
        guard !ids.isEmpty, ids.count <= 10_000 else { issue = .invalidInput; return }
        await mutate { [self] in
            let lease = try deletions.begin(.conversations(ids)); defer { deletions.release(lease) }
            try await deletions.quiesce(lease); try check(); try deletions.assertQuiescent(lease)
            var deletedAll = Set<String>()
            for batch in Self.batches(ids) {
                let requested = Set(batch)
                let response = try await service.batchDeleteConversations(ids: batch); try check()
                guard Set(response.data.map(\.id)) == requested, response.data.count == batch.count else {
                    throw LibraryIssue.invalidResponse
                }
                let deleted = Set(response.data.filter(\.deleted).map(\.id))
                deletedAll.formUnion(deleted); remove(deleted, all: false)
                try await deletions.finish(lease, deletedConversationIDs: deleted)
            }
            if deletedAll != ids { throw LibraryIssue.partialMutation }
        }
    }
    public func deleteAllConversations() async {
        await mutate { [self] in
            let lease = try deletions.begin(.all); defer { deletions.release(lease) }
            try await deletions.quiesce(lease); try check(); try deletions.assertQuiescent(lease)
            let result = try await service.deleteAllConversations(); try check()
            guard result.deleted else { throw LibraryIssue.invalidResponse }
            remove(Set(conversations.map(\.id)), all: true)
            try await deletions.finish(lease, deleteScope: true)
        }
    }
    public func deleteProject(id: String) async {
        await mutate { [self] in
            let lease = try deletions.begin(.all); defer { deletions.release(lease) }
            let ids = try await discoverProjectConversations(id); try check()
            try deletions.retarget(lease, to: .project(id, knownConversationIDs: ids))
            try await deletions.quiesce(lease); try check(); try deletions.assertQuiescent(lease)
            let result = try await service.deleteProject(id: id); try check()
            guard result.deleted, result.id == nil || result.id == id else { throw LibraryIssue.invalidResponse }
            projects.removeAll { $0.id == id }; remove(ids, all: false); onProjectDeleted?(id)
            try await deletions.finish(lease, deleteScope: true)
        }
    }

    public func invalidate() {
        valid = false; loadID = UUID(); projectLoadID = UUID(); revision &+= 1
        conversations = []; projects = []; selectedIDs = []; isLoading = false; isLoadingProjects = false;
        isMutating = false; searchText = ""
    }
    private func mutate(_ action: @escaping @MainActor () async throws -> Void) async {
        guard valid, !isMutating, !deletions.hasActiveFence else { issue = valid ? .busy : .retired; return }
        isMutating = true; issue = nil; revision &+= 1
        do { try await action() } catch { if valid { issue = LibraryIssue.present(error) } }
        // Reads begun during the mutation may contain a pre-mutation snapshot.
        // Settlement invalidates them even when they return after publication.
        if valid { revision &+= 1; isMutating = false }
    }
    private func check() throws { guard valid else { throw LibraryIssue.retired }; try Task.checkCancellation() }
    private static func batches(_ ids: Set<String>) -> [[String]] {
        let sorted = ids.sorted()
        // The authenticated backend contract admits at most twenty IDs.
        return stride(from: 0, to: sorted.count, by: 20).map { Array(sorted[$0..<min($0 + 20, sorted.count)]) }
    }
    private func publish(_ value: Conversation) {
        if let index = conversations.firstIndex(where: { $0.id == value.id }) { conversations[index] = value }
        deletions.updateProject(conversationID: value.id, projectID: value.projectId); onConversationUpdated?(value)
    }
    private func publish(_ value: ConversationProject) {
        let item = ConversationProjectListItem(
            id: value.id, object: value.object, name: value.name, createdAt: value.createdAt, updatedAt: value.updatedAt
        )
        if let index = projects.firstIndex(where: { $0.id == value.id }) {
            projects[index] = item
        } else {
            projects.insert(item, at: 0)
        }
    }
    private func remove(_ ids: Set<String>, all: Bool) {
        conversations.removeAll { all || ids.contains($0.id) }; selectedIDs.subtract(ids);
        onConversationsDeleted?(ids, all)
    }
    private func accepts(_ conversation: Conversation) -> Bool {
        switch filter {
        case .all: true
        case .pinned: conversation.pinned
        case .unassigned: conversation.projectId == nil
        case .project(let id): conversation.projectId == id
        }
    }
    private func parameters(_ filter: LibraryFilter, after: String?) -> ConversationsListParameters {
        let page = ListParameters(limit: 100, after: after, order: .desc)
        switch filter {
        case .all: return .init(page: page)
        case .pinned: return .init(page: page, pinned: true)
        case .unassigned: return .init(page: page, unassignedProject: true)
        case .project(let id): return .init(page: page, projectId: id)
        }
    }
    private func discoverProjectConversations(_ projectID: String) async throws -> Set<String> {
        var result = Set<String>(), after: String?, visited = Set<String>()
        for _ in 0..<100 {
            let page = try await service.listConversations(parameters(.project(projectID), after: after)); try check()
            guard page.data.allSatisfy({ $0.projectId == projectID }) else { throw LibraryIssue.invalidResponse }
            result.formUnion(page.data.map(\.id))
            if !page.hasMore { return result }
            guard let next = page.lastId, !page.data.isEmpty, visited.insert(next).inserted else {
                throw LibraryIssue.invalidResponse
            }
            after = next
        }
        throw LibraryIssue.invalidResponse
    }
}
