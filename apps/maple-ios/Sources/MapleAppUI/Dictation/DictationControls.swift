#if os(iOS)
    import MapleAppCore
    import SwiftUI
    import UIKit

    public struct DictationControls: View {
        private let coordinator: DictationCoordinator
        private let runtime: ChatRuntime
        @Environment(\.openURL) private var openURL

        public init(coordinator: DictationCoordinator, runtime: ChatRuntime) {
            self.coordinator = coordinator; self.runtime = runtime
        }

        public var body: some View {
            if let status = coordinator.status(for: runtime) {
                HStack(spacing: 12) {
                    switch status.phase {
                    case .requestingPermission:
                        ProgressView()
                        Text("Waiting for microphone permission", tableName: "Dictation", bundle: .module)
                    case .recording:
                        Image(systemName: "record.circle.fill").foregroundStyle(.red)
                        Text(Duration.seconds(status.elapsedSeconds), format: .time(pattern: .minuteSecond))
                            .monospacedDigit()
                            .accessibilityLabel(Text("Recording time", tableName: "Dictation", bundle: .module))
                        Button(action: { coordinator.stop(runtime: runtime) }) {
                            Text("Stop", tableName: "Dictation", bundle: .module)
                        }.accessibilityIdentifier("dictation.stop")
                    case .transcribing:
                        ProgressView()
                        Text("Transcribing…", tableName: "Dictation", bundle: .module)
                    case .waitingForComposition:
                        Text("Finish typing to insert dictation", tableName: "Dictation", bundle: .module)
                    case .waitingForInsertionPoint:
                        Text("Place the cursor to insert dictation", tableName: "Dictation", bundle: .module)
                        Button(action: { coordinator.insertAtCurrentPosition(runtime: runtime) }) {
                            Text("Insert", tableName: "Dictation", bundle: .module)
                        }.accessibilityIdentifier("dictation.insert")
                    case .failed:
                        Text("Transcription unavailable", tableName: "Dictation", bundle: .module)
                        Button(action: { coordinator.retry(runtime: runtime) }) {
                            Text("Retry", tableName: "Dictation", bundle: .module)
                        }.accessibilityIdentifier("dictation.retry")
                    }
                    Spacer(minLength: 0)
                    Button(action: { coordinator.cancel(runtime: runtime) }) {
                        Text("Discard", tableName: "Dictation", bundle: .module)
                    }.accessibilityIdentifier("dictation.discard")
                }
                .font(.subheadline)
                .accessibilityElement(children: .contain)
                .padding(.vertical, 8)
            } else if let issue = coordinator.issue(for: runtime) {
                VStack(alignment: .leading, spacing: 8) {
                    issueMessage(issue)
                    HStack {
                        if issue == .permissionDenied {
                            Button(action: {
                                guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
                                openURL(url)
                            }) {
                                Text("Open Settings", tableName: "Dictation", bundle: .module)
                            }
                        }
                        Button(action: { coordinator.dismissIssue() }) {
                            Text("Dismiss", tableName: "Dictation", bundle: .module)
                        }
                    }
                }.font(.subheadline).accessibilityIdentifier("dictation.error")
            }
        }

        private func issueMessage(_ issue: DictationIssue) -> Text {
            switch issue {
            case .permissionDenied:
                Text(
                    "Allow microphone access in Settings to dictate. Your draft is unchanged.", tableName: "Dictation",
                    bundle: .module)
            case .captureUnavailable:
                Text(
                    "The microphone is unavailable. Try again when another audio operation has finished.",
                    tableName: "Dictation", bundle: .module)
            case .emptyRecording:
                Text("No audio was recorded. Your draft is unchanged.", tableName: "Dictation", bundle: .module)
            case .recordingTooLarge:
                Text(
                    "This recording could not be processed. Record up to five minutes at a time.",
                    tableName: "Dictation", bundle: .module)
            case .tooManyOperations:
                Text(
                    "Finish or discard an earlier dictation before starting another.", tableName: "Dictation",
                    bundle: .module)
            case .draftUnavailable:
                Text(
                    "Finish the current draft operation before starting dictation.", tableName: "Dictation",
                    bundle: .module)
            case .transcriptionUnavailable, .invalidTranscription:
                Text("Transcription unavailable", tableName: "Dictation", bundle: .module)
            }
        }
    }

    public struct DictationMicrophoneButton: View {
        private let coordinator: DictationCoordinator
        private let runtime: ChatRuntime
        private let isEnabled: Bool
        public init(coordinator: DictationCoordinator, runtime: ChatRuntime, isEnabled: Bool = true) {
            self.coordinator = coordinator; self.runtime = runtime; self.isEnabled = isEnabled
        }
        public var body: some View {
            Button(action: { coordinator.start(runtime: runtime) }) {
                Label {
                    Text("Dictate", tableName: "Dictation", bundle: .module)
                } icon: {
                    Image(systemName: "mic")
                }
            }
            .disabled(!isEnabled || runtime.busyDraftIDs.contains(runtime.snapshot.draft.id))
            .accessibilityIdentifier("dictation.start")
        }
    }
#endif
