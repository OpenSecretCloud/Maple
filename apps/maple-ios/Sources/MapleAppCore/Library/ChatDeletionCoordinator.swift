import Foundation
import Observation

public enum ChatDeletionTarget: Sendable, Equatable {
    case conversations(Set<String>)
    case project(String, knownConversationIDs: Set<String>)
    case all
}
public struct ChatDeletionLease: Sendable, Hashable { public let id: UUID }
public enum ChatDeletionError: Error, Sendable { case retired, invalidLease, timedOut, cleanupRequired }
public struct ChatDeletionHooks: Sendable {
    public let elapsed: @Sendable () -> Duration
    public let sleep: @Sendable (Duration) async throws -> Void
    public init(
        elapsed: (@Sendable () -> Duration)? = nil,
        sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        let clock = ContinuousClock(), start = ContinuousClock.now
        self.elapsed = elapsed ?? { start.duration(to: clock.now) }; self.sleep = sleep
    }
}

/// Account-wide admission and cleanup authority. Retained snapshots participate
/// even when their chats have never been opened in this process.
@MainActor @Observable public final class ChatDeletionCoordinator {
    public let scope: AccountScope
    public private(set) var cleanupRequired = false
    @ObservationIgnored private let hooks: ChatDeletionHooks
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var targets: [UUID: ChatDeletionTarget] = [:]
    @ObservationIgnored private var owners: [ObjectIdentifier: ChatRuntime] = [:]
    @ObservationIgnored private var shadows: Set<ObjectIdentifier> = []
    @ObservationIgnored private var projects: [ObjectIdentifier: String] = [:]
    @ObservationIgnored private var cleanups: [ObjectIdentifier: (ChatRuntime, UUID)] = [:]
    @ObservationIgnored private var cleanupIdentities:
        [ObjectIdentifier: (key: String, owner: UUID, conversationID: String?)] = [:]
    private var deletedConversationIDs: Set<String> = []
    public func isConversationDeleted(_ id: String) -> Bool { deletedConversationIDs.contains(id) }
    public var onRemoved: (@MainActor (ChatRuntime) -> Void)?
    public var hasActiveFence: Bool { !targets.isEmpty }

    public init(
        scope: AccountScope, retained: [ChatSnapshot], service: any ChatServing,
        persistence: any ChatPersistence, hooks: ChatDeletionHooks = ChatDeletionHooks()
    ) {
        self.scope = scope; self.hooks = hooks
        for snapshot in retained {
            let runtime = ChatRuntime(
                scope: scope, service: service, persistence: persistence,
                conversationID: snapshot.conversationID)
            if runtime.loadRetainedSnapshot(snapshot) {
                let id = ObjectIdentifier(runtime)
                owners[id] = runtime; shadows.insert(id)
            }
        }
        for runtime in owners.values { attachAdmission(runtime) }
    }

    /// Always use the returned owner. An in-flight destructive lease may already
    /// own the shadow restored from this same protected snapshot.
    @discardableResult public func register(_ runtime: ChatRuntime, projectID: String? = nil) -> ChatRuntime {
        guard valid, runtime.scope == scope else { runtime.invalidate(); return runtime }
        let id = ObjectIdentifier(runtime)
        if let existing = owners.values.first(where: {
            guard $0 !== runtime, let identity = cleanupIdentities[ObjectIdentifier($0)] else { return false }
            return identity.key == runtime.snapshot.storageKey || identity.owner == runtime.snapshot.storageOwner
                || (identity.conversationID != nil && identity.conversationID == runtime.conversationID)
        }) {
            runtime.invalidate(); return existing
        }
        if let conversationID = runtime.conversationID, deletedConversationIDs.contains(conversationID) {
            runtime.invalidate(); return runtime
        }
        if let existing = owners.values.first(where: {
            $0 !== runtime
                && ($0.snapshot.storageOwner == runtime.snapshot.storageOwner
                    || $0.snapshot.storageKey == runtime.snapshot.storageKey)
        }) {
            if !targets.isEmpty { runtime.invalidate(); return existing }
            let oldID = ObjectIdentifier(existing)
            if projectID == nil { projects[id] = projects[oldID] }
            owners.removeValue(forKey: oldID); shadows.remove(oldID); projects.removeValue(forKey: oldID)
            existing.invalidate()
        }
        owners[id] = runtime; shadows.remove(id)
        if let projectID { projects[id] = projectID }
        attachAdmission(runtime)
        for (lease, target) in targets where matches(runtime, target: target) { runtime.beginDeletionFence(lease) }
        return runtime
    }

    public func updateProject(conversationID: String, projectID: String?) {
        for (id, runtime) in owners where runtime.conversationID == conversationID {
            projects[id] = projectID
            for (lease, target) in targets where matches(runtime, target: target) { runtime.beginDeletionFence(lease) }
        }
    }

    public func begin(_ target: ChatDeletionTarget) throws -> ChatDeletionLease {
        guard valid else { throw ChatDeletionError.retired }
        guard !cleanupRequired else { throw ChatDeletionError.cleanupRequired }
        let lease = ChatDeletionLease(id: UUID()); targets[lease.id] = target
        for runtime in owners.values where matches(runtime, target: target) { runtime.beginDeletionFence(lease.id) }
        return lease
    }

