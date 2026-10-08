import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatDeletionTests {
    private func pending(conversationID: String? = "conversation", responseID: String? = "response") -> ChatSnapshot {
        var run = ChatPendingRun(
            id: UUID(), messageID: UUID().uuidString.lowercased(), outgoingDraft: ChatDraft(text: "retained"),
            options: ChatSendOptions())
        run.dispatched = true; run.responseID = responseID; run.phase = .recovering
        return ChatSnapshot(
            storageKey: conversationID ?? "pending:fixture", conversationID: conversationID, pendingRun: run)
    }
    private func fixture(_ snapshot: ChatSnapshot, hooks: ChatDeletionHooks = ChatDeletionHooks()) -> (
        ChatDeletionCoordinator, ChatFixtureService, ChatFixtureStore, AccountScope
    ) {
        let store = ChatFixtureStore(), service = ChatFixtureService(store: store)
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        return (
            ChatDeletionCoordinator(
                scope: scope, retained: [snapshot], service: service, persistence: store, hooks: hooks), service, store,
            scope
        )
    }
    @Test func unopenedPendingChatRequiresCancelAndTerminalProofBeforeRemoval() async throws {
        let snapshot = pending(), (coordinator, service, store, scope) = fixture(snapshot)
        await store.seed(snapshot)
        await service.setCancelError(ResearchAPIError.http(status: 400, reason: .unspecified))
        let lease = try coordinator.begin(.all)
        try await coordinator.quiesce(lease, timeout: .seconds(2))
        #expect(await service.cancellations == ["response"])
        #expect(await service.retrievals == ["response"])
        let replacement = ChatRuntime(
            scope: scope, service: service, persistence: store, conversationID: "conversation")
        let owner = coordinator.register(replacement)
        #expect(owner !== replacement && owner.isDeletionQuiescent)
        #expect(!owner.send())
        try await coordinator.finish(lease, deleteScope: true)
        #expect(await store.recorded("conversation") == nil)
        coordinator.release(lease); coordinator.invalidate()
    }
    @Test func genericCancelFailureCannotUseTerminalDatabaseRowAsDeletionProof() async throws {
        let (coordinator, service, _, _) = fixture(pending())
        await service.setCancelError(ResearchAPIError.http(status: 503, reason: .unspecified))
        let lease = try coordinator.begin(.all)
        do {
            try await coordinator.quiesce(lease, timeout: .milliseconds(300));
            Issue.record("Unverified cancellation allowed deletion")
        } catch ChatDeletionError.timedOut {}
        #expect(await service.retrievals.isEmpty)
        #expect(!(await service.cancellations).isEmpty)
        coordinator.release(lease); coordinator.invalidate()
    }
    @Test func missingExactPreIDItemRemainsUnresolvedWithoutReplay() async throws {
        let (coordinator, service, _, _) = fixture(pending(responseID: nil))
        let lease = try coordinator.begin(.all)
        do {
            try await coordinator.quiesce(lease, timeout: .milliseconds(300));
            Issue.record("Missing exact item incorrectly proved rejection")
        } catch ChatDeletionError.timedOut {}
        #expect(await service.posts.isEmpty)
        #expect(await service.creations == 0)
        #expect(await service.cancellations.isEmpty)
        coordinator.release(lease); coordinator.invalidate()
    }
    @Test func projectFenceAdmitsUnrelatedDraftAndBlocksFutureProjectCreation() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(conversationID: nil)
        let coordinator = ChatDeletionCoordinator(
            scope: runtime.scope, retained: [], service: service, persistence: store)
        coordinator.register(runtime)
        let lease = try coordinator.begin(.project("removed", knownConversationIDs: []))
        runtime.draftText = "kept composer"
        #expect(!runtime.send(options: ChatSendOptions(projectID: "removed")))
        #expect(runtime.draftText == "kept composer")
        #expect(runtime.send(options: ChatSendOptions(projectID: "another")))
        await service.waitForPosts()
        coordinator.release(lease); runtime.invalidate(); coordinator.invalidate()
    }
    @Test func inFlightCreationFencesDeletionAndLateResultNeverDispatchesResponse() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(conversationID: nil), gate = ChatTestGate()
        await service.setCreationGate(gate)
        let coordinator = ChatDeletionCoordinator(
            scope: runtime.scope, retained: [], service: service, persistence: store)
        coordinator.register(runtime)
        runtime.draftText = "retained"; #expect(runtime.send(options: ChatSendOptions(projectID: "project")))
        await gate.waitForArrival()
        let lease = try coordinator.begin(.project("project", knownConversationIDs: []))
        do {
            try await coordinator.quiesce(lease, timeout: .zero); Issue.record("Active creation passed deletion")
        } catch ChatDeletionError.timedOut {}
        await gate.release(); await runtime.waitForCurrentWork()
        try await coordinator.quiesce(lease, timeout: .seconds(2))
        #expect(runtime.conversationID == "conversation" && runtime.draftText == "retained")
        #expect(await service.posts.isEmpty)
        coordinator.release(lease); runtime.invalidate(); coordinator.invalidate()
    }
    @Test func openingUnopenedDeletedChatDuringCleanupCannotRegisterReplacementOwner() async throws {
        let snapshot = ChatSnapshot(
            storageKey: "conversation", conversationID: "conversation", draft: ChatDraft(text: "private"))
        let (coordinator, service, store, scope) = fixture(snapshot), gate = ChatTestGate()
        await store.seed(snapshot); await store.holdRemoval(gate)
        let lease = try coordinator.begin(.conversations(["conversation"]))
        try await coordinator.quiesce(lease)
        var removed: [ChatRuntime] = []
        coordinator.onRemoved = { removed.append($0) }
        let finishing = Task { try await coordinator.finish(lease, deletedConversationIDs: ["conversation"]) }
        await gate.waitForArrival()
        let incoming = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "conversation")
        let owner = coordinator.register(incoming)
        #expect(owner !== incoming && coordinator.isConversationDeleted("conversation"))
        await owner.load(chatID: "conversation")
        #expect(owner.draftText.isEmpty && !owner.canSend)
        await gate.release(); try await finishing.value
        #expect(removed.count == 1 && removed.first === owner)
        coordinator.release(lease)
        let late = ChatRuntime(scope: scope, service: service, persistence: store, conversationID: "conversation")
        let denied = coordinator.register(late)
        await denied.load(chatID: "conversation"); denied.draftText = "must not revive"
        #expect(!denied.canSend && denied.draftText.isEmpty)
        #expect(await store.recorded("conversation") == nil)
        coordinator.invalidate()
    }

    @Test func projectDiscoveryDoesNotCancelUnrelatedReplyAndReleasesItsRunningQueue() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        let coordinator = ChatDeletionCoordinator(
            scope: runtime.scope, retained: [], service: service, persistence: store)
        coordinator.register(runtime, projectID: "other")
        let libraryService = LibraryFixture()
        await libraryService.configure(conversations: [], failDelete: true, projectDiscoveryGate: gate)
        let library = LibraryModel(scope: runtime.scope, service: libraryService, deletions: coordinator)
        runtime.draftText = "running"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "queued"; #expect(runtime.send())
        let deletion = Task { await library.deleteProject(id: "removed") }
        await gate.waitForArrival()
        #expect(runtime.snapshot.pendingRun?.stopRequested == false)
        #expect(!runtime.isQueuePaused)
        await service.emit(.completed(try chatLifecycle(chatResponse("response", .completed))))
        await runtime.waitForCurrentWork()
        #expect(await service.cancellations.isEmpty)
        #expect(await service.posts.count == 1)
        await gate.release(); await deletion.value; await service.waitForPosts(2)
        #expect(library.issue == .connection && !runtime.isQueuePaused)
        #expect(await service.cancellations.isEmpty)
        runtime.invalidate(); coordinator.invalidate()
    }

    @Test func typedCompletionRetainsOnlyCurrentAssistantAndCanonicalTurn() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        await service.setPage(
            APIPage(data: [
                ConversationItem(
                    id: "old", type: "message", role: "assistant",
                    content: [MessageContent(type: "output_text", text: "old reply")])
            ]))
        await runtime.load(chatID: "conversation")
        var completion: ChatCompletion?
        runtime.isVisible = true; runtime.isForeground = true; runtime.onCompletion = { completion = $0 }
        let output = ConversationItem(
            id: "new", type: "message", role: "assistant",
            content: [MessageContent(type: "output_text", text: "new reply")])
        await service.setScript([.completed(try chatLifecycle(chatResponse("response", .completed, output: [output])))])
        runtime.draftText = "new turn"; #expect(runtime.send())
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        await runtime.waitForCurrentWork()
        let event = try #require(completion)
        #expect(event.scope == runtime.scope && event.messageID == messageID && event.responseID == "response")
        #expect(event.assistantItems.map(\.id) == ["new"] && event.assistantText == "new reply")
        #expect(event.wasVisible && event.wasForeground && !event.recovered && !event.wasStopped)
        #expect(runtime.snapshot.pendingRun == nil)
        runtime.invalidate()
    }

    @Test func backgroundCompletionPublishesOnceWithoutVisibleHaptic() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var completions: [String] = [], haptics = 0
        runtime.onSuccessfulCompletion = { completions.append($0) }
        runtime.onSuccessfulVisibleCompletion = { _ in haptics += 1 }
        await service.setScript([.completed(try chatLifecycle(chatResponse("response", .completed)))])
        runtime.draftText = "hello"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        await runtime.reconcile()
        #expect(completions == ["conversation"] && haptics == 0)
        runtime.invalidate()
    }
}

