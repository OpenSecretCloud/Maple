import Foundation

/// Only an actual backward reader gesture can load one older history page.
/// A second gesture at the old boundary can queue one page while the owned
/// prepend settles. Layout and the first gesture's momentum never queue it.
struct TranscriptHistoryPagination {
    private var activeGesture = false
    private var consumed = false
    private(set) var isLoadInFlight = false
    private var queued = false

    mutating func gestureChanged(active: Bool) {
        if active && !activeGesture { consumed = false }
        activeGesture = active
    }

    mutating func shouldLoad(
        previousOffset: CGFloat, offset: CGFloat, canLoad: Bool, requestInFlight: Bool,
        directUserInput: Bool = true
    )
        -> Bool
    {
        guard previousOffset.isFinite, offset.isFinite else { return false }
        if activeGesture, directUserInput, offset > 100 || offset - previousOffset > 2 { queued = false }
        if !canLoad { queued = false; return false }
        guard activeGesture, !consumed,
            previousOffset - offset > 2, offset <= 100
        else { return false }
        consumed = true
        if isLoadInFlight { queued = true; return false }
        // Another projection's request is not owned by this gate and cannot
        // turn a discarded gesture into an automatic post-failure retry.
        guard !requestInFlight else { return false }
        isLoadInFlight = true
        return true
    }

    mutating func finishLoad(preserveQueued: Bool = false) {
        isLoadInFlight = false
        if !preserveQueued { queued = false }
        consumed = true
    }

    mutating func startQueuedLoad(canLoad: Bool) -> Bool {
        guard queued, !isLoadInFlight else { return false }
        queued = false
        guard canLoad else { return false }
        isLoadInFlight = true
        return true
    }

    mutating func resetIntent() {
        activeGesture = false; consumed = false; queued = false
    }
}
