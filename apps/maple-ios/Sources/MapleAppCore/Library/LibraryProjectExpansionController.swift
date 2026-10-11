import Foundation
import Observation

/// One owner per account session survives menu-view recreation. A failed read
/// keeps the record untouched; folder interactions still work in memory until
/// the store can be read safely. Late restores merge explicit local choices.
@MainActor @Observable public final class LibraryProjectExpansionController {
    public enum State: Equatable, Sendable {
        case idle, loading, ready, failed(LibraryProjectExpansionIssue), retired
    }
    public let scope: AccountScope
    public private(set) var state: State = .idle
    public private(set) var saveIssue: LibraryProjectExpansionIssue?
    private var expanded = Set<String>()
    private var pendingChoices: [String: Bool] = [:]
    private let store: any LibraryProjectExpansionStoring
    private var active = true
    private var operation = UUID()
    private var revision: UInt64 = 0
    private var saveTask: Task<Void, Never>?

    public init(scope: AccountScope, store: any LibraryProjectExpansionStoring) {
        self.scope = scope; self.store = store
    }

    public func load() async {
        guard active, state != .loading, state != .ready else { return }
        let ticket = UUID(); operation = ticket; state = .loading
        do {
            var restored = try await store.activate(scope)
            guard active, operation == ticket else { return }
            for (digest, choice) in pendingChoices {
                if choice { restored.insert(digest) } else { restored.remove(digest) }
            }
            guard restored.count <= LibraryProjectExpansionStore.maximumProjects else {
                throw LibraryProjectExpansionIssue.invalidRecord
            }
            expanded = restored; state = .ready
            if !pendingChoices.isEmpty { pendingChoices.removeAll(); enqueueSave() }
        } catch {
            guard active, operation == ticket else { return }
            state = .failed(Self.safe(error))
        }
    }

    public func isExpanded(id: String) -> Bool {
        guard active, let digest = LibraryProjectExpansionStore.projectDigest(id) else { return false }
        return expanded.contains(digest)
    }

    @discardableResult public func setExpanded(id: String, expanded choice: Bool) -> Bool {
        guard active, let digest = LibraryProjectExpansionStore.projectDigest(id) else { return false }
        var next = expanded
        if choice { next.insert(digest) } else { next.remove(digest) }
        guard next.count <= LibraryProjectExpansionStore.maximumProjects else { return false }
        if state != .ready {
            guard
                pendingChoices[digest] != nil
                    || pendingChoices.count < LibraryProjectExpansionStore.maximumProjects
            else { return false }
            pendingChoices[digest] = choice
        }
        guard next != expanded else { return true }
        expanded = next
        if state == .ready { enqueueSave() }
        return true
    }

    public func flush() async { await saveTask?.value }

    public func retrySave() {
        guard active, state == .ready, saveIssue != nil else { return }
        enqueueSave()
    }

    public func prepareAccountDeletion() async throws {
        guard active else { throw LibraryProjectExpansionIssue.sessionChanged }
        await saveTask?.value
        guard active else { throw LibraryProjectExpansionIssue.sessionChanged }
        try await store.prepareRemoval(scope)
    }

    /// A failed remote request can cancel its exact lease's marker even after
    /// ordinary logout; replacement-generation markers remain fenced.
    public func cancelAccountDeletion() async throws { try await store.cancelRemoval(scope) }

    /// Revoke UI mutations synchronously, before account retirement awaits.
    public func invalidate() {
        active = false; operation = UUID(); state = .retired
        expanded.removeAll(); pendingChoices.removeAll()
    }

    public func retire(removePersisted: Bool = false) async throws {
        invalidate()
        await saveTask?.value
        try await store.retire(scope, removePersisted: removePersisted)
        saveIssue = nil
    }

    private func enqueueSave() {
        revision += 1
        let nextRevision = revision, next = expanded, prior = saveTask
        saveIssue = nil
        saveTask = Task { [weak self, store, scope] in
            await prior?.value
            do {
                try await store.save(next, scope: scope, revision: nextRevision)
                guard let self, self.active, self.revision == nextRevision else { return }
                self.saveIssue = nil
            } catch {
                guard let self, self.active, self.revision == nextRevision else { return }
                self.saveIssue = Self.safe(error)
            }
        }
    }

    private static func safe(_ error: any Error) -> LibraryProjectExpansionIssue {
        error as? LibraryProjectExpansionIssue ?? .unavailable
    }
}
