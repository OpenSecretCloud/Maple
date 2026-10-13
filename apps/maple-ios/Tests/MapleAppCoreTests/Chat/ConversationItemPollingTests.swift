import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

private actor ConversationItemSleepRecorder {
    private(set) var delays: [Duration] = []
    let gate: ChatTestGate?
    let fails: Bool

    init(gate: ChatTestGate? = nil, fails: Bool = false) { self.gate = gate; self.fails = fails }

    func sleep(_ duration: Duration) async throws {
        delays.append(duration)
        if let gate { await gate.hold() }
        if fails { throw URLError(.timedOut) }
    }
}

@MainActor @Suite struct ConversationItemPollingTests {
    @Test func progressiveIntervalsRetainMinuteRefreshWhileIdle() async {
        let clock = ConversationItemSleepRecorder()
        var polls = 0
        await ConversationItemPolling.run(
            shouldContinue: { polls < 9 }, poll: { polls += 1 }, sleep: { try await clock.sleep($0) })
        #expect(polls == 9)
        #expect(
            await clock.delays == [
                .seconds(2), .seconds(5), .seconds(10), .seconds(15), .seconds(20), .seconds(30),
                .seconds(60), .seconds(60), .seconds(60),
            ])
    }

    enum LoopRevocation: CaseIterable, Sendable {
        case deselected, backgrounded, accountReplaced, taskReplaced
    }

    @Test(arguments: LoopRevocation.allCases)
    func ownershipChangeDuringInjectedSleepPreventsTheNextRequest(_ change: LoopRevocation) async {
        let gate = ChatTestGate(), clock = ConversationItemSleepRecorder(gate: gate)
        var selected = true, foreground = true, polls = 0
        var generation = UUID(), ticket = UUID()
        let capturedGeneration = generation, capturedTicket = ticket
        let polling = Task {
            await ConversationItemPolling.run(
                shouldContinue: {
                    selected && foreground && generation == capturedGeneration && ticket == capturedTicket
                }, poll: { polls += 1 }, sleep: { try await clock.sleep($0) })
        }
        await gate.waitForArrival()
        switch change {
        case .deselected: selected = false
        case .backgrounded: foreground = false
        case .accountReplaced: generation = UUID()
        case .taskReplaced: ticket = UUID()
        }
        await gate.release(); await polling.value
        #expect(polls == 0)
        #expect(await clock.delays == [.seconds(2)])
    }

    @Test func cancellationIsCheckedEvenWhenInjectedSleepReturnsNormally() async {
        let gate = ChatTestGate(), clock = ConversationItemSleepRecorder(gate: gate)
        var polls = 0
        let polling = Task {
            await ConversationItemPolling.run(
                shouldContinue: { true }, poll: { polls += 1 }, sleep: { try await clock.sleep($0) })
        }
        await gate.waitForArrival(); polling.cancel()
        await gate.release(); await polling.value
        #expect(polls == 0)
        #expect(await clock.delays == [.seconds(2)])
    }

    @Test func failingClockStopsWithoutRequesting() async {
        let clock = ConversationItemSleepRecorder(fails: true)
        var polls = 0
        await ConversationItemPolling.run(
            shouldContinue: { true }, poll: { polls += 1 }, sleep: { try await clock.sleep($0) })
        #expect(polls == 0)
        #expect(await clock.delays == [.seconds(2)])
    }

    @Test func presentationOwnerRestartsPollingAfterTemporaryDiscoveryFenceRelease() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        let firstSleep = ChatTestGate(), restartedSleep = ChatTestGate(), fence = UUID()
        var task: Task<Void, Never>?, ticket: UUID?, starts = 0, polls = 0

        func updatePolling() {
            ticket = nil; task?.cancel(); task = nil
            guard runtime.isVisible, !runtime.isRetired, runtime.deletionFenceIDs.isEmpty else { return }
            let capturedTicket = UUID(), gate = starts == 0 ? firstSleep : restartedSleep
            starts += 1; ticket = capturedTicket
            task = Task {
                defer { if ticket == capturedTicket { ticket = nil; task = nil } }
                await ConversationItemPolling.run(
                    shouldContinue: {
                        ticket == capturedTicket && runtime.isVisible && !runtime.isRetired
                            && runtime.deletionFenceIDs.isEmpty && polls == 0
                    },
                    poll: {
                        polls += 1; await runtime.pollNewItems()
                    },
                    sleep: { _ in await gate.hold() })
            }
        }

        runtime.onDeletionFenceChanged = { updatePolling() }
        updatePolling(); await firstSleep.waitForArrival()
        let firstTask = task
        runtime.beginDeletionFence(fence)
        await firstSleep.release(); await firstTask?.value
        #expect(starts == 1 && polls == 0 && task == nil)
        runtime.releaseDeletionFence(fence); await restartedSleep.waitForArrival()
        #expect(starts == 2)
        let restartedTask = task
        await restartedSleep.release(); await restartedTask?.value
        #expect(polls == 1 && task == nil)
        #expect(await service.pageRequests.count == 1)
        runtime.onDeletionFenceChanged = nil; runtime.invalidate()
    }

    @Test func loadedHistorySeedsAscendingCursorAndInProgressTailIsFetchedAgain() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        await service.setPage(APIPage(data: [item("partial", status: "in_progress"), item("latest"), item("older")]))
        await runtime.load(chatID: "conversation")
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("new"), item("tail", text: "Partial", status: "in_progress")]))
        await runtime.pollNewItems()
        #expect(runtime.messages.map(\.id) == ["older", "latest", "partial", "new", "tail"])
        #expect(runtime.polledMessagesRevision == 1)
        await service.setPage(APIPage(data: [item("tail", text: "Finished")]))
        await runtime.pollNewItems()
        let requests = await service.pageRequests
        #expect(requests.count == 3)
        #expect(requests[1].after == "latest" && requests[1].order == .asc && requests[1].limit == 20)
        #expect(requests[2].after == "new" && requests[2].order == .asc && requests[2].limit == 20)
        #expect(runtime.messages.last?.content?.first?.text == "Finished")
        #expect(runtime.messages.last?.status == "completed")
        #expect(runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test func noCursorRequestsNewestPageDescendingAndNormalizesBeforeMerging() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("newest"), item("older")]))
        await runtime.pollNewItems()
        #expect(runtime.messages.map(\.id) == ["older", "newest"])
        #expect(runtime.polledMessagesRevision == 1)
        await service.setPage(APIPage(data: [])); await runtime.pollNewItems()
        let requests = await service.pageRequests
        #expect(requests[0].after == nil && requests[0].order == .desc && requests[0].limit == 20)
        #expect(requests[1].after == "newest" && requests[1].order == .asc)
        #expect(runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test func duplicateUpdatesPreserveLongerTextAndComposerFeedback() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("answer", text: "Longer existing answer")]))
        await runtime.pollNewItems()
        runtime.draftText = String(repeating: "x", count: 32 * 1_024 + 1)
        #expect(!runtime.enqueueDraft() && runtime.lastIssue == .draftTooLarge)
        let issueID = runtime.lastIssueID, snapshot = runtime.snapshot
        await service.setPage(APIPage(data: [item("answer", text: "Short")]))
        await runtime.pollNewItems()
        #expect(runtime.messages.first?.content?.first?.text == "Longer existing answer")
        #expect(runtime.lastIssue == .draftTooLarge && runtime.lastIssueID == issueID)
        #expect(runtime.snapshot == snapshot && runtime.polledMessagesRevision == 1)
        await service.setPageError(URLError(.networkConnectionLost)); await runtime.pollNewItems()
        #expect(runtime.lastIssue == .draftTooLarge && runtime.lastIssueID == issueID)
        #expect(runtime.snapshot == snapshot && runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test func recoveringPollPreservesEntireRicherMessageAndReasoningContentWithServerTerminalStatus() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        let snapshot = runtime.snapshot, issueID = runtime.lastIssueID
        let parts = [
            MessageContent(type: "text", text: "Long first part"),
            MessageContent(type: "text", text: "Keep the second part too"),
        ]
        let retained = [
            ConversationItem(id: "answer", type: "message", status: "in_progress", role: "assistant", content: parts),
            ConversationItem(id: "thought", type: "reasoning", status: "in_progress", content: parts),
        ]
        await service.setPage(APIPage(data: Array(retained.reversed()))); await runtime.pollNewItems()
        await service.setPage(
            APIPage(data: [
                ConversationItem(
                    id: "thought", type: "reasoning", status: "error", content: [.init(type: "text", text: "A")]),
                ConversationItem(
                    id: "answer", type: "message", status: "incomplete", content: [.init(type: "text", text: "B")]),
            ]))
        await runtime.pollNewItems()
        #expect(runtime.messages.first { $0.id == "answer" }?.content == parts)
        #expect(runtime.messages.first { $0.id == "thought" }?.content == parts)
        #expect(runtime.messages.first { $0.id == "answer" }?.status == "incomplete")
        #expect(runtime.messages.first { $0.id == "thought" }?.status == "error")
        #expect(runtime.snapshot == snapshot && runtime.lastIssueID == issueID && runtime.hasUnresolvedRun)
        #expect(runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test(arguments: [
        JSONValue.string("Long retained tool payload 🧪"),
        JSONValue.object(["query": .string("Long retained query"), "count": .integer(20)]),
        JSONValue.array([.string("Long retained page"), .string("Second retained page")]),
    ])
    func recoveringPollRetainsRicherToolPayloadsAndPairingWithServerTerminalStatus(_ payload: JSONValue) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        let snapshot = runtime.snapshot, issueID = runtime.lastIssueID
        let types = ["function_call", "tool_call", "function_call_output", "tool_output"]
        let retained = types.map {
            ConversationItem(
                id: $0, type: $0, status: "in_progress", callId: "paired", name: "web_search",
                arguments: payload, output: payload)
        }
        await service.setPage(APIPage(data: retained)); await runtime.pollNewItems()
        await service.setPage(
            APIPage(
                data: types.enumerated().map { index, type in
                    ConversationItem(
                        id: type, type: type, status: index.isMultiple(of: 2) ? "completed" : "failed",
                        arguments: index.isMultiple(of: 2) ? .string("x") : nil,
                        output: index.isMultiple(of: 2) ? nil : .string("x"))
                }))
        await runtime.pollNewItems()
        for (index, type) in types.enumerated() {
            let tool = try #require(runtime.messages.first { $0.id == type })
            #expect(tool.arguments == payload && tool.output == payload)
            #expect(tool.callId == "paired" && tool.name == "web_search")
            #expect(tool.status == (index.isMultiple(of: 2) ? "completed" : "failed"))
        }
        #expect(runtime.snapshot == snapshot && runtime.lastIssueID == issueID && runtime.hasUnresolvedRun)
        #expect(runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    enum ToolOwnerConflict: CaseIterable, Sendable {
        case type, callID, responseID, name
    }

    @Test(arguments: ToolOwnerConflict.allCases)
    func conflictingToolOwnerNeverInheritsAnUnrelatedRetainedPayload(_ conflict: ToolOwnerConflict) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        let retained = ConversationItem(
            id: "tool", responseId: "first-response", type: "function_call", status: "completed",
            callId: "first-call", name: "web_search", arguments: .string("Retained private query"),
            output: .string("Retained tool output"))
        await service.setPage(APIPage(data: [retained])); await runtime.pollNewItems()
        var incoming = ConversationItem(
            id: "tool", responseId: "first-response", type: "tool_call", status: "completed",
            callId: "first-call", name: "web_search", arguments: .string("x"))
        switch conflict {
        case .type: incoming.type = "function_call_output"
        case .callID: incoming.callId = "different-call"
        case .responseID: incoming.responseId = "different-response"
        case .name: incoming.name = "open_url"
        }
        await service.setPage(APIPage(data: [incoming])); await runtime.pollNewItems()
        #expect(runtime.messages == [incoming])
        #expect(runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test func overlappingTicksDoNotDuplicateRequestsOrPublishScrollTwice() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("new")]))
        let gate = ChatTestGate(); await service.setPageGate(gate)
        let first = Task { await runtime.pollNewItems() }
        await gate.waitForArrival(); await runtime.pollNewItems()
        #expect(await service.pageRequests.count == 1)
        await gate.release(); await first.value
        #expect(runtime.messages.map(\.id) == ["new"] && runtime.polledMessagesRevision == 1)
        runtime.invalidate()
    }

    @Test func replacementPollOwnsItsCursorWhenOldVisibilityRequestReturnsLater() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("late")]))
        let gate = ChatTestGate(); await service.setPageGate(gate)
        let old = Task { await runtime.pollNewItems() }
        await gate.waitForArrival(); runtime.isVisible = false; runtime.isVisible = true
        await service.setPageGate(nil); await service.setPage(APIPage(data: [item("current")]))
        await runtime.pollNewItems(); await gate.release(); await old.value
        #expect(runtime.messages.map(\.id) == ["current"] && runtime.polledMessagesRevision == 1)
        await service.setPage(APIPage(data: [])); await runtime.pollNewItems()
        #expect(await service.pageRequests.last?.after == "current")
        runtime.invalidate()
    }

    @Test func recoveryPageMergeDoesNotSettleOrReplaceOwnedRunFeedback() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        var completions = 0
        runtime.onSuccessfulCompletion = { _ in completions += 1 }
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        let snapshot = runtime.snapshot, issueID = runtime.lastIssueID
        #expect(runtime.phase == .recovering && runtime.lastIssue == .uncertainResponse)
        await service.setPage(APIPage(data: [item("answer")]))
        await runtime.pollNewItems()
        #expect(runtime.snapshot == snapshot && runtime.hasUnresolvedRun && completions == 0)
        #expect(runtime.lastIssue == .uncertainResponse && runtime.lastIssueID == issueID)
        #expect(runtime.polledMessagesRevision == 1)
        #expect(await service.retrievals.isEmpty)
        #expect(await service.cancellations.isEmpty)
        runtime.invalidate()
    }

    enum Revocation: CaseIterable, Sendable, Equatable {
        case hidden, backgrounded, offline, retired, deleted, reloaded, cancelled
    }

    @Test(arguments: Revocation.allCases)
    func stalePollCannotPublishAfterItsOwnershipWasRevoked(_ revocation: Revocation) async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("late")]))
        let gate = ChatTestGate(); await service.setPageGate(gate)
        let polling = Task { await runtime.pollNewItems() }
        await gate.waitForArrival()
        switch revocation {
        case .hidden: runtime.isVisible = false; runtime.isVisible = true
        case .backgrounded: runtime.isForeground = false; runtime.isForeground = true
        case .offline: runtime.isOnline = false; runtime.isOnline = true
        case .retired: runtime.invalidate()
        case .deleted:
            let fence = UUID(); runtime.beginDeletionFence(fence); runtime.releaseDeletionFence(fence)
        case .reloaded:
            await service.setPageGate(nil); await service.setPage(APIPage(data: [item("replacement")]))
            await runtime.load(chatID: "replacement")
        case .cancelled: polling.cancel()
        }
        await gate.release(); await polling.value
        #expect(!runtime.messages.contains { $0.id == "late" })
        #expect(runtime.polledMessagesRevision == 0)
        if revocation == .reloaded { #expect(runtime.messages.map(\.id) == ["replacement"]) }
        runtime.invalidate()
    }

    @Test func activeStreamingSkipsPollAndAStreamStartingDuringPollRejectsLateHistory() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture()
        runtime.isVisible = true
        await service.setPage(APIPage(data: [item("late")]))
        let gate = ChatTestGate(); await service.setPageGate(gate)
        let polling = Task { await runtime.pollNewItems() }
        await gate.waitForArrival()
        runtime.draftText = "Synthetic turn"; #expect(runtime.send()); await service.waitForPosts()
        let storageGate = ChatTestGate(); await store.configure(gate: storageGate)
        // The created event publishes the stream phase before its durable write.
        await service.emit(.created(try chatLifecycle(chatResponse("response", .inProgress))))
        await storageGate.waitForArrival()
        #expect(runtime.phase == .streaming)
        await runtime.pollNewItems()
        #expect(await service.pageRequests.count == 1)
        await gate.release(); await polling.value
        #expect(!runtime.messages.contains { $0.id == "late" })
        #expect(runtime.polledMessagesRevision == 0 && runtime.hasUnresolvedRun)
        await storageGate.release()
        await service.emit(.outputTextDelta(try chatDelta("Streaming answer")))
        await service.finish(); await runtime.waitForCurrentWork()
        runtime.invalidate()
    }

    private func item(_ id: String, text: String = "Synthetic answer", status: String = "completed")
        -> ConversationItem
    {
        ConversationItem(
            id: id, type: "message", status: status, role: "assistant",
            content: [MessageContent(type: "output_text", text: text)])
    }
}