@MainActor @Suite struct ChatCreationRecoveryTests {
    private let nonce = String(repeating: "a", count: 64)
    private func snapshot() -> ChatSnapshot {
        var run = ChatPendingRun(
            id: UUID(), messageID: UUID().uuidString.lowercased(), outgoingDraft: ChatDraft(text: "safe unsent"),
            options: ChatSendOptions(projectID: "project"))
        run.creationID = nonce; run.creationDispatched = true; run.phase = .recovering
        return ChatSnapshot(storageKey: "pending:" + run.id.uuidString.lowercased(), pendingRun: run)
    }
    private func conversation(_ id: String, project: String? = "project") -> Conversation {
        Conversation(
            id: id, object: "conversation", createdAt: 0, metadata: ["internal_native_creation_id": .string(nonce)],
            projectId: project, pinned: false, lastActivityAt: 0)
    }
    @Test func exactCreationNonceAcrossPagesRestoresUnsentWithoutAnyPOST() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(conversationID: nil), retained = snapshot()
        await store.seed(retained); #expect(runtime.loadRetainedSnapshot(retained))
        await service.configureConversationPages([
            APIPage(
                data: [
                    Conversation(id: "other", object: "conversation", createdAt: 0, pinned: false, lastActivityAt: 0)
                ], lastId: "other", hasMore: true),
            APIPage(data: [conversation("found")]),
        ])
        await runtime.reconcile()
        #expect(runtime.conversationID == "found" && runtime.draftText == "safe unsent")
        #expect(!runtime.hasUnresolvedRun)
        #expect(await store.recorded(retained.storageKey) == nil)
        #expect(await store.recorded("found")?.draft.text == "safe unsent")
        #expect(await service.creations == 0)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }
    @Test(arguments: ["missing", "duplicate", "wrong-project", "cursor-loop"])
    func ambiguousCreationIsRetainedAndNeverRecreated(reason: String) async {
        let (runtime, service, store) = ChatRuntimeTests().fixture(conversationID: nil), retained = snapshot()
        await store.seed(retained); #expect(runtime.loadRetainedSnapshot(retained))
        let pages: [APIPage<Conversation>]
        switch reason {
        case "duplicate": pages = [APIPage(data: [conversation("a"), conversation("b")])]
        case "wrong-project": pages = [APIPage(data: [conversation("a", project: "other")])]
        case "cursor-loop":
            pages = [
                APIPage(data: [conversation("a")], lastId: "a", hasMore: true),
                APIPage(data: [conversation("a")], lastId: "a", hasMore: true),
            ]
        default: pages = [APIPage(data: [])]
        }
        await service.configureConversationPages(pages); await runtime.reconcile()
        #expect(runtime.conversationID == nil && runtime.hasUnresolvedRun)
        #expect(runtime.lastIssue == .uncertainResponse)
        #expect(await service.creations == 0)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }
}
