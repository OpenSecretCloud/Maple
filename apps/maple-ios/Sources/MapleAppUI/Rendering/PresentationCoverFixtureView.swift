#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import UIKit

    /// Explicit debug fixture. All messages, image pixels, storage, and playback
    /// are synthetic. The viewer, presentation lease, runtime completion gate,
    /// and listening state machine are the shipping implementations.
    struct PresentationCoverFixtureView: View {
        @State private var model = PresentationCoverFixtureModel()
        @State private var preview: ChatImageAttachment?
        @State private var compactLease = CompactMenuLease()
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @Environment(\.layoutDirection) private var layoutDirection

        private var navigationProof: Bool {
            ProcessInfo.processInfo.arguments.contains("--maple-presentation-cover-navigation-proof")
        }

        var body: some View {
            Group {
                if navigationProof {
                    nativeNavigation
                } else {
                    coverContent
                }
            }
        }

        private var coverContent: some View {
            VStack(spacing: 20) {
                Text("Image cover completion eligibility").font(.headline)
                Text(model.summary).accessibilityIdentifier("cover.summary")
                Button("Open synthetic image") { preview = model.image }
                    .accessibilityIdentifier("cover.open")
                Button("Complete visible reply") { model.send() }
                    .accessibilityIdentifier("cover.visible-completion")
                    .disabled(model.runtime.hasUnresolvedRun || model.completions != 1 || !model.blocks.isEmpty)
            }
            .padding()
            .chatFullScreenCover(item: $preview) { image in
                ImageZoomView(attachment: image)
                    .safeAreaInset(edge: .bottom) {
                        VStack {
                            Text(model.summary).accessibilityIdentifier("cover.inside-summary")
                            Button("Complete covered reply") { model.send() }
                                .accessibilityIdentifier("cover.covered-completion")
                                .disabled(model.runtime.hasUnresolvedRun || model.completions != 0)
                        }.padding().background(.regularMaterial)
                    }
            }
            .environment(\.chatPresentationChanged) { id, active in model.presentationChanged(id: id, active: active) }
        }

        /// The same viewer and lease run inside the shipping retained native
        /// navigation. This flag adds no alternate dismissal or recovery path.
        private var nativeNavigation: some View {
            let scope = model.runtime.scope, owner = model.runtime.snapshot.storageOwner
            return CompactMenuNavigation(
                scope: scope, storageOwner: owner,
                requestedMenu: model.menuPresented, reduceMotion: reduceMotion, lease: compactLease,
                library: NavigationStack {
                    Text("Synthetic retained Library")
                        .accessibilityIdentifier("cover.library")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(MapleSurface.sidebar)
                        .toolbarVisibility(.hidden, for: .navigationBar)
                        .compactMenuRootBar(
                            role: .library, title: "Synthetic retained Library",
                            trailing: AnyView(closeMenu(scope: scope, owner: owner)))
                }
                .environment(\.layoutDirection, layoutDirection),
                chat:
                    coverContent
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(MapleSurface.canvas)
                    .compactMenuRootBar(role: .chat, title: "Synthetic cover chat")
                    .environment(\.layoutDirection, layoutDirection)
                    .modifier(
                        CompactMenuChatAccessibility(
                            lease: compactLease, scope: scope, storageOwner: owner,
                            isCurrent: { [weak model] in
                                model?.isCurrent(scope: scope, storageOwner: owner) ?? false
                            })),
                isCurrent: { [weak model] in model?.isCurrent(scope: scope) ?? false },
                presentationBlocked: { [weak model] in !(model?.blocks.isEmpty ?? false) },
                requestMenu: { [weak model] in model?.setMenu($0, scope: scope) },
                blockPresentation: { [weak model] id, active in
                    guard let model, model.isCurrent(scope: scope) else { return }
                    model.presentationChanged(id: id, active: active)
                },
                chatShown: { _ in }
            )
            .id(scope)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .ignoresSafeArea(.container)
        }

        private func closeMenu(scope: AccountScope, owner: UUID) -> some View {
            Button {
                guard model.isCurrent(scope: scope, storageOwner: owner) else { return }
                model.setMenu(false, scope: scope)
            } label: {
                Label("Close menu", systemImage: "xmark")
                    .labelStyle(.iconOnly)
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("cover.library-close")
            .disabled(!compactLease.menuVisible)
        }
    }

    @MainActor @Observable private final class PresentationCoverFixtureModel {
        let runtime: ChatRuntime
        let listening: ReplyListeningSession
        let player: CoverFixturePlayer
        let image: ChatImageAttachment
        var blocks = Set<UUID>()
        var completions = 0
        var blockedCompletions = 0
        var hapticEligible = 0
        var failure = false
        private(set) var menuPresented = false

        init() {
            let scope = AccountScope(apiOrigin: "https://synthetic.invalid", principalID: "cover-fixture")
            let player = CoverFixturePlayer()
            self.player = player
            listening = ReplyListeningSession(scope: scope, preparer: CoverFixturePreparer(), player: player)
            runtime = ChatRuntime(
                scope: scope, service: CoverFixtureService(), persistence: CoverFixtureStore(),
                conversationID: "cover-chat")
            // Encoded locally, never downloaded or written to a file.
            let pixels = UIGraphicsImageRenderer(size: CGSize(width: 80, height: 80)).pngData { context in
                UIColor.systemTeal.setFill(); context.fill(CGRect(x: 0, y: 0, width: 80, height: 80))
            }
            image = ChatImageAttachment(data: pixels, contentType: "image/png")
            runtime.isVisible = true
            listening.setVisibility(chatID: "cover-chat", foreground: true)
            listening.setAutoplay(true, chatID: "cover-chat")
            runtime.onCompletion = { [weak self] completion in
                guard let self else { return }
                completions += 1
                if !blocks.isEmpty && !completion.wasVisible && completion.wasForeground {
                    blockedCompletions += 1
                }
                listening.completed(completion)
            }
            // This is precisely the production haptic admission callback. Count
            // invocation rather than emitting a physical haptic in the fixture.
            runtime.onSuccessfulVisibleCompletion = { [weak self] _ in self?.hapticEligible += 1 }
        }

        var summary: String {
            "Leases \(blocks.count); completions \(completions); covered \(blockedCompletions); plays \(player.plays); haptics \(hapticEligible); pending \(listening.pendingCount); failed \(failure ? 1 : 0)"
        }

        func presentationChanged(id: UUID, active: Bool) {
            if active { blocks.insert(id) } else { blocks.remove(id) }
            // Same visibility inputs as AppEnvironment.updatePresentationVisibility.
            // The fixture deliberately does not claim to exercise account/root wiring.
            updateVisibility()
        }

        func isCurrent(scope: AccountScope, storageOwner: UUID? = nil) -> Bool {
            !runtime.isRetired && runtime.scope == scope
                && (storageOwner == nil || runtime.snapshot.storageOwner == storageOwner)
        }

        func setMenu(_ presented: Bool, scope: AccountScope) {
            guard isCurrent(scope: scope) else { return }
            menuPresented = presented
            updateVisibility()
        }

        private func updateVisibility() {
            let visible = !runtime.isRetired && !menuPresented && blocks.isEmpty
            runtime.isVisible = visible
            listening.setVisibility(chatID: visible ? "cover-chat" : nil, foreground: true)
        }

        func send() {
            runtime.draftText = "Synthetic cover eligibility turn"
            if !runtime.send() { failure = true }
        }
    }

    private actor CoverFixtureStore: ChatPersistence {
        private var snapshot: ChatSnapshot?
        func load(chatID: String, scope: AccountScope) async throws -> ChatSnapshot? { snapshot }
        func loadAll(scope: AccountScope) async throws -> [ChatSnapshot] { snapshot.map { [$0] } ?? [] }
        func save(_ snapshot: ChatSnapshot, scope: AccountScope) async throws { self.snapshot = snapshot }
        func remove(chatID: String, scope: AccountScope) async throws { snapshot = nil }
        func rekey(from: String, to snapshot: ChatSnapshot, scope: AccountScope) async throws {
            self.snapshot = snapshot
        }
    }

    private actor CoverFixtureService: ChatServing {
        private var responseNumber = 0
        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
            -> Conversation
        {
            Conversation(id: "cover-chat", object: "conversation", createdAt: 0, pinned: false, lastActivityAt: 0)
        }
        func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
            APIPage(data: [])
        }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        {
            responseNumber += 1
            let id = "cover-response-\(responseNumber)"
            let created = ResearchResponse(
                id: id, object: "response", createdAt: 0, status: .inProgress, model: "auto:quick")
            let completed = ResearchResponse(
                id: id, object: "response", createdAt: 0, status: .completed, model: "auto:quick",
                output: [
                    ConversationItem(
                        id: "cover-assistant-\(responseNumber)", responseId: id, type: "message", status: "completed",
                        role: "assistant",
                        content: [MessageContent(type: "output_text", text: "Synthetic readable completion")])
                ])
            let events: [ResponseStreamEvent] = [
                .created(try lifecycle(created)), .completed(try lifecycle(completed)),
            ]
            return AsyncThrowingStream { continuation in
                for event in events { continuation.yield(event) }
                continuation.finish()
            }
        }
        private func lifecycle(_ response: ResearchResponse) throws -> ResponseLifecycleEvent {
            struct Wrapper: Encodable { let response: ResearchResponse }
            let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(ResponseLifecycleEvent.self, from: encoder.encode(Wrapper(response: response)))
        }
        func retrieveResponse(id: String) async throws -> ResearchResponse { throw ChatIssue.uncertainResponse }
        func cancelResponse(id: String) async throws -> ResearchResponse { throw ChatIssue.cancellationUnavailable }
        func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
            throw ResearchAPIError.http(status: 404, reason: .notFound)
        }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
            ConversationItem
        > { APIPage(data: []) }
    }

    private actor CoverFixturePreparer: SpeechPreparing {
        func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) async throws -> PreparedSpeech {
            PreparedSpeech(
                owner: owner, responseID: reply.responseID,
                chunks: [
                    SpeechChunk(
                        file: StoredSpeechFile(
                            id: UUID(), url: URL(fileURLWithPath: "/synthetic/never-read.wav"), byteCount: 40),
                        duration: 1)
                ])
        }
        func discard(_ speech: PreparedSpeech) async throws {}
        func retire(_ owner: SpeechOwner) async throws {}
    }

    @MainActor @Observable private final class CoverFixturePlayer: SpeechPlaying {
        var onEvent: (@MainActor (SpeechPlayerEvent) -> Void)?
        var position: TimeInterval = 0
        var duration: TimeInterval = 0
        var plays = 0
        func load(_ speech: PreparedSpeech, playbackID: UUID) async throws { duration = speech.duration }
        func play() throws { plays += 1 }
        func pause() {}
        func stop() { position = 0; duration = 0 }
        func seek(to position: TimeInterval) { self.position = position }
        func setSpeed(_ speed: MaplePlaybackSpeed) {}
    }
#endif
