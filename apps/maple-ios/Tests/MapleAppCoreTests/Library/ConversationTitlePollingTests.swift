import Foundation
import Testing
@testable import MapleAppCore

private actor ConversationTitleSleepRecorder {
    private(set) var delays: [Duration] = []
    let gate: ChatTestGate?
    let fails: Bool

    init(gate: ChatTestGate? = nil, fails: Bool = false) {
        self.gate = gate; self.fails = fails
    }

    func sleep(_ duration: Duration) async throws {
        delays.append(duration)
        if let gate { await gate.hold() }
        if fails { throw URLError(.timedOut) }
    }
}

@MainActor @Suite struct ConversationTitlePollingTests {
    @Test func titleRefreshBackoffDoublesAndRetainsTenSecondCap() async {
        let clock = ConversationTitleSleepRecorder()
        var refreshes = 0
        await ConversationTitlePolling.run(
            shouldContinue: { refreshes < 7 },
            refresh: { refreshes += 1 },
            sleep: { try await clock.sleep($0) })
        #expect(refreshes == 7)
        #expect(
            await clock.delays == [
                .milliseconds(500), .seconds(1), .seconds(2), .seconds(4),
                .seconds(8), .seconds(10), .seconds(10),
            ])
    }

    @Test func generatedTitleStopsFurtherSleepsAndRefreshes() async {
        let clock = ConversationTitleSleepRecorder()
        var title = "New Conversation", refreshes = 0
        await ConversationTitlePolling.run(
            shouldContinue: { title == "New Conversation" },
            refresh: {
                refreshes += 1
                if refreshes == 3 { title = "Generated title" }
            },
            sleep: { try await clock.sleep($0) })
        #expect(title == "Generated title" && refreshes == 3)
        #expect(await clock.delays == [.milliseconds(500), .seconds(1), .seconds(2)])
    }

    enum OwnershipChange: CaseIterable, Sendable {
        case deselected, backgrounded, accountReplaced, taskReplaced
    }

    @Test(arguments: OwnershipChange.allCases)
    func ownershipChangesDuringSleepPreventTheNextRefresh(_ change: OwnershipChange) async {
        let gate = ChatTestGate(), clock = ConversationTitleSleepRecorder(gate: gate)
        var selectedID: String? = "chat", foreground = true
        var accountGeneration = UUID(), taskTicket = UUID()
        let capturedGeneration = accountGeneration, capturedTicket = taskTicket
        var refreshes = 0
        let polling = Task {
            await ConversationTitlePolling.run(
                shouldContinue: {
                    selectedID == "chat" && foreground && accountGeneration == capturedGeneration
                        && taskTicket == capturedTicket
                },
                refresh: { refreshes += 1 },
                sleep: { try await clock.sleep($0) })
        }
        await gate.waitForArrival()
        switch change {
        case .deselected: selectedID = nil
        case .backgrounded: foreground = false
        case .accountReplaced: accountGeneration = UUID()
        case .taskReplaced: taskTicket = UUID()
        }
        await gate.release(); await polling.value
        #expect(refreshes == 0)
        #expect(await clock.delays == [.milliseconds(500)])
    }

    @Test func cancellationDuringSleepPreventsRefreshEvenIfInjectedSleepReturnsNormally() async {
        let gate = ChatTestGate(), clock = ConversationTitleSleepRecorder(gate: gate)
        var refreshes = 0
        let polling = Task {
            await ConversationTitlePolling.run(
                shouldContinue: { true }, refresh: { refreshes += 1 },
                sleep: { try await clock.sleep($0) })
        }
        await gate.waitForArrival(); polling.cancel()
        await gate.release(); await polling.value
        #expect(refreshes == 0)
        #expect(await clock.delays == [.milliseconds(500)])
    }

    @Test func alreadyCancelledTaskDoesNotSleepOrRefresh() async {
        let clock = ConversationTitleSleepRecorder()
        var refreshes = 0
        let polling = Task {
            await ConversationTitlePolling.run(
                shouldContinue: { true }, refresh: { refreshes += 1 },
                sleep: { try await clock.sleep($0) })
        }
        polling.cancel(); await polling.value
        #expect(refreshes == 0)
        #expect(await clock.delays.isEmpty)
    }

    @Test func sleepFailureStopsWithoutFetching() async {
        let clock = ConversationTitleSleepRecorder(fails: true)
        var refreshes = 0
        await ConversationTitlePolling.run(
            shouldContinue: { true }, refresh: { refreshes += 1 },
            sleep: { try await clock.sleep($0) })
        #expect(refreshes == 0)
        #expect(await clock.delays == [.milliseconds(500)])
    }
}
