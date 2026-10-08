import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatLifecycleRecoveryTests {
    @Test func reopeningHealthyStreamDoesNotReplaceItOrSendAgain() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Outgoing"; #expect(runtime.send())
        await service.waitForPosts()
        await runtime.reconcileOnLifecycle(foreground: false)
        #expect(runtime.hasUnresolvedRun)
        #expect(await service.retrievals.isEmpty)
        #expect(await service.itemRequests.isEmpty)
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func foregroundDuringInitialDurableAdmissionLeavesTheOriginalSenderRunning() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        await store.configure(gate: gate)
        runtime.draftText = "Outgoing"; #expect(runtime.send())
        await gate.waitForArrival()
        await runtime.reconcileOnLifecycle(foreground: true)
        #expect(runtime.snapshot.pendingRun?.dispatched == false)
        #expect(runtime.hasUnresolvedRun && runtime.snapshot.unsentTurn == nil)
        #expect(await service.posts.isEmpty)
        await gate.release(); await service.waitForPosts()
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func foregroundRecoversExactPendingMessageAndKeepsDraftAndQueuePaused() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Outgoing"; #expect(runtime.send())
        await service.waitForPosts()
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        runtime.draftText = "Next queued"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Visible draft kept"; let draft = runtime.snapshot.draft
        await service.setExactItem(
            ConversationItem(id: messageID, responseId: "response", type: "message", role: "user"))
        await runtime.reconcileOnLifecycle(foreground: true)
        #expect(runtime.lastOutcome == .completed && !runtime.hasUnresolvedRun)
        #expect(runtime.snapshot.draft == draft)
        #expect(runtime.queuedTurns.count == 1 && runtime.isQueuePaused)
        #expect(await service.posts.count == 1)
        #expect(await service.itemRequests == [messageID])
        await runtime.reconcileOnLifecycle(foreground: true)
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func reopeningUncertainRunUsesExactLookupAndOfflineReconnectNeverDispatches() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Outgoing"; #expect(runtime.send())
        await service.waitForPosts()
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        await service.finish(error: URLError(.networkConnectionLost)); await runtime.waitForCurrentWork()
        runtime.draftText = "Typed while unavailable"; let owner = runtime.snapshot.draft.id
        runtime.isOnline = false
        await runtime.reconcileOnLifecycle(foreground: true)
        #expect(await service.itemRequests.isEmpty)
        runtime.isOnline = true
        #expect(await service.posts.count == 1)
        await runtime.reconcileOnLifecycle(foreground: false)
        #expect(runtime.phase == .recovering && runtime.hasUnresolvedRun)
        #expect(runtime.snapshot.draft.id == owner && runtime.draftText == "Typed while unavailable")
        #expect(await service.itemRequests == [messageID])
        #expect(await service.posts.count == 1)
        await service.setExactItem(
            ConversationItem(id: messageID, responseId: "response", type: "message", role: "user"))
        await runtime.reconcileOnLifecycle(foreground: false)
        #expect(runtime.lastOutcome == .completed)
        #expect(await service.itemRequests == [messageID, messageID])
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func overlappingOlderPageRequestsCannotRegressCursorOrBlockComposer() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        await service.setPage(
            APIPage(
                data: [ConversationItem(id: "latest", type: "message", role: "assistant")], lastId: "cursor-one",
                hasMore: true))
        await runtime.load(chatID: "conversation")
        runtime.draftText = "Editable during pagination"
        await service.setPage(
            APIPage(
                data: [ConversationItem(id: "older", type: "message", role: "assistant")], lastId: "cursor-two",
                hasMore: true))
        await service.setPageGate(gate)
        let first = Task { await runtime.loadOlderMessages() }
        await gate.waitForArrival()
        #expect(runtime.isLoadingOlder && !runtime.isLoading && runtime.canSend)
        await runtime.loadOlderMessages()
        #expect(await service.pageRequests.count == 2)
        runtime.draftText = "Still editable"
        await gate.release(); await first.value
        await service.setPageGate(nil)
        await service.setPage(
            APIPage(
                data: [ConversationItem(id: "oldest", type: "message", role: "assistant")], lastId: "cursor-three",
                hasMore: false))
        await runtime.loadOlderMessages()
        #expect(await service.pageRequests.map(\.after) == [nil, "cursor-one", "cursor-two"])
        #expect(runtime.messages.map(\.id) == ["oldest", "older", "latest"])
        #expect(runtime.draftText == "Still editable" && !runtime.isLoadingOlder)
        runtime.invalidate()
    }

    @Test func lifecycleDoesNotReplayPausedQueueWithoutPendingOwner() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isOnline = false; runtime.draftText = "Queued offline"; #expect(runtime.enqueueDraft())
        runtime.isOnline = true
        await runtime.reconcileOnLifecycle(foreground: true)
        await runtime.reconcileOnLifecycle(foreground: false)
        #expect(await service.posts.isEmpty)
        #expect(await service.creations == 0)
        #expect(runtime.queuedTurns.count == 1)
        runtime.invalidate()
    }
}
