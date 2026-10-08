import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatCompletionFeedbackTests {
    enum HiddenReason: CaseIterable, Sendable, Equatable { case anotherScreen, background }

    private func hide(_ runtime: ChatRuntime, _ reason: HiddenReason) {
        runtime.isVisible = reason != .anotherScreen
        runtime.isForeground = reason != .background
    }
    private func waitUntil(_ condition: @MainActor () -> Bool) async {
        for _ in 0..<20_000 {
            if condition() { return }
            await Task.yield()
        }
        #expect(condition())
    }

    @Test func sendHasNoFeedbackAndDuplicateSuccessfulTerminalProducesOnlyOneRequest() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        var feedback = 0, completions = 0
        runtime.isVisible = true; runtime.isForeground = true
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { _ in completions += 1 }
        let completed = ResponseStreamEvent.completed(try chatLifecycle(chatResponse("response", .completed)))
        await service.setResponseGate(gate); await service.setScript([completed, completed])
        runtime.draftText = "Synthetic feedback turn"; #expect(runtime.send())
        await gate.waitForArrival()
        #expect(feedback == 0 && completions == 0)
        await gate.release(); await runtime.waitForCurrentWork()
        #expect(runtime.lastOutcome == .completed && feedback == 1 && completions == 1)
        runtime.isVisible = false; runtime.isVisible = true
        await runtime.reconcile()
        #expect(feedback == 1 && completions == 1)
        runtime.invalidate()
    }

    @Test(arguments: [ResponseStatus.failed, .cancelled, .incomplete])
    func unsuccessfulTerminalNeverRequestsSuccessFeedback(_ status: ResponseStatus) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var feedback = 0, completions = 0
        runtime.isVisible = true; runtime.isForeground = true
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onSuccessfulCompletion = { _ in completions += 1 }
        let event: ResponseStreamEvent
        if status == .cancelled {
            event = .cancelled(
                try JSONDecoder().decode(ResponseCancelledEvent.self, from: Data(#"{"id":"response"}"#.utf8)))
        } else {
            event = .failed(try chatLifecycle(chatResponse("response", status)))
        }
        await service.setScript([event])
        runtime.draftText = "Synthetic unsuccessful turn"; #expect(runtime.send())
        await runtime.waitForCurrentWork()
        #expect(runtime.lastOutcome == ChatRuntime.outcome(status))
        #expect(!runtime.hasUnresolvedRun && feedback == 0 && completions == 0)
        runtime.isVisible = false; runtime.isVisible = true
        await runtime.reconcile()
        #expect(feedback == 0 && completions == 0)
        runtime.invalidate()
    }

    @Test(arguments: HiddenReason.allCases)
    func successfulInactiveCompletionDoesNotPlayOnReturn(_ reason: HiddenReason) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var feedback = 0
        var completion: ChatCompletion?
        hide(runtime, reason)
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { completion = $0 }
        await service.setScript([.completed(try chatLifecycle(chatResponse("response", .completed)))])
        runtime.draftText = "Synthetic hidden turn"; #expect(runtime.send())
        await runtime.waitForCurrentWork()
        let value = try #require(completion)
        #expect(value.wasVisible == (reason != .anotherScreen))
        #expect(value.wasForeground == (reason != .background))
        #expect(feedback == 0)
        runtime.isVisible = true; runtime.isForeground = true
        await runtime.reconcileOnLifecycle(foreground: true)
        #expect(feedback == 0)
        runtime.invalidate()
    }

    @Test(arguments: HiddenReason.allCases)
    func returningDuringTerminalPersistenceCannotCreateDelayedFeedback(_ reason: HiddenReason) async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        var feedback = 0
        var completion: ChatCompletion?
        hide(runtime, reason)
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { completion = $0 }
        runtime.draftText = "Synthetic saved completion"; #expect(runtime.send())
        await service.waitForPosts(); await store.configure(gate: gate)
        await service.emit(.completed(try chatLifecycle(chatResponse("response", .completed))))
        await gate.waitForArrival()
        // The reply is already terminal; only its durable commit is outstanding.
        #expect(runtime.lastOutcome == .completed && !runtime.hasUnresolvedRun)
        #expect(completion == nil && feedback == 0)
        runtime.isVisible = true; runtime.isForeground = true
        await gate.release(); await waitUntil { completion != nil }
        let value = try #require(completion)
        #expect(value.wasVisible == (reason != .anotherScreen))
        #expect(value.wasForeground == (reason != .background))
        #expect(feedback == 0)
        runtime.invalidate()
    }

    @Test(arguments: HiddenReason.allCases)
    func leavingDuringTerminalPersistenceSuppressesFeedback(_ reason: HiddenReason) async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture(), gate = ChatTestGate()
        var feedback = 0, completions = 0
        runtime.isVisible = true; runtime.isForeground = true
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { _ in completions += 1 }
        runtime.draftText = "Synthetic visible completion"; #expect(runtime.send())
        await service.waitForPosts(); await store.configure(gate: gate)
        await service.emit(.completed(try chatLifecycle(chatResponse("response", .completed))))
        await gate.waitForArrival(); hide(runtime, reason)
        await gate.release(); await waitUntil { completions == 1 }
        #expect(feedback == 0)
        runtime.isVisible = true; runtime.isForeground = true
        await runtime.reconcile()
        #expect(feedback == 0)
        runtime.invalidate()
    }

    @Test func stopRacingSuccessfulServerCompletionDoesNotRequestFeedback() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var feedback = 0
        var completion: ChatCompletion?
        runtime.isVisible = true; runtime.isForeground = true
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { completion = $0 }
        await service.setScript([.created(try chatLifecycle(chatResponse("response", .inProgress)))])
        runtime.draftText = "Stopped synthetic turn"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        await service.setCancelError(ResearchAPIError.http(status: 400, reason: .unspecified))
        await service.setRetrieved(chatResponse("response", .completed))
        runtime.stop(); await runtime.waitForCurrentWork()
        let value = try #require(completion)
        #expect(value.wasStopped && runtime.lastOutcome == .completed)
        #expect(await service.cancellations == ["response"])
        #expect(feedback == 0)
        runtime.invalidate()
    }

    @Test func failedTerminalCommitDoesNotPublishCompletionFeedback() async throws {
        let (runtime, service, store) = ChatRuntimeTests().fixture()
        var feedback = 0, completions = 0
        runtime.isVisible = true; runtime.isForeground = true
        runtime.onSuccessfulVisibleCompletion = { _ in feedback += 1 }
        runtime.onCompletion = { _ in completions += 1 }
        runtime.draftText = "Synthetic storage failure"; #expect(runtime.send())
        await service.waitForPosts(); await store.configure(fail: true)
        await service.emit(.completed(try chatLifecycle(chatResponse("response", .completed))))
        await waitUntil { runtime.lastIssue == .storageUnavailable }
        #expect(feedback == 0 && completions == 0)
        runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func streamedOutputAndCompletionShareResponseIdentityWithoutRelabelingHistory(sparse: Bool) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        let history = [
            ConversationItem(id: "old-unlinked", type: "message", role: "assistant"),
            ConversationItem(id: "old-linked", responseId: "old-response", type: "message", role: "assistant"),
        ]
        await service.setPage(APIPage(data: history))
        await runtime.load(chatID: "conversation")
        var completion: ChatCompletion?
        runtime.onCompletion = { completion = $0 }
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        // Backend Responses output items omit response_id; the envelope owns that identity.
        let added = try decoder.decode(
            ResponseItemEvent.self,
            from: Data(
                #"{"output_index":0,"item":{"id":"assistant","type":"message","role":"assistant","status":"in_progress","content":[]}}"#
                    .utf8))
        let output = ConversationItem(
            id: "assistant", type: "message", status: "completed", role: "assistant",
            content: [MessageContent(type: "output_text", text: "Associated output")])
        var events: [ResponseStreamEvent] = [.created(try chatLifecycle(chatResponse("response", .inProgress)))]
        if !sparse { events.append(.outputItemAdded(added)) }
        events.append(.outputTextDelta(try chatDelta("Associated output")))
        events.append(
            .completed(try chatLifecycle(chatResponse("response", .completed, output: sparse ? nil : [output]))))
        await service.setScript(events)
        runtime.draftText = "Synthetic identity turn"; #expect(runtime.send())
        await runtime.waitForCurrentWork()
        let finished = try #require(completion)
        let visible = try #require(runtime.messages.first { $0.id == "assistant" })
        #expect(runtime.lastOutcome == .completed && !runtime.hasUnresolvedRun)
        #expect(finished.responseID == "response" && !finished.recovered)
        #expect(visible.responseId == finished.responseID)
        #expect(finished.assistantItems.first { $0.id == "assistant" }?.responseId == finished.responseID)
        #expect(visible.content?.first?.text == "Associated output")
        #expect(runtime.messages.first { $0.id == "old-unlinked" }?.responseId == nil)
        #expect(runtime.messages.first { $0.id == "old-linked" }?.responseId == "old-response")
        #expect(await service.posts.count == 1)
        runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func recoveredOutputKeepsResponseIdentityThroughSparseHistory(sparse: Bool) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        let explicit = ConversationItem(
            id: "explicit-item", responseId: "explicit-server-link", type: "message", role: "assistant")
        await service.setPage(APIPage(data: [explicit]))
        await runtime.load(chatID: "conversation")
        var completion: ChatCompletion?
        runtime.onCompletion = { completion = $0 }
        await service.setScript([
            .created(try chatLifecycle(chatResponse("response", .inProgress))),
            .outputTextDelta(try chatDelta("Recovered output")),
        ])
        runtime.draftText = "Synthetic interrupted identity turn"; #expect(runtime.send())
        await runtime.waitForCurrentWork()
        #expect(runtime.hasUnresolvedRun && completion == nil)
        let output = ConversationItem(
            id: "assistant", type: "message", status: "completed", role: "assistant",
            content: [MessageContent(type: "output_text", text: "Recovered output")])
        var sparseExplicit = explicit
        sparseExplicit.responseId = nil
        await service.setRetrieved(
            chatResponse("response", .completed, output: sparse ? nil : [output, sparseExplicit]))
        await service.setPage(
            APIPage(data: [
                output, sparseExplicit,
                ConversationItem(id: "old-unlinked", type: "message", role: "assistant"),
                ConversationItem(id: "old-linked", responseId: "old-response", type: "message", role: "assistant"),
            ]))
        await runtime.reconcile()
        let finished = try #require(completion)
        let visible = try #require(runtime.messages.first { $0.id == "assistant" })
        #expect(runtime.lastOutcome == .completed && !runtime.hasUnresolvedRun)
        #expect(finished.recovered && finished.responseID == "response")
        #expect(visible.responseId == finished.responseID)
        #expect(finished.assistantItems.first { $0.id == "assistant" }?.responseId == finished.responseID)
        #expect(visible.content?.first?.text == "Recovered output")
        #expect(runtime.messages.first { $0.id == "explicit-item" }?.responseId == "explicit-server-link")
        #expect(runtime.messages.first { $0.id == "old-unlinked" }?.responseId == nil)
        #expect(runtime.messages.first { $0.id == "old-linked" }?.responseId == "old-response")
        #expect(await service.posts.count == 1)
        #expect(await service.retrievals == ["response"])
        runtime.invalidate()
    }
}
