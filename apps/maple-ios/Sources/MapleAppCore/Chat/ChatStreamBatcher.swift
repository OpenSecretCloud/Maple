import Foundation
import MapleSDK

/// Consumes SDK events away from the main actor. Text deltas are published in
/// bounded batches; structural and terminal events first flush preceding text.
enum ChatStreamBatcher {
    static func batches(
        from source: AsyncThrowingStream<ResponseStreamEvent, Error>,
        sleep: @escaping @Sendable (Duration) async throws -> Void
    ) -> AsyncThrowingStream<[ResponseStreamEvent], Error> {
        AsyncThrowingStream(bufferingPolicy: .bufferingOldest(32)) { continuation in
            let buffer = Buffer(continuation: continuation, sleep: sleep)
            let task = Task.detached {
                do {
                    for try await event in source {
                        try Task.checkCancellation()
                        try await buffer.append(event)
                    }
                    await buffer.finish()
                } catch { await buffer.finish(error: error) }
            }
            continuation.onTermination = { @Sendable _ in
                task.cancel()
                Task { await buffer.close() }
            }
        }
    }

    private actor Buffer {
        let continuation: AsyncThrowingStream<[ResponseStreamEvent], Error>.Continuation
        let sleep: @Sendable (Duration) async throws -> Void
        var pending: [ResponseStreamEvent] = []
        var bytes = 0
        var timer: Task<Void, Never>?
        var closed = false

        init(
            continuation: AsyncThrowingStream<[ResponseStreamEvent], Error>.Continuation,
            sleep: @escaping @Sendable (Duration) async throws -> Void
        ) {
            self.continuation = continuation; self.sleep = sleep
        }

        func append(_ event: ResponseStreamEvent) throws {
            guard !closed else { throw CancellationError() }
            switch event {
            case let .outputTextDelta(value), let .reasoningTextDelta(value):
                pending.append(event); bytes += value.delta?.utf8.count ?? 0
                if pending.count >= 128 || bytes >= 64 * 1024 { try flush(); return }
                if timer == nil {
                    timer = Task {
                        do { try await sleep(.milliseconds(40)); try flush() } catch is CancellationError {} catch {
                            finish(error: error)
                        }
                    }
                }
            default:
                try flush()
                if case .dropped = continuation.yield([event]) { throw ChatIssue.uncertainResponse }
            }
        }

        func flush() throws {
            timer?.cancel(); timer = nil
            guard !pending.isEmpty, !closed else { return }
            let batch = pending; pending.removeAll(keepingCapacity: true); bytes = 0
            if case .dropped = continuation.yield(batch) { throw ChatIssue.uncertainResponse }
        }

        func finish(error: (any Error)? = nil) {
            guard !closed else { return }
            do { try flush() } catch { continuation.finish(throwing: error); close(); return }
            if let error { continuation.finish(throwing: error) } else { continuation.finish() }
            close()
        }

        func close() {
            closed = true; timer?.cancel(); timer = nil; pending.removeAll(); bytes = 0
        }
    }
}
