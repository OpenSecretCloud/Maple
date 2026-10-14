import Foundation
import Testing
@testable import MapleAppCore

private actor DictationIssueExpiryClock {
    let gate = ChatTestGate()
    private(set) var delays: [Duration] = []

    func sleep(_ duration: Duration) async throws {
        // Recording's separate elapsed-time task is irrelevant to these tests.
        guard duration == .seconds(5) else { throw CancellationError() }
        delays.append(duration)
        await gate.hold()
    }
}

@MainActor @Suite struct DictationIssueExpiryTests {
    @Test func permissionFeedbackStaysUntilFiveSecondsAndLeavesTheDraftUnchanged() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, _) = fixture(clock: clock)
        runtime.draftText = "Keep this draft"; let draft = runtime.snapshot.draft
        capture.permission = false
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait { coordinator.issue(for: runtime) == .permissionDenied }
        let id = try #require(coordinator.issueID(for: runtime))
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: id, runtime: runtime) }
        await clock.gate.waitForArrival()
        #expect(coordinator.issue(for: runtime) == .permissionDenied && coordinator.issueID(for: runtime) == id)
        #expect(await clock.delays == [.seconds(5)])
        await clock.gate.release(); await expiry.value
        #expect(coordinator.issue(for: runtime) == nil && coordinator.lastIssueID == nil)
        #expect(runtime.snapshot.draft == draft && runtime.canSend && capture.starts == 0)
        coordinator.invalidate(); runtime.invalidate()
    }

    @Test func oldTimerCannotClearALaterOccurrenceOfTheSameIssue() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, _) = fixture(clock: clock)
        capture.permission = false
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait { coordinator.issue(for: runtime) == .permissionDenied }
        let oldID = try #require(coordinator.issueID(for: runtime))
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: oldID, runtime: runtime) }
        await clock.gate.waitForArrival()
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait {
            coordinator.issue(for: runtime) == .permissionDenied && coordinator.issueID(for: runtime) != oldID
        }
        let replacementID = try #require(coordinator.issueID(for: runtime))
        await clock.gate.release(); await expiry.value
        #expect(
            coordinator.issue(for: runtime) == .permissionDenied && coordinator.issueID(for: runtime) == replacementID)
        coordinator.invalidate(); runtime.invalidate()
    }

    @Test func cancellationOnDisappearancePreservesFeedbackEvenIfSleepReturnsNormally() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, _) = fixture(clock: clock)
        capture.permission = false
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait { coordinator.issue(for: runtime) == .permissionDenied }
        let id = try #require(coordinator.issueID(for: runtime))
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: id, runtime: runtime) }
        await clock.gate.waitForArrival()
        expiry.cancel(); await clock.gate.release(); await expiry.value
        #expect(coordinator.issue(for: runtime) == .permissionDenied && coordinator.issueID(for: runtime) == id)
        coordinator.invalidate(); runtime.invalidate()
    }

    @Test func aClonedDraftCannotExpireAnotherRuntimeAndRetirementClearsTheOccurrence() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, _) = fixture(clock: clock)
        capture.permission = false
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait { coordinator.issue(for: runtime) == .permissionDenied }
        let id = try #require(coordinator.issueID(for: runtime))
        let store = ChatFixtureStore()
        let clone = ChatRuntime(scope: runtime.scope, service: ChatFixtureService(store: store), persistence: store)
        #expect(clone.loadRetainedSnapshot(runtime.snapshot))
        await coordinator.expireIssueAfterDelay(id: id, runtime: clone)
        #expect(await clock.delays.isEmpty)
        #expect(coordinator.issue(for: clone) == nil && coordinator.issueID(for: runtime) == id)
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: id, runtime: runtime) }
        await clock.gate.waitForArrival()
        coordinator.invalidate(); runtime.invalidate()
        await clock.gate.release(); await expiry.value
        #expect(coordinator.lastIssue == nil && coordinator.lastIssueID == nil && coordinator.issue(for: clone) == nil)
        clone.invalidate()
    }

    @Test func changedComposerOwnerCannotExpireTheHeldDraftIssue() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, _) = fixture(clock: clock)
        runtime.draftText = "Queued draft"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held draft"; let owner = runtime.snapshot.draft.id
        capture.permission = false
        #expect(coordinator.start(runtime: runtime))
        try await dictationWait { coordinator.issue(for: runtime) == .permissionDenied }
        let id = try #require(coordinator.issueID(for: runtime))
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: id, runtime: runtime) }
        await clock.gate.waitForArrival()
        #expect(runtime.beginQueueEdit(id: queued.id))
        #expect(runtime.snapshot.draft.id != owner && coordinator.issue(for: runtime) == nil)
        await clock.gate.release(); await expiry.value
        #expect(coordinator.lastIssueID == id)
        runtime.cancelQueueEdit()
        #expect(runtime.snapshot.draft.id == owner && coordinator.issueID(for: runtime) == id)
        #expect(runtime.draftText == "Held draft" && runtime.queuedTurns == [queued])
        coordinator.invalidate(); runtime.invalidate()
    }

    @Test func expiryKeepsTheFailedClipAndRetryUsesItWithoutAnotherMicrophoneCapture() async throws {
        let clock = DictationIssueExpiryClock()
        let (runtime, coordinator, capture, service) = fixture(clock: clock)
        runtime.draftText = "Keep this draft"; let owner = runtime.snapshot.draft.id
        #expect(coordinator.start(runtime: runtime)); try await dictationWait { capture.starts == 1 }
        coordinator.stop(runtime: runtime)
        try await service.waitForCalls(); await service.complete(fails: true)
        try await dictationWait { coordinator.status(for: runtime)?.phase == .failed }
        let id = try #require(coordinator.issueID(for: runtime))
        let expiry = Task { await coordinator.expireIssueAfterDelay(id: id, runtime: runtime) }
        await clock.gate.waitForArrival(); await clock.gate.release(); await expiry.value
        #expect(coordinator.issue(for: runtime) == nil)
        #expect(coordinator.status(for: runtime)?.phase == .failed)
        #expect(coordinator.status(for: runtime)?.issue == .transcriptionUnavailable)
        #expect(runtime.busyDraftIDs.contains(owner) && runtime.draftText == "Keep this draft")
        coordinator.retry(runtime: runtime); try await service.waitForCalls(2)
        let clips = await service.clips
        #expect(clips.count == 2 && clips[0] == clips[1] && capture.starts == 1)
        coordinator.cancel(runtime: runtime); await service.complete(1)
        #expect(runtime.canSend && runtime.draftText == "Keep this draft")
        coordinator.invalidate(); runtime.invalidate()
    }

    private func fixture(clock: DictationIssueExpiryClock) -> (
        ChatRuntime, DictationCoordinator, DictationFixtureCapture, DictationFixtureTranscriber
    ) {
        let (runtime, _, _) = ChatRuntimeTests().fixture()
        let capture = DictationFixtureCapture(), transcriber = DictationFixtureTranscriber()
        let coordinator = DictationCoordinator(
            scope: runtime.scope, capture: capture, transcriber: transcriber,
            hooks: ChatRuntimeHooks(sleep: { try await clock.sleep($0) }))
        return (runtime, coordinator, capture, transcriber)
    }
}
