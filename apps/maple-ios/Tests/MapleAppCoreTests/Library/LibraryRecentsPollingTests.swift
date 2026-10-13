import Foundation
import Testing
@testable import MapleAppCore

actor LibraryRecentsSleepRecorder {
    private(set) var delays: [Duration] = []
    let gates: [ChatTestGate]
    let fails: Bool

    init(gates: [ChatTestGate] = [], fails: Bool = false) {
        self.gates = gates; self.fails = fails
    }

    func sleep(_ duration: Duration) async throws {
        delays.append(duration)
        if delays.count <= gates.count { await gates[delays.count - 1].hold() }
        if fails { throw URLError(.timedOut) }
    }
}

@MainActor @Suite struct LibraryRecentsPollingTests {
    @Test func recentsRefreshesRemainSixtySecondsApart() async {
        let clock = LibraryRecentsSleepRecorder()
        var refreshes = 0
        await LibraryRecentsPolling.run(
            shouldContinue: { refreshes < 3 }, refresh: { refreshes += 1 },
            sleep: { try await clock.sleep($0) })
        #expect(refreshes == 3)
        #expect(await clock.delays == [.seconds(60), .seconds(60), .seconds(60)])
    }

    @Test(arguments: [true, false])
    func cancellationOrOwnershipLossDuringDelayPreventsRead(_ cancelled: Bool) async {
        let gate = ChatTestGate(), clock = LibraryRecentsSleepRecorder(gates: [gate])
        var owned = true, refreshes = 0
        let task = Task {
            await LibraryRecentsPolling.run(
                shouldContinue: { owned }, refresh: { refreshes += 1 },
                sleep: { try await clock.sleep($0) })
        }
        await gate.waitForArrival()
        if cancelled { task.cancel() } else { owned = false }
        await gate.release(); await task.value
        #expect(refreshes == 0)
        #expect(await clock.delays == [.seconds(60)])
    }

    @Test func hiddenMenuAndFailedSleepNeverFetch() async {
        let clock = LibraryRecentsSleepRecorder(fails: true)
        var refreshes = 0
        await LibraryRecentsPolling.run(
            shouldContinue: { false }, refresh: { refreshes += 1 },
            sleep: { try await clock.sleep($0) })
        #expect(await clock.delays.isEmpty && refreshes == 0)
        await LibraryRecentsPolling.run(
            shouldContinue: { true }, refresh: { refreshes += 1 },
            sleep: { try await clock.sleep($0) })
        #expect(await clock.delays == [.seconds(60)] && refreshes == 0)
    }
}
