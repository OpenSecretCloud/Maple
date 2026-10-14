#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import UIKit

    /// Deterministic, synthetic-only UI proof for the shipping Core/input/control
    /// surfaces. The fixture neither requests microphone access nor contacts an API.
    struct DictationFixtureView: View {
        @State private var model = DictationFixtureModel()
        @State private var focused = false
        @State private var caret = 0
        @State private var reviewing = false

        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Dictation controls").font(.title2)
                    Text(
                        model.coordinator.status(for: model.runtime)?.phase.rawValue
                            ?? model.coordinator.statuses.values.first?.phase.rawValue ?? "idle"
                    )
                    .accessibilityIdentifier("dictation.fixture.phase")
                    Text(model.coordinator.issue(for: model.runtime) == nil ? "none" : "visible")
                        .accessibilityIdentifier("dictation.fixture.feedback")
                    Text("Sends \(model.sends)").accessibilityIdentifier("dictation.fixture.sends")
                    Text("Requests \(model.transcriber.requests); returned \(model.transcriber.returned)")
                        .accessibilityIdentifier("dictation.fixture.requests")
                    Text(
                        model.runtime.queuedTurns.isEmpty
                            ? "none"
                            : model.runtime.queuedTurns.map(\.draft.text).joined(separator: " | ")
                    )
                    .accessibilityIdentifier("dictation.fixture.queued")
                    Text(model.runtime.isEditingQueue ? "editing" : "ordinary")
                        .accessibilityIdentifier("dictation.fixture.editor-state")
                    HStack {
                        Button("Return transcript") { Task { await model.transcriber.finish(failing: false) } }
                            .accessibilityIdentifier("dictation.fixture.complete")
                        Button("Fail request") { Task { await model.transcriber.finish(failing: true) } }
                            .accessibilityIdentifier("dictation.fixture.fail")
                    }.buttonStyle(.bordered)
                    Button("Return latest editor transcript") {
                        Task { await model.transcriber.finish(failing: false, latest: true, text: "late editor text") }
                    }.accessibilityIdentifier("dictation.fixture.complete-latest")
                    HStack {
                        Button("Review queued messages") { reviewing = true }
                            .accessibilityIdentifier("dictation.fixture.queue")
                        if model.runtime.isEditingQueue {
                            Button(MapleStrings.text("Save changes")) { _ = model.runtime.saveQueueEdit() }
                                .disabled(model.runtime.busyDraftIDs.contains(model.runtime.snapshot.draft.id))
                                .accessibilityIdentifier("queue.save")
                        }
                    }
                    HStack {
                        Button("Deny microphone") { model.capture.permitted = false }
                            .accessibilityIdentifier("dictation.fixture.deny")
                        Button("Allow microphone") { model.capture.permitted = true }
                            .accessibilityIdentifier("dictation.fixture.allow")
                    }.font(.caption)
                    editor
                    HStack {
                        Button("Cursor at end") {
                            guard let input = Self.fixtureInput() else { return }
                            input.selectedTextRange = input.textRange(
                                from: input.endOfDocument, to: input.endOfDocument)
                        }.accessibilityIdentifier("dictation.fixture.cursor-end")
                        Text("Caret \(caret)").accessibilityIdentifier("dictation.fixture.caret")
                    }.font(.caption)
                    DictationControls(coordinator: model.coordinator, runtime: model.runtime)
                    HStack {
                        DictationMicrophoneButton(coordinator: model.coordinator, runtime: model.runtime)
                            .labelStyle(.iconOnly).buttonStyle(.plain)
                        Button("Send") { model.sends += 1 }
                            .disabled(!model.runtime.canSend).accessibilityIdentifier("dictation.fixture.send")
                    }.buttonStyle(.bordered)
                    HStack {
                        Button("Mark text") {
                            Self.fixtureInput()?.setMarkedText("にほん", selectedRange: NSRange(location: 3, length: 0))
                        }
                        .accessibilityIdentifier("dictation.fixture.mark")
                        Button("Edit queued A") {
                            if let first = model.runtime.queuedTurns.first {
                                _ = model.runtime.beginQueueEdit(id: first.id)
                            }
                        }.accessibilityIdentifier("dictation.fixture.edit")
                        Button("Cancel edit") { model.runtime.cancelQueueEdit() }
                            .accessibilityIdentifier("dictation.fixture.cancel-edit")
                    }.font(.caption)
                    Text(model.runtime.snapshot.queue?.edit?.heldDraft.text ?? "No held draft")
                        .accessibilityIdentifier("dictation.fixture.held")
                }.padding()
            }
            .sheet(isPresented: $reviewing) { QueueReview(runtime: model.runtime) { focused = true } }
            .onDisappear {
                model.coordinator.invalidate(); model.runtime.invalidate(); model.transcriber.tearDown()
            }
        }

        private var editor: some View {
            let runtime = model.runtime, owner = runtime.snapshot.draft.id
            return ComposerEditor(
                text: Binding(
                    get: { runtime.ownedDraft(id: owner)?.text ?? "" },
                    set: { value in
                        guard var draft = runtime.ownedDraft(id: owner) else { return }
                        draft.text = value; _ = runtime.updateOwnedDraft(draft)
                    }), focused: $focused, ownerID: owner,
                placeholder: "Synthetic draft", accessibilityIdentifier: "dictation.fixture.composer",
                externalSelection: model.coordinator.editorState(runtime: runtime, draftID: owner),
                onChange: { state, edits in
                    caret = state.selectionUTF16.location
                    model.coordinator.updateEditor(runtime: runtime, draftID: owner, state: state, edits: edits)
                },
                onSubmit: {
                    guard runtime.snapshot.draft.id == owner else { return }
                    if runtime.canSend { model.sends += 1 }
                }
            )
            .frame(minHeight: 72).padding(8)
            .background(.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        }

        // Debug-only input probe uses public UITextInput APIs. Shipping code owns
        // its UITextView directly and does not inspect the SwiftUI hierarchy.
        private static func fixtureInput() -> (any UITextInput)? {
            func find(_ view: UIView) -> (any UITextInput)? {
                if view.isFirstResponder, let input = view as? any UITextInput { return input }
                for child in view.subviews { if let input = find(child) { return input } }
                return nil
            }
            for scene in UIApplication.shared.connectedScenes {
                guard let scene = scene as? UIWindowScene else { continue }
                for window in scene.windows where window.isKeyWindow {
                    if let input = find(window) { return input }
                }
            }
            return nil
        }
    }

    @MainActor @Observable private final class DictationFixtureModel {
        let runtime: ChatRuntime
        let capture: FixtureMicrophone
        let transcriber: FixtureTranscriber
        let coordinator: DictationCoordinator
        var sends = 0
        init() {
            let scope = AccountScope(apiOrigin: "http://127.0.0.1:1", principalID: "synthetic-dictation")
            let runtime = ChatRuntime(
                scope: scope, service: FixtureChatService(), persistence: FixtureDraftStore(),
                conversationID: "synthetic-chat")
            let capture = FixtureMicrophone(), transcriber = FixtureTranscriber()
            let coordinator = DictationCoordinator(scope: scope, capture: capture, transcriber: transcriber)
            self.runtime = runtime; self.capture = capture; self.transcriber = transcriber;
            self.coordinator = coordinator
            runtime.onDraftOwnerChanging = { [weak coordinator, weak runtime] id in
                guard let runtime else { return }; coordinator?.ownerWillChange(runtime: runtime, draftID: id)
            }
            runtime.onDraftOwnerRetired = { [weak coordinator, weak runtime] id in
                guard let runtime else { return }; coordinator?.ownerRetired(runtime: runtime, draftID: id)
            }
            runtime.draftText = "Queued A"; _ = runtime.enqueueDraft()
            runtime.draftText = "Keep this draft"
        }
    }

    @MainActor private final class FixtureMicrophone: DictationCapturing {
        var permitted = true
        var owner: UUID?
        func requestPermission() async -> Bool { permitted }
        func start(id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void) throws {
            owner = id
        }
        func stop(id: UUID) throws -> Data {
            guard owner == id else { throw DictationIssue.captureUnavailable }
            owner = nil; return Data(repeating: 0, count: 320)
        }
        func cancel(id: UUID) { if owner == id { owner = nil } }
    }

    @MainActor @Observable private final class FixtureTranscriber: DictationTranscribing {
        @ObservationIgnored private var pending: [Int: CheckedContinuation<String, Error>] = [:]
        @ObservationIgnored private var nextResult: (failing: Bool, text: String)?
        private(set) var requests = 0
        private(set) var returned = 0

        func transcribe(wav: Data) async throws -> String {
            requests += 1
            let id = requests
            if let nextResult {
                self.nextResult = nil; returned += 1
                return try result(failing: nextResult.failing, text: nextResult.text)
            }
            // Deliberately ignore task cancellation: late service callbacks must
            // be rejected by the real account/draft coordinator, not the fixture.
            return try await withCheckedThrowingContinuation { pending[id] = $0 }
        }
        func finish(failing: Bool, latest: Bool = false, text: String = "spoken text") async {
            let id = latest ? pending.keys.max() : pending.keys.min()
            guard let id, let continuation = pending.removeValue(forKey: id) else {
                if !latest { nextResult = (failing, text) }
                return
            }
            returned += 1
            if failing {
                continuation.resume(throwing: DictationIssue.transcriptionUnavailable)
            } else {
                continuation.resume(returning: text)
            }
        }
        func tearDown() {
            let previous = pending.values; pending = [:]; nextResult = nil
            for continuation in previous { continuation.resume(throwing: CancellationError()) }
        }
        private func result(failing: Bool, text: String) throws -> String {
            if failing { throw DictationIssue.transcriptionUnavailable }; return text
        }
    }

    private actor FixtureDraftStore: ChatPersistence {
        private var snapshots: [String: ChatSnapshot] = [:]
        func load(chatID: String, scope: AccountScope) -> ChatSnapshot? { snapshots[chatID] }
        func loadAll(scope: AccountScope) -> [ChatSnapshot] { Array(snapshots.values) }
        func save(_ snapshot: ChatSnapshot, scope: AccountScope) { snapshots[snapshot.storageKey] = snapshot }
        func remove(chatID: String, scope: AccountScope) { snapshots[chatID] = nil }
        func rekey(from: String, to: ChatSnapshot, scope: AccountScope) {
            snapshots[from] = nil; snapshots[to.storageKey] = to
        }
    }

    private struct FixtureChatService: ChatServing {
        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
            -> Conversation
        { throw URLError(.unsupportedURL) }
        func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
            APIPage(data: [])
        }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        { throw URLError(.unsupportedURL) }
        func retrieveResponse(id: String) async throws -> ResearchResponse { throw URLError(.unsupportedURL) }
        func cancelResponse(id: String) async throws -> ResearchResponse { throw URLError(.unsupportedURL) }
        func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
            throw URLError(.unsupportedURL)
        }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
            ConversationItem
        > { APIPage(data: []) }
    }
#endif
