import Foundation

/// The account session owns visibility and task cancellation. Check its lease
/// after each delay so an old conversation cannot issue another refresh.
public enum ConversationItemPolling {
    @MainActor public static func run(
        shouldContinue: @MainActor () -> Bool,
        poll: @MainActor () async -> Void,
        sleep: @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) async {
        let intervals: [Duration] = [
            .seconds(2), .seconds(5), .seconds(10), .seconds(15), .seconds(20),
            .seconds(30), .seconds(60),
        ]
        var index = 0
        while !Task.isCancelled, shouldContinue() {
            do { try await sleep(intervals[index]) } catch { return }
            guard !Task.isCancelled, shouldContinue() else { return }
            await poll()
            index = min(index + 1, intervals.count - 1)
        }
    }
}
