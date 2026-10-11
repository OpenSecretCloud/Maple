import Foundation
import Observation
import Textual

protocol MessagePreparation: Sendable {
    func prepare(_ source: String) async throws -> PreparedMessage
    func resolve(_ draft: PreparedMessage, equations: [String: AnyAttachment]) async throws -> PreparedMessage
}

/// Throttles work without restarting the publication deadline for every token.
/// Equation enrichment has a separate cancellable quiet-period task, so it can
/// never delay ordinary streamed text. No state survives generation revocation.
@MainActor @Observable
final class MessageRenderModel {
    private(set) var prepared: PreparedMessage?
    private(set) var generation: UUID?
    @ObservationIgnored private let worker: any MessagePreparation
    @ObservationIgnored private let equations:
        @MainActor @Sendable ([MessageFormula]) async throws -> [String: AnyAttachment]
    @ObservationIgnored private let pause: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var latest: Request?
    @ObservationIgnored private var completedRequest: Request?
    @ObservationIgnored private var epoch = UUID()
    @ObservationIgnored private var parseTask: Task<Void, Never>?
    @ObservationIgnored private var equationTask: Task<Void, Never>?

    init(
        worker: any MessagePreparation = MessageDocumentWorker(),
        pause: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
        equations: @escaping @MainActor @Sendable ([MessageFormula]) async throws -> [String: AnyAttachment] = { _ in
            [:]
        }
    ) {
        self.worker = worker
        self.pause = pause
        self.equations = equations
    }

    func submit(text: String, generation: UUID?) {
        let request = Request(text: text, generation: generation)
        guard latest != request else { return }
        if self.generation != generation { cancel() }
        self.generation = generation
        completedRequest = nil
        latest = request
        equationTask?.cancel()
        equationTask = nil
        if parseTask == nil {
            let owner = epoch
            parseTask = Task { @MainActor [weak self] in await self?.drain(owner: owner) }
        }
    }

    /// Retain only a completed projection during owned viewport removal or occlusion.
    /// Rotate the epoch even when work is complete: late tasks cannot publish.
    func suspend() {
        epoch = UUID()
        parseTask?.cancel()
        equationTask?.cancel()
        parseTask = nil
        equationTask = nil
        if latest != completedRequest {
            latest = nil
            prepared = nil
            completedRequest = nil
        }
    }

    func cancel() {
        suspend()
        latest = nil
        prepared = nil
        completedRequest = nil
    }

    private func drain(owner: UUID) async {
        defer { if epoch == owner { parseTask = nil } }
        while !Task.isCancelled, epoch == owner {
            do { try await pause(.milliseconds(65)) } catch { return }
            guard !Task.isCancelled, epoch == owner, let request = latest else { return }
            do {
                let draft = try await worker.prepare(request.text)
                let plainEquations = try await worker.resolve(draft, equations: [:])
                guard !Task.isCancelled, epoch == owner else { return }
                prepared = plainEquations
                if latest == request {
                    if draft.formulas.isEmpty {
                        completedRequest = request
                    } else {
                        enrich(draft, request: request, owner: owner)
                    }
                    return
                }
            } catch {
                guard !Task.isCancelled, epoch == owner else { return }
                prepared = .init(
                    revision: request.text, content: AttributedString(request.text), codeBlocks: [:], formulas: [])
                if latest == request { completedRequest = request; return }
            }
        }
    }

    private func enrich(_ draft: PreparedMessage, request: Request, owner: UUID) {
        equationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                try await pause(.milliseconds(180))
                guard !Task.isCancelled, epoch == owner, latest == request else { return }
                let rendered = try await equations(draft.formulas)
                let result = try await worker.resolve(draft, equations: rendered)
                guard !Task.isCancelled, epoch == owner, latest == request else { return }
                prepared = result
                completedRequest = request
            } catch {
                // Complete original TeX remains selectable on a terminal failure.
                guard !Task.isCancelled, epoch == owner, latest == request else { return }
                completedRequest = request
            }
        }
    }

    private struct Request: Equatable { let text: String; let generation: UUID? }
}
