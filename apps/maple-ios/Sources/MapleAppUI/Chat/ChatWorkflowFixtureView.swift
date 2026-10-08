#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import UIKit

    /// Synthetic service, real protected persistence/runtime/input/queue review.
    /// The isolated fixture namespace intentionally survives process termination.
    struct ChatWorkflowFixtureView: View {
        @State private var model = ChatWorkflowFixtureModel()
        @State private var focused = false
        @State private var reviewing = false
        @State private var away = false
        @Environment(\.scenePhase) private var scenePhase
        private var lifecycleProof: Bool {
            ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-lifecycle")
        }

        var body: some View {
            Group {
                if let runtime = model.runtime {
                    VStack {
                        Button(away ? "Return to chat" : "Visit another screen") { away.toggle() }
                            .accessibilityIdentifier("chat.fixture.navigate")
                        if away {
                            Text("Other screen; retained chat continues").accessibilityIdentifier("chat.fixture.away")
                        } else {
                            content(runtime)
                        }
                    }
                    .sheet(isPresented: $reviewing) { QueueReview(runtime: runtime) { focused = true } }
                } else {
                    Text(model.failure ?? "Loading protected fixture…").accessibilityIdentifier("chat.fixture.loading")
                }
            }
            .task {
                await model.prepare()
                if lifecycleProof { await model.runtime?.reconcileOnLifecycle(foreground: false) }
            }
            .onChange(of: scenePhase) { _, phase in
                guard lifecycleProof, let runtime = model.runtime else { return }
                runtime.isForeground = phase == .active
                if phase == .active { Task { await runtime.reconcileOnLifecycle(foreground: true) } }
            }
            .onChange(of: away) { _, hidden in
                guard lifecycleProof, let runtime = model.runtime else { return }
                runtime.isVisible = !hidden
                if !hidden { Task { await runtime.reconcileOnLifecycle(foreground: false) } }
            }
        }

        private func content(_ runtime: ChatRuntime) -> some View {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Protected chat workflow").font(.headline)
                    diagnostics(runtime)
                    if runtime.isEditingQueue {
                        HStack {
                            Text(MapleStrings.text("Editing queued message"))
                            Button(MapleStrings.text("Cancel")) { runtime.cancelQueueEdit() }
                                .accessibilityIdentifier("chat.fixture.cancel-edit")
                            Button(MapleStrings.text("Save changes")) { _ = runtime.saveQueueEdit() }
                                .disabled(runtime.busyDraftIDs.contains(runtime.snapshot.draft.id))
                                .accessibilityIdentifier("queue.save")
                        }.font(.footnote)
                    }
                    if !runtime.isOnline {
                        Label(MapleStrings.text("Offline — your draft stays here"), systemImage: "wifi.slash")
                            .accessibilityIdentifier("chat.fixture.offline")
                    }
                    if let importer = model.importer { DraftImageStrip(importer: importer) }
                    ChatRecoveryControls(runtime: runtime)
                    editor(runtime)
                    if let dictation = model.dictation {
                        DictationControls(coordinator: dictation, runtime: runtime)
                        DictationMicrophoneButton(coordinator: dictation, runtime: runtime)
                        Text(dictation.status(for: runtime)?.phase.rawValue ?? "idle")
                            .accessibilityIdentifier("chat.fixture.dictation-phase")
                    }
                    HStack {
                        Button(MapleStrings.text("Send")) { _ = runtime.send() }
                            .disabled(!runtime.canSend).accessibilityIdentifier("chat.send")
                        if runtime.hasUnresolvedRun {
                            Button(MapleStrings.text("Stop")) { runtime.stop() }.accessibilityIdentifier("chat.stop")
                        }
                        Button(MapleStrings.text("Queued messages")) { reviewing = true }
                            .accessibilityIdentifier("queue.open")
                    }.buttonStyle(.bordered).font(.caption)
                    HStack {
                        Button("Save checkpoint") { Task { await model.checkpoint() } }
                            .accessibilityIdentifier("chat.fixture.checkpoint")
                        Text(model.checkpointState).accessibilityIdentifier("chat.fixture.saved")
                        Button(runtime.isOnline ? "Go offline" : "Reconnect") { runtime.isOnline.toggle() }
                            .accessibilityIdentifier("chat.fixture.connectivity")
                    }.font(.caption)
                    HStack {
                        Button("Break before ID") { model.service?.interrupt() }
                            .accessibilityIdentifier("chat.fixture.interrupt")
                        Button("Allow recovery") { model.service?.recoveryAvailable = true }
                            .accessibilityIdentifier("chat.fixture.allow-recovery")
                        Button("Finish response") { model.service?.deliverCompleted() }
                            .accessibilityIdentifier("chat.fixture.complete")
                        Button("Deliver created") { model.service?.deliverCreated() }
                            .accessibilityIdentifier("chat.fixture.created")
                    }.font(.caption)
                    if ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-context") {
                        HStack {
                            Button("Hold next context rejection") { model.service?.holdNextContextRejection() }
                                .accessibilityIdentifier("chat.fixture.hold-context")
                            Button("Return context rejection") { model.service?.returnContextRejection() }
                                .accessibilityIdentifier("chat.fixture.reject-context")
                        }.font(.caption)
                        Text(model.service?.contextRejectionHeld == true ? "held" : "idle")
                            .accessibilityIdentifier("chat.fixture.context-gate")
                        Text(runtime.snapshot.unsentTurn?.text ?? "none")
                            .accessibilityIdentifier("chat.fixture.unsent")
                    }
                    ForEach(runtime.messages, id: \.id) { item in
                        Text(item.content?.compactMap(\.text).joined(separator: " ") ?? "")
                            .accessibilityIdentifier("chat.fixture.message." + item.id)
                    }
                }.padding()
            }
            .onChange(of: runtime.snapshot) { _, _ in model.checkpointState = "unsaved" }
        }

        private func diagnostics(_ runtime: ChatRuntime) -> some View {
            VStack(alignment: .leading, spacing: 4) {
                Text(
                    "Composer images \(runtime.snapshot.draft.images.count); held images \(runtime.snapshot.queue?.edit?.heldDraft.images.count ?? 0); queued images \(runtime.queuedTurns.first?.draft.images.count ?? 0)"
                )
                .accessibilityIdentifier("chat.fixture.image-counts")
                Text(imageIDs(runtime.snapshot.draft.images))
                    .accessibilityIdentifier("chat.fixture.composer-image-ids")
                Text(imageIDs(runtime.snapshot.queue?.edit?.heldDraft.images ?? []))
                    .accessibilityIdentifier("chat.fixture.held-image-ids")
                Text(imageIDs(runtime.queuedTurns.first?.draft.images ?? []))
                    .accessibilityIdentifier("chat.fixture.queued-image-ids")
                Text(runtime.phase?.rawValue ?? "idle").accessibilityIdentifier("chat.fixture.phase")
                Text(runtime.isQueuePaused ? "paused" : "running").accessibilityIdentifier("chat.fixture.queue-state")
                Text(runtime.snapshot.queue?.edit?.heldDraft.text ?? "no held draft")
                    .accessibilityIdentifier("chat.fixture.held")
                Text(runtime.queuedTurns.map(\.draft.text).joined(separator: " | "))
                    .accessibilityIdentifier("chat.fixture.queued")
                Text(runtime.snapshot.pendingRun?.messageID ?? "none").accessibilityIdentifier(
                    "chat.fixture.message-id")
                Text("POSTs \(model.service?.posts ?? 0)").accessibilityIdentifier("chat.fixture.posts")
                Text("Exact lookups \(model.service?.lookups ?? 0)").accessibilityIdentifier("chat.fixture.lookups")
                Text("Cancels \(model.service?.cancels ?? 0)").accessibilityIdentifier("chat.fixture.cancels")
                Text(model.service?.lastLookup ?? "none").accessibilityIdentifier("chat.fixture.last-lookup")
                Text(model.service?.durableBeforePOST == true ? "durable" : "no dispatch")
                    .accessibilityIdentifier("chat.fixture.durability")
                Text(runtime.lastIssue?.rawValue ?? "none").accessibilityIdentifier("chat.fixture.issue")
            }.font(.caption2)
        }

        private func imageIDs(_ images: [ChatImageAttachment]) -> String {
            images.isEmpty ? "none" : images.map { $0.id.uuidString }.joined(separator: ",")
        }

        private func editor(_ runtime: ChatRuntime) -> some View {
            let owner = runtime.snapshot.draft.id
            return ComposerEditor(
                text: Binding(
                    get: { runtime.ownedDraft(id: owner)?.text ?? "" },
                    set: { text in
                        guard var draft = runtime.ownedDraft(id: owner) else { return }
                        draft.text = text; _ = runtime.updateOwnedDraft(draft)
                    }), focused: $focused, ownerID: owner, accountGeneration: runtime.scope.generation,
                placeholder: "Synthetic composer", accessibilityIdentifier: "chat.fixture.composer",
                externalSelection: model.dictation?.editorState(runtime: runtime, draftID: owner),
                onChange: { state, edits in
                    model.dictation?.updateEditor(runtime: runtime, draftID: owner, state: state, edits: edits)
                },
                onSubmit: {
                    guard runtime.snapshot.draft.id == owner else { return }
                    _ = runtime.send()
                }
            ).frame(minHeight: 72).padding(8).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
    }

    @MainActor @Observable private final class ChatWorkflowFixtureModel {
        private let scope = AccountScope(
            apiOrigin: "http://127.0.0.1:1234", principalID: "native-chat-workflow-fixture")
        private var store: ProtectedChatStore?
        private(set) var service: ChatWorkflowFixtureService?
        private(set) var runtime: ChatRuntime?
        private(set) var importer: ImageImportController?
        private(set) var dictation: DictationCoordinator?
        private(set) var failure: String?
        var checkpointState = "unsaved"
        private var preparing = false
        static let conversation = "native-workflow-conversation"

        func prepare() async {
            guard !preparing, runtime == nil else { return }; preparing = true
            defer { preparing = false }
            do {
                let support = try FileManager.default.url(
                    for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                let store = ProtectedChatStore(
                    root: support.appendingPathComponent("NativeChatWorkflowFixture", isDirectory: true))
                try await store.activate(scope)
                if ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-reset") {
                    try await store.remove(chatID: Self.conversation, scope: scope)
                }
                var snapshot = try await store.load(chatID: Self.conversation, scope: scope)
                if snapshot == nil {
                    let empty = ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-empty")
                    let images = ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-images")
                    let queue =
                        empty
                        ? nil
                        : ChatQueueState(
                            items: [
                                ChatQueuedTurn(
                                    draft: ChatDraft(text: "Queued A", images: images ? [Self.image(.systemBlue)] : []))
                            ], paused: true)
                    let initial = ChatSnapshot(
                        storageKey: Self.conversation, conversationID: Self.conversation,
                        draft: ChatDraft(
                            text: "Held draft B",
                            images: images ? [Self.image(.systemOrange), Self.image(.systemGreen)] : []), queue: queue)
                    try await store.save(initial, scope: scope); snapshot = initial
                }
                guard let snapshot else { throw ChatStorageError.corruptData }
                let service = ChatWorkflowFixtureService(
                    store: store, scope: scope, expectedMessageID: snapshot.pendingRun?.messageID)
                let runtime = ChatRuntime(
                    scope: scope, service: service, persistence: store, conversationID: Self.conversation)
                guard runtime.loadRetainedSnapshot(snapshot) else { throw ChatStorageError.corruptData }
                runtime.isVisible = true
                self.store = store; self.service = service; self.runtime = runtime
                importer = ImageImportController(runtime: runtime, processor: ImageImportProcessor())
                if ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-dictation") {
                    let dictation = DictationCoordinator(
                        scope: scope, capture: LocalLifecycleCapture(), transcriber: LocalLifecycleTranscriber())
                    runtime.onDraftOwnerChanging = { [weak dictation, weak runtime] id in
                        guard let runtime else { return }
                        dictation?.ownerWillChange(runtime: runtime, draftID: id)
                    }
                    runtime.onDraftOwnerRetired = { [weak dictation, weak runtime] id in
                        guard let runtime else { return }
                        dictation?.ownerRetired(runtime: runtime, draftID: id)
                    }
                    self.dictation = dictation
                }

            } catch { failure = "Protected fixture could not load" }
        }

        private static func image(_ color: UIColor) -> ChatImageAttachment {
            let data = UIGraphicsImageRenderer(size: CGSize(width: 64, height: 64)).pngData { context in
                color.setFill(); context.fill(CGRect(x: 0, y: 0, width: 64, height: 64))
            }
            return ChatImageAttachment(data: data, contentType: "image/png")
        }

        func checkpoint() async {
            guard let runtime, let store else { return }
            await runtime.saveDraft()
            do {
                let saved = try await store.load(chatID: Self.conversation, scope: scope)
                checkpointState = saved == runtime.snapshot ? "saved" : "unsaved"
            } catch { checkpointState = "unsaved" }
        }
    }

    @MainActor @Observable private final class ChatWorkflowFixtureService: ChatServing {
        private let store: ProtectedChatStore
        private let scope: AccountScope
        private var expectedMessageID: String?
        private var continuation: AsyncThrowingStream<ResponseStreamEvent, Error>.Continuation?
        private let responseID = "native-workflow-response"
        var recoveryAvailable = false
        private(set) var posts = 0
        private(set) var lookups = 0
        private(set) var cancels = 0
        private(set) var lastLookup: String?
        private(set) var durableBeforePOST = false
        @ObservationIgnored private var rejectNextContext = false
        @ObservationIgnored private var contextWaiter: CheckedContinuation<Void, Never>?
        private(set) var contextRejectionHeld = false

        init(store: ProtectedChatStore, scope: AccountScope, expectedMessageID: String?) {
            self.store = store; self.scope = scope; self.expectedMessageID = expectedMessageID
        }
        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
            -> Conversation
        {
            throw ResearchAPIError.invalidArgument
        }
        func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
            APIPage(data: [])
        }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        {
            guard maximumInferenceSends == 2, case .some(.string(let id)) = request.metadata?["internal_message_id"]
            else {
                throw ResearchAPIError.invalidArgument
            }
            let saved = try await store.load(chatID: request.conversation, scope: scope)
            durableBeforePOST = saved?.pendingRun?.messageID == id && saved?.pendingRun?.dispatched == true
            guard durableBeforePOST else { throw ResearchAPIError.invalidArgument }
            posts += 1; expectedMessageID = id
            if rejectNextContext {
                rejectNextContext = false; contextRejectionHeld = true
                await withCheckedContinuation { contextWaiter = $0 }
                contextRejectionHeld = false
                throw ResearchAPIError.http(status: 413, reason: .contextLimit)
            }
            let pair = AsyncThrowingStream<ResponseStreamEvent, Error>.makeStream()
            continuation = pair.continuation; return pair.stream
        }
        func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
            lookups += 1; lastLookup = itemId
            guard recoveryAvailable, conversationId == ChatWorkflowFixtureModel.conversation,
                itemId == expectedMessageID
            else {
                throw ResearchAPIError.http(status: 404, reason: .notFound)
            }
            return ConversationItem(
                id: itemId, responseId: responseID, type: "message", role: "user",
                content: [.init(type: "input_text", text: "Held draft B")])
        }
        func retrieveResponse(id: String) async throws -> ResearchResponse {
            guard id == responseID else { throw ResearchAPIError.invalidArgument }
            return response(.completed)
        }
        func cancelResponse(id: String) async throws -> ResearchResponse {
            guard id == responseID else { throw ResearchAPIError.invalidArgument }
            cancels += 1; return response(.cancelled)
        }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
            ConversationItem
        > { APIPage(data: []) }
        func holdNextContextRejection() { rejectNextContext = true }
        func returnContextRejection() {
            let waiter = contextWaiter; contextWaiter = nil; waiter?.resume()
        }
        func interrupt() { continuation?.finish(throwing: ResearchAPIError.truncatedStream); continuation = nil }
        func deliverCreated() {
            struct Wrapper: Encodable { let response: ResearchResponse }
            do {
                let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
                let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
                let event = try decoder.decode(
                    ResponseLifecycleEvent.self, from: encoder.encode(Wrapper(response: response(.inProgress))))
                continuation?.yield(.created(event))
            } catch { continuation?.finish(throwing: ResearchAPIError.invalidResponse) }
        }
        func deliverCompleted() {
            struct Wrapper: Encodable { let response: ResearchResponse }
            do {
                let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
                let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
                let event = try decoder.decode(
                    ResponseLifecycleEvent.self, from: encoder.encode(Wrapper(response: response(.completed))))
                continuation?.yield(.completed(event)); continuation?.finish(); continuation = nil
            } catch { continuation?.finish(throwing: ResearchAPIError.invalidResponse) }
        }

        private func response(_ status: ResponseStatus) -> ResearchResponse {
            ResearchResponse(
                id: responseID, object: "response", createdAt: 0, status: status, model: "auto:quick",
                output: status == .completed
                    ? [
                        ConversationItem(
                            id: "recovered-assistant", responseId: responseID, type: "message", status: "completed",
                            role: "assistant",
                            content: [.init(type: "output_text", text: "Recovered without replay")])
                    ] : nil)
        }
    }
#endif