    /// Narrow a conservative discovery fence only after authoritative project
    /// membership is fully enumerated. Matching owners keep their fence without a gap.
    public func retarget(_ lease: ChatDeletionLease, to target: ChatDeletionTarget) throws {
        try require(lease)
        targets[lease.id] = target
        for runtime in owners.values {
            if matches(runtime, target: target) {
                runtime.beginDeletionFence(lease.id)
            } else {
                runtime.releaseDeletionFence(lease.id)
            }
        }
    }

    public func quiesce(_ lease: ChatDeletionLease, timeout: Duration = .seconds(30)) async throws {
        try require(lease)
        let deadline = hooks.elapsed() + max(.zero, timeout)
        while true {
            try Task.checkCancellation(); try require(lease)
            let runtimes = matching(lease)
            for runtime in runtimes { runtime.requestDeletionQuiescence(lease.id) }
            if runtimes.allSatisfy(\.isDeletionQuiescent) { return }
            let remaining = deadline - hooks.elapsed()
            guard remaining > .zero else { throw ChatDeletionError.timedOut }
            try await hooks.sleep(min(.milliseconds(250), remaining))
        }
    }

    public func assertQuiescent(_ lease: ChatDeletionLease) throws {
        try require(lease)
        guard matching(lease).allSatisfy(\.isDeletionQuiescent) else { throw ChatDeletionError.timedOut }
    }

    /// Clear only server-confirmed IDs, or all members after an all-history,
    /// project, or account deletion succeeds. Failures stay retryable locally.
    public func finish(_ lease: ChatDeletionLease, deletedConversationIDs: Set<String> = [], deleteScope: Bool = false)
        async throws
    {
        try require(lease)
        self.deletedConversationIDs.formUnion(deletedConversationIDs)
        if deleteScope, let target = targets[lease.id], case .project(_, let knownIDs) = target {
            self.deletedConversationIDs.formUnion(knownIDs)
        }
        let removing = matching(lease).filter {
            deleteScope || $0.conversationID.map(deletedConversationIDs.contains) == true
        }
        // Freeze every cleanup identity before the first suspension. Invalidating
        // an owner erases its snapshot, but navigation must still resolve to it.
        for runtime in removing {
            let owner = ObjectIdentifier(runtime)
            cleanupIdentities[owner] = (
                runtime.snapshot.storageKey, runtime.snapshot.storageOwner, runtime.conversationID
            )
            if let id = runtime.conversationID { self.deletedConversationIDs.insert(id) }
        }
        for runtime in removing {
            let owner = ObjectIdentifier(runtime)
            cleanups[owner] = (runtime, lease.id)
            do {
                try await runtime.clearAfterDeletion(lease.id)
                guard valid else { throw ChatDeletionError.retired }
                cleanups.removeValue(forKey: owner); cleanupIdentities.removeValue(forKey: owner);
                owners.removeValue(forKey: owner);
                projects.removeValue(forKey: owner)
                shadows.remove(owner); onRemoved?(runtime)
            } catch { cleanupRequired = true }
        }
        if cleanupRequired { throw ChatDeletionError.cleanupRequired }
    }

    public func retryCleanup() async throws {
        guard valid else { throw ChatDeletionError.retired }
        for (owner, entry) in cleanups {
            try await entry.0.clearAfterDeletion(entry.1)
            guard valid else { throw ChatDeletionError.retired }
            cleanups.removeValue(forKey: owner); cleanupIdentities.removeValue(forKey: owner);
            owners.removeValue(forKey: owner); projects.removeValue(forKey: owner)
            shadows.remove(owner); onRemoved?(entry.0)
        }
        cleanupRequired = !cleanups.isEmpty
    }

    public func release(_ lease: ChatDeletionLease) {
        targets.removeValue(forKey: lease.id)
        for runtime in owners.values { runtime.releaseDeletionFence(lease.id) }
    }

    public func invalidate() {
        valid = false
        for id in shadows { owners[id]?.invalidate() }
        for (lease, _) in targets { for runtime in owners.values { runtime.releaseDeletionFence(lease) } }
        targets.removeAll(); owners.removeAll(); cleanups.removeAll(); cleanupIdentities.removeAll();
        deletedConversationIDs.removeAll(); projects.removeAll(); shadows.removeAll()
    }

    private func require(_ lease: ChatDeletionLease) throws {
        guard valid else { throw ChatDeletionError.retired }
        guard targets[lease.id] != nil else { throw ChatDeletionError.invalidLease }
    }
    private func matching(_ lease: ChatDeletionLease) -> [ChatRuntime] {
        guard let target = targets[lease.id] else { return [] }
        // A pending project creation can settle and lose its run options before
        // deletion. Its already admitted fence remains membership evidence.
        return owners.values.filter { $0.deletionFenceIDs.contains(lease.id) || matches($0, target: target) }
    }
    private func matches(_ runtime: ChatRuntime, target: ChatDeletionTarget, options: ChatSendOptions? = nil) -> Bool {
        switch target {
        case .all: true
        case .conversations(let ids): runtime.conversationID.map(ids.contains) ?? false
        case .project(let project, let ids):
            runtime.conversationID.map(ids.contains) == true
                || projects[ObjectIdentifier(runtime)] == project
                || runtime.snapshot.pendingRun?.options.projectID == project
                || options?.projectID == project
        }
    }
    private func attachAdmission(_ runtime: ChatRuntime) {
        runtime.deletionAdmission = { [weak self, weak runtime] options in
            guard let self, let runtime, valid, !cleanupRequired else { return false }
            return !targets.values.contains { matches(runtime, target: $0, options: options) }
        }
    }
}
