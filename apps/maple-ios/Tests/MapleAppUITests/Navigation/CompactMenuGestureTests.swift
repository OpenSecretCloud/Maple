import Foundation
import Testing
@testable import MapleAppUI

@Suite struct CompactMenuGestureTests {
    @Test func progressTracksTheFingerInBothDirections() {
        #expect(CompactMenuGesture.progress(translationX: 120, width: 400) == 0.3)
        #expect(CompactMenuGesture.progress(translationX: 40, width: 400) == 0.1)
        #expect(CompactMenuGesture.progress(translationX: -10, width: 400) == 0)
        #expect(CompactMenuGesture.progress(translationX: 450, width: 400) == 1)
    }

    @Test func releaseUsesDistanceAndForwardVelocity() {
        #expect(CompactMenuGesture.shouldFinish(translationX: 220, velocityX: 0, width: 400))
        #expect(!CompactMenuGesture.shouldFinish(translationX: 120, velocityX: 0, width: 400))
        #expect(CompactMenuGesture.shouldFinish(translationX: 100, velocityX: 600, width: 400))
        #expect(CompactMenuGesture.shouldFinish(translationX: 180, velocityX: 150, width: 400))
    }

    @Test func reversalAndShortPullCancelEvenAfterAnEarlierLongPull() {
        #expect(!CompactMenuGesture.shouldFinish(translationX: 320, velocityX: -400, width: 400))
        #expect(!CompactMenuGesture.shouldFinish(translationX: 60, velocityX: 0, width: 400))
        #expect(!CompactMenuGesture.shouldFinish(translationX: 44, velocityX: 800, width: 400))
    }

    @Test func invalidGeometryCannotStartOrCommitNavigation() {
        #expect(CompactMenuGesture.progress(translationX: .nan, width: 400) == 0)
        #expect(CompactMenuGesture.progress(translationX: 100, width: 0) == 0)
        #expect(!CompactMenuGesture.shouldFinish(translationX: 300, velocityX: .infinity, width: 400))
        #expect(!CompactMenuGesture.shouldFinish(translationX: 300, velocityX: 600, width: .nan))
    }
}
