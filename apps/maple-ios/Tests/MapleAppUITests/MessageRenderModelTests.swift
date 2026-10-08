import Foundation
import Testing
import SwiftUI
import Textual
@testable import MapleAppUI

@MainActor
@Suite struct MessageRenderModelTests {
    @Test func continuousChunksDoNotRestartPublicationDeadline() async throws {
        let clock = RenderTestClock()
        let model = MessageRenderModel(pause: { _ in try await clock.pause() })
        defer { model.cancel() }
        let owner = UUID()
        model.submit(text: "first", generation: owner)
        try await eventually { await clock.count == 1 }
        for index in 0..<100 { model.submit(text: "chunk \(index)", generation: owner) }
        #expect(await clock.count == 1)
        await clock.tick()
        try await eventually { model.prepared?.revision == "chunk 99" }
        #expect(String(model.prepared!.content.characters) == "chunk 99")
    }

    @Test func latePreparationCannotCrossGenerationOrCancellation() async throws {
        let clock = RenderTestClock(), worker = SuspendedPreparation()
        let model = MessageRenderModel(worker: worker, pause: { _ in try await clock.pause() })
        defer { model.cancel() }
        model.submit(text: "old account", generation: UUID())
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains("old account") }
        let next = UUID()
        model.submit(text: "new account", generation: next)
        #expect(model.prepared == nil)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains("new account") }
        await worker.finish("old account")
        await Task.yield()
        #expect(model.prepared == nil)
        await worker.finish("new account")
        try await eventually { model.prepared?.revision == "new account" }
        #expect(model.generation == next)
        model.cancel()
        #expect(model.prepared == nil)
    }

    @Test func completedEquationSurvivesSuspensionButNotGenerationChange() async throws {
        let clock = RenderTestClock()
        let model = MessageRenderModel(
            pause: { _ in try await clock.pause() },
            equations: { formulas in
                Dictionary(uniqueKeysWithValues: formulas.map { ($0.id, AnyAttachment(RetainedTestEquation())) })
            })
        defer { model.cancel() }
        let owner = UUID(), source = "Equation $x$."
        model.submit(text: source, generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await clock.count == 1 && model.prepared != nil }
        await clock.tick()
        try await eventually {
            model.prepared?.content.runs.contains { $0.textual.attachment?.description == "$x$" } == true
        }
        let projection = try #require(model.prepared?.presentationKey)
        model.suspend()
        #expect(model.prepared?.presentationKey == projection)
        model.submit(text: source, generation: owner)
        await Task.yield()
        #expect(await clock.count == 0)
        #expect(model.prepared?.presentationKey == projection)
        model.submit(text: source, generation: UUID())
        #expect(model.prepared == nil)
    }

    @Test func suspensionRejectsLatePreparationAndRestartsTheSameIncompleteRequest() async throws {
        let clock = RenderTestClock(), worker = SuspendedPreparation()
        let model = MessageRenderModel(worker: worker, pause: { _ in try await clock.pause() })
        defer { model.cancel() }
        let owner = UUID(), source = "Incomplete owned message", newest = "Latest owned message"
        model.submit(text: source, generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains(source) }
        model.suspend()
        model.submit(text: source, generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.count(source) == 2 }
        await worker.finish(source)
        try await eventually { await worker.resolutionCount(source) == 1 }
        await Task.yield()
        #expect(model.prepared == nil)
        model.submit(text: newest, generation: owner)
        await Task.yield()
        #expect(await clock.count == 0, "The old drain must not clear the replacement task")
        await worker.finish(source)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains(newest) }
        await worker.finish(newest)
        try await eventually { model.prepared?.revision == newest }
        #expect(model.generation == owner)
    }

    @Test func replacingARequestCannotReuseItsOldCompletionAfterReturningToTheSameText() async throws {
        let clock = RenderTestClock(), worker = SuspendedPreparation()
        let model = MessageRenderModel(worker: worker, pause: { _ in try await clock.pause() })
        defer { model.cancel() }
        let owner = UUID()
        model.submit(text: "A", generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains("A") }
        await worker.finish("A")
        try await eventually { model.prepared?.revision == "A" }
        model.submit(text: "B", generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains("B") }
        model.submit(text: "A", generation: owner)
        await worker.finish("B")
        try await eventually {
            let waiting = await clock.count
            return model.prepared?.revision == "B" && waiting == 1
        }
        model.suspend()
        #expect(model.prepared == nil, "Incomplete B cannot inherit the old completed A marker")
        try await eventually { await clock.count == 0 }
        model.submit(text: "A", generation: owner)
        try await eventually { await clock.count == 1 }
        await clock.tick()
        try await eventually { await worker.contains("A") }
        await worker.finish("A")
        try await eventually { model.prepared?.revision == "A" }
    }

    private func eventually(_ condition: () async -> Bool) async throws {
        for _ in 0..<300 {
            if await condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        Issue.record("Controlled renderer task did not reach its expected state")
        throw CancellationError()
    }
}

private actor RenderTestClock {
    private var sleepers: [(UUID, CheckedContinuation<Void, Error>)] = []
    var count: Int { sleepers.count }
    func pause() async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                if Task.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else {
                    sleepers.append((id, continuation))
                }
            }
        } onCancel: {
            Task { await self.cancel(id) }
        }
    }
    func tick() { if !sleepers.isEmpty { sleepers.removeFirst().1.resume() } }
    private func cancel(_ id: UUID) {
        guard let index = sleepers.firstIndex(where: { $0.0 == id }) else { return }
        sleepers.remove(at: index).1.resume(throwing: CancellationError())
    }
}

private actor SuspendedPreparation: MessagePreparation {
    private var pending: [String: [CheckedContinuation<PreparedMessage, Never>]] = [:]
    private var resolutions: [String: Int] = [:]
    func prepare(_ source: String) async throws -> PreparedMessage {
        await withCheckedContinuation { pending[source, default: []].append($0) }
    }
    func resolve(_ draft: PreparedMessage, equations: [String: AnyAttachment]) async throws -> PreparedMessage {
        resolutions[draft.revision, default: 0] += 1
        return draft
    }
    func contains(_ source: String) -> Bool { count(source) > 0 }
    func count(_ source: String) -> Int { pending[source]?.count ?? 0 }
    func resolutionCount(_ source: String) -> Int { resolutions[source] ?? 0 }
    func finish(_ source: String) {
        guard var requests = pending[source], !requests.isEmpty else { return }
        let continuation = requests.removeFirst()
        pending[source] = requests.isEmpty ? nil : requests
        continuation.resume(
            returning: .init(revision: source, content: AttributedString(source), codeBlocks: [:], formulas: []))
    }
}

private struct RetainedTestEquation: Textual.Attachment {
    var description: String { "$x$" }
    var body: some View { Text("x") }
    func sizeThatFits(_ proposal: ProposedViewSize, in environment: TextEnvironmentValues) -> CGSize {
        CGSize(width: 10, height: 10)
    }
}
