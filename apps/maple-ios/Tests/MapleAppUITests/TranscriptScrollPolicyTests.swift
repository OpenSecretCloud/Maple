import Foundation
import Testing
@testable import MapleAppUI

@Suite struct TranscriptScrollPolicyTests {
    private func mutation(_ id: String?, assistant: Bool = true, streaming: Bool = false)
        -> TranscriptScrollPolicy.Mutation
    {
        .init(lastItemID: id, lastItemIsAssistant: assistant, hasStreamingMessage: streaming)
    }

    @Test func initialHistoryAndPrependingDoNotCountAsAppendedReplies() {
        var policy = TranscriptScrollPolicy()
        #expect(policy.receive(mutation(nil)).isEmpty)
        #expect(policy.receive(mutation("loaded-last")).isEmpty)
        #expect(policy.receive(mutation("loaded-last")).isEmpty)
    }

    @Test func newAssistantAndToolItemsFollowOnlyWhileNearBottom() {
        var policy = TranscriptScrollPolicy()
        _ = policy.receive(mutation("user", assistant: false))
        #expect(policy.receive(mutation("assistant", streaming: true)) == [.appendedItem, .streamingStarted])
        policy.readerScrolled(distanceFromBottom: 99)
        #expect(policy.receive(mutation("tool", streaming: true)) == [.appendedItem])
        policy.readerScrolled(distanceFromBottom: 100)
        #expect(policy.receive(mutation("tool-result", streaming: true)).isEmpty)
        policy.readerScrolled(distanceFromBottom: 0)
        #expect(policy.receive(mutation("next-assistant", streaming: true)) == [.appendedItem])
    }

    @Test func tokenGrowthAndCompletionDoNotRepeatedlyScroll() {
        var policy = TranscriptScrollPolicy()
        _ = policy.receive(mutation("assistant"))
        #expect(policy.receive(mutation("assistant", streaming: true)) == [.streamingStarted])
        for _ in 0..<40 { #expect(policy.receive(mutation("assistant", streaming: true)).isEmpty) }
        #expect(policy.receive(mutation("assistant", streaming: false)).isEmpty)
    }

    @Test func scrollingAwayAlsoSuppressesTheFirstStreamingNudge() {
        var policy = TranscriptScrollPolicy()
        _ = policy.receive(mutation("user", assistant: false))
        policy.readerScrolled(distanceFromBottom: 350)
        #expect(policy.receive(mutation("assistant", streaming: true)).isEmpty)
        #expect(policy.isUserScrolling)
        policy.readerScrolled(distanceFromBottom: .nan)
        #expect(policy.isUserScrolling)
    }

    @Test func streamNudgeIsAtMost100PointsAndUsesTheNativeViewport() {
        #expect(TranscriptScrollPolicy.maximumOffset(contentHeight: 900, viewportHeight: 500) == 400)
        #expect(TranscriptScrollPolicy.streamStartOffset(current: 200, maximum: 400) == 300)
        #expect(TranscriptScrollPolicy.streamStartOffset(current: 350, maximum: 400) == 400)
        #expect(TranscriptScrollPolicy.streamStartOffset(current: -20, maximum: 400) == 100)
        #expect(TranscriptScrollPolicy.maximumOffset(contentHeight: 300, viewportHeight: 500) == 0)
        #expect(TranscriptScrollPolicy.maximumOffset(contentHeight: .nan, viewportHeight: 500) == nil)
        #expect(TranscriptScrollPolicy.maximumOffset(contentHeight: 500, viewportHeight: 0) == nil)
    }

    @Test func actualIPadBottomIsNotMisclassifiedByCountingInsetsTwice() throws {
        // Observed at the physical bottom in the shipping iPad fixture:
        // content=2223, container=928, top=182, bottom=100, rawOffset=1113.
        // Its 1210-point visibleRect includes those 282 points of insets;
        // its 928-point containerSize has already removed them.
        let maximum = try #require(TranscriptScrollPolicy.maximumOffset(contentHeight: 2_223, viewportHeight: 928))
        #expect(maximum == 1_295)
        var policy = TranscriptScrollPolicy()
        policy.readerScrolled(distanceFromBottom: maximum - (1_113 + 182))
        #expect(!policy.isUserScrolling)
    }

    @Test func appendedReplyWaitsForItsOwnLayoutBeforeChoosingTheBottomTarget() {
        // A queued main-actor turn may still have the previous item's valid
        // geometry. That is not evidence the newly appended reply fits there.
        #expect(
            TranscriptScrollPolicy.appendedItemOffset(
                expectedItemID: "new-reply", renderedItemID: "old-reply", tailBottom: 900,
                viewportHeight: 500) == nil)
        #expect(
            TranscriptScrollPolicy.appendedItemOffset(
                expectedItemID: "new-reply", renderedItemID: "new-reply", tailBottom: 1_200,
                viewportHeight: 500) == 720)
    }
}
