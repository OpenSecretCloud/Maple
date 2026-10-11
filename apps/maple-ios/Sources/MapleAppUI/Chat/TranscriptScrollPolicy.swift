import Foundation

struct TranscriptViewportMetrics: Codable, Equatable {
    let offset: CGFloat
    let distanceFromBottom: CGFloat
    let viewportHeight: CGFloat
    let rawOffset: CGFloat
    let topInset: CGFloat
    let bottomInset: CGFloat
    let contentHeight: CGFloat
    let visibleMinY: CGFloat
    let visibleHeight: CGFloat
    let visibleMaxY: CGFloat
    let boundsMinY: CGFloat
    let boundsHeight: CGFloat
}

/// Research's UnifiedChat follows appended assistant items when the reader is
/// within 100 points of the bottom, then nudges once when a reply starts. Token
/// deltas and completion do not move the reader's current position.
struct TranscriptScrollPolicy {
    enum Request: Equatable { case appendedItem, streamingStarted }
    struct Mutation: Equatable {
        let lastItemID: String?
        let lastItemIsAssistant: Bool
        let hasStreamingMessage: Bool
    }

    private(set) var isUserScrolling = false
    private var previous: Mutation?

    mutating func receive(_ mutation: Mutation) -> [Request] {
        defer { previous = mutation }
        // Loading the first history page is a projection, not an append.
        guard let previous, previous.lastItemID != nil else { return [] }
        var requests: [Request] = []
        if mutation.lastItemID != nil, mutation.lastItemID != previous.lastItemID,
            mutation.lastItemIsAssistant, !isUserScrolling
        {
            requests.append(.appendedItem)
        }
        if mutation.hasStreamingMessage, !previous.hasStreamingMessage, !isUserScrolling {
            requests.append(.streamingStarted)
        }
        return requests
    }

    mutating func readerScrolled(distanceFromBottom: CGFloat) {
        guard distanceFromBottom.isFinite else { return }
        isUserScrolling = distanceFromBottom >= 100
    }

    static func maximumOffset(contentHeight: CGFloat, viewportHeight: CGFloat) -> CGFloat? {
        guard [contentHeight, viewportHeight].allSatisfy(\.isFinite),
            contentHeight >= 0, viewportHeight > 0
        else { return nil }
        // ScrollGeometry.containerSize already excludes the safe-area insets.
        // Targets use content coordinates; contentOffset + topInset normalizes
        // the reader into those same coordinates. Adding the insets again
        // falsely reports distance remaining even at the physical scroll end.
        return max(0, contentHeight - viewportHeight)
    }

    static func streamStartOffset(current: CGFloat, maximum: CGFloat) -> CGFloat? {
        guard current.isFinite, maximum.isFinite else { return nil }
        return min(max(0, current) + 100, max(0, maximum))
    }

    static func appendedItemOffset(
        expectedItemID: String, renderedItemID: String?, tailBottom: CGFloat,
        viewportHeight: CGFloat
    ) -> CGFloat? {
        guard expectedItemID == renderedItemID else { return nil }
        return maximumOffset(contentHeight: tailBottom + 20, viewportHeight: viewportHeight)
    }
}
