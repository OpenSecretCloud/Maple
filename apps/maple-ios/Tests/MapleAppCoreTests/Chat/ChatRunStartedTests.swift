import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ChatRunStartedTests {
    @Test(arguments: [ResponseStatus.failed, .cancelled])
    func queuedPromotionClearsBackgroundUnreadBeforeItsFailedOrCancelledRun(_ status: ResponseStatus) async throws {
        let (runtime, service, _) = ChatRuntimeTests().fixture()
        var unread = false, starts = 0, completions = 0
        runtime.onRunStarted = {
            starts += 1; unread = false
        }
        runtime.onSuccessfulCompletion = { _ in
            completions += 1; unread = true
        }
        runtime.draftText = "First run"; #expect(runtime.send()); await service.waitForPosts()
        runtime.draftText = "Queued run"; #expect(runtime.enqueueDraft())
        #expect(starts == 1 && !unread)
        let terminal: ResponseStreamEvent
        if status == .failed {
            terminal = .failed(try chatLifecycle(chatResponse("second", status)))
        } else {
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            terminal = .cancelled(
                try decoder.decode(
                    ResponseCancelledEvent.self,
                    from: Data(
                        #"{"response":{"id":"second","object":"response","created_at":0,"status":"cancelled","model":"auto:quick"}}"#
                            .utf8)))
        }
        await service.setScript([.created(try chatLifecycle(chatResponse("second", .inProgress))), terminal])
        await service.emit(.completed(try chatLifecycle(chatResponse("first", .completed))))
        await service.waitForPosts(2); await runtime.waitForCurrentWork()
        #expect(starts == 2 && completions == 1 && !unread)
        #expect(!runtime.hasUnresolvedRun && runtime.queuedTurns.isEmpty)
        #expect(runtime.lastOutcome == (status == .failed ? .failed : .cancelled))
        runtime.invalidate(); #expect(runtime.onRunStarted == nil)
    }

    @Test func newConversationAndExplicitRetryNotifyOncePerAdmittedRun() async {
        let (runtime, service, _) = ChatRuntimeTests().fixture(conversationID: nil)
        var starts = 0
        runtime.onRunStarted = { starts += 1 }
        await service.setResponseErrors([ResearchAPIError.http(status: 400, reason: .unsupportedValue)])
        runtime.draftText = "Restored after rejection"; #expect(runtime.send()); await runtime.waitForCurrentWork()
        #expect(starts == 1 && !runtime.hasUnresolvedRun)
        #expect(await service.creations == 1)
        #expect(runtime.send()); await service.waitForPosts(2)
        #expect(starts == 2)
        runtime.invalidate(); #expect(runtime.onRunStarted == nil)
    }
}
