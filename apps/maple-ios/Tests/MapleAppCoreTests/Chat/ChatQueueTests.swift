import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatQueueTests {
    private func fixture() -> (ChatRuntime, ChatFixtureService, ChatFixtureStore) {
        ChatRuntimeTests().fixture()
    }

    @Test func liveSuccessPromotesFIFOWithOriginalMessageUUIDAndKeepsComposer() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "First"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Queued A"; #expect(runtime.send())
        runtime.draftText = "Queued C"; #expect(runtime.send())
        let firstQueued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Unsubmitted B"
        await service.emit(.completed(try chatLifecycle(chatResponse("first", .completed))))
        await service.waitForPosts(2)
        let posts = await service.posts
        #expect(posts[1].metadata?["internal_message_id"] == .string(firstQueued.messageID))
        #expect(firstQueued.messageID != firstQueued.id.uuidString.lowercased())
        #expect(runtime.snapshot.pendingRun?.queuedTurn == firstQueued)
        #expect(runtime.queuedTurns.map(\.draft.text) == ["Queued C"])
        #expect(runtime.draftText == "Unsubmitted B")
        #expect(await service.durableAtDispatch == [true, true])
        await service.finish(error: URLError(.networkConnectionLost)); await runtime.waitForCurrentWork()
        #expect(runtime.isQueuePaused && runtime.queuedTurns.count == 1)
        runtime.invalidate()
    }

    enum EmptyQueuePause: CaseIterable, Sendable { case reload, offline, stop }

    @Test(arguments: EmptyQueuePause.allCases)
    func freshRunAfterDrainedQueuePromotesNewFollowUp(_ pause: EmptyQueuePause) async throws {
        let (original, service, store) = fixture()
        var runtime = original, completions = 0
        defer { runtime.invalidate() }
        runtime.onCompletion = { _ in completions += 1 }
        runtime.draftText = "First run"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "First follow-up"; #expect(runtime.send())
        await service.emit(.completed(try chatLifecycle(chatResponse("first", .completed))))
        await service.waitForPosts(2)
        await service.emit(.completed(try chatLifecycle(chatResponse("first-follow-up", .completed))))
        await waitUntil { completions == 2 }
        try #require(!runtime.hasUnresolvedRun && runtime.queuedTurns.isEmpty)
        await runtime.saveDraft()

        var nextPost = 3
        switch pause {
        case .reload:
            let scope = runtime.scope
            runtime.invalidate()
            runtime = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "conversation")
            await runtime.load(chatID: "conversation")
            runtime.onCompletion = { _ in completions += 1 }
            try #require(runtime.isQueuePaused)
        case .offline:
            runtime.isOnline = false; runtime.isOnline = true
            try #require(runtime.isQueuePaused)
        case .stop:
            runtime.draftText = "Stopped without a queued follow-up"
            #expect(runtime.send()); await service.waitForPosts(3)
            await service.emit(.created(try chatLifecycle(chatResponse("stopped", .inProgress))))
            await waitUntil { runtime.snapshot.pendingRun?.responseID == "stopped" }
            runtime.stop(); await service.waitForCancellation(); await runtime.waitForCurrentWork()
            try #require(runtime.lastOutcome == .cancelled)
            #expect(await service.cancellations == ["stopped"])
            nextPost = 4
        }
        try #require(runtime.queuedTurns.isEmpty && !runtime.hasUnresolvedRun)

        runtime.draftText = "Second run"; #expect(runtime.send()); await service.waitForPosts(nextPost)
        #expect(!runtime.isQueuePaused)
        runtime.draftText = "Second follow-up"; #expect(runtime.send())
        let followUp = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Keep this unsubmitted draft"
        let heldDraft = runtime.snapshot.draft
        await service.emit(.completed(try chatLifecycle(chatResponse("second", .completed))))
        await waitUntil { completions == 3 }
        guard let promoted = runtime.snapshot.pendingRun else {
            #expect(runtime.queuedTurns.isEmpty)
            #expect(await service.posts.count == nextPost + 1)
            return
        }
        #expect(promoted.queuedTurn == followUp && promoted.messageID == followUp.messageID)
        #expect(runtime.snapshot.draft == heldDraft)
        await service.waitForPosts(nextPost + 1)
        #expect(await service.posts.last?.metadata?["internal_message_id"] == .string(followUp.messageID))
        await service.emit(.completed(try chatLifecycle(chatResponse("second-follow-up", .completed))))
        await waitUntil { completions == 4 }
        #expect(!runtime.hasUnresolvedRun && runtime.queuedTurns.isEmpty && !runtime.isQueuePaused)
        #expect(runtime.snapshot.draft == heldDraft)
        #expect(await service.durableAtDispatch == Array(repeating: true, count: nextPost + 1))
    }

    @Test func stopPreservesStagedFollowUpUntilExplicitResume() async throws {
        let (runtime, service, _) = fixture()
        defer { runtime.invalidate() }
        runtime.draftText = "Active run"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Keep this staged follow-up"; #expect(runtime.send())
        let staged = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Keep this composer draft"
        let held = runtime.snapshot.draft
        await service.emit(.created(try chatLifecycle(chatResponse("stopped", .inProgress))))
        await waitUntil { runtime.snapshot.pendingRun?.responseID == "stopped" }
        runtime.stop(); await service.waitForCancellation(); await runtime.waitForCurrentWork()
        #expect(runtime.lastOutcome == .cancelled && !runtime.hasUnresolvedRun)
        #expect(runtime.isQueuePaused && runtime.queuedTurns == [staged] && runtime.snapshot.draft == held)
        #expect(await service.posts.count == 1)
        #expect(await service.cancellations == ["stopped"])
        #expect(runtime.resumeQueue()); await service.waitForPosts(2)
        #expect(runtime.snapshot.pendingRun?.queuedTurn == staged && runtime.snapshot.draft == held)
        #expect(await service.posts.last?.metadata?["internal_message_id"] == .string(staged.messageID))
    }

    @Test(arguments: [false, true])
    func sendPreservesNonemptyPausedQueueAndWorkingEdit(_ editing: Bool) async throws {
        let (runtime, service, _) = fixture()
        defer { runtime.invalidate() }
        runtime.isOnline = false
        runtime.draftText = "Already staged"; #expect(runtime.enqueueDraft())
        let staged = try #require(runtime.queuedTurns.first)
        runtime.isOnline = true
        runtime.draftText = "Held composer"
        if editing {
            #expect(runtime.beginQueueEdit(id: staged.id))
            runtime.draftText = "Uncommitted edit"
            let prior = runtime.snapshot
            #expect(!runtime.send() && runtime.lastIssue == .unfinishedQueueEdit)
            #expect(runtime.snapshot == prior)
        } else {
            #expect(runtime.send())
            #expect(runtime.queuedTurns.count == 2 && runtime.queuedTurns.first == staged)
            #expect(runtime.queuedTurns.last?.draft.text == "Held composer")
        }
        #expect(runtime.isQueuePaused && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
    }

    enum FreshRunRejection: CaseIterable, Sendable { case empty, invalidImages, accountCapacity }

    @Test(arguments: FreshRunRejection.allCases)
    func rejectedFreshRunPreservesDrainedPausedQueue(_ rejection: FreshRunRejection) async throws {
        let (runtime, service, _) = fixture()
        defer { runtime.invalidate() }
        runtime.draftText = "Staged then removed"; #expect(runtime.enqueueDraft())
        runtime.deleteQueuedTurn(id: try #require(runtime.queuedTurns.first?.id))
        try #require(runtime.queuedTurns.isEmpty && runtime.isQueuePaused)
        let expectedIssue: ChatIssue
        switch rejection {
        case .empty: expectedIssue = .emptyDraft
        case .invalidImages:
            runtime.updateDraft(
                ChatDraft(
                    id: runtime.snapshot.draft.id, text: "Preserve this invalid image draft",
                    images: [ChatImageAttachment(data: Data(), contentType: "image/png")]))
            expectedIssue = .invalidImages
        case .accountCapacity:
            runtime.draftText = "Preserve this rejected draft"
            runtime.canAdmitSnapshot = { _ in false }
            expectedIssue = .queueLimit
        }
        let prior = runtime.snapshot
        #expect(!runtime.send() && runtime.lastIssue == expectedIssue)
        #expect(runtime.snapshot == prior && runtime.isQueuePaused && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
    }

    private func waitUntil(_ condition: @MainActor () -> Bool) async {
        for _ in 0..<20_000 {
            if condition() { return }
            await Task.yield()
        }
        #expect(condition())
    }

    @Test func completionDuringEditWaitsThenSaveRestoresHeldDraftAndPromotesEditedTurn() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Active"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Original A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        let item = try #require(runtime.queuedTurns.first)
        #expect(runtime.beginQueueEdit(id: item.id))
        let editID = runtime.snapshot.draft.id
        runtime.draftText = "Edited A"
        await service.emit(.completed(try chatLifecycle(chatResponse("active", .completed))))
        await runtime.waitForCurrentWork()
        #expect(runtime.isEditingQueue && runtime.queuedTurns == [item])
        #expect(await service.posts.count == 1)
        #expect(runtime.saveQueueEdit())
        await service.waitForPosts(2)
        #expect(runtime.snapshot.pendingRun?.outgoingDraft.text == "Edited A")
        #expect(runtime.snapshot.pendingRun?.messageID == item.messageID)
        #expect(runtime.draftText == "Held B")
        #expect(!runtime.updateOwnedDraft(ChatDraft(id: editID, text: "Late transcription")))
        runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func cancelAndDeleteEditorPreserveHeldDraft(delete: Bool) async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Original A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Second C"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        let item = try #require(runtime.queuedTurns.first)
        #expect(runtime.beginQueueEdit(id: item.id))
        runtime.draftText = "Uncommitted A"
        let retired = runtime.snapshot.draft.id
        var retiredIDs: [UUID] = []; runtime.onDraftOwnerRetired = { retiredIDs.append($0) }
        if delete { runtime.deleteQueuedTurn(id: item.id) } else { runtime.cancelQueueEdit() }
        #expect(runtime.draftText == "Held B" && !runtime.isEditingQueue && runtime.isQueuePaused)
        #expect(runtime.queuedTurns.map(\.draft.text) == (delete ? ["Second C"] : ["Original A", "Second C"]))
        #expect(retiredIDs == [retired])
        #expect(!runtime.updateOwnedDraft(ChatDraft(id: retired, text: "Late")))
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }

    @Test func restoredEditPreservesAllOwnersAndSaveStillRequiresExplicitResume() async throws {
        let (runtime, service, store) = fixture()
        runtime.draftText = "Original A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        let item = try #require(runtime.queuedTurns.first)
        #expect(runtime.beginQueueEdit(id: item.id)); runtime.draftText = "Working A"
        await runtime.saveDraft()
        let restored = ChatRuntime(
            scope: runtime.scope, service: service, persistence: store, conversationID: "conversation")
        runtime.invalidate()
        await restored.load(chatID: "conversation")
        #expect(restored.isEditingQueue && restored.isQueuePaused && restored.draftText == "Working A")
        #expect(restored.snapshot.queue?.edit?.heldDraft.text == "Held B")
        #expect(restored.queuedTurns == [item])
        #expect(!restored.resumeQueue())
        #expect(restored.saveQueueEdit())
        #expect(restored.draftText == "Held B" && restored.isQueuePaused)
        #expect(await service.posts.isEmpty)
        #expect(restored.resumeQueue()); await service.waitForPosts()
        #expect(restored.snapshot.pendingRun?.outgoingDraft.text == "Working A")
        restored.invalidate()
    }

    @Test func offlineCompletionAndReconnectionNeverResumeQueue() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Active"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Queued"; #expect(runtime.enqueueDraft())
        runtime.isOnline = false
        await service.emit(.completed(try chatLifecycle(chatResponse("active", .completed))))
        await runtime.waitForCurrentWork()
        runtime.isOnline = true
        #expect(runtime.isQueuePaused && !runtime.hasUnresolvedRun)
        #expect(await service.posts.count == 1)
        #expect(runtime.resumeQueue()); await service.waitForPosts(2)
        runtime.invalidate()
    }

    @Test func authoritativeRecoveryPreservesStagedQueueUntilResume() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Active"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Queued"; #expect(runtime.enqueueDraft())
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        await service.finish(error: URLError(.networkConnectionLost)); await runtime.waitForCurrentWork()
        await service.setExactItem(ConversationItem(id: messageID, responseId: "active", type: "message", role: "user"))
        await service.setRetrieved(chatResponse("active", .completed))
        await runtime.reconcile()
        #expect(!runtime.hasUnresolvedRun && runtime.isQueuePaused && runtime.queuedTurns.count == 1)
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func rejectedPromotedTurnReturnsToFrontWithoutOverwritingComposer() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Rejected A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Next C"; #expect(runtime.enqueueDraft())
        let original = runtime.queuedTurns
        runtime.draftText = "Held composer B"
        await service.setResponseErrors([ResearchAPIError.http(status: 413, reason: .contextLimit)])
        #expect(runtime.resumeQueue()); await runtime.waitForCurrentWork()
        #expect(runtime.queuedTurns == original && runtime.isQueuePaused && !runtime.hasUnresolvedRun)
        #expect(runtime.draftText == "Held composer B")
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func rejectedActiveQueueHeadRemainsDurableWithSixteenAlreadyWaiting() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "maple-chat-full-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ProtectedChatStore(root: directory)
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        try await store.activate(scope)
        let service = ChatFixtureService(store: ChatFixtureStore())
        let gate = ChatTestGate(); await service.setResponseGate(gate)
        await service.setResponseErrors([ResearchAPIError.http(status: 413, reason: .contextLimit)])
        let runtime = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "conversation")
        runtime.draftText = "Already admitted head"; #expect(runtime.enqueueDraft())
        let head = try #require(runtime.queuedTurns.first)
        #expect(runtime.resumeQueue()); await gate.waitForArrival()
        for number in 0..<16 { runtime.draftText = "Waiting \(number)"; #expect(runtime.enqueueDraft()) }
        runtime.draftText = "Held B"
        await gate.release(); await runtime.waitForCurrentWork()
        let saved = try #require(try await store.load(chatID: "conversation", scope: scope))
        #expect(saved.queue?.items.count == 17 && saved.queue?.recoveredHeadID == head.id)
        #expect(saved.draft.text == "Held B" && saved.pendingRun == nil)
        #expect(runtime.lastIssue == .contextLimit && runtime.isQueuePaused)
        #expect(!runtime.enqueueDraft()); #expect(runtime.draftText == "Held B")
        #expect(runtime.resumeQueue()); await service.waitForPosts(2)
        #expect(runtime.queuedTurns.count == 16 && runtime.snapshot.queue?.recoveredHeadID == nil)
        #expect(runtime.snapshot.pendingRun?.messageID == head.messageID)
        runtime.invalidate(); try await store.retire(scope)
    }

    @Test(arguments: [false, true])
    func rejectionDuringAnotherQueueEditPreservesWorkingOwnerAndPosition(queued: Bool) async throws {
        let (runtime, service, store) = fixture()
        let gate = ChatTestGate(); await service.setResponseGate(gate)
        await service.setResponseErrors([ResearchAPIError.http(status: 413, reason: .contextLimit)])
        runtime.draftText = "Rejected turn"
        if queued { #expect(runtime.enqueueDraft()); #expect(runtime.resumeQueue()) } else { #expect(runtime.send()) }
        await gate.waitForArrival()
        runtime.draftText = "Original editor A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        let editing = try #require(runtime.queuedTurns.first)
        #expect(runtime.beginQueueEdit(id: editing.id))
        let editID = runtime.snapshot.draft.id
        runtime.draftText = ""
        await gate.release(); await runtime.waitForCurrentWork()
        #expect(runtime.isEditingQueue && runtime.snapshot.draft.id == editID && runtime.draftText.isEmpty)
        #expect(runtime.snapshot.queue?.edit?.originalIndex == (queued ? 1 : 0))
        #expect(runtime.snapshot.queue?.edit?.heldDraft.text == "Held B")
        if queued {
            #expect(runtime.queuedTurns.map(\.draft.text) == ["Rejected turn", "Original editor A"])
        } else {
            #expect(runtime.snapshot.unsentTurn?.text == "Rejected turn")
        }
        #expect(await store.recorded("conversation") == runtime.snapshot)
        runtime.cancelQueueEdit(); #expect(runtime.draftText == "Held B")
        runtime.invalidate()
    }

    @Test func heldDraftDictationDoesNotBlockOtherEditorButRetiredEditorRejectsLateInsertion() async throws {
        let (runtime, _, _) = fixture()
        runtime.draftText = "A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "B"
        let heldID = runtime.snapshot.draft.id
        runtime.busyDraftIDs.insert(heldID)
        #expect(runtime.beginQueueEdit(id: try #require(runtime.queuedTurns.first?.id)))
        var held = try #require(runtime.ownedDraft(id: heldID)); held.text += " transcribed"
        #expect(runtime.updateOwnedDraft(held))
        #expect(runtime.draftText == "A")
        let editID = runtime.snapshot.draft.id
        runtime.busyDraftIDs.insert(editID)
        #expect(!runtime.saveQueueEdit())
        runtime.busyDraftIDs.remove(editID)
        #expect(runtime.saveQueueEdit())
        #expect(runtime.draftText == "B transcribed" && !runtime.canSend)
        #expect(!runtime.updateOwnedDraft(ChatDraft(id: editID, text: "Too late")))
        runtime.busyDraftIDs.remove(heldID); #expect(runtime.canSend)
        runtime.invalidate()
    }

    @Test func rejectedTurnAndRestoreActionDoNotReplaceEmptyBusyComposer() async throws {
        let (runtime, service, _) = fixture()
        let gate = ChatTestGate(); await service.setResponseGate(gate)
        await service.setResponseErrors([ResearchAPIError.http(status: 413, reason: .contextLimit)])
        runtime.draftText = "Rejected A"; #expect(runtime.send()); await gate.waitForArrival()
        let ownerB = runtime.snapshot.draft.id
        runtime.busyDraftIDs.insert(ownerB)
        await gate.release(); await runtime.waitForCurrentWork()
        #expect(runtime.snapshot.draft.id == ownerB && runtime.draftText.isEmpty)
        #expect(runtime.snapshot.unsentTurn?.text == "Rejected A")
        runtime.restoreUnsentTurn()
        #expect(runtime.snapshot.draft.id == ownerB)
        #expect(runtime.updateOwnedDraft(ChatDraft(id: ownerB, text: "Transcribed B")))
        runtime.busyDraftIDs.remove(ownerB)
        runtime.restoreUnsentTurn()
        #expect(runtime.draftText == "Rejected A" && runtime.snapshot.unsentTurn?.text == "Transcribed B")
        runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func composerChangeDuringFailedRekeyUsesStableStorageOwner(committed: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "maple-chat-rekey-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let actual = ProtectedChatStore(root: directory)
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        try await actual.activate(scope)
        let gate = ChatTestGate()
        let store = ChatFailingRekeyStore(underlying: actual, gate: gate, commitBeforeFailure: committed)
        let service = ChatFixtureService(store: ChatFixtureStore())
        let runtime = ChatRuntime(scope: scope, service: service, persistence: store)
        let storageOwner = runtime.snapshot.storageOwner
        runtime.draftText = "Unsent initial turn"; #expect(runtime.send())
        await gate.waitForArrival()
        runtime.draftText = "Queued original A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        #expect(runtime.beginQueueEdit(id: try #require(runtime.queuedTurns.first?.id)))
        runtime.draftText = "Working edit A"
        await gate.release(); await runtime.waitForCurrentWork()
        let saved = try #require(try await actual.load(chatID: "conversation", scope: scope))
        #expect(saved.storageOwner == storageOwner && saved == runtime.snapshot)
        #expect(saved.draft.text == "Working edit A" && saved.queue?.edit?.heldDraft.text == "Held B")
        #expect(saved.unsentTurn?.text == "Unsent initial turn")
        #expect(runtime.lastIssue == .storageUnavailable && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
        runtime.invalidate(); try await actual.retire(scope)
    }

    @Test func editBeforeConversationCreationReturnsKeepsStableMoveOwner() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "maple-chat-create-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ProtectedChatStore(root: directory)
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        try await store.activate(scope)
        let service = ChatFixtureService(store: ChatFixtureStore())
        let creation = ChatTestGate(), autosave = ChatTestGate()
        await service.setCreationGate(creation)
        let runtime = ChatRuntime(
            scope: scope, service: service, persistence: store,
            hooks: ChatRuntimeHooks(sleep: { _ in
                await autosave.hold(); try Task.checkCancellation()
            }))
        runtime.draftText = "Original turn"; #expect(runtime.send()); await creation.waitForArrival()
        let owner = runtime.snapshot.storageOwner
        runtime.draftText = "Queued A"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Held B"
        #expect(runtime.beginQueueEdit(id: try #require(runtime.queuedTurns.first?.id)))
        runtime.draftText = "Edited A"
        runtime.stop(); await creation.release(); await runtime.waitForCurrentWork()
        let saved = try #require(try await store.load(chatID: "conversation", scope: scope))
        #expect(saved.storageOwner == owner && saved == runtime.snapshot)
        #expect(saved.draft.text == "Edited A" && saved.queue?.edit?.heldDraft.text == "Held B")
        #expect(saved.unsentTurn?.text == "Original turn" && runtime.lastIssue == .stoppedBeforeSending)
        #expect(await service.posts.isEmpty)
        runtime.invalidate(); await autosave.release(); try await store.retire(scope)
    }

    @Test func queueUTF8AndCountBoundsRejectWithoutChangingDraft() async {
        let (runtime, service, _) = fixture()
        runtime.draftText = String(repeating: "界", count: 10_922)
        #expect(runtime.enqueueDraft())
        runtime.draftText = String(repeating: "界", count: 10_923)
        let oversized = runtime.snapshot.draft
        #expect(!runtime.enqueueDraft()); #expect(runtime.snapshot.draft == oversized)
        for number in 1..<16 { runtime.draftText = "Synthetic \(number)"; #expect(runtime.enqueueDraft()) }
        runtime.draftText = "Seventeenth"
        let full = runtime.snapshot.draft
        #expect(!runtime.enqueueDraft()); #expect(runtime.snapshot.draft == full && runtime.queuedTurns.count == 16)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }

    @Test func accountAdmissionCountsSharedImagesOnceAndRejectsExtraTurnWithoutDetaching() async {
        let image = ChatImageAttachment(data: Data([1, 2, 3]), contentType: "image/png")
        let original = ChatQueuedTurn(draft: ChatDraft(text: "A", images: [image]))
        let edited = ChatSnapshot(
            draft: ChatDraft(text: "Working A", images: [image]),
            queue:
                ChatQueueState(
                    items: [original],
                    edit: ChatQueueEdit(
                        original: original, originalIndex: 0,
                        heldDraft: ChatDraft(text: "B", images: [image]))))
        #expect(ChatResourceUsage(snapshots: [edited]).images == 1)
        #expect(ChatResourceUsage(snapshots: [edited]).imageBytes == 3)
        let retained = ChatSnapshot(
            storageKey: "other",
            queue: ChatQueueState(items: (0..<64).map { ChatQueuedTurn(draft: ChatDraft(text: "Synthetic \($0)")) }))
        let (runtime, _, _) = fixture()
        runtime.canAdmitSnapshot = { ChatResourceUsage(snapshots: [retained, $0]).fitsAccount }
        runtime.draftText = "Exceeds account queue count"
        let prior = runtime.snapshot
        #expect(!runtime.enqueueDraft())
        #expect(runtime.snapshot == prior && runtime.lastIssue == .queueLimit)
        runtime.invalidate()
    }
}
