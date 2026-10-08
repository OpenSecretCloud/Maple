#if os(iOS) && DEBUG
    import Foundation
    import Observation
    import SwiftUI
    import Textual
    import UIKit

    /// Only fixed synthetic content is hosted. The actual MessageView owns all
    /// preparation, publication, generation changes and disappearance callbacks.
    struct MessageEquationLifecycleFixtureView: View {
        @State private var probe = MessageEquationLifecycleProbe()
        @State private var generation = UUID()
        @State private var revision = 0
        @State private var shown = true
        @State private var coldPublished = 0
        @State private var coldInk = 0

        var body: some View {
            if ProcessInfo.processInfo.arguments.contains("--maple-renderer-cold-equations") {
                VStack {
                    Text("Published equations: \(coldPublished); ink: \(coldInk)")
                        .accessibilityIdentifier("renderer.cold-state")
                    ScrollView {
                        MessageView(
                            text: coldSource, accountGeneration: generation,
                            diagnosticPublication: {
                                coldPublished = $0
                                coldInk = $1
                            }
                        )
                        .accessibilityIdentifier("renderer.cold-message")
                    }
                }.padding()
            } else {
                lifecycleContent
            }
        }

        private var coldSource: String {
            EquationDiagnosticView.formulas.map { "$$\n" + $0 + "\n$$" }.joined(separator: "\n\n")
        }

        private var lifecycleContent: some View {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Button("Release equations") { probe.gate.release() }
                        .accessibilityIdentifier("renderer.lifecycle-release")
                        .disabled(probe.gate.pendingCount != 1)
                    Button("Pending revision") { revision += 1 }
                        .accessibilityIdentifier("renderer.lifecycle-revise")
                }
                HStack {
                    Button("Revoke generation") {
                        generation = UUID(); revision += 1
                    }
                    .accessibilityIdentifier("renderer.lifecycle-revoke")
                    Button("Remove message") { shown = false }
                        .accessibilityIdentifier("renderer.lifecycle-remove")
                }
                Text("Published equations: \(probe.publishedCount)")
                    .font(.caption)
                    .accessibilityLabel(probe.snapshot(source: source, generation: generation, shown: shown))
                    .accessibilityIdentifier("renderer.lifecycle-state")
                ScrollView {
                    if shown {
                        MessageView(
                            text: source, accountGeneration: generation, diagnosticModel: probe.model,
                            diagnosticHost: probe.windowHost,
                            diagnosticAppearance: { appeared in
                                if appeared { probe.result.appeared += 1 } else { probe.result.disappeared += 1 }
                            }
                        )
                        .accessibilityIdentifier("renderer.lifecycle-message")
                    }
                }
            }.padding()
        }

        private var source: String {
            """
            Synthetic revision \(revision).

            ```swift
            let lifecycle = true
            ```

            Inline $x^2$.

            $$
            \\frac{a}{b} + \\frac{c}{d} = \\frac{ad + bc}{bd}
            $$
            """
        }
    }

    @MainActor @Observable
    private final class MessageEquationLifecycleProbe {
        let gate: MessageEquationLifecycleGate
        let result: MessageEquationLifecycleResult
        let model: MessageRenderModel
        let windowHost: EquationLifecycleWindowHost

        init() {
            let windowHost = EquationLifecycleWindowHost(frame: .zero)
            self.windowHost = windowHost
            let gate = MessageEquationLifecycleGate()
            let result = MessageEquationLifecycleResult()
            self.gate = gate
            self.result = result
            model = MessageRenderModel(
                pause: { duration in try await gate.pause(duration) },
                equations: { formulas in
                    result.started += 1
                    result.requested = formulas.count
                    result.rasterBytesBefore = EquationRasterBudget.shared.reservedBytes
                    result.rasterCapacity = EquationRasterBudget.shared.capacity
                    result.activeBefore = EquationWorkAdmission.shared.activeCount
                    result.queuedBefore = EquationWorkAdmission.shared.waitingCount
                    result.renderer = EquationPhaseDiagnostic()
                    let renderer = EquationRenderer(
                        diagnostic: result.renderer, host: windowHost)
                    defer { renderer.invalidate() }
                    do {
                        let rendered = try await renderer.render(formulas)
                        result.completed += 1
                        result.rasterBytesAfter = EquationRasterBudget.shared.reservedBytes
                        result.attachments = rendered.count
                        result.ink =
                            rendered.values.filter {
                                EquationDiagnosticView.hasVisibleGlyphs($0.png)
                            }.count
                        return rendered.mapValues { AnyAttachment($0) }
                    } catch {
                        result.failed += 1
                        result.rasterBytesAfter = EquationRasterBudget.shared.reservedBytes
                        switch error {
                        case is CancellationError: result.failure = "cancelled"
                        case EquationRenderer.Failure.cancelled: result.failure = "cancelled"
                        case EquationRenderer.Failure.timedOut: result.failure = "timedOut"
                        case EquationRenderer.Failure.rejected: result.failure = "rejected"
                        case EquationRenderer.Failure.unavailable: result.failure = "unavailable"
                        case EquationWorkAdmission.Failure.queueFull: result.failure = "queueFull"
                        default: result.failure = "other"
                        }
                        throw error
                    }
                })
        }

        var publishedCount: Int {
            model.prepared?.content.runs.filter { $0.textual.attachment != nil }.count ?? 0
        }

        func snapshot(source: String, generation: UUID, shown: Bool) -> String {
            // No formula, revision text, UUID, account or arbitrary error leaves
            // this diagnostic. The event counters are numeric and no log grows.
            let values: [String: Any] = [
                "shown": shown, "prepared": model.prepared != nil,
                "appeared": result.appeared, "disappeared": result.disappeared,
                "currentSource": model.prepared?.revision == source,
                "currentGeneration": model.generation == generation,
                "sources": model.prepared?.equationSources.count ?? 0,
                "published": publishedCount, "waiting": gate.pendingCount,
                "entered": gate.entered, "released": gate.released, "cancelled": gate.cancelled,
                "started": result.started, "completed": result.completed, "failed": result.failed,
                "attachments": result.attachments, "ink": result.ink, "failure": result.failure,
                "requested": result.requested, "rasterBytesBefore": result.rasterBytesBefore,
                "rasterBytesAfter": result.rasterBytesAfter, "rasterCapacity": result.rasterCapacity,
                "activeBefore": result.activeBefore, "queuedBefore": result.queuedBefore,
                "rendererPhase": result.renderer.phase.rawValue,
                "rendererLastCompletedPhase": result.renderer.lastCompleted.rawValue,
                "rendererTimedOutPhase": result.renderer.timedOutPhase.rawValue,
                "rendererOutcome": result.renderer.outcome.rawValue,
                "rulesStarted": result.renderer.started(.rules),
                "rulesCompleted": result.renderer.completed(.rules),
                "navigationStarted": result.renderer.started(.navigation),
                "navigationCompleted": result.renderer.completed(.navigation),
                "navigationDeadlineMilliseconds": result.renderer.navigationDeadlineMilliseconds,
                "navigationElapsedMilliseconds": result.renderer.navigationElapsedMilliseconds,
                "navigationCrossedOriginalDeadline": result.renderer.navigationCrossedOriginalDeadline ? 1 : 0,
                "navigationElapsedCapped": result.renderer.navigationElapsedCapped ? 1 : 0,
                "metricStarted": result.renderer.started(.metric),
                "metricCompleted": result.renderer.completed(.metric),
                "snapshotStarted": result.renderer.started(.snapshot),
                "snapshotCompleted": result.renderer.completed(.snapshot),
                "navigationActions": result.renderer.count(.actions),
                "navigationActionAllowed": result.renderer.count(.actionAllowed),
                "navigationActionDenied": result.renderer.count(.actionDenied),
                "navigationResponseAllowed": result.renderer.count(.responseAllowed),
                "navigationResponseDenied": result.renderer.count(.responseDenied),
                "navigationDidStart": result.renderer.count(.didStart),
                "navigationDidCommit": result.renderer.count(.didCommit),
                "navigationDidFinish": result.renderer.count(.didFinish),
                "navigationDidFail": result.renderer.count(.didFail),
                "navigationProvisionalFailed": result.renderer.count(.provisionalFailed),
                "navigationProcessTerminated": result.renderer.count(.processTerminated),
                "navigationLoadReturnedNil": result.renderer.count(.loadReturnedNil),
                "assetsAllowedStarted": result.renderer.count(.assetStarted),
                "assetsFinished": result.renderer.count(.assetFinished),
                "assetsRejected": result.renderer.count(.assetRejected),
                "navigationLastRejectionBits": result.renderer.lastRejectionBits,
                "navigationRejectionBits": result.renderer.rejectionBits,
                "navigationDetailSaturated": result.renderer.detailSaturated ? 1 : 0,
                "windowHostRequested": 1,
                "windowHostFound": result.renderer.count(.hostFound),
                "windowHostAttached": result.renderer.count(.hostAttached),
                "windowHostDetached": result.renderer.count(.hostDetached),
            ]
            guard let data = try? JSONSerialization.data(withJSONObject: values, options: [.sortedKeys]),
                let text = String(data: data, encoding: .utf8)
            else { return "diagnostic unavailable" }
            return text
        }
    }

    @MainActor @Observable
    private final class MessageEquationLifecycleResult {
        var renderer = EquationPhaseDiagnostic()
        var appeared = 0
        var disappeared = 0
        var started = 0
        var completed = 0
        var failed = 0
        var attachments = 0
        var ink = 0
        var failure = "none"
        var requested = 0
        var rasterBytesBefore = 0
        var rasterBytesAfter = 0
        var rasterCapacity = 0
        var activeBefore = 0
        var queuedBefore = 0
    }

    @MainActor @Observable
    private final class MessageEquationLifecycleGate {
        var entered = 0
        var released = 0
        var cancelled = 0
        private(set) var pendingCount = 0
        @ObservationIgnored private var pending: [UUID: CheckedContinuation<Void, any Error>] = [:]

        func pause(_ duration: Duration) async throws {
            guard duration == .milliseconds(180) else {
                try await Task.sleep(for: duration)
                return
            }
            let id = UUID()
            entered += 1
            try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                    guard !Task.isCancelled else {
                        cancelled += 1
                        continuation.resume(throwing: CancellationError())
                        return
                    }
                    pending[id] = continuation
                    pendingCount = pending.count
                }
            } onCancel: {
                Task { @MainActor [weak self] in self?.cancel(id) }
            }
        }

        func release() {
            guard pending.count == 1, let id = pending.keys.first,
                let continuation = pending.removeValue(forKey: id)
            else { return }
            pendingCount = pending.count
            released += 1
            continuation.resume()
        }

        private func cancel(_ id: UUID) {
            guard let continuation = pending.removeValue(forKey: id) else { return }
            pendingCount = pending.count
            cancelled += 1
            continuation.resume(throwing: CancellationError())
        }
    }
#endif
