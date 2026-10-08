import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatRequestBudgetTests {
    private func imageDraft(id: UUID, byte: UInt8 = 0) -> ChatDraft {
        ChatDraft(
            id: id, text: "Synthetic size fixture",
            images: [
                ChatImageAttachment(data: Data(repeating: byte, count: 20 * 1_024 * 1_024), contentType: "image/png")
            ])
    }

    @Test(arguments: [UInt8(0), UInt8(255)])
    func exactEncodingAcceptsSupportedImageAndRejectsActualEscapedOversize(byte: UInt8) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        await service.setScript([.completed(try chatLifecycle(chatResponse("response", .completed)))])
        let draft = imageDraft(id: runtime.snapshot.draft.id, byte: byte)
        runtime.updateDraft(draft)
        #expect(runtime.send())
        #expect(runtime.snapshot.draft == draft && runtime.snapshot.pendingRun == nil)
        #expect(runtime.hasUnresolvedRun && !runtime.canSend && !runtime.send())
        await runtime.waitForCurrentWork()
        if byte == 0 {
            #expect(await service.posts.count == 1)
            #expect(runtime.lastOutcome == .completed && runtime.snapshot.draft.isEmpty)
        } else {
            #expect(await service.posts.isEmpty)
            #expect(runtime.snapshot.draft == draft && runtime.lastIssue == .draftTooLarge)
        }
        runtime.invalidate()
    }

    @Test func changedDraftDuringExactPreflightIsNeverSilentlySubmitted() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        runtime.updateDraft(imageDraft(id: runtime.snapshot.draft.id))
        #expect(runtime.send())
        runtime.draftText = "Edited while preparing"
        await runtime.waitForCurrentWork()
        #expect(runtime.draftText == "Edited while preparing" && runtime.snapshot.draft.images.count == 1)
        #expect(await service.posts.isEmpty)
        #expect(!runtime.hasUnresolvedRun)
        runtime.invalidate()
    }

    @Test func stopDuringExactPreflightPreservesOriginalOwnerWithoutPosting() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        let draft = imageDraft(id: runtime.snapshot.draft.id)
        runtime.updateDraft(draft); #expect(runtime.send())
        Task { runtime.stop() }
        await runtime.waitForCurrentWork()
        #expect(runtime.snapshot.draft == draft && !runtime.hasUnresolvedRun)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }

    @Test func queueAndEditKeepOwnersUntilExactPreflightFinishes() async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        let draft = imageDraft(id: runtime.snapshot.draft.id)
        runtime.updateDraft(draft); #expect(runtime.enqueueDraft())
        #expect(runtime.snapshot.draft == draft && runtime.queuedTurns.isEmpty)
        await runtime.waitForCurrentWork()
        let queued = try #require(runtime.queuedTurns.first)
        runtime.draftText = "Held B"
        #expect(runtime.beginQueueEdit(id: queued.id)); runtime.draftText = "Edited A"
        let editor = runtime.snapshot.draft
        #expect(runtime.saveQueueEdit())
        #expect(runtime.isEditingQueue && runtime.snapshot.draft == editor)
        await runtime.waitForCurrentWork()
        #expect(!runtime.isEditingQueue && runtime.draftText == "Held B")
        #expect(runtime.queuedTurns.first?.draft.text == "Edited A")
        #expect(runtime.queuedTurns.first?.draft.images == draft.images)
        #expect(await service.posts.isEmpty)
        runtime.invalidate()
    }
}
