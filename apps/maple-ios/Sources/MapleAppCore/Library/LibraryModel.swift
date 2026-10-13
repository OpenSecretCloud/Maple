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
    public var onProjectUpdated: (@MainActor (ConversationProject) -> Void)?
    @ObservationIgnored private let service: any LibraryServing
    @ObservationIgnored private let deletions: ChatDeletionCoordinator
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var loadID = UUID()
    @ObservationIgnored private var projectLoadID = UUID()
    @ObservationIgnored private var revision: UInt64 = 0
    @ObservationIgnored private var cursor: String?
    @ObservationIgnored private var cursors: Set<String> = []
    private var loadedPages = 0

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

    public var hasLoadedConversations: Bool { loadedPages > 0 }
    public var isActive: Bool { valid }

    /// The menu observes the actual trailing row's visibility. Search remains
    /// local to loaded titles and never drains every page as a side effect.
    public func loadMoreIfNeeded(visibleID: String) async {
        guard searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            visibleConversations.suffix(3).contains(where: { $0.id == visibleID }), issue == nil
        else { return }
        await loadMore()
    }

    public func load(_ filter: LibraryFilter = .all) async {
        guard valid, !Task.isCancelled else { return }
        if self.filter != filter {
            self.filter = filter; conversations = []; cursor = nil; cursors = []; loadedPages = 0
            hasMore = false; selectedIDs = []; loadID = UUID(); isLoading = false
        }
        await refresh()
    }

    /// Revalidate only the pages this owner has loaded. Publish an entire
    /// refreshed window atomically so returning to the menu never blanks it or
    /// resets its pagination cursor while a request is in flight.
    public func refresh() async {
        guard valid, !isLoading, !Task.isCancelled else { return }
        let ticket = UUID(), generation = revision, requested = filter; loadID = ticket
        isLoading = true; issue = nil
        defer { if valid, loadID == ticket { isLoading = false } }
        do {
            var results: [Conversation] = [], ids = Set<String>(), visited = Set<String>()
            var after: String?, more = false, pageCount = 0
            for _ in 0..<max(1, loadedPages) {
                let page = try await service.listConversations(parameters(requested, after: after))
                try Task.checkCancellation()
                guard valid, loadID == ticket, revision == generation, !isMutating else { return }
                guard page.data.count <= 100 else { throw LibraryIssue.invalidResponse }
                for value in page.data where ids.insert(value.id).inserted { results.append(value) }
                pageCount += 1; more = page.hasMore
                if !more { break }
                guard let next = page.lastId, !page.data.isEmpty, visited.insert(next).inserted else {
                    throw LibraryIssue.invalidResponse
                }
                after = next
            }
            if conversations != results { conversations = results }
            for value in results { deletions.updateProject(conversationID: value.id, projectID: value.projectId) }
            cursor = after; cursors = visited; loadedPages = pageCount; hasMore = more
        } catch is CancellationError {
            // Navigation cancellation leaves the last successful cache intact.
        } catch {
            if valid, loadID == ticket, revision == generation { issue = LibraryIssue.present(error) }
        }
    }
    public func loadMore() async {
        guard valid, !isLoading, hasMore, !Task.isCancelled else { return }
        let ticket = UUID(), generation = revision, requested = filter; loadID = ticket
        isLoading = true; issue = nil
        do {
            let page = try await service.listConversations(parameters(requested, after: cursor))
            try Task.checkCancellation()
            guard valid, loadID == ticket else { return }
            guard revision == generation, !isMutating else { isLoading = false; return }
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
            loadedPages += 1; hasMore = page.hasMore
        } catch is CancellationError {
        } catch { if valid, loadID == ticket, revision == generation { issue = LibraryIssue.present(error) } }
        if valid, loadID == ticket { isLoading = false }
    }

    /// Research polls the newest twenty recents without re-reading older pages.
    /// Unchanged titles keep their cached row; server order precedes the loaded
    /// older window. Silent failures never replace an existing library issue.
    public func pollRecentConversations(shouldApply: @MainActor () -> Bool = { true }) async {
        guard valid, filter == .recent, hasLoadedConversations, !isLoading, !isMutating,
            !Task.isCancelled, shouldApply()
        else { return }
        let ticket = loadID, generation = revision
        do {
            let page = try await service.listConversations(parameters(.recent, after: nil))
            try Task.checkCancellation()
            guard valid, loadID == ticket, revision == generation, !isLoading, !isMutating, shouldApply(),
                !page.data.isEmpty, page.data.count <= 20
            else { return }
            let cached = Dictionary(conversations.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
            var merged: [Conversation] = [], ids = Set<String>()
            for value in page.data where ids.insert(value.id).inserted {
                if let existing = cached[value.id], existing.metadata?["title"] == value.metadata?["title"] {
                    merged.append(existing)
                } else {
                    merged.append(value)
                }
            }
            for value in conversations where ids.insert(value.id).inserted { merged.append(value) }
            if conversations != merged { conversations = merged }
            for value in page.data {
                deletions.updateProject(conversationID: value.id, projectID: value.projectId)
            }
        } catch {
            // Periodic reads leave the previous complete menu and issue intact.
        }
    }
    public func loadProjects() async {
        guard valid, !isLoadingProjects, !Task.isCancelled else { return }
        let ticket = UUID(), generation = revision; projectLoadID = ticket; isLoadingProjects = true
        do {
            var results: [ConversationProjectListItem] = [], after: String?, visited = Set<String>()
            for _ in 0..<100 {
                let page = try await service.listProjects(ListParameters(limit: 100, after: after, order: .desc))
                try Task.checkCancellation()
                guard valid, projectLoadID == ticket else { return }
                guard generation == revision, !isMutating else { isLoadingProjects = false; return }
                results += page.data
                if !page.hasMore {
                    guard Set(results.map(\.id)).count == results.count else { throw LibraryIssue.invalidResponse }
                    if projects != results { projects = results }
                    isLoadingProjects = false; return
                }
                guard let next = page.lastId, !page.data.isEmpty, visited.insert(next).inserted else {
                    throw LibraryIssue.invalidResponse
                }
                after = next
            }
            throw LibraryIssue.invalidResponse
        } catch is CancellationError {
            if valid, projectLoadID == ticket { isLoadingProjects = false }
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
    public func relocateConversation(
        id: String, to destination: LibraryConversationDestination, from owner: AccountScope
    ) async {
        guard owner == scope else { return }
        guard !id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            issue = .invalidInput; return
        }
        let project: NullableUpdate<String>, pinned: Bool
        switch destination {
        case .project(let id):
            guard !id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                issue = .invalidInput; return
            }
            project = .set(id); pinned = false
        case .pinned:
            project = .unchanged; pinned = true
        case .recent:
            project = .clear; pinned = false
        }
        await mutate { [self] in
            // One authenticated update prevents an intermediate project/pin
            // combination and uses the same revocable owner as other actions.
            let updated = try await service.updateConversation(
                id: id, metadata: nil, projectId: project, pinned: pinned)
            try check()
            guard updated.id == id, updated.pinned == pinned else { throw LibraryIssue.invalidResponse }
            switch project {
            case .set(let expected):
                guard updated.projectId == expected else { throw LibraryIssue.invalidResponse }
            case .clear:
                guard updated.projectId == nil else { throw LibraryIssue.invalidResponse }
            case .unchanged: break
            }
            publish(updated); selectedIDs.remove(id)
        }
    }
    @discardableResult public func moveConversations(ids: Set<String>, projectID: String?) async -> Bool {
        guard !ids.isEmpty, ids.count <= 10_000 else { issue = .invalidInput; return false }
        var completed = false
        await mutate { [self] in
            for batch in Self.batches(ids) {
                let result = try await service.batchUpdateConversationProject(ids: batch, projectId: projectID)
                try check(); guard result.success else { throw LibraryIssue.invalidResponse }
                let changed = Set(batch)
                for index in conversations.indices where changed.contains(conversations[index].id) {
                    conversations[index].projectId = projectID; onConversationUpdated?(conversations[index])
                }
                let retained = conversations.filter { accepts($0) }
                if conversations != retained { conversations = retained }
                for id in batch { deletions.updateProject(conversationID: id, projectID: projectID) }
                selectedIDs.subtract(changed)
                // Bulk selection can span independently paged menu sections.
                // Publish acknowledged membership even when this owner has no row snapshot.
                onConversationProjectUpdated?(changed, projectID)
            }
            completed = true
        }
        return completed && valid
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

    @discardableResult public func deleteConversations(ids: Set<String>) async -> Bool {
        guard !ids.isEmpty, ids.count <= 10_000 else { issue = .invalidInput; return false }
        var completed = false
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
            completed = true
        }
        return completed && valid
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

    // Acknowledged changes from sibling lists share the exact account generation.
    // Invalidate pending reads before reconciling, without starting another request.
    public func receiveConversationUpdate(_ value: Conversation, from owner: AccountScope) {
        guard valid, scope == owner else { return }
        revision &+= 1
        if let index = conversations.firstIndex(where: { $0.id == value.id }) {
            if accepts(value) {
                if conversations[index] != value { conversations[index] = value }
            } else {
                conversations.remove(at: index)
            }
        } else if loadedPages > 0, accepts(value) {
            conversations.insert(value, at: 0)
        }
    }
    public func receiveProjectMembership(_ ids: Set<String>, projectID: String?, from owner: AccountScope) {
        guard valid, scope == owner else { return }
        revision &+= 1
        for index in conversations.indices
        where ids.contains(conversations[index].id) && conversations[index].projectId != projectID {
            conversations[index].projectId = projectID
        }
        let retained = conversations.filter { accepts($0) }
        if conversations != retained { conversations = retained }
    }
    public func receiveDeletion(_ ids: Set<String>, all: Bool, from owner: AccountScope) {
        guard valid, scope == owner else { return }
        revision &+= 1
        let retained = conversations.filter { !all && !ids.contains($0.id) }
        if conversations != retained { conversations = retained }
        if all { selectedIDs = []; hasMore = false } else { selectedIDs.subtract(ids) }
    }
    public func receiveProjectUpdate(_ value: ConversationProject, from owner: AccountScope) {
        guard valid, scope == owner else { return }
        revision &+= 1; updateProjectSnapshot(value)
    }
    public func receiveProjectDeletion(_ id: String, from owner: AccountScope) {
        guard valid, scope == owner else { return }
        revision &+= 1
        if let index = projects.firstIndex(where: { $0.id == id }) { projects.remove(at: index) }
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
        if let index = conversations.firstIndex(where: { $0.id == value.id }) {
            if accepts(value) { conversations[index] = value } else { conversations.remove(at: index) }
        }
        deletions.updateProject(conversationID: value.id, projectID: value.projectId); onConversationUpdated?(value)
    }
    private func publish(_ value: ConversationProject) {
        updateProjectSnapshot(value); onProjectUpdated?(value)
    }
    private func updateProjectSnapshot(_ value: ConversationProject) {
        let item = ConversationProjectListItem(
            id: value.id, object: value.object, name: value.name, createdAt: value.createdAt, updatedAt: value.updatedAt
        )
        if let index = projects.firstIndex(where: { $0.id == value.id }) {
            if projects[index] != item { projects[index] = item }
        } else {
            projects.insert(item, at: 0)
        }
    }
    private func remove(_ ids: Set<String>, all: Bool) {
        conversations.removeAll { all || ids.contains($0.id) }
        if all { selectedIDs = []; hasMore = false } else { selectedIDs.subtract(ids) }
        onConversationsDeleted?(ids, all)
    }
    private func accepts(_ conversation: Conversation) -> Bool {
        switch filter {
        case .all: true
        case .pinned: conversation.pinned
        case .recent: conversation.projectId == nil && !conversation.pinned
        case .unassigned: conversation.projectId == nil
        case .project(let id): conversation.projectId == id
        }
    }
    private func parameters(_ filter: LibraryFilter, after: String?) -> ConversationsListParameters {
        // Research's shared web/desktop sidebar starts with twenty recent
        // chats and requests ten older chats after the oldest loaded ID.
        let limit = filter == .recent ? (after == nil ? 20 : 10) : 100
        let page = ListParameters(limit: limit, after: after, order: .desc)
        switch filter {
        case .all: return .init(page: page)
        case .pinned: return .init(page: page, pinned: true)
        case .recent: return .init(page: page, unassignedProject: true, pinned: false)
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
