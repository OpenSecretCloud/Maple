#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import Observation
    import SwiftUI

    /// Explicit local-profile test inputs. The normal root, account, navigation,
    /// storage and lifecycle handlers remain in use. No microphone is opened,
    /// speech service is requested or account credentials are changed by this fixture.
    @MainActor @Observable final class LocalLifecycleFixtureDriver {
        let capture = LocalLifecycleCapture()
        let transcriber = LocalLifecycleTranscriber()
        var online = true
        var listeningProof: LocalListeningProof?
        var onConnectivity: (@MainActor (Bool) -> Void)?

        func setOnline(_ value: Bool) {
            online = value; onConnectivity?(value)
        }
    }

    @MainActor @Observable final class LocalLifecycleCapture: DictationCapturing {
        private var owner: UUID?
        private(set) var starts = 0
        private(set) var stops = 0
        private(set) var cancellations = 0
        func requestPermission() async -> Bool { true }
        func start(id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void) throws {
            guard owner == nil else { throw DictationIssue.captureUnavailable }
            owner = id; starts += 1
        }
        func stop(id: UUID) throws -> Data {
            guard owner == id else { throw DictationIssue.captureUnavailable }
            owner = nil; stops += 1
            return Data(repeating: 0, count: 320)
        }
        func cancel(id: UUID) {
            guard owner == id else { return }
            owner = nil; cancellations += 1
        }
    }

    actor LocalLifecycleTranscriber: DictationTranscribing {
        private var pending: [CheckedContinuation<String, Error>] = []
        private var completionWaiting = false
        func transcribe(wav: Data) async throws -> String {
            if completionWaiting { completionWaiting = false; return "synthetic dictated text" }
            return try await withCheckedThrowingContinuation { pending.append($0) }
        }
        func pendingCount() -> Int { pending.count }
        @discardableResult func finish() -> Int {
            if pending.isEmpty { completionWaiting = true; return 0 }
            let active = pending; pending = []
            for continuation in active { continuation.resume(returning: "synthetic dictated text") }
            return active.count
        }
    }

    struct LocalLifecycleFixtureControls: View {
        let driver: LocalLifecycleFixtureDriver
        var body: some View {
            if let proof = driver.listeningProof {
                LocalListeningFixtureControls(proof: proof, transcriber: driver.transcriber)
            } else {
                VStack(spacing: 4) {
                    Text(
                        "Starts \(driver.capture.starts), stops \(driver.capture.stops), cancelled \(driver.capture.cancellations)"
                    )
                    .font(.caption).accessibilityIdentifier("lifecycle.capture-counts")
                    HStack {
                        Button(driver.online ? "Go offline" : "Go online") { driver.setOnline(!driver.online) }
                            .accessibilityIdentifier("lifecycle.connectivity")
                        Button("Complete synthetic transcript") { Task { await driver.transcriber.finish() } }
                            .accessibilityIdentifier("lifecycle.transcript")
                    }.font(.caption).buttonStyle(.bordered)
                }.padding(4).background(.background)
            }
        }
    }
#endif
