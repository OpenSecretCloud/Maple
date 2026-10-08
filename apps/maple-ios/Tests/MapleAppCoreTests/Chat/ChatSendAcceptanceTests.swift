import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatSendAcceptanceTests {
    @Test func ordinarySendPublishesOnlyAfterDurableAdmissionAndNeverAgainForRecovery() async {
        let (runtime, service, store) = ChatRuntimeTests().fixture()
        let gate = ChatTestGate(); await store.configure(gate: gate)
        var accepted: [ChatSendAcceptance] = []; runtime.onTurnAccepted = { accepted.append($0) }
        runtime.draftText = "Dictated draft"; let draftID = runtime.snapshot.draft.id
        #expect(runtime.send()); await gate.waitForArrival()
        #expect(accepted.isEmpty)
        await gate.release(); await service.waitForPosts()
        #expect(accepted.count == 1 && accepted.first?.draftID == draftID)
        #expect(accepted.first?.messageID == runtime.snapshot.pendingRun?.messageID)
        #expect(accepted.first?.scope == runtime.scope && accepted.first?.conversationID == "conversation")
        await service.finish(error: URLError(.networkConnectionLost)); await runtime.waitForCurrentWork()
        await runtime.saveDraft(); await runtime.reconcile()
        #expect(accepted.count == 1)
        runtime.invalidate()
    }

    @Test func queuedAdmissionsPublishInOrderAndPromotionDoesNotPublishAgain() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var accepted: [ChatSendAcceptance] = []; runtime.onTurnAccepted = { accepted.append($0) }
        runtime.draftText = "A"; let ownerA = runtime.snapshot.draft.id; #expect(runtime.enqueueDraft())
        runtime.draftText = "B"; let ownerB = runtime.snapshot.draft.id; #expect(runtime.enqueueDraft())
        await runtime.saveDraft()
        #expect(accepted.map(\.draftID) == [ownerA, ownerB])
        #expect(accepted.map(\.messageID) == runtime.queuedTurns.map(\.messageID))
        #expect(runtime.resumeQueue()); await service.waitForPosts()
        #expect(accepted.count == 2)
        await service.finish(error: URLError(.networkConnectionLost)); await runtime.waitForCurrentWork()
        runtime.invalidate()
    }

    @Test func failedStorageDeletedQueueAndStoppedPreDispatchDoNotPublish() async throws {
        let (runtime, _, store) = ChatRuntimeTests().fixture()
        var accepted: [ChatSendAcceptance] = []; runtime.onTurnAccepted = { accepted.append($0) }
        await store.configure(fail: true)
        runtime.draftText = "Rejected"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(accepted.isEmpty)
        await store.configure()
        runtime.draftText = "Deleted queue"; #expect(runtime.enqueueDraft())
        let queued = try #require(runtime.queuedTurns.first)
        runtime.deleteQueuedTurn(id: queued.id); await runtime.saveDraft()
        #expect(accepted.isEmpty)
        let gate = ChatTestGate(); await store.configure(gate: gate)
        runtime.draftText = "Stopped"; #expect(runtime.send()); await gate.waitForArrival()
        runtime.stop(); await gate.release(); await runtime.waitForCurrentWork()
        #expect(accepted.isEmpty)
        runtime.invalidate()
    }
}
