import Foundation
import Synchronization

/// Process-wide admission contains only opaque tickets and continuations, never
/// account identities, expressions or images. A canceled queued task is removed.
@MainActor final class EquationWorkAdmission {
    enum Failure: Error, Equatable { case queueFull }
    static let shared = EquationWorkAdmission()
    private struct Waiter {
        let id: UUID
        let continuation: CheckedContinuation<UUID, any Error>
    }

    private var active: UUID?
    private var waiters: [Waiter] = []
    var activeCount: Int { active == nil ? 0 : 1 }
    var waitingCount: Int { waiters.count }

    /// Release the returned ticket in a defer around the complete renderer
    /// lifetime. Cancellation after admission never prematurely releases another
    /// task's slot; the active renderer remains responsible for its own cleanup.
    func acquire() async throws -> UUID {
        let id = UUID()
        let granted: UUID = try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                guard !Task.isCancelled else {
                    continuation.resume(throwing: CancellationError())
                    return
                }
                if active == nil {
                    active = id
                    continuation.resume(returning: id)
                } else if waiters.count < 16 {
                    waiters.append(Waiter(id: id, continuation: continuation))
                } else {
                    continuation.resume(throwing: Failure.queueFull)
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.cancelQueued(id) }
        }
        // Cancellation may race a release that granted this waiter. The cancel
        // handler only owns queue removal; this continuation owns a granted slot.
        if Task.isCancelled {
            release(granted)
            throw CancellationError()
        }
        return granted
    }

    func release(_ id: UUID) {
        guard active == id else { return }
        active = nil
        guard !waiters.isEmpty else { return }
        let next = waiters.removeFirst()
        active = next.id
        next.continuation.resume(returning: next.id)
    }

    private func cancelQueued(_ id: UUID) {
        guard let index = waiters.firstIndex(where: { $0.id == id }) else { return }
        waiters.remove(at: index).continuation.resume(throwing: CancellationError())
    }
}

/// A reservation accounts for retained decoded raster cost across all messages.
/// It contains no content and is returned only when the aggregate budget fits.
final class EquationRasterBudget: Sendable {
    static let shared = EquationRasterBudget()
    let capacity: Int
    private let retained = Mutex(0)

    init(capacity: Int = 16 * 1024 * 1024) { self.capacity = max(0, capacity) }
    var reservedBytes: Int { retained.withLock { $0 } }

    func reserve(bytes: Int) -> EquationRasterLease? {
        guard bytes > 0, bytes <= capacity else { return nil }
        let accepted = retained.withLock { count in
            guard count <= capacity - bytes else { return false }
            count += bytes
            return true
        }
        return accepted ? EquationRasterLease(bytes: bytes, budget: self) : nil
    }

    fileprivate func release(bytes: Int) {
        retained.withLock { $0 -= bytes }
    }
}

/// Attachment copies share this immutable lease. Only the final release returns
/// the reservation, on whichever thread drops the last attachment reference.
final class EquationRasterLease: Sendable {
    let bytes: Int
    private let budget: EquationRasterBudget
    fileprivate init(bytes: Int, budget: EquationRasterBudget) {
        self.bytes = bytes
        self.budget = budget
    }
    deinit { budget.release(bytes: bytes) }
}
