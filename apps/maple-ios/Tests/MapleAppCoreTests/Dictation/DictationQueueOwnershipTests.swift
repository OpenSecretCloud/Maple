import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct DictationQueueOwnershipTests {
    @Test func beginEditingStopsCaptureSynchronouslyAndHeldBResultNeverChangesA() async throws {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Queued A"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held B"; let ownerB = runtime.snapshot.draft.id
        #expect(dictation.start(runtime: runtime)); try await dictationWait { capture.starts == 1 }
        #expect(runtime.beginQueueEdit(id: queued.id))
        #expect(capture.active == nil && capture.stops == 1)
        #expect(runtime.snapshot.draft.text == "Queued A" && !runtime.busyDraftIDs.contains(runtime.snapshot.draft.id))
        try await service.waitForCalls(); await service.complete(text: "voice B")
        try await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.snapshot.draft.text == "Queued A")
        #expect(runtime.snapshot.queue?.edit?.heldDraft.text == "Held B voice B")
        #expect(runtime.snapshot.queue?.edit?.heldDraft.id == ownerB)
        #expect(runtime.queuedTurns == [queued] && runtime.isEditingQueue)
        #expect(runtime.saveQueueEdit()); #expect(runtime.draftText == "Held B voice B")
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func heldBProcessingDoesNotBlockSavingAAndBInsertionFindsRestoredOwner() async throws {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Queued A"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held B"; let ownerB = runtime.snapshot.draft.id
        #expect(dictation.start(runtime: runtime)); try await dictationWait { capture.starts == 1 }
        #expect(runtime.beginQueueEdit(id: queued.id)); try await service.waitForCalls()
        runtime.draftText = "Edited A"
        #expect(runtime.saveQueueEdit())
        #expect(runtime.queuedTurns.first?.draft.text == "Edited A" && runtime.snapshot.draft.id == ownerB)
        #expect(!runtime.canSend)
        await service.complete(text: "voice B"); try await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.draftText == "Held B voice B" && runtime.canSend)
        #expect(runtime.queuedTurns.first?.draft.text == "Edited A" && runtime.isQueuePaused)
        dictation.invalidate(); runtime.invalidate()
    }

    @Test(arguments: [false, true])
    func cancelOrDeleteARejectsItsLateResultAndKeepsIndependentB(delete: Bool) async throws {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Queued A"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held B"
        #expect(dictation.start(runtime: runtime)); try await dictationWait { capture.starts == 1 }
        #expect(runtime.beginQueueEdit(id: queued.id)); try await service.waitForCalls()
        let ownerA = runtime.snapshot.draft.id
        #expect(dictation.start(runtime: runtime)); try await dictationWait { capture.starts == 2 }
        dictation.stop(runtime: runtime); try await service.waitForCalls(2)
        #expect(!runtime.saveQueueEdit())
        if delete { runtime.deleteQueuedTurn(id: queued.id) } else { runtime.cancelQueueEdit() }
        #expect(!runtime.busyDraftIDs.contains(ownerA) && !runtime.canSend)
        await service.complete(1, text: "Late A"); await service.complete(0, text: "voice B")
        try await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.draftText == "Held B voice B")
        #expect(runtime.queuedTurns == (delete ? [] : [queued]))
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func completedAEnablesSaveWithoutApplyingItAutomaticallyAndRestartHasNoJob() async throws {
        let (runtime, dictation, capture, service, store) = dictationFixture()
        runtime.draftText = "Queued A"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held B"; #expect(runtime.beginQueueEdit(id: queued.id))
        #expect(dictation.start(runtime: runtime)); try await dictationWait { capture.starts == 1 }
        dictation.stopCaptureForNavigation(); try await service.waitForCalls()
        await service.complete(text: "voice A"); try await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.isEditingQueue && runtime.queuedTurns == [queued])
        #expect(runtime.draftText == "Queued A voice A")
        await runtime.saveDraft()
        let snapshot = try #require(await store.recorded("conversation"))
        #expect(snapshot.queue?.edit?.heldDraft.text == "Held B")
        let fresh = DictationCoordinator(scope: runtime.scope, capture: DictationFixtureCapture(), transcriber: service)
        #expect(fresh.statuses.isEmpty)
        #expect(runtime.saveQueueEdit())
        #expect(runtime.queuedTurns.first?.draft.text == "Queued A voice A" && runtime.draftText == "Held B")
        fresh.invalidate(); dictation.invalidate(); runtime.invalidate()
    }
}
