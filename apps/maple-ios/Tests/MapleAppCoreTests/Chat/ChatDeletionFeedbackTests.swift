import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatDeletionFeedbackTests {
    @Test(arguments: [false, true], [false, true])
    func deletionAdmissionFeedbackPreservesTheDraftAndNeverDispatches(fenced: Bool, enqueue: Bool) async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Retained draft"
        let snapshot = runtime.snapshot
        if fenced { runtime.beginDeletionFence(.init()) } else { runtime.deletionAdmission = { _ in false } }
        let accepted = enqueue ? runtime.enqueueDraft() : runtime.send()
        #expect(!accepted && runtime.lastIssue == .conversationDeleting)
        #expect(runtime.snapshot == snapshot && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
        #expect(await service.creations == 0)
        runtime.invalidate()
    }

    @Test func onlyActualDiscoveryFenceChangesNotifyAndPausedQueueNeverReplays() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Paused queued turn"; #expect(runtime.enqueueDraft())
        runtime.draftText = "Unsent draft"
        let snapshot = runtime.snapshot, first = UUID(), second = UUID()
        var changes: [Set<UUID>] = []
        runtime.onDeletionFenceChanged = { changes.append(runtime.deletionFenceIDs) }
        runtime.beginDeletionFence(first); runtime.beginDeletionFence(first)
        runtime.beginDeletionFence(second); runtime.releaseDeletionFence(UUID())
        runtime.releaseDeletionFence(first); runtime.releaseDeletionFence(first)
        runtime.releaseDeletionFence(second); runtime.releaseDeletionFence(second)
        #expect(changes == [[first], [first, second], [second], []])
        #expect(runtime.snapshot == snapshot && runtime.isQueuePaused && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
        runtime.beginDeletionFence(first); runtime.invalidate()
        #expect(runtime.onDeletionFenceChanged == nil)
        runtime.releaseDeletionFence(first)
        #expect(changes.count == 5)
    }

    @Test func discoveryAndReleaseLeaveTheExistingRunAndQueueOwnerUntouched() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.draftText = "Owned run"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Owned queued turn"; #expect(runtime.enqueueDraft())
        let snapshot = runtime.snapshot, fence = UUID()
        var notifications = 0
        runtime.onDeletionFenceChanged = { notifications += 1 }
        runtime.beginDeletionFence(fence); runtime.releaseDeletionFence(fence)
        #expect(notifications == 2 && runtime.snapshot == snapshot)
        #expect(runtime.hasUnresolvedRun && runtime.snapshot.pendingRun?.stopRequested == false)
        #expect(await service.posts.count == 1)
        #expect(await service.cancellations.isEmpty)
        runtime.onDeletionFenceChanged = nil; runtime.invalidate()
    }
}
