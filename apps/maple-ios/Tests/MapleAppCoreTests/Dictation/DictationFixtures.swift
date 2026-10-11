import Foundation
import Testing
@testable import MapleAppCore

@MainActor final class DictationFixtureCapture: DictationCapturing {
    var permission = true
    var permissionGate: ChatTestGate?
    var active: UUID?
    var starts = 0
    var stops = 0
    var failStart = false
    var limits: (() -> Void)?
    var pcm = Data([0, 0, 1, 0, 2, 0])
    func requestPermission() async -> Bool {
        if let permissionGate { await permissionGate.hold() }
        return permission
    }
    func start(id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void) throws {
        #expect(active == nil && maximumFrames == DictationAudio.maximumFrames)
        if failStart { throw DictationIssue.captureUnavailable }
        active = id; starts += 1; limits = onLimit
    }
    func stop(id: UUID) throws -> Data {
        #expect(active == id); active = nil; stops += 1; return pcm
    }
    func cancel(id: UUID) { if active == id { active = nil } }
}

actor DictationFixtureTranscriber: DictationTranscribing {
    private struct CallWaiter {
        let count: Int
        let continuation: CheckedContinuation<Void, Error>
        let timer: Task<Void, Never>
    }
    var clips: [Data] = []
    var pending: [Int: CheckedContinuation<String, Error>] = [:]
    private var callWaiters: [UUID: CallWaiter] = [:]
    func transcribe(wav: Data) async throws -> String {
        let index = clips.count; clips.append(wav)
        return try await withCheckedThrowingContinuation {
            pending[index] = $0
            // A waiter may complete the call immediately after resuming. Publish
            // the arrival only once its result continuation is ready as well.
            for (id, waiter) in callWaiters where clips.count >= waiter.count {
                guard let completed = callWaiters.removeValue(forKey: id) else { continue }
                completed.timer.cancel()
                completed.continuation.resume()
            }
        }
    }
    func complete(_ index: Int = 0, text: String = "spoken words", fails: Bool = false) {
        if fails {
            pending.removeValue(forKey: index)?.resume(throwing: URLError(.notConnectedToInternet))
        } else {
            pending.removeValue(forKey: index)?.resume(returning: text)
        }
    }
    func waitForCalls(_ count: Int = 1, timeout: Duration = .seconds(60)) async throws {
        try Task.checkCancellation()
        guard clips.count < count else { return }
        let id = UUID()
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await withCheckedThrowingContinuation { continuation in
                if clips.count >= count {
                    continuation.resume()
                } else {
                    // Register the actor-owned deadline alongside its waiter,
                    // so even an immediate deadline cannot race installation.
                    let timer = Task.detached { [weak self] in
                        do { try await Task.sleep(for: timeout) } catch { return }
                        await self?.expireCallWaiter(id)
                    }
                    callWaiters[id] = CallWaiter(count: count, continuation: continuation, timer: timer)
                }
            }
        } onCancel: {
            Task { await self.cancelCallWaiter(id) }
        }
    }

    private func expireCallWaiter(_ id: UUID) {
        guard let waiter = callWaiters.removeValue(forKey: id) else { return }
        waiter.timer.cancel()
        waiter.continuation.resume(
            throwing: DictationFixtureWaitFailure.transcriptionTimedOut(expected: waiter.count, received: clips.count))
    }

    private func cancelCallWaiter(_ id: UUID) {
        guard let waiter = callWaiters.removeValue(forKey: id) else { return }
        waiter.timer.cancel()
        waiter.continuation.resume(throwing: CancellationError())
    }
}

enum DictationFixtureWaitFailure: Error, Equatable {
    case transcriptionTimedOut(expected: Int, received: Int), transitionTimedOut
}

@MainActor func dictationWait(_ condition: () -> Bool, timeout: Duration = .seconds(60)) async throws {
    let clock = ContinuousClock(), deadline = ContinuousClock.now.advanced(by: timeout)
    while !condition() {
        try Task.checkCancellation()
        guard clock.now < deadline else { throw DictationFixtureWaitFailure.transitionTimedOut }
        // Native state has no fixture event hook. Suspend between observations
        // instead of spending the VM's executor budget on repeated yields.
        try await clock.sleep(until: min(deadline, clock.now.advanced(by: .milliseconds(2))))
    }
}

@MainActor func dictationFixture() -> (
    ChatRuntime, DictationCoordinator, DictationFixtureCapture, DictationFixtureTranscriber, ChatFixtureStore
) {
    let (runtime, _, store) = ChatRuntimeTests().fixture()
    let capture = DictationFixtureCapture(), service = DictationFixtureTranscriber()
    let coordinator = DictationCoordinator(scope: runtime.scope, capture: capture, transcriber: service)
    runtime.onDraftOwnerChanging = { [weak coordinator, weak runtime] id in
        guard let runtime else { return }; coordinator?.ownerWillChange(runtime: runtime, draftID: id)
    }
    runtime.onDraftOwnerRetired = { [weak coordinator, weak runtime] id in
        guard let runtime else { return }; coordinator?.ownerRetired(runtime: runtime, draftID: id)
    }
    return (runtime, coordinator, capture, service, store)
}

@MainActor func dictationEditor(
    _ runtime: ChatRuntime, _ coordinator: DictationCoordinator, offset: Int,
    marked: NSRange? = nil, draftID: UUID? = nil
) {
    let owner = draftID ?? runtime.snapshot.draft.id
    guard let draft = runtime.ownedDraft(id: owner) else { return }
    coordinator.updateEditor(
        runtime: runtime, draftID: owner,
        state: DictationEditorState(
            text: draft.text, selectionUTF16: NSRange(location: offset, length: 0), markedRangeUTF16: marked))
}

@Suite struct DictationFixtureWaitTests {
    @Test func arrivalsReleaseCountWaitersWithResultContinuationsReady() async throws {
        let service = DictationFixtureTranscriber()
        let secondArrival = Task { try await service.waitForCalls(2) }
        let first = Task { try await service.transcribe(wav: Data([1])) }
        try await service.waitForCalls()
        await service.complete(text: "first")
        #expect(try await first.value == "first")
        let second = Task { try await service.transcribe(wav: Data([2])) }
        try await secondArrival.value
        await service.complete(1, text: "second")
        #expect(try await second.value == "second")
        // An already-observed threshold must not require another arrival.
        try await service.waitForCalls(2, timeout: .zero)
        #expect(await service.clips == [Data([1]), Data([2])])
    }

    @Test func missingArrivalThrowsCountFailureAndCancelledWaitDoesNotRemainPending() async throws {
        let service = DictationFixtureTranscriber()
        await #expect(throws: DictationFixtureWaitFailure.transcriptionTimedOut(expected: 1, received: 0)) {
            try await service.waitForCalls(timeout: .zero)
        }
        let cancelled = Task { try await service.waitForCalls() }
        cancelled.cancel()
        await #expect(throws: CancellationError.self) { try await cancelled.value }
        let call = Task { try await service.transcribe(wav: Data([3])) }
        try await service.waitForCalls()
        await service.complete(text: "after cancellation")
        #expect(try await call.value == "after cancellation")
    }
}
