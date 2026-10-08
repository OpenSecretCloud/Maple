import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatRuntimeTests {
    func fixture(conversationID: String? = "conversation", hooks: ChatRuntimeHooks = ChatRuntimeHooks())
        -> (ChatRuntime, ChatFixtureService, ChatFixtureStore)
    {
        let store = ChatFixtureStore(), scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let service = ChatFixtureService(store: store)
        return (
            ChatRuntime(
                scope: scope, service: service, persistence: store, conversationID: conversationID, hooks: hooks),
            service, store
        )
    }

    @Test func exactUUIDIsDurableBeforeDispatchAndDuplicateTapCannotSend() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Synthetic turn"
        #expect(runtime.send())
        #expect(!runtime.send())
        await service.waitForPosts()
        #expect(await service.posts.count == 1)
        #expect(await service.durableAtDispatch == [true])
        let request = try #require(await service.posts.first)
        #expect(
            request.metadata?["internal_message_id"] == .string(try #require(runtime.snapshot.pendingRun?.messageID)))
        await service.finish(error: URLError(.networkConnectionLost))
        await runtime.waitForCurrentWork()
        #expect(runtime.phase == .recovering)
        #expect(!runtime.send())
        runtime.invalidate()
    }

    @Test func offlineDraftEditingNeverSendsOnReconnection() async {
        let (runtime, service, _) = fixture()
        runtime.isOnline = false; runtime.draftText = "Edited offline"
        #expect(!runtime.canSend && !runtime.send())
        #expect(runtime.lastIssue == .offline)
        runtime.isOnline = true
        #expect(runtime.canSend)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }

    @Test func aggregateImageRequestLimitRejectsBeforeChangingDraftOwnership() async {
        let (runtime, service, _) = fixture()
        let data = Data(repeating: 1, count: 20 * 1_024 * 1_024)
        let draft = ChatDraft(
            id: runtime.snapshot.draft.id, text: "Keep all images",
            images: [
                ChatImageAttachment(data: data, contentType: "image/png"),
                ChatImageAttachment(data: data, contentType: "image/png"),
            ])
        runtime.updateDraft(draft)
        #expect(!runtime.send())
        #expect(runtime.snapshot.draft == draft && !runtime.hasUnresolvedRun)
        #expect(runtime.lastIssue == .draftTooLarge)
        #expect(await service.posts.isEmpty)
        #expect(await service.creations == 0)
        runtime.invalidate()
    }

    @Test func batchedTextFlushesBeforeTerminalAndVisibleCompletionOccursOnce() async throws {
        let (runtime, service, _) = fixture()
        var haptics = 0
        runtime.isVisible = true; runtime.onSuccessfulVisibleCompletion = { _ in haptics += 1 }
        let deltas = try (0..<200).map { _ in ResponseStreamEvent.outputTextDelta(try chatDelta("x")) }
        await service.setScript(
            [.created(try chatLifecycle(chatResponse("response", .inProgress)))] + deltas
                + [.completed(try chatLifecycle(chatResponse("response", .completed)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send())
        await runtime.waitForCurrentWork()
        #expect(
            runtime.messages.first { $0.id == "assistant" }?.content?.first?.text == String(repeating: "x", count: 200))
        #expect(runtime.lastOutcome == .completed && !runtime.hasUnresolvedRun)
        #expect(haptics == 1)
        await runtime.reconcile(); #expect(haptics == 1)
        runtime.invalidate()
    }

    @Test func olderPageLoadedDuringCreatedPersistenceIsNotOverwrittenByStreamSnapshot() async throws {
        let (runtime, service, store) = fixture()
        await service.setPage(
            APIPage(
                data: [ConversationItem(id: "recent", type: "message", role: "assistant")],
                lastId: "recent", hasMore: true))
        await runtime.load(chatID: "conversation")
        runtime.draftText = "New turn"; #expect(runtime.send()); await service.waitForPosts()
        let gate = ChatTestGate(); await store.configure(gate: gate)
        await service.emit(.created(try chatLifecycle(chatResponse("response", .inProgress))))
        await gate.waitForArrival()
        await service.setPage(APIPage(data: [ConversationItem(id: "older", type: "message", role: "assistant")]))
        await runtime.loadOlderMessages()
        #expect(runtime.messages.first?.id == "older")
        await gate.release()
        await service.emit(.completed(try chatLifecycle(chatResponse("response", .completed))))
        await runtime.waitForCurrentWork()
        #expect(runtime.messages.first?.id == "older")
        #expect(runtime.messages.contains { $0.id == "recent" })
        runtime.invalidate()
    }

    @Test func staleCapacityRetryWriteFailureCannotChangeReplacementRecoveryDispatchState() async throws {
        let (runtime, service, store) = fixture(hooks: ChatRuntimeHooks(sleep: { _ in }))
        let gate = ChatTestGate(); await store.failDispatchSave(2, gate: gate)
        let response = MapleResponse(
            status: 503,
            headers: [
                "x-opensecret-error-contract": "1", "x-opensecret-error-code": "inference_capacity",
                "x-opensecret-client-replay": "safe", "retry-after": "0",
            ], body: AsyncThrowingStream { $0.finish() }, inferenceSendCount: 1)
        await service.setResponseErrors([try #require(OpenSecretInferenceCapacityError(response: response))])
        runtime.draftText = "Potentially accepted"; #expect(runtime.send())
        await gate.waitForArrival()
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        await service.setExactItem(
            ConversationItem(id: messageID, responseId: "replacement", type: "message", role: "user"))
        await service.setRetrieved(chatResponse("replacement", .inProgress))
        // Capture and await the original task before the queued actor task
        // installs replacement recovery, then release its failed write.
        let replacement = Task {
            let recovery = Task { await runtime.reconcile() }
            await service.waitForItemRequest(); await gate.release(); await recovery.value
        }
        await runtime.waitForCurrentWork(); await replacement.value
        #expect(runtime.snapshot.pendingRun?.dispatched == true)
        #expect(runtime.snapshot.pendingRun?.responseID == "replacement")
        #expect(runtime.hasUnresolvedRun && runtime.phase == .recovering)
        await runtime.reconcile()
        #expect(runtime.hasUnresolvedRun && runtime.snapshot.pendingRun?.dispatched == true)
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func preIDRecoveryUsesExactUUIDAnd404OrWrongRoleDoesNotReleaseOwnership() async throws {
        let (runtime, service, _) = fixture()
        var haptics = 0
        runtime.isVisible = true; runtime.onSuccessfulVisibleCompletion = { _ in haptics += 1 }
        await service.setResponseErrors([URLError(.networkConnectionLost)])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        let messageID = try #require(runtime.snapshot.pendingRun?.messageID)
        await runtime.reconcile()
        #expect(runtime.hasUnresolvedRun)
        await service.setExactItem(
            ConversationItem(id: messageID, responseId: "wrong", type: "message", role: "assistant"))
        await runtime.reconcile(); #expect(runtime.snapshot.pendingRun?.responseID == nil)
        await service.setExactItem(
            ConversationItem(id: messageID, responseId: "response", type: "message", role: "user"))
        await service.setRetrieved(
            chatResponse(
                "response", .completed,
                output: [
                    ConversationItem(
                        id: "assistant", type: "message", role: "assistant",
                        content: [.init(type: "output_text", text: "Recovered")])
                ]))
        await runtime.reconcile()
        #expect(!runtime.hasUnresolvedRun && runtime.lastOutcome == .completed)
        #expect(await service.posts.count == 1)
        #expect(await service.itemRequests == [messageID, messageID, messageID])
        #expect(haptics == 0)
        runtime.invalidate()
    }

    @Test(arguments: [ResponseStatus.queued, .inProgress, .completed, .failed, .cancelled, .incomplete])
    func recoveryUsesAuthoritativeStatus(_ status: ResponseStatus) async throws {
        let (runtime, service, _) = fixture()
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(runtime.hasUnresolvedRun)
        await service.setRetrieved(chatResponse("response", status))
        await runtime.reconcile()
        #expect(runtime.hasUnresolvedRun == (status == .queued || status == .inProgress))
        #expect(runtime.lastOutcome == ChatRuntime.outcome(status))
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test(arguments: [ResponseStatus.queued, .inProgress])
    func terminalEventNameCannotOverrideNonterminalAuthoritativeStatus(_ status: ResponseStatus) async throws {
        let (runtime, service, _) = fixture()
        await service.setScript([
            .created(try chatLifecycle(chatResponse("response", .inProgress))),
            .completed(try chatLifecycle(chatResponse("response", status))),
        ])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(runtime.hasUnresolvedRun && runtime.phase == .recovering)
        #expect(runtime.lastOutcome == nil && runtime.lastIssue == .uncertainResponse)
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test func stopBeforeDurableAdmissionDoesNotCreateOrDispatch() async {
        let (runtime, service, store) = fixture(conversationID: nil)
        let gate = ChatTestGate(); await store.configure(gate: gate)
        runtime.draftText = "Keep this turn"; #expect(runtime.send())
        await gate.waitForArrival(); runtime.stop(); await gate.release()
        await runtime.waitForCurrentWork()
        #expect(await service.creations == 0)
        #expect(await service.posts.isEmpty)
        #expect(runtime.draftText == "Keep this turn" && !runtime.hasUnresolvedRun)
        runtime.invalidate()
    }

    @Test func stopDuringConversationCreationRekeysDraftWithoutResponsesPOST() async {
        let (runtime, service, store) = fixture(conversationID: nil)
        let gate = ChatTestGate(); await service.setCreationGate(gate)
        runtime.draftText = "Keep this turn"; #expect(runtime.send())
        await gate.waitForArrival(); runtime.stop(); await gate.release()
        await runtime.waitForCurrentWork()
        #expect(runtime.conversationID == "conversation" && runtime.draftText == "Keep this turn")
        #expect(await service.posts.isEmpty)
        #expect(await store.recorded(ChatSnapshot.newChatKey) == nil)
        #expect(await store.recorded("conversation")?.draft.text == "Keep this turn")
        runtime.invalidate()
    }

    @Test func stopBeforeCreatedKeepsIntentAndCancelsExactLateResponse() async throws {
        let (runtime, service, _) = fixture()
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await service.waitForPosts()
        runtime.stop()
        #expect(runtime.hasUnresolvedRun)
        await service.emit(.created(try chatLifecycle(chatResponse("response", .inProgress))))
        await service.waitForCancellation(); await runtime.waitForCurrentWork()
        #expect(await service.cancellations == ["response"])
        #expect(runtime.lastOutcome == .cancelled && !runtime.hasUnresolvedRun)
        runtime.invalidate()
    }

    @Test(arguments: [400, 503])
    func cancellationRaceRequiresSuccessfulCancelOrAuthenticated400(_ status: Int) async throws {
        let (runtime, service, _) = fixture()
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        await service.setCancelError(ResearchAPIError.http(status: status, reason: .unspecified))
        await service.setRetrieved(chatResponse("response", .completed))
        runtime.stop(); await runtime.waitForCurrentWork()
        #expect(runtime.hasUnresolvedRun == (status == 503))
        #expect(await service.retrievals.count == (status == 400 ? 1 : 0))
        if status == 503 { #expect(runtime.lastIssue == .cancellationUnavailable) }
        runtime.invalidate()
    }

    @Test func storageFailurePreventsPOSTAndPreservesDraft() async {
        let (runtime, service, store) = fixture()
        await store.configure(fail: true)
        runtime.draftText = "Unsent"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(await service.posts.isEmpty)
        #expect(runtime.draftText == "Unsent" && runtime.lastIssue == .storageUnavailable)
        runtime.invalidate()
    }

    @Test func rejectedTurnCannotOverwriteNewComposerEdits() async throws {
        let (runtime, service, _) = fixture()
        let gate = ChatTestGate(); await service.setResponseGate(gate)
        await service.setResponseErrors([ResearchAPIError.http(status: 413, reason: .contextLimit)])
        runtime.draftText = "Rejected original"; #expect(runtime.send())
        await gate.waitForArrival(); runtime.draftText = "New draft"; await gate.release()
        await runtime.waitForCurrentWork()
        #expect(runtime.draftText == "New draft" && runtime.snapshot.unsentTurn?.text == "Rejected original")
        runtime.restoreUnsentTurn()
        #expect(runtime.draftText == "Rejected original" && runtime.snapshot.unsentTurn?.text == "New draft")
        runtime.invalidate()
    }

    @Test func invalidatedOwnerRejectsLateConversationAndDraftWrites() async {
        let (runtime, service, store) = fixture(conversationID: nil)
        let gate = ChatTestGate(); await service.setCreationGate(gate)
        runtime.draftText = "Old account content"; #expect(runtime.send()); await gate.waitForArrival()
        let scope = runtime.scope
        let countBeforeRetirement = await store.saved.count
        // waitForCurrentWork captures the live task before this queued MainActor
        // mutation invalidates it, so the assertions observe the late return.
        Task {
            runtime.invalidate(); await store.retire(scope); await gate.release()
        }
        await runtime.waitForCurrentWork()
        #expect(runtime.messages.isEmpty && runtime.draftText.isEmpty && runtime.conversationID == nil)
        #expect(await service.posts.isEmpty)
        #expect(await store.saved.count == countBeforeRetirement)
        #expect(await store.recorded(ChatSnapshot.newChatKey) == nil)
    }

    @Test func newChatDuringCreationGetsIndependentDurableDraftOwner() async throws {
        let (runtime, service, store) = fixture(conversationID: nil)
        let gate = ChatTestGate(); await service.setCreationGate(gate)
        runtime.draftText = "First turn"; #expect(runtime.send())
        let pendingKey = runtime.snapshot.storageKey
        #expect(pendingKey.hasPrefix("pending:"))
        #expect(await runtime.prepareForNewChat())
        await gate.waitForArrival()
        #expect(await store.recorded(ChatSnapshot.newChatKey) == nil)
        #expect(await store.recorded(pendingKey)?.pendingRun?.outgoingDraft.text == "First turn")
        let fresh = ChatRuntime(scope: runtime.scope, service: service, persistence: store)
        await fresh.load(chatID: nil)
        fresh.draftText = "Second draft"; await fresh.saveDraft()
        runtime.stop(); await gate.release(); await runtime.waitForCurrentWork()
        #expect(runtime.draftText == "First turn")
        #expect(fresh.draftText == "Second draft")
        #expect(await store.recorded(ChatSnapshot.newChatKey)?.draft.text == "Second draft")
        #expect(await store.recorded("conversation")?.draft.text == "First turn")
        #expect(await store.recorded(pendingKey) == nil)
        #expect(await service.posts.isEmpty)
        runtime.invalidate(); fresh.invalidate()
    }

    @Test func interruptedConversationCreationIsDiscoverableAndNeverAutomaticallyReplayed() async throws {
        let (runtime, service, store) = fixture(conversationID: nil)
        let run = ChatPendingRun(
            id: UUID(), messageID: UUID().uuidString.lowercased(),
            outgoingDraft: ChatDraft(text: "Unsent before conversation ID"), options: .init())
        let pending = ChatSnapshot(
            storageKey: "pending:" + run.id.uuidString.lowercased(),
            pendingRun: run, revision: 12)
        await store.seed(pending)
        let discovered = try await store.loadAll(scope: runtime.scope)
        #expect(discovered == [pending])
        #expect(runtime.loadRecovered(try #require(discovered.first)))
        #expect(runtime.hasUnresolvedRun)
        #expect(await service.creations == 0)
        await runtime.reconcile()
        #expect(!runtime.hasUnresolvedRun && runtime.draftText == "Unsent before conversation ID")
        #expect(await service.creations == 0)
        #expect(await service.posts.isEmpty)
        #expect(runtime.snapshot.revision > pending.revision)
        runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func runtimeRekeyPreservesFreshNewChatWithProtectedStore(queued: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("maple-chat-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ProtectedChatStore(root: directory)
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        try await store.activate(scope)
        let service = ChatFixtureService(store: ChatFixtureStore())
        let gate = ChatTestGate(); await service.setCreationGate(gate)
        let first = ChatRuntime(scope: scope, service: service, persistence: store)
        first.draftText = "First turn"
        if queued { #expect(first.enqueueDraft()); #expect(first.resumeQueue()) } else { #expect(first.send()) }
        #expect(await first.prepareForNewChat())
        await gate.waitForArrival()
        let second = ChatRuntime(scope: scope, service: service, persistence: store)
        await second.load(chatID: nil)
        second.draftText = "New account draft"; await second.saveDraft()
        first.stop(); await gate.release(); await first.waitForCurrentWork()
        let savedFirst = try #require(try await store.load(chatID: "conversation", scope: scope))
        #expect((queued ? savedFirst.queue?.items.first?.draft.text : savedFirst.draft.text) == "First turn")
        #expect(try await store.load(chatID: ChatSnapshot.newChatKey, scope: scope)?.draft.text == "New account draft")
        #expect(first.lastIssue == nil && !first.hasUnresolvedRun)
        first.invalidate(); second.invalidate(); try await store.retire(scope)
    }

    @Test func coldLaunchRestoresNewChatWithoutSelectingInterruptedConversation() async throws {
        let (runtime, service, store) = fixture(conversationID: nil)
        let draft = ChatDraft(text: "Saved New Chat")
        await store.seed(ChatSnapshot(draft: draft, revision: 8))
        var run = ChatPendingRun(
            id: UUID(), messageID: UUID().uuidString.lowercased(), outgoingDraft: ChatDraft(text: "Older"),
            options: .init())
        run.dispatched = true
        await store.seed(ChatSnapshot(storageKey: "older", conversationID: "older", pendingRun: run, revision: 10))
        await runtime.load(chatID: nil)
        #expect(runtime.conversationID == nil && runtime.snapshot.draft == draft)
        #expect(await service.itemRequests.isEmpty)
        #expect(await service.posts.isEmpty)
        #expect(runtime.snapshot.revision == 8)
        runtime.invalidate()
    }

    @Test(arguments: [1, 2])
    func capacityRetrySharesTransportSendBudget(_ sends: Int) async throws {
        let (runtime, service, _) = fixture(hooks: ChatRuntimeHooks(sleep: { _ in }))
        let response = MapleResponse(
            status: 503,
            headers: [
                "x-opensecret-error-contract": "1", "x-opensecret-error-code": "inference_capacity",
                "x-opensecret-client-replay": "safe", "retry-after": "0",
            ], body: AsyncThrowingStream { $0.finish() }, inferenceSendCount: sends)
        let error = try #require(OpenSecretInferenceCapacityError(response: response))
        await service.setResponseErrors([error])
        await service.setScript([.completed(try chatLifecycle(chatResponse("response", .completed)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(await service.budgets == (sends == 1 ? [2, 1] : [2]))
        if sends == 1 {
            let calls = await service.posts
            #expect(calls[0].metadata == calls[1].metadata && runtime.lastOutcome == .completed)
        } else {
            #expect(runtime.draftText == "Synthetic turn" && runtime.lastOutcome == .rejected)
        }
        runtime.invalidate()
    }
}
