import Foundation
import Testing

@testable import MapleAppCore

private func expansionScope(
    origin: String = "https://expansion.invalid", principal: String = "fixture"
) -> AccountScope {
    AccountScope(apiOrigin: origin, principalID: principal)
}

private func expansionDirectory() -> URL {
    FileManager.default.temporaryDirectory.appendingPathComponent("expansion-tests-" + UUID().uuidString)
}

private func expansionRecord(_ root: URL, scope: AccountScope) throws -> URL {
    root.appendingPathComponent(try LibraryProjectExpansionStore.accountName(scope) + ".json")
}

private func expansionDigest(_ id: String) throws -> String {
    try #require(LibraryProjectExpansionStore.projectDigest(id))
}

private func writeExpansionBytes(_ bytes: Data, root: URL, scope: AccountScope) throws {
    let files = ProtectedChatFiles(root: root), account = try LibraryProjectExpansionStore.accountName(scope)
    try files.withDirectory { directory in
        try files.publish(bytes, name: account + ".json", account: account, directory: directory) {}
    }
}

@Suite struct LibraryProjectExpansionStoreTests {
    @Test func accountAndBackendNamespacesRetainOnlyHashedMetadataAcrossRelaunch() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        let expanded: Set<String> = [try expansionDigest("project-private-id")]
        #expect(try await store.activate(owner).isEmpty)
        try await store.save(expanded, scope: owner, revision: 1)
        try await store.retire(owner)
        for other in [expansionScope(origin: "https://other.invalid"), expansionScope(principal: "other")] {
            #expect(try await store.activate(other).isEmpty)
            try await store.retire(other)
        }
        let reopened = LibraryProjectExpansionStore(root: root)
        #expect(try await reopened.activate(expansionScope()) == expanded)
        let file = try expansionRecord(root, scope: owner), bytes = try Data(contentsOf: file)
        let object = try #require(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        #expect(Set(object.keys) == ["version", "expanded"])
        let text = String(decoding: bytes, as: UTF8.self)
        #expect(!text.contains("project-private-id") && !text.contains(owner.principalID))
        #expect(!text.contains(owner.apiOrigin) && !file.lastPathComponent.contains(owner.principalID))
        #expect(bytes.count < LibraryProjectExpansionStore.maximumBytes)
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        #expect((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600)
        #expect(
            (try FileManager.default.attributesOfItem(atPath: root.path)[.posixPermissions] as? NSNumber)?.intValue
                == 0o700)
        for item in [root, file] {
            #expect(try item.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)
        }
        #if os(iOS)
            #expect(attributes[.protectionKey] as? FileProtectionType == .complete)
        #endif
    }

    @Test func retiredGenerationCannotWriteOrClearReplacementAndStaleRevisionCannotRewind() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), old = expansionScope(), current = expansionScope()
        _ = try await store.activate(old)
        try await store.retire(old)
        _ = try await store.activate(current)
        let latest: Set<String> = [try expansionDigest("latest")]
        try await store.save(latest, scope: current, revision: 3)
        try await store.save([], scope: current, revision: 2)
        #expect(try await store.activate(current) == latest)
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
            try await store.save([], scope: old, revision: 100)
        }
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) { try await store.activate(old) }
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
            try await store.retire(old, removePersisted: true)
        }
        try await store.retire(current)
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
            try await store.retire(old, removePersisted: true)
        }
        #expect(try await store.activate(expansionScope()) == latest)
    }

    @Test func confirmedDeletionIsIdempotentAndClearsOnlyExactAccount() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), old = expansionScope()
        let current = expansionScope(principal: "different"), retained: Set<String> = [try expansionDigest("kept")]
        _ = try await store.activate(old)
        try await store.save([try expansionDigest("removed")], scope: old, revision: 1)
        try await store.retire(old)
        _ = try await store.activate(current)
        try await store.save(retained, scope: current, revision: 1)
        try await store.retire(old, removePersisted: true)
        try await store.retire(old)
        #expect(try await store.activate(current) == retained)
        try await store.retire(current)
        #expect(try await store.activate(expansionScope()).isEmpty)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 1)
    }

    @Test func corruptFutureAndOversizedRecordsStayUntouched() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        _ = try await store.activate(owner)
        let file = try expansionRecord(root, scope: owner), digest = try expansionDigest("duplicate")
        let invalid = [
            Data("invalid".utf8), Data(repeating: 0, count: LibraryProjectExpansionStore.maximumBytes + 1),
            try JSONSerialization.data(withJSONObject: ["version": 1, "expanded": ["raw-project-id"]]),
            try JSONSerialization.data(withJSONObject: ["version": 1, "expanded": [digest, digest]]),
        ]
        for bytes in invalid {
            try writeExpansionBytes(bytes, root: root, scope: owner)
            await #expect(throws: LibraryProjectExpansionIssue.invalidRecord) { try await store.activate(owner) }
            #expect(try Data(contentsOf: file) == bytes)
        }
        let future = try JSONSerialization.data(withJSONObject: ["version": 2, "expanded": []])
        try writeExpansionBytes(future, root: root, scope: owner)
        await #expect(throws: LibraryProjectExpansionIssue.unsupportedVersion) { try await store.activate(owner) }
        #expect(try Data(contentsOf: file) == future)
    }

    @Test func lockedDeletionRevokesImmediatelyAndRetriesBeforeReplacementLoad() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), old = expansionScope(), current = expansionScope()
        _ = try await store.activate(old)
        try await store.save([try expansionDigest("removed")], scope: old, revision: 1)
        await store.setProtectedDataAvailable(false)
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await store.retire(old, removePersisted: true)
        }
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
            try await store.save([], scope: old, revision: 2)
        }
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await store.activate(current)
        }
        await store.setProtectedDataAvailable(true)
        #expect(try await store.activate(current).isEmpty)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }

    @Test func failedLockedReadStillAcquiresRetirableGenerationLease() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), old = expansionScope(), current = expansionScope()
        _ = try await store.activate(old)
        try await store.retire(old)
        await store.setProtectedDataAvailable(false)
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await store.activate(current)
        }
        try await store.retire(current)
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) { try await store.activate(current) }
        await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
            try await store.retire(old, removePersisted: true)
        }
        await store.setProtectedDataAvailable(true)
        #expect(try await store.activate(expansionScope()).isEmpty)
    }

    @Test func preparedDeletionSurvivesLockedTerminationAndCleansOnlyMarkedAccount() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        let other = expansionScope(principal: "retained"), retained: Set<String> = [try expansionDigest("retained")]
        _ = try await store.activate(other)
        try await store.save(retained, scope: other, revision: 1)
        try await store.retire(other)
        _ = try await store.activate(owner)
        try await store.save([try expansionDigest("deleted")], scope: owner, revision: 1)
        try await store.prepareRemoval(owner)
        // Calling startup cleanup in a live process must not consume the
        // current account's marker before its remote request finishes.
        try await store.startupCleanup()
        #expect(!((try await store.activate(owner)).isEmpty))
        let marker = root.appendingPathComponent(try LibraryProjectExpansionStore.accountName(owner) + ".removal")
        #expect(try Data(contentsOf: marker).count < 128)
        #expect(try marker.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)
        await store.setProtectedDataAvailable(false)
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await store.retire(owner, removePersisted: true)
        }
        let reopened = LibraryProjectExpansionStore(root: root, protectionAvailable: false)
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await reopened.startupCleanup()
        }
        await reopened.setProtectedDataAvailable(true)
        try await reopened.startupCleanup()
        #expect(try await reopened.activate(expansionScope()).isEmpty)
        #expect(!FileManager.default.fileExists(atPath: marker.path))
        let otherReopened = LibraryProjectExpansionStore(root: root)
        #expect(try await otherReopened.activate(other) == retained)
    }

    @Test func failedDeletionWhileLockedRetriesCancellationBeforeRestart() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        let retained: Set<String> = [try expansionDigest("retained")]
        _ = try await store.activate(owner)
        try await store.save(retained, scope: owner, revision: 1)
        try await store.prepareRemoval(owner)
        await store.setProtectedDataAvailable(false)
        await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
            try await store.cancelRemoval(owner)
        }
        await store.setProtectedDataAvailable(true)
        try await store.startupCleanup()
        try await store.retire(owner)
        let reopened = LibraryProjectExpansionStore(root: root)
        try await reopened.startupCleanup()
        #expect(try await reopened.activate(expansionScope()) == retained)
    }

    @Test func uncertainOldMarkerIsConsumedBeforeReplacementIncludingLockedHandoff() async throws {
        for locked in [false, true] {
            let root = expansionDirectory()
            defer { try? FileManager.default.removeItem(at: root) }
            let store = LibraryProjectExpansionStore(root: root), old = expansionScope(), current = expansionScope()
            _ = try await store.activate(old)
            try await store.save([try expansionDigest("old")], scope: old, revision: 1)
            try await store.prepareRemoval(old)
            try await store.retire(old)
            if locked {
                await store.setProtectedDataAvailable(false)
                await #expect(throws: LibraryProjectExpansionIssue.protectedDataUnavailable) {
                    try await store.activate(current)
                }
                await store.setProtectedDataAvailable(true)
                try await store.startupCleanup()
            }
            #expect(try await store.activate(current).isEmpty)
            let replacement: Set<String> = [try expansionDigest("replacement")]
            try await store.save(replacement, scope: current, revision: 1)
            try await store.prepareRemoval(current)
            await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
                try await store.cancelRemoval(old)
            }
            await #expect(throws: LibraryProjectExpansionIssue.sessionChanged) {
                try await store.retire(old, removePersisted: true)
            }
            // The stale result must not remove the current prepared marker.
            let marker = root.appendingPathComponent(try LibraryProjectExpansionStore.accountName(current) + ".removal")
            #expect(FileManager.default.fileExists(atPath: marker.path))
            try await store.cancelRemoval(current)
            try await store.retire(current)
            let reopened = LibraryProjectExpansionStore(root: root)
            try await reopened.startupCleanup()
            #expect(try await reopened.activate(expansionScope()) == replacement)
        }
    }

    @Test func invalidProjectIdentifiersAndRecordCountsAreBounded() async throws {
        #expect(LibraryProjectExpansionStore.projectDigest("") == nil)
        #expect(LibraryProjectExpansionStore.projectDigest(String(repeating: "a", count: 1_025)) == nil)
        #expect(LibraryProjectExpansionStore.projectDigest(String(repeating: "a", count: 1_024)) != nil)
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        _ = try await store.activate(owner)
        await #expect(throws: LibraryProjectExpansionIssue.invalidRecord) {
            try await store.save(["raw-project-id"], scope: owner, revision: 1)
        }
        let excess = Set(
            (0...LibraryProjectExpansionStore.maximumProjects).map { ProtectedChatFiles.digest(Data("\($0)".utf8)) })
        await #expect(throws: LibraryProjectExpansionIssue.invalidRecord) {
            try await store.save(excess, scope: owner, revision: 1)
        }
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
}

