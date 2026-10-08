import Darwin
import Foundation
import Testing

@testable import MapleAppCore

@Suite("Protected draft storage")
struct ProtectedChatStoreTests {
    private let account = AccountScope(apiOrigin: "http://127.0.0.1:1234", principalID: "fixture-A")

    private func directory() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(
            "maple-storage-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        return root
    }

    private func snapshot(key: String = ChatSnapshot.newChatKey, revision: UInt64 = 1, byte: UInt8 = 1) -> ChatSnapshot
    {
        ChatSnapshot(
            storageKey: key,
            draft: ChatDraft(
                text: "local fixture", images: [ChatImageAttachment(data: Data([byte]), contentType: "image/png")]),
            revision: revision)
    }

    @Test func relaunchRestoresProtectedImagesWithoutInflatingMetadata() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        var saved = snapshot()
        saved.draft.images[0].data = Data(repeating: 7, count: 20 * 1_024 * 1_024)
        try await store.save(saved, scope: account)
        let names = try FileManager.default.contentsOfDirectory(atPath: root.path)
        #expect(names.count == 2)
        let manifest = root.appendingPathComponent(ProtectedChatFiles.accountName(account) + ".json")
        #expect(try Data(contentsOf: manifest).count < 2_048)
        for name in names {
            let file = root.appendingPathComponent(name)
            let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
            #expect((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600)
            #expect(try file.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)
            #if os(iOS)
                #expect(attributes[.protectionKey] as? FileProtectionType == .complete)
            #endif
        }
        #expect(
            (try FileManager.default.attributesOfItem(atPath: root.path)[.posixPermissions] as? NSNumber)?.intValue
                == 0o700)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(
            AccountScope(apiOrigin: account.apiOrigin, principalID: account.principalID, generation: account.generation)
        )
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func lockedFilesDeferChangesWithoutDeletingSavedDraft() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        await store.setProtectedDataAvailable(false)
        var edited = saved
        edited.draft.text = "waiting for unlock"
        edited.revision += 1
        await #expect(throws: ChatStorageError.protectedDataUnavailable) {
            try await store.save(edited, scope: account)
        }
        await #expect(throws: ChatStorageError.protectedDataUnavailable) { try await store.loadAll(scope: account) }
        await store.setProtectedDataAvailable(true)
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
        try await store.save(edited, scope: account)
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == edited)
    }

    @Test func retiredGenerationCannotReactivateEvenForSamePrincipal() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        try await store.retire(account)
        await #expect(throws: ChatStorageError.retiredOwner) { try await store.activate(account) }
        await #expect(throws: ChatStorageError.retiredOwner) { try await store.save(saved, scope: account) }
        let replacement = AccountScope(apiOrigin: account.apiOrigin, principalID: account.principalID)
        try await store.activate(replacement)
        #expect(try await store.loadAll(scope: replacement).isEmpty)
        await #expect(throws: ChatStorageError.retiredOwner) { try await store.retire(account) }
    }

    @Test func cleanupWhileLockedRevokesImmediatelyAndBlocksReplacement() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        try await store.save(snapshot(), scope: account)
        await store.setProtectedDataAvailable(false)
        await #expect(throws: ChatStorageError.protectedDataUnavailable) { try await store.retire(account) }
        await #expect(throws: ChatStorageError.retiredOwner) { try await store.loadAll(scope: account) }
        let replacement = AccountScope(apiOrigin: account.apiOrigin, principalID: account.principalID)
        await #expect(throws: ChatStorageError.protectedDataUnavailable) { try await store.activate(replacement) }
        await store.setProtectedDataAvailable(true)
        try await store.activate(replacement)
        #expect(try await store.loadAll(scope: replacement).isEmpty)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }

    @Test func accountAndOriginNamespacesDoNotShareAttachments() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let first = ProtectedChatStore(root: root)
        try await first.activate(account)
        let saved = snapshot()
        try await first.save(saved, scope: account)
        let other = AccountScope(apiOrigin: "http://127.0.0.1:5678", principalID: account.principalID)
        let second = ProtectedChatStore(root: root)
        try await second.activate(other)
        #expect(try await second.loadAll(scope: other).isEmpty)
        try await second.save(saved, scope: other)
        #expect(
            try FileManager.default.contentsOfDirectory(atPath: root.path).filter { $0.hasSuffix(".blob") }.count == 2)
        try await first.retire(account)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(other)
        #expect(try await reopened.load(chatID: saved.storageKey, scope: other) == saved)
    }

    @Test func rekeyTombstonesOldOwnerAndRetryPreservesFreshNewChatAcrossRestart() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let original = snapshot()
        try await store.save(original, scope: account)
        var moved = original
        moved.storageKey = "conversation-A"
        moved.conversationID = "conversation-A"
        moved.revision += 1
        try await store.rekey(from: original.storageKey, to: moved, scope: account)
        var late = original
        late.revision += 100
        await #expect(throws: ChatStorageError.staleRevision) { try await store.save(late, scope: account) }
        let fresh = snapshot(byte: 2)
        try await store.save(fresh, scope: account)
        var edited = moved
        edited.draft.text = "later conversation edit"
        edited.revision += 1
        try await store.save(edited, scope: account)
        try await store.rekey(from: original.storageKey, to: moved, scope: account)
        #expect(try await store.load(chatID: fresh.storageKey, scope: account) == fresh)
        #expect(try await store.load(chatID: moved.storageKey, scope: account) == edited)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(account)
        await #expect(throws: ChatStorageError.staleRevision) { try await reopened.save(late, scope: account) }
        try await reopened.rekey(from: original.storageKey, to: moved, scope: account)
        #expect(try await reopened.loadAll(scope: account).count == 2)
    }

    @Test func detachedPendingOwnerUsesOutgoingDraftAndCanBeDiscoveredAfterRestart() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let original = snapshot()
        try await store.save(original, scope: account)
        var pending = ChatSnapshot(
            storageKey: "pending:" + UUID().uuidString, storageOwnerID: original.storageOwner,
            pendingRun: ChatPendingRun(
                id: UUID(), messageID: UUID().uuidString.lowercased(), outgoingDraft: original.draft,
                options: ChatSendOptions()),
            revision: original.revision + 1)
        try await store.rekey(from: original.storageKey, to: pending, scope: account)
        var oldComposer = original
        oldComposer.revision = 99
        await #expect(throws: ChatStorageError.staleRevision) { try await store.save(oldComposer, scope: account) }
        var blankComposer = pending
        blankComposer.storageKey = original.storageKey
        blankComposer.revision = 99
        await #expect(throws: ChatStorageError.staleRevision) { try await store.save(blankComposer, scope: account) }
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(account)
        #expect(try await reopened.loadAll(scope: account) == [pending])
        let pendingKey = pending.storageKey
        pending.storageKey = "conversation-B"
        pending.conversationID = pending.storageKey
        pending.revision += 1
        try await reopened.rekey(from: pendingKey, to: pending, scope: account)
        #expect(try await reopened.load(chatID: pendingKey, scope: account) == nil)
    }

    @Test func collisionsAndStaleRevisionsLeaveBothSnapshotsIntact() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let source = snapshot()
        let destination = snapshot(key: "occupied", byte: 2)
        try await store.save(source, scope: account)
        try await store.save(destination, scope: account)
        var moved = source
        moved.storageKey = destination.storageKey
        moved.revision += 1
        await #expect(throws: ChatStorageError.destinationOccupied) {
            try await store.rekey(from: source.storageKey, to: moved, scope: account)
        }
        var stale = source
        stale.draft.text = "conflicting same revision"
        await #expect(throws: ChatStorageError.staleRevision) { try await store.save(stale, scope: account) }
        try await store.save(source, scope: account)
        #expect(try await store.load(chatID: source.storageKey, scope: account) == source)
        #expect(try await store.load(chatID: destination.storageKey, scope: account) == destination)
    }

    @Test(arguments: [ProtectedChatStore.Publication.image, .manifest])
    func interruptedPublicationPreservesLastCommittedSnapshot(point: ProtectedChatStore.Publication) async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let failure = PublicationFailure()
        let store = ProtectedChatStore(root: root, limits: .init(), beforePublication: { try failure.check($0) })
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        var edited = saved
        edited.draft.images[0].data = Data([9, 8, 7])
        edited.revision += 1
        failure.arm(point)
        await #expect(throws: ChatStorageError.writeFailed) { try await store.save(edited, scope: account) }
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(account)
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 2)
    }

    @Test func failedRekeyDoesNotPublishTombstoneOrLoseSource() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let failure = PublicationFailure()
        let store = ProtectedChatStore(root: root, limits: .init(), beforePublication: { try failure.check($0) })
        try await store.activate(account)
        let original = snapshot()
        try await store.save(original, scope: account)
        var moved = original
        moved.storageKey = "conversation-pending"
        moved.revision += 1
        failure.arm(.manifest)
        await #expect(throws: ChatStorageError.writeFailed) {
            try await store.rekey(from: original.storageKey, to: moved, scope: account)
        }
        #expect(try await store.load(chatID: original.storageKey, scope: account) == original)
        #expect(try await store.load(chatID: moved.storageKey, scope: account) == nil)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(account)
        var edited = original
        edited.revision += 1
        edited.draft.text = "preserved source remains editable"
        try await reopened.save(edited, scope: account)
        moved.revision = edited.revision + 1
        try await reopened.rekey(from: edited.storageKey, to: moved, scope: account)
        #expect(try await reopened.load(chatID: original.storageKey, scope: account) == nil)
    }

    @Test(arguments: [false, true])
    func linkedAttachmentCannotBeReadThroughManifest(hardlink: Bool) async throws {
        let root = try directory()
        let outside = try directory()
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: outside) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        try await store.save(snapshot(), scope: account)
        let blobName = try #require(
            FileManager.default.contentsOfDirectory(atPath: root.path).first { $0.hasSuffix(".blob") })
        let blob = root.appendingPathComponent(blobName)
        let target = outside.appendingPathComponent("sentinel")
        try FileManager.default.moveItem(at: blob, to: target)
        if hardlink {
            try FileManager.default.linkItem(at: target, to: blob)
        } else {
            try FileManager.default.createSymbolicLink(at: blob, withDestinationURL: target)
        }
        let reopened = ProtectedChatStore(root: root)
        await #expect(throws: ChatStorageError.unsafePath) { try await reopened.activate(account) }
        #expect(try Data(contentsOf: target) == Data([1]))
    }

    @Test func changedAttachmentBytesDoNotMutatePreviousCommittedBlob() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        var second = saved
        second.storageKey = "second-chat"
        second.storageOwnerID = UUID()
        second.draft.images[0].data = Data([2])
        try await store.save(second, scope: account)
        let reopened = ProtectedChatStore(root: root)
        try await reopened.activate(account)
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
        #expect(try await reopened.load(chatID: second.storageKey, scope: account) == second)
        try await store.remove(chatID: saved.storageKey, scope: account)
        #expect(
            try FileManager.default.contentsOfDirectory(atPath: root.path).filter { $0.hasSuffix(".blob") }.count == 1)
    }

    @Test func limitsRejectWholeSaveWithoutPublishingPartialAttachments() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let limits = ProtectedChatStore.Limits(
            metadataBytes: 2_048, imageBytes: 4, accountImageBytes: 5, retainedImages: 2, imagesPerDraft: 2)
        let store = ProtectedChatStore(root: root, limits: limits, beforePublication: { _ in })
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        var edited = saved
        edited.revision += 1
        edited.draft.images[0].data = Data(count: 5)
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(edited, scope: account) }
        edited.draft.images = [
            ChatImageAttachment(data: Data(count: 3), contentType: "image/png"),
            ChatImageAttachment(data: Data(count: 3), contentType: "image/png"),
        ]
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(edited, scope: account) }
        edited.draft.images = Array(repeating: saved.draft.images[0], count: 3)
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(edited, scope: account) }
        edited.draft.images = []
        edited.draft.text = String(repeating: "x", count: 3_000)
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(edited, scope: account) }
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 2)
        let production = ProtectedChatStore.Limits()
        #expect(production.imageBytes == 20 * 1_024 * 1_024)
        #expect(production.accountImageBytes == 256 * 1_024 * 1_024)
        #expect(production.retainedImages == 640)
        #expect(production.imagesPerDraft == 10)
    }

    @Test func sharedImageIdentityAcrossDraftsCountsOnceButChangedBytesCountSeparately() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let limits = ProtectedChatStore.Limits(
            metadataBytes: 8_192, imageBytes: 2, accountImageBytes: 2, retainedImages: 1, imagesPerDraft: 10)
        let store = ProtectedChatStore(root: root, limits: limits, beforePublication: { _ in })
        try await store.activate(account)
        let image = ChatImageAttachment(data: Data([1, 2]), contentType: "image/png")
        var saved = snapshot()
        saved.draft.images = [image]
        saved.pendingRun = ChatPendingRun(
            id: UUID(), messageID: UUID().uuidString.lowercased(), outgoingDraft: ChatDraft(images: [image]),
            options: ChatSendOptions())
        saved.unsentTurn = ChatDraft(images: [image])
        try await store.save(saved, scope: account)
        let reopened = ProtectedChatStore(root: root, limits: limits, beforePublication: { _ in })
        try await reopened.activate(account)
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
        var edited = saved
        edited.revision += 1
        edited.unsentTurn?.images[0].data = Data([3, 4])
        await #expect(throws: ChatStorageError.capacityExceeded) { try await reopened.save(edited, scope: account) }
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func signedOutCleanupFailureAlsoBlocksReplacementActivation() async throws {
        let root = try directory()
        let outside = try directory()
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: outside) }
        let target = outside.appendingPathComponent("sentinel")
        try Data([7]).write(to: target)
        let manifest = root.appendingPathComponent(ProtectedChatFiles.accountName(account) + ".json")
        try FileManager.default.createSymbolicLink(at: manifest, withDestinationURL: target)
        let store = ProtectedChatStore(root: root)
        await #expect(throws: ChatStorageError.cleanupRequired) { try await store.clearSignedOutData() }
        let other = AccountScope(apiOrigin: account.apiOrigin, principalID: "fixture-B")
        await #expect(throws: ChatStorageError.cleanupRequired) { try await store.activate(other) }
        #expect(try Data(contentsOf: target) == Data([7]))
        try FileManager.default.removeItem(at: manifest)
        try await store.activate(other)
        #expect(try await store.loadAll(scope: other).isEmpty)
    }

    @Test func queueEditOriginalWorkingAndHeldImagesSurviveAsOneAtomicSnapshot() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let limits = ProtectedChatStore.Limits(
            metadataBytes: 64 * 1_024, imageBytes: 1, accountImageBytes: 3, retainedImages: 3)
        let store = ProtectedChatStore(root: root, limits: limits, beforePublication: { _ in })
        try await store.activate(account)
        let original = ChatQueuedTurn(draft: snapshot(byte: 1).draft)
        let held = snapshot(byte: 2).draft
        var working = snapshot(byte: 3).draft
        working.text = String(repeating: "a", count: 33 * 1_024)
        let saved = ChatSnapshot(
            storageKey: "queue-edit-chat", draft: working,
            queue: ChatQueueState(
                items: [original], edit: ChatQueueEdit(original: original, originalIndex: 0, heldDraft: held),
                paused: true), revision: 1)
        try await store.save(saved, scope: account)
        let reopened = ProtectedChatStore(root: root, limits: limits, beforePublication: { _ in })
        try await reopened.activate(account)
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 4)
        var attemptedSave = saved
        attemptedSave.revision += 1
        attemptedSave.queue?.items[0].draft = working
        attemptedSave.queue?.edit = nil
        await #expect(throws: ChatStorageError.capacityExceeded) {
            try await reopened.save(attemptedSave, scope: account)
        }
        #expect(try await reopened.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func accountQueueCapacityIncludesPoppedActiveItem() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        for index in 0..<4 {
            let items = (0..<16).map { ChatQueuedTurn(draft: ChatDraft(text: "queued \($0)")) }
            try await store.save(
                ChatSnapshot(storageKey: "chat-\(index)", queue: ChatQueueState(items: items), revision: 1),
                scope: account)
        }
        let active = ChatQueuedTurn(draft: ChatDraft(text: "popped but reserved"))
        var run = ChatPendingRun(
            id: UUID(), messageID: active.messageID, outgoingDraft: active.draft, options: active.options)
        run.queuedTurn = active
        let overflow = ChatSnapshot(storageKey: "active-queue-chat", pendingRun: run, revision: 1)
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(overflow, scope: account) }
        #expect(try await store.loadAll(scope: account).count == 4)
        try await store.remove(chatID: "chat-0", scope: account)
        try await store.save(overflow, scope: account)
        #expect(try await store.load(chatID: overflow.storageKey, scope: account) == overflow)
    }

    @Test func perChatWaitingLimitDoesNotChargePoppedActiveItemTwice() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let items = (0..<16).map { ChatQueuedTurn(draft: ChatDraft(text: "queued \($0)")) }
        let active = ChatQueuedTurn(draft: ChatDraft(text: "active queue item"))
        var run = ChatPendingRun(
            id: UUID(), messageID: active.messageID, outgoingDraft: active.draft, options: active.options)
        run.queuedTurn = active
        let saved = ChatSnapshot(pendingRun: run, queue: ChatQueueState(items: items), revision: 1)
        try await store.save(saved, scope: account)
        var overflow = saved
        overflow.revision += 1
        overflow.queue?.items.append(ChatQueuedTurn(draft: ChatDraft(text: "seventeenth waiting item")))
        await #expect(throws: ChatStorageError.capacityExceeded) { try await store.save(overflow, scope: account) }
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func inconsistentQueueEditCannotReplaceCommittedTransaction() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let original = ChatQueuedTurn(draft: ChatDraft(text: "original"))
        let saved = ChatSnapshot(queue: ChatQueueState(items: [original]), revision: 1)
        try await store.save(saved, scope: account)
        var invalid = saved
        invalid.revision += 1
        invalid.queue?.edit = ChatQueueEdit(original: original, originalIndex: 1, heldDraft: ChatDraft(text: "held"))
        await #expect(throws: ChatStorageError.corruptData) { try await store.save(invalid, scope: account) }
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func malformedClientMessageIdentityCannotReplaceSavedWork() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        let saved = snapshot()
        try await store.save(saved, scope: account)
        var invalid = saved
        invalid.revision += 1
        invalid.pendingRun = ChatPendingRun(
            id: UUID(), messageID: "malformed-message-id", outgoingDraft: ChatDraft(text: "outgoing"),
            options: ChatSendOptions())
        await #expect(throws: ChatStorageError.corruptData) { try await store.save(invalid, scope: account) }
        let turn = ChatQueuedTurn(draft: ChatDraft(text: "queued"))
        invalid.pendingRun?.messageID = UUID().uuidString.lowercased()
        invalid.pendingRun?.queuedTurn = turn
        await #expect(throws: ChatStorageError.corruptData) { try await store.save(invalid, scope: account) }
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
    }

    @Test func legacyStorageOwnerCanBeFrozenBeforeComposerChanges() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        var legacy = snapshot()
        legacy.storageOwnerID = nil
        try await store.save(legacy, scope: account)
        var edited = legacy
        edited.storageOwnerID = legacy.storageOwner
        edited.draft = ChatDraft(text: "new visible owner")
        edited.revision += 1
        try await store.save(edited, scope: account)
        var moved = edited
        moved.storageKey = "conversation-with-new-composer"
        moved.revision += 1
        try await store.rekey(from: edited.storageKey, to: moved, scope: account)
        #expect(try await store.load(chatID: moved.storageKey, scope: account) == moved)
        var late = legacy
        late.revision = 100
        await #expect(throws: ChatStorageError.staleRevision) { try await store.save(late, scope: account) }
    }

    @Test(arguments: [false, true])
    func linkedManifestsFailClosedIncludingDanglingSymlinks(hardlink: Bool) async throws {
        let root = try directory()
        let outside = try directory()
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: outside) }
        let target = outside.appendingPathComponent("sentinel")
        let manifest = root.appendingPathComponent(ProtectedChatFiles.accountName(account) + ".json")
        if hardlink {
            try Data("unrelated".utf8).write(to: target)
            try FileManager.default.linkItem(at: target, to: manifest)
        } else {
            try FileManager.default.createSymbolicLink(at: manifest, withDestinationURL: target)
        }
        let store = ProtectedChatStore(root: root)
        await #expect(throws: ChatStorageError.unsafePath) { try await store.activate(account) }
        if hardlink {
            #expect(try String(contentsOf: target, encoding: .utf8) == "unrelated")
        } else {
            #expect(!FileManager.default.fileExists(atPath: target.path))
        }
    }

    @Test func symlinkedRootIsNotUsed() async throws {
        let parent = try directory()
        defer { try? FileManager.default.removeItem(at: parent) }
        let target = parent.appendingPathComponent("target")
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: false)
        let link = parent.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: target)
        let store = ProtectedChatStore(root: link)
        await #expect(throws: ChatStorageError.unsafePath) { try await store.activate(account) }
        #expect(try FileManager.default.contentsOfDirectory(atPath: target.path).isEmpty)
    }

    @Test func blobCorruptionIsDetectedAndCleanupFailureBlocksSameAccountSignIn() async throws {
        let root = try directory()
        let outside = try directory()
        defer { try? FileManager.default.removeItem(at: root); try? FileManager.default.removeItem(at: outside) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        try await store.save(snapshot(), scope: account)
        let blobName = try #require(
            FileManager.default.contentsOfDirectory(atPath: root.path).first { $0.hasSuffix(".blob") })
        let blob = root.appendingPathComponent(blobName)
        try Data([2]).write(to: blob)
        let reopened = ProtectedChatStore(root: root)
        await #expect(throws: ChatStorageError.corruptData) { try await reopened.activate(account) }
        try FileManager.default.removeItem(at: blob)
        let target = outside.appendingPathComponent("sentinel")
        try Data([42]).write(to: target)
        try FileManager.default.createSymbolicLink(at: blob, withDestinationURL: target)
        await #expect(throws: ChatStorageError.cleanupRequired) { try await store.retire(account) }
        let replacement = AccountScope(apiOrigin: account.apiOrigin, principalID: account.principalID)
        await #expect(throws: ChatStorageError.cleanupRequired) { try await store.activate(replacement) }
        #expect(try Data(contentsOf: target) == Data([42]))
        try FileManager.default.removeItem(at: blob)
        try await store.activate(replacement)
        #expect(try await store.loadAll(scope: replacement).isEmpty)
    }

    @Test func signedOutCleanupOnlyRemovesRecognizedOwnedNamesAndCrashTemporaries() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        try await store.save(snapshot(), scope: account)
        let temporary = ".pending-" + ProtectedChatFiles.accountName(account) + "-" + UUID().uuidString
        try Data([3]).write(to: root.appendingPathComponent(temporary))
        try Data([4]).write(to: root.appendingPathComponent("unrelated.json"))
        let signedOut = ProtectedChatStore(root: root)
        try await signedOut.clearSignedOutData()
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path) == ["unrelated.json"])
    }

    @Test func inlineVersionOneMigratesAndUnknownVersionDoesNotOverwriteData() async throws {
        let root = try directory()
        defer { try? FileManager.default.removeItem(at: root) }
        struct Legacy: Codable {
            let version: Int; let origin: String; let principal: String; let chats: [String: ChatSnapshot]
        }
        let saved = snapshot()
        let manifest = root.appendingPathComponent(ProtectedChatFiles.accountName(account) + ".json")
        let legacy = Legacy(
            version: 1, origin: account.apiOrigin, principal: account.principalID, chats: [saved.storageKey: saved])
        try JSONEncoder().encode(legacy).write(to: manifest)
        let store = ProtectedChatStore(root: root)
        try await store.activate(account)
        #expect(try await store.load(chatID: saved.storageKey, scope: account) == saved)
        try await store.save(saved, scope: account)
        let migrated = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: manifest)) as? [String: Any])
        #expect(migrated["version"] as? Int == 2)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 2)
        let future = Legacy(version: 99, origin: account.apiOrigin, principal: account.principalID, chats: [:])
        let futureData = try JSONEncoder().encode(future)
        try futureData.write(to: manifest)
        let unsupported = ProtectedChatStore(root: root)
        await #expect(throws: ChatStorageError.unsupportedVersion) { try await unsupported.activate(account) }
        #expect(try Data(contentsOf: manifest) == futureData)
    }
}

private final class PublicationFailure: @unchecked Sendable {
    private let lock = NSLock()
    private var point: ProtectedChatStore.Publication?
    func arm(_ point: ProtectedChatStore.Publication) { lock.withLock { self.point = point } }
    func check(_ point: ProtectedChatStore.Publication) throws {
        try lock.withLock { if self.point == point { throw ChatStorageError.writeFailed } }
    }
}
