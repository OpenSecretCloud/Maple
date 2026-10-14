import Foundation
import Testing
@testable import MapleAppUI

@Suite struct TranscriptHistoryPaginationTests {
    @Test func initialLayoutRestorationAndForwardScrollNeverLoadHistory() {
        var gate = TranscriptHistoryPagination()
        let initialLayout = gate.shouldLoad(previousOffset: 200, offset: 20, canLoad: true, requestInFlight: false)
        #expect(!initialLayout)
        gate.gestureChanged(active: true)
        let stationary = gate.shouldLoad(previousOffset: 20, offset: 20, canLoad: true, requestInFlight: false)
        #expect(!stationary)
        let forward = gate.shouldLoad(previousOffset: 20, offset: 30, canLoad: true, requestInFlight: false)
        #expect(!forward)
        let outsideThreshold = gate.shouldLoad(previousOffset: 200, offset: 101, canLoad: true, requestInFlight: false)
        #expect(!outsideThreshold)
        let subIntentMovement = gate.shouldLoad(previousOffset: 101, offset: 100, canLoad: true, requestInFlight: false)
        #expect(!subIntentMovement)
        let backwardAtThreshold = gate.shouldLoad(
            previousOffset: 104, offset: 100, canLoad: true, requestInFlight: false)
        #expect(backwardAtThreshold)
    }

    @Test func eachPhysicalGestureLoadsAtMostOnePageIncludingMomentum() {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        let firstGesture = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        #expect(firstGesture)
        gate.gestureChanged(active: true)
        let sameGestureMomentum = gate.shouldLoad(previousOffset: 90, offset: 50, canLoad: true, requestInFlight: false)
        #expect(!sameGestureMomentum)
        gate.finishLoad(preserveQueued: true)
        let noMomentumQueue = gate.startQueuedLoad(canLoad: true)
        #expect(!noMomentumQueue)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        let nextGesture = gate.shouldLoad(previousOffset: 50, offset: -5, canLoad: true, requestInFlight: false)
        #expect(nextGesture)
    }

    @Test func unavailableInflightAndInvalidGeometryDoNotAdmitHistory() {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        let unavailable = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: false, requestInFlight: false)
        #expect(!unavailable)
        let inFlight = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: true)
        #expect(!inFlight)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        let invalidPrevious = gate.shouldLoad(previousOffset: .nan, offset: 90, canLoad: true, requestInFlight: false)
        #expect(!invalidPrevious)
        let invalidOffset = gate.shouldLoad(
            previousOffset: 110, offset: .infinity, canLoad: true, requestInFlight: false)
        #expect(!invalidOffset)
        let valid = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        #expect(valid)
    }

    @Test func aSecondBackwardGestureQueuesOnePageEvenWhenItEndsBeforeTheSlowPage() {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        let first = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        #expect(first && gate.isLoadInFlight)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        let queued = gate.shouldLoad(previousOffset: 90, offset: 70, canLoad: true, requestInFlight: true)
        #expect(!queued)
        // More physical gestures at the held boundary are still a bounded
        // single queued request, rather than an unbounded pagination backlog.
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        let repeatedQueue = gate.shouldLoad(previousOffset: 70, offset: 50, canLoad: true, requestInFlight: true)
        #expect(!repeatedQueue)
        gate.gestureChanged(active: false)
        gate.finishLoad(preserveQueued: true)
        let drain = gate.startQueuedLoad(canLoad: true)
        #expect(drain && gate.isLoadInFlight)
        gate.finishLoad(preserveQueued: true)
        let noCascade = gate.startQueuedLoad(canLoad: true)
        let noLayoutLoad = gate.shouldLoad(previousOffset: 150, offset: 50, canLoad: true, requestInFlight: false)
        #expect(!noCascade && !noLayoutLoad)
    }

    @Test func aNewProjectionDoesNotQueueAnUnownedRequestOrRetryAfterItSettles() {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        let external = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: true)
        #expect(!external && !gate.isLoadInFlight)
        let sameGesture = gate.shouldLoad(previousOffset: 90, offset: 70, canLoad: true, requestInFlight: false)
        let noQueue = gate.startQueuedLoad(canLoad: true)
        #expect(!sameGesture && !noQueue)
    }

    @Test func elasticRecoilAfterTheSecondBackwardGestureDoesNotCancelItsQueue() {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 0, offset: -10, canLoad: true, requestInFlight: true)
        _ = gate.shouldLoad(
            previousOffset: -10, offset: 0, canLoad: true, requestInFlight: true,
            directUserInput: false)
        gate.gestureChanged(active: false)
        gate.finishLoad(preserveQueued: true)
        let drain = gate.startQueuedLoad(canLoad: true)
        #expect(drain)
    }

    @Test(arguments: [true, false])
    func laterUserMotionAwayFromTheBoundaryCancelsTheQueuedPage(forward: Bool) {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 90, offset: 70, canLoad: true, requestInFlight: true)
        _ = gate.shouldLoad(
            previousOffset: forward ? 70 : 150, offset: forward ? 80 : 120,
            canLoad: true, requestInFlight: true)
        gate.gestureChanged(active: false)
        gate.finishLoad(preserveQueued: true)
        let drain = gate.startQueuedLoad(canLoad: true)
        #expect(!drain)
    }

    @Test(arguments: [true, false])
    func invalidatedProjectionOrUnsuccessfulPageDiscardsQueuedIntent(resetProjection: Bool) {
        var gate = TranscriptHistoryPagination()
        gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 110, offset: 90, canLoad: true, requestInFlight: false)
        gate.gestureChanged(active: false); gate.gestureChanged(active: true)
        _ = gate.shouldLoad(previousOffset: 90, offset: 70, canLoad: true, requestInFlight: true)
        if resetProjection { gate.resetIntent() }
        gate.finishLoad(preserveQueued: resetProjection)
        let drain = gate.startQueuedLoad(canLoad: true)
        #expect(!drain)
    }
}