@MainActor @Suite struct LibraryProjectExpansionControllerTests {
    @Test func expandAndCollapseRestoreAcrossFreshOwnersAndFreshStore() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root)
        let first = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await first.load()
        #expect(first.state == .ready && !first.isExpanded(id: "project-A"))
        #expect(first.setExpanded(id: "project-A", expanded: true))
        try await first.retire()
        let recreated = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await recreated.load()
        #expect(recreated.isExpanded(id: "project-A"))
        #expect(recreated.setExpanded(id: "project-A", expanded: false))
        #expect(recreated.setExpanded(id: "project-B", expanded: true))
        try await recreated.retire()
        let relaunched = LibraryProjectExpansionController(
            scope: expansionScope(), store: LibraryProjectExpansionStore(root: root))
        await relaunched.load()
        #expect(!relaunched.isExpanded(id: "project-A") && relaunched.isExpanded(id: "project-B"))
        // A missing/partial library page is not a deletion signal. Reading only
        // another folder and saving a new choice must retain the absent folder.
        #expect(!relaunched.isExpanded(id: "not-in-this-page"))
        relaunched.setExpanded(id: "project-C", expanded: true)
        try await relaunched.retire()
        let later = LibraryProjectExpansionController(
            scope: expansionScope(), store: LibraryProjectExpansionStore(root: root))
        await later.load()
        #expect(later.isExpanded(id: "project-B") && later.isExpanded(id: "project-C"))
    }

    @Test func rapidChoicesDrainOnRetirementAndInvalidatedOwnerCannotMutate() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root)
        let controller = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await controller.load()
        controller.setExpanded(id: "A", expanded: true)
        controller.setExpanded(id: "B", expanded: true)
        controller.setExpanded(id: "A", expanded: false)
        controller.invalidate(); controller.invalidate()
        #expect(!controller.setExpanded(id: "B", expanded: false))
        try await controller.retire()
        #expect(controller.state == .retired && !controller.isExpanded(id: "B"))
        #expect(try await store.activate(expansionScope()) == [try expansionDigest("B")])
    }

    @Test func lockedReadKeepsChoicesInMemoryAndMergesOnUnlockWithoutEarlyWrite() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), saved = expansionScope()
        _ = try await store.activate(saved)
        try await store.save(
            [try expansionDigest("retained"), try expansionDigest("collapse")], scope: saved, revision: 1)
        try await store.retire(saved)
        let before = try Data(contentsOf: expansionRecord(root, scope: saved))
        await store.setProtectedDataAvailable(false)
        let controller = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await controller.load()
        #expect(controller.state == .failed(.protectedDataUnavailable))
        controller.setExpanded(id: "new", expanded: true)
        controller.setExpanded(id: "collapse", expanded: false)
        await controller.flush()
        #expect(controller.isExpanded(id: "new"))
        #expect(try Data(contentsOf: expansionRecord(root, scope: saved)) == before)
        await store.setProtectedDataAvailable(true)
        await controller.load(); await controller.flush()
        #expect(controller.state == .ready && controller.isExpanded(id: "retained"))
        #expect(controller.isExpanded(id: "new") && !controller.isExpanded(id: "collapse"))
        #expect(
            try await store.activate(controller.scope) == [try expansionDigest("retained"), try expansionDigest("new")])
    }

    @Test func corruptReadNeverOverwritesUnknownRecordUntilSuccessfulRetry() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root), owner = expansionScope()
        _ = try await store.activate(owner)
        let file = try expansionRecord(root, scope: owner), corrupt = Data("future-or-corrupt".utf8)
        try writeExpansionBytes(corrupt, root: root, scope: owner)
        let controller = LibraryProjectExpansionController(scope: owner, store: store)
        await controller.load()
        #expect(controller.state == .failed(.invalidRecord))
        controller.setExpanded(id: "local", expanded: true)
        controller.retrySave(); await controller.flush()
        #expect(try Data(contentsOf: file) == corrupt)
        let retained = try expansionDigest("retained")
        let repaired = try JSONSerialization.data(withJSONObject: ["version": 1, "expanded": [retained]])
        try writeExpansionBytes(repaired, root: root, scope: owner)
        await controller.load(); await controller.flush()
        #expect(controller.state == .ready && controller.isExpanded(id: "local"))
        #expect(controller.isExpanded(id: "retained"))
    }

    @Test func lockedWriteRetainsPreviousRecordAndRetryPublishesLatestChoice() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root)
        let controller = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await controller.load()
        controller.setExpanded(id: "previous", expanded: true)
        await controller.flush()
        let file = try expansionRecord(root, scope: controller.scope), before = try Data(contentsOf: file)
        await store.setProtectedDataAvailable(false)
        controller.setExpanded(id: "previous", expanded: false)
        controller.setExpanded(id: "latest", expanded: true)
        await controller.flush()
        #expect(controller.saveIssue == .protectedDataUnavailable && controller.isExpanded(id: "latest"))
        #expect(try Data(contentsOf: file) == before)
        await store.setProtectedDataAvailable(true)
        controller.retrySave(); await controller.flush()
        #expect(controller.saveIssue == nil)
        #expect(try await store.activate(controller.scope) == [try expansionDigest("latest")])
    }

    @Test func failedDeletionCancelsMarkerAndPreservesChoicesAcrossRestart() async throws {
        let root = expansionDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = LibraryProjectExpansionStore(root: root)
        let controller = LibraryProjectExpansionController(scope: expansionScope(), store: store)
        await controller.load()
        controller.setExpanded(id: "retained", expanded: true)
        try await controller.prepareAccountDeletion()
        try await controller.cancelAccountDeletion()
        try await controller.retire()
        let reopened = LibraryProjectExpansionStore(root: root)
        try await reopened.startupCleanup()
        #expect(try await reopened.activate(expansionScope()) == [try expansionDigest("retained")])
    }

    @Test func lateLoadCannotPublishAfterRetirement() async throws {
        let store = ControlledExpansionStore(),
            controller = LibraryProjectExpansionController(
                scope: expansionScope(), store: store)
        let loading = Task { await controller.load() }
        await store.waitForLoad()
        controller.setExpanded(id: "local", expanded: true)
        try await controller.retire()
        await store.finishLoad([try expansionDigest("late")])
        await loading.value
        #expect(controller.state == .retired)
        #expect(!controller.isExpanded(id: "late") && !controller.isExpanded(id: "local"))
        #expect(await store.saved == nil)
    }

    @Test func pendingChoicesAndMergedRecordAreBoundedWithoutEvictingStoredChoices() async throws {
        let store = ControlledExpansionStore(),
            controller = LibraryProjectExpansionController(
                scope: expansionScope(), store: store)
        let loading = Task { await controller.load() }
        await store.waitForLoad()
        for index in 0..<LibraryProjectExpansionStore.maximumProjects {
            #expect(controller.setExpanded(id: "local-\(index)", expanded: false))
        }
        #expect(!controller.setExpanded(id: "overflow", expanded: false))
        await store.finishLoad([]); await loading.value; await controller.flush()
        #expect(controller.state == .ready)
        let otherStore = ControlledExpansionStore(),
            other = LibraryProjectExpansionController(
                scope: expansionScope(), store: otherStore)
        let otherLoading = Task { await other.load() }
        await otherStore.waitForLoad()
        #expect(other.setExpanded(id: "new-local", expanded: true))
        let full = Set(
            (0..<LibraryProjectExpansionStore.maximumProjects).map {
                ProtectedChatFiles.digest(Data("saved-\($0)".utf8))
            })
        await otherStore.finishLoad(full); await otherLoading.value; await other.flush()
        #expect(other.state == .failed(.invalidRecord) && other.isExpanded(id: "new-local"))
        #expect(await otherStore.saved == nil)
    }
}

private actor ControlledExpansionStore: LibraryProjectExpansionStoring {
    var saved: Set<String>?
    private var continuation: CheckedContinuation<Set<String>, Never>?
    private var started = false
    func activate(_ scope: AccountScope) async throws -> Set<String> {
        started = true
        return await withCheckedContinuation { continuation = $0 }
    }
    func save(_ digests: Set<String>, scope: AccountScope, revision: UInt64) { saved = digests }
    func prepareRemoval(_ scope: AccountScope) {}
    func cancelRemoval(_ scope: AccountScope) {}
    func retire(_ scope: AccountScope, removePersisted: Bool) {}
    func waitForLoad() async { while !started { await Task.yield() } }
    func finishLoad(_ digests: Set<String>) { continuation?.resume(returning: digests); continuation = nil }
}
