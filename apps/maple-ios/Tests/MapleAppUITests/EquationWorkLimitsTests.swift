import Foundation
import Testing
@testable import MapleAppUI

@MainActor struct EquationWorkLimitsTests {
    private func waitForQueue(_ count: Int, in admission: EquationWorkAdmission) async {
        for _ in 0..<10_000 {
            if admission.waitingCount == count { return }
            await Task.yield()
        }
        Issue.record("Expected equation admission queue size was not reached")
    }

    @Test func onlyOneRendererOwnsAdmissionAndStaleReleaseCannotFreeIt() async throws {
        let admission = EquationWorkAdmission()
        let first = try await admission.acquire()
        let queued = Task { try await admission.acquire() }
        await waitForQueue(1, in: admission)
        #expect(admission.activeCount == 1)
        admission.release(UUID())
        #expect(admission.waitingCount == 1)
        admission.release(first)
        let second = try await queued.value
        #expect(second != first)
        admission.release(first)
        #expect(admission.activeCount == 1)
        admission.release(second)
        #expect(admission.activeCount == 0 && admission.waitingCount == 0)
    }

    @Test func queuedCancellationDoesNotWaitForActiveRenderer() async throws {
        let admission = EquationWorkAdmission()
        let active = try await admission.acquire()
        let queued = Task { try await admission.acquire() }
        await waitForQueue(1, in: admission)
        queued.cancel()
        await #expect(throws: CancellationError.self) { try await queued.value }
        #expect(admission.waitingCount == 0 && admission.activeCount == 1)
        admission.release(active)
    }

    @Test func queuedLimitRejectsFurtherWorkWithoutLosingTickets() async throws {
        let admission = EquationWorkAdmission()
        let active = try await admission.acquire()
        let queued = (0..<16).map { _ in Task { try await admission.acquire() } }
        await waitForQueue(16, in: admission)
        await #expect(throws: EquationWorkAdmission.Failure.queueFull) { try await admission.acquire() }
        for task in queued { task.cancel() }
        for task in queued { await #expect(throws: CancellationError.self) { try await task.value } }
        #expect(admission.waitingCount == 0 && admission.activeCount == 1)
        admission.release(active)
    }

    @Test func cancellationRacingGrantReturnsTheSlot() async throws {
        let admission = EquationWorkAdmission()
        let active = try await admission.acquire()
        let queued = Task { try await admission.acquire() }
        await waitForQueue(1, in: admission)
        // These synchronous MainActor mutations force grant before the canceled
        // continuation resumes; its post-grant check must return the reservation.
        admission.release(active)
        queued.cancel()
        await #expect(throws: CancellationError.self) { try await queued.value }
        #expect(admission.activeCount == 0 && admission.waitingCount == 0)
        let replacement = try await admission.acquire()
        admission.release(replacement)
    }

    @Test func preCancelledTaskNeverReservesWork() async throws {
        let admission = EquationWorkAdmission()
        let task = Task { try await admission.acquire() }
        task.cancel()
        await #expect(throws: CancellationError.self) { try await task.value }
        #expect(admission.activeCount == 0 && admission.waitingCount == 0)
    }

    @Test func rasterBudgetRetainsReservationUntilFinalLeaseReference() throws {
        let budget = EquationRasterBudget(capacity: 100)
        var original = budget.reserve(bytes: 80)
        #expect(original != nil && budget.reservedBytes == 80)
        var copy = original
        original = nil
        #expect(copy?.bytes == 80 && budget.reserve(bytes: 21) == nil)
        copy = nil
        #expect(budget.reservedBytes == 0)
        let entire = try #require(budget.reserve(bytes: 100))
        withExtendedLifetime(entire) { #expect(budget.reserve(bytes: 1) == nil) }
    }

    @Test func rasterBudgetRejectsInvalidSizesAndReleasesOffMainActor() async {
        let budget = EquationRasterBudget(capacity: 100)
        for bytes in [Int.min, -1, 0, 101, Int.max] { #expect(budget.reserve(bytes: bytes) == nil) }
        await Task.detached {
            let lease = budget.reserve(bytes: 100)
            withExtendedLifetime(lease) { #expect(budget.reservedBytes == 100) }
        }.value
        #expect(budget.reservedBytes == 0)
    }

    @Test func concurrentReservationsNeverExceedGlobalRasterCapacity() async {
        let budget = EquationRasterBudget(capacity: 100)
        let leases = await withTaskGroup(of: EquationRasterLease?.self) { group in
            for _ in 0..<100 { group.addTask { budget.reserve(bytes: 30) } }
            var results: [EquationRasterLease] = []
            for await lease in group { if let lease { results.append(lease) } }
            return results
        }
        #expect(leases.count == 3)
        withExtendedLifetime(leases) { #expect(budget.reservedBytes == 90) }
    }
}
