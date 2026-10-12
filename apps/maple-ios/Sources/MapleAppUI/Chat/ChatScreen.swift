import Foundation
import MapleSDK

struct ChatTranscriptRow: Identifiable {
    let item: ConversationItem
    let toolResult: ConversationItem?
    let order: Int
    var id: String { item.id }

    // Conversation pages use function_call/function_call_output, while a
    // retrieved Responses run uses tool_call/tool_output. Keep presentation
    // admission consistent for both SDK-decoded representations.
    static func isToolCall(_ item: ConversationItem) -> Bool {
        ["function_call", "tool_call", "web_search_call"].contains(item.type)
    }
    static func isToolOutput(_ item: ConversationItem) -> Bool {
        ["function_call_output", "tool_output"].contains(item.type)
    }
    static func isTool(_ item: ConversationItem) -> Bool { isToolCall(item) || isToolOutput(item) }

    static func rows(_ items: [ConversationItem]) -> [Self] {
        let calls = Dictionary(
            grouping: items.indices.filter {
                isToolCall(items[$0]) && usableCallID(items[$0])
            }, by: { items[$0].callId! })
        let results = Dictionary(
            grouping: items.indices.filter {
                isToolOutput(items[$0]) && usableCallID(items[$0])
            }, by: { items[$0].callId! })
        var paired: [Int: Int] = [:]
        for (callID, callIndices) in calls {
            guard callIndices.count == 1, let call = callIndices.first,
                let resultIndices = results[callID], resultIndices.count == 1,
                let result = resultIndices.first, result > call
            else { continue }
            paired[call] = result
        }
        let absorbed = Set(paired.values)
        return items.indices.compactMap { index in
            guard !absorbed.contains(index) else { return nil }
            return Self(item: items[index], toolResult: paired[index].map { items[$0] }, order: index)
        }
    }

    private static func usableCallID(_ item: ConversationItem) -> Bool {
        guard let id = item.callId else { return false }
        return !id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

enum ChatComposerPresentation {
    static func expanded(newConversation: Bool, editingQueue: Bool, focused: Bool, keyboardVisible: Bool?) -> Bool {
        newConversation || editingQueue || (focused && keyboardVisible != false)
    }
}

#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import SwiftUI
    import UIKit
    import UniformTypeIdentifiers

    struct ChatScreen: View {
        @Bindable var environment: AppEnvironment
        @Environment(\.horizontalSizeClass) private var sizeClass
        @Environment(\.layoutDirection) private var layoutDirection
        @Environment(\.mapleLaunchTransition) private var launchTransition
        @State private var columns: NavigationSplitViewVisibility = .all
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @AccessibilityFocusState private var menuButtonFocused: Bool
        @State private var compactMenuLease = CompactMenuLease()

        var body: some View {
            Group {
                if sizeClass == .regular {
                    NavigationSplitView(columnVisibility: $columns) {
                        library(wide: true)
                    } detail: {
                        NavigationStack {
                            detail.safeAreaInset(edge: .bottom) { audioPlayer }
                        }
                    }.navigationSplitViewStyle(.balanced)
                        .onAppear {
                            // The regular split owns its own appearance boundary.
                            environment.menuPresented = false
                            environment.finishNavigationDismissal()
                        }
                } else {
                    compactNavigation
                }
            }
            .onChange(of: environment.menuPresented) { _, presented in
                if presented { menuButtonFocused = false }
            }
            .onChange(of: environment.session.account?.scope) { oldScope, _ in
                if let oldScope { compactMenuLease.retireAccount(scope: oldScope) }
                menuButtonFocused = false
            }
        }

        /// Library is the navigation root. The same Chat hosting controller is
        /// popped for Menu and pushed for Close, retaining its editor and scroll state.
        @ViewBuilder private var compactNavigation: some View {
            if let scope = environment.session.account?.scope, environment.libraryContext?.scope == scope {
                CompactMenuNavigation(
                    scope: scope, storageOwner: environment.selectedChat?.snapshot.storageOwner,
                    requestedMenu: environment.menuPresented, reduceMotion: reduceMotion, lease: compactMenuLease,
                    library: library(wide: false)
                        .environment(\.layoutDirection, layoutDirection)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(MapleSurface.sidebar)
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("library.drawer"),
                    chat: detailContent.safeAreaInset(edge: .bottom) { audioPlayer }
                        .compactMenuRootBar(
                            role: .chat, title: chatTitle,
                            principal: AnyView(chatHeader), trailing: AnyView(newChatControl)
                        )
                        .environment(\.layoutDirection, layoutDirection)
                        .environment(\.mapleLaunchTransition, launchTransition)
                        .environment(\.compactMenuChatState) { [weak compactMenuLease] scope, owner in
                            compactMenuLease?.state(scope: scope, storageOwner: owner) ?? CompactMenuChatState()
                        }
                        .background(MapleSurface.canvas)
                        .modifier(
                            CompactMenuChatAccessibility(
                                lease: compactMenuLease, scope: scope,
                                storageOwner: environment.selectedChat?.snapshot.storageOwner,
                                isCurrent: { [weak environment] in
                                    environment?.session.account?.scope == scope
                                        && environment?.libraryContext?.scope == scope
                                })
                        ),
                    isCurrent: { [weak environment] in
                        environment?.session.account?.scope == scope && environment?.libraryContext?.scope == scope
                    },
                    presentationBlocked: { [weak environment] in environment?.chatNavigationBlocked ?? true },
                    requestMenu: { [weak environment] presented in
                        guard environment?.session.account?.scope == scope else { return }
                        environment?.menuPresented = presented
                    },
                    blockPresentation: { [weak environment] presentationID, blocked in
                        environment?.setPresentationBlocked(blocked, id: presentationID, scope: scope)
                    },
                    chatShown: { reason in
                        guard environment.session.account?.scope == scope,
                            environment.libraryContext?.scope == scope, !environment.menuPresented
                        else { return }
                        environment.finishNavigationDismissal()
                        if reason == .navigation {
                            menuButtonFocused = environment.accountFlow == nil && environment.session.account != nil
                        }
                    }
                )
                .id(scope)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .ignoresSafeArea(.container)
            } else {
                Color.clear.accessibilityHidden(true)
            }
        }

        @ViewBuilder private var audioPlayer: some View {
            if let listening = environment.listening { CompactReplyPlayer(session: listening).padding(.horizontal, 8) }
        }

        private var detailContent: some View {
            Group {
                if let chat = environment.selectedChat, let details = environment.entitlements,
                    let preferences = environment.preferences
                {
                    ChatDetail(environment: environment, runtime: chat, account: details, preferences: preferences)
                        .id(chat.snapshot.storageOwner)
                } else {
                    Color.clear.accessibilityHidden(true)
                }
            }
            #if DEBUG
                .safeAreaInset(edge: .top, spacing: 0) {
                    if environment.configuration.profile == .local,
                        ProcessInfo.processInfo.arguments.contains("--maple-local-listening-fixture"),
                        let fixture = environment.lifecycleFixture
                    {
                        LocalLifecycleFixtureControls(driver: fixture)
                    }
                }
            #endif
            .background(MapleSurface.canvas)
        }

        private var detail: some View {
            detailContent
                .navigationTitle(chatTitle)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar(removing: .sidebarToggle)
                .toolbar {
                    ToolbarItem(placement: .principal) {
                        chatHeader
                    }
                    ToolbarItem(placement: .topBarLeading) {
                        Button {
                            showMenu()
                        } label: {
                            Label(MapleStrings.text("Menu"), systemImage: "line.3.horizontal")
                                .labelStyle(.iconOnly)
                                .frame(minWidth: 44, minHeight: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("chat.menu")
                        .accessibilityFocused($menuButtonFocused)
                    }
                    ToolbarItem(placement: .topBarTrailing) {
                        newChatControl
                    }
                }
        }

        private var chatHeader: some View {
            HStack(spacing: 6) {
                if !chatTitle.isEmpty { Text(chatTitle).font(.headline).lineLimit(1) }
                MaplePrivacyLock()
            }
        }

        @ViewBuilder private var newChatControl: some View {
            let scope = environment.session.account?.scope
            Button {
                guard let scope, scope == environment.session.account?.scope else { return }
                Task {
                    guard scope == environment.session.account?.scope else { return }
                    await environment.selectNewChat()
                }
            } label: {
                Label(MapleStrings.text("New chat"), systemImage: "square.and.pencil")
                    .labelStyle(.iconOnly)
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("chat.new")
        }

        @ViewBuilder private func library(wide: Bool) -> some View {
            if let context = environment.libraryContext {
                LibraryView(
                    library: context.root, makeLibrary: context.makeLibrary,
                    makeInstructionEditor: context.makeInstructionEditor,
                    onSelectChat: { id in
                        guard context.scope == environment.session.account?.scope else { return }
                        Task {
                            guard context.scope == environment.session.account?.scope else { return }
                            await environment.selectChat(id) {
                                guard context.scope == environment.session.account?.scope else { return }
                                environment.menuPresented = false
                            }
                        }
                    },
                    onNewChat: { projectID in
                        guard context.scope == environment.session.account?.scope else { return }
                        environment.menuPresented = false
                        Task {
                            guard context.scope == environment.session.account?.scope else { return }
                            await environment.selectNewChat(projectID: projectID)
                        }
                    },
                    settings: { AnyView(SettingsScreen(environment: environment, context: context)) },
                    projectExpansion: context.projectExpansion,
                    menu: context.menu,
                    onClose: wide
                        ? nil
                        : { @MainActor @Sendable in
                            guard context.scope == environment.session.account?.scope else { return }
                            environment.menuPresented = false
                        },
                    settingsFullScreen: wide,
                    settingsPresented: $environment.settingsPresented,
                    onSettingsDismiss: environment.finishNavigationDismissal,
                    drawerNavigation: !wide,
                    compactRootBar: !wide,
                    navigationAvailable: true,
                    navigationFocusAvailable: wide || compactMenuLease.menuVisible,
                    supplementary: {
                        AnyView(unfinishedWork)
                    }
                )
                .id(context.scope)
                .safeAreaInset(edge: .bottom) { if !wide { audioPlayer } }
                .safeAreaInset(edge: libraryFixtureInsetEdge(wide: wide)) {
                    #if DEBUG
                        if !wide, let fixture = environment.lifecycleFixture, let proof = fixture.listeningProof {
                            LocalListeningFixtureControls(proof: proof, transcriber: fixture.transcriber)
                        }
                    #endif
                }
            } else {
                Color.clear.accessibilityHidden(true)
            }
        }

        private func libraryFixtureInsetEdge(wide: Bool) -> VerticalEdge {
            #if DEBUG
                if !wide, environment.lifecycleFixture?.listeningProof != nil {
                    return .bottom
                }
            #endif
            return .top
        }

        @ViewBuilder private var unfinishedWork: some View {
            if !environment.unfinishedChats.isEmpty {
                Section(MapleStrings.text("Unfinished messages")) {
                    ForEach(environment.unfinishedChats, id: \.snapshot.storageOwner) { runtime in
                        Button(MapleStrings.text("Review unsent work")) {
                            guard runtime.scope == environment.session.account?.scope else { return }
                            environment.selectUnfinished(runtime); environment.menuPresented = false
                        }
                    }
                }
            }
        }

        private func showMenu() {
            guard let context = environment.libraryContext,
                context.scope == environment.session.account?.scope
            else { return }
            if sizeClass == .regular {
                columns = columns == .all ? .detailOnly : .all
            } else {
                environment.menuPresented = true
            }
        }

        private var chatTitle: String {
            guard let id = environment.selectedChat?.conversationID,
                case .string(let title) = environment.conversationDetails[id]?.metadata?["title"]
            else { return "" }
            return title
        }
    }

    /// Evaluate visibility inside the hosted hierarchy, after the controller
    /// has acknowledged this projection's owner. A parent-built Bool can still
    /// describe the previous cached chat during that installation pass.
    struct CompactMenuChatAccessibility: ViewModifier {
        let lease: CompactMenuLease
        let scope: AccountScope
        let storageOwner: UUID?
        let isCurrent: @MainActor () -> Bool

        func body(content: Content) -> some View {
            content.accessibilityHidden(
                !isCurrent() || lease.chatProjectionAccessibilityHidden(scope: scope, storageOwner: storageOwner))
        }
    }

    #if DEBUG
        /// Isolated UI fixtures supply library services without restoring or
        /// activating the SDK client. Their existing projection fence remains
        /// required for every interaction and asynchronous completion.
        struct ChatOptionsFixtureContext {
            let library: LibraryModel
            let projectID: @MainActor () -> String?
            let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
            let selectProject: @MainActor (String?) async -> Bool
        }

        /// Content-free laid-out dimensions for synthetic transcript regressions.
        struct TranscriptItemHorizontalGeometry: Equatable {
            let minX: CGFloat
            let width: CGFloat
        }
    #endif

    struct ChatDetail: View {
        @Bindable var environment: AppEnvironment
        @Environment(\.layoutDirection) private var layoutDirection
        @Environment(\.horizontalSizeClass) private var sizeClass
        @Environment(\.compactMenuChatState) private var compactMenuState
        @Bindable var runtime: ChatRuntime
        @Bindable var account: AccountEntitlements
        @Bindable var preferences: PreferencesController
        #if DEBUG
            var fixtureProjectionIsCurrent: (@MainActor @Sendable (AccountScope, UUID) -> Bool)? = nil
            var fixtureOptions: ChatOptionsFixtureContext? = nil
            var fixtureScrollMetricsChanged: (@MainActor (TranscriptViewportMetrics) -> Void)? = nil
            var fixtureItemHorizontalGeometryChanged: (@MainActor (String, TranscriptItemHorizontalGeometry) -> Void)? =
                nil
            var fixtureHorizontalScrollMetricsChanged: (@MainActor ([String: CGFloat]) -> Void)? = nil
        #endif
        @State private var queuePresented = false
        @State private var modelPresented = false
        @State private var externalLink: URL?
        @State private var browserPresentationID = UUID()
        @State private var mediaNotice: MediaFeature?
        @State private var composerFocused = false
        @State private var softwareKeyboardVisible: Bool?
        @State private var readingPosition = ScrollPosition(idType: String.self)
        @State private var turnFrames: [String: TranscriptTurnGeometry] = [:]
        @State private var readingAnchor: ReadingAnchor?
        @State private var scrollMetrics = TranscriptScrollMetrics(returnRevision: 0)
        @State private var correctionTask: Task<Void, Never>?
        @State private var acceptedTurnScrollTask: Task<Void, Never>?
        @State private var pendingScrollMessage: String?
        @State private var pendingScrollToken = UUID()
        @State private var transcriptSpace = UUID()
        @State private var followPolicy = TranscriptScrollPolicy()
        @State private var userScrollPhase = false
        @State private var followTasks: [UUID: Task<Void, Never>] = [:]
        @State private var appendFollowTask: Task<Void, Never>?
        @State private var pendingAppendFollow: AppendFollowRequest?
        @State private var scrollGestureRevision = 0

        private struct ReadingAnchor {
            let token = UUID()
            let turnID: String
            let viewportY: CGFloat
            var restoring = false
            var corrections = 0
        }
        private struct AppendFollowRequest {
            let id = UUID()
            let lastItemID: String
            let storageOwner: UUID
            let returnRevision: Int
            let gestureRevision: Int
        }
        private struct TranscriptScrollMetrics: Equatable {
            var offset: CGFloat = 0
            var topInset: CGFloat = 0
            var bottomInset: CGFloat = 0
            var contentHeight: CGFloat = 0
            var height: CGFloat = 0
            var visibleMinY: CGFloat = 0
            var visibleHeight: CGFloat = 0
            var boundsMinY: CGFloat = 0
            var boundsHeight: CGFloat = 0
            let returnRevision: Int
            var maximumOffset: CGFloat? {
                TranscriptScrollPolicy.maximumOffset(
                    contentHeight: contentHeight, viewportHeight: height)
            }
            var distanceFromBottom: CGFloat {
                maximumOffset.map { max(0, $0 - offset - topInset) } ?? .infinity
            }
        }
        private struct TranscriptTurnGeometry: Equatable {
            let contentFrame: CGRect
            let firstMessageID: String?
            let lastMessageID: String?
            let returnRevision: Int
        }

        private var menuState: CompactMenuChatState {
            compactMenuState(runtime.scope, runtime.snapshot.storageOwner)
        }
        private var currentProjection: Bool {
            #if DEBUG
                return fixtureProjectionIsCurrent?(runtime.scope, runtime.snapshot.storageOwner)
                    ?? productionCurrentProjection
            #else
                return productionCurrentProjection
            #endif
        }
        private var productionCurrentProjection: Bool {
            environment.session.account?.scope == runtime.scope && runtime.scope == account.scope
                && environment.selectedChat?.snapshot.storageOwner == runtime.snapshot.storageOwner
        }

        private var transcriptMutation: TranscriptScrollPolicy.Mutation {
            .init(
                lastItemID: runtime.messages.last?.id,
                lastItemIsAssistant: runtime.messages.last.map { $0.type != "message" || $0.role != "user" } ?? false,
                hasStreamingMessage: runtime.hasUnresolvedRun
                    && runtime.messages.contains {
                        $0.type == "message" && $0.role != "user"
                            && ["streaming", "in_progress"].contains($0.status ?? "")
                    })
        }

        private func cancelFollowTasks() {
            for task in followTasks.values { task.cancel() }
            followTasks.removeAll()
            appendFollowTask?.cancel(); appendFollowTask = nil; pendingAppendFollow = nil
        }

        private func scheduleAppendFollow() {
            guard let request = pendingAppendFollow else { return }
            appendFollowTask?.cancel()
            appendFollowTask = Task { @MainActor in
                await Task.yield()
                guard !Task.isCancelled, pendingAppendFollow?.id == request.id,
                    currentProjection, !menuState.occluded,
                    runtime.snapshot.storageOwner == request.storageOwner,
                    menuState.returnRevision == request.returnRevision,
                    scrollGestureRevision == request.gestureRevision, !followPolicy.isUserScrolling,
                    !runtime.isLoadingOlder, readingAnchor == nil, !userScrollPhase,
                    runtime.messages.last?.id == request.lastItemID,
                    let lastTurnID = turns.last?.id, let sample = turnFrames[lastTurnID],
                    sample.returnRevision == request.returnRevision,
                    let target = TranscriptScrollPolicy.appendedItemOffset(
                        expectedItemID: request.lastItemID, renderedItemID: sample.lastMessageID,
                        tailBottom: sample.contentFrame.maxY,
                        viewportHeight: scrollMetrics.height)
                else { return }
                // Layout, not Task.yield(), proves the appended item has joined
                // the transcript. Consume this once so later text growth cannot
                // keep dragging the reader to a moving bottom anchor.
                pendingAppendFollow = nil
                // The timeline scrolls vertically; nested rich-content scrollers
                // must not let an implicit horizontal target move its gutters.
                withAnimation(.smooth(duration: 0.25)) { readingPosition.scrollTo(x: 0, y: target) }
            }
        }

        private func followAppendedReplies(_ mutation: TranscriptScrollPolicy.Mutation) {
            let requests = followPolicy.receive(mutation)
            guard currentProjection, !menuState.occluded, !runtime.isLoadingOlder, readingAnchor == nil else { return }
            for request in requests {
                if request == .appendedItem, let lastItemID = mutation.lastItemID {
                    pendingAppendFollow = AppendFollowRequest(
                        lastItemID: lastItemID, storageOwner: runtime.snapshot.storageOwner,
                        returnRevision: menuState.returnRevision, gestureRevision: scrollGestureRevision)
                    scheduleAppendFollow()
                    continue
                }
                let id = UUID(), owner = runtime.snapshot.storageOwner
                let returnRevision = menuState.returnRevision, gestureRevision = scrollGestureRevision
                followTasks[id] = Task { @MainActor in
                    defer { followTasks.removeValue(forKey: id) }
                    try? await Task.sleep(for: .milliseconds(100))
                    guard !Task.isCancelled, currentProjection, !menuState.occluded,
                        runtime.snapshot.storageOwner == owner, menuState.returnRevision == returnRevision,
                        scrollGestureRevision == gestureRevision, !followPolicy.isUserScrolling,
                        !runtime.isLoadingOlder, readingAnchor == nil, !userScrollPhase,
                        let maximum = scrollMetrics.maximumOffset
                    else { return }
                    let target: CGFloat?
                    switch request {
                    case .appendedItem: target = maximum
                    case .streamingStarted:
                        guard transcriptMutation.hasStreamingMessage else { return }
                        target = TranscriptScrollPolicy.streamStartOffset(
                            current: scrollMetrics.offset + scrollMetrics.topInset, maximum: maximum)
                    }
                    guard let target else { return }
                    withAnimation(.smooth(duration: 0.25)) { readingPosition.scrollTo(x: 0, y: target) }
                }
            }
        }

        private var retainsMessageRendering: @MainActor @Sendable () -> Bool {
            #if DEBUG
                let fixtureProjection = fixtureProjectionIsCurrent
            #endif
            return {
                [
                    weak environment, weak runtime, weak account, scope = runtime.scope,
                    owner = runtime.snapshot.storageOwner
                ] in
                guard let environment, let runtime, let account,
                    account.scope == scope, runtime.scope == scope, runtime.snapshot.storageOwner == owner
                else { return false }
                #if DEBUG
                    if let fixtureProjection {
                        return fixtureProjection(scope, owner)
                    }
                #endif
                guard environment.session.account?.scope == scope,
                    environment.selectedChat?.snapshot.storageOwner == owner
                else { return false }
                // Leaving the viewport is temporary too. Keep completed
                // formatting while this exact chat/account owner is current,
                // so lazy-row re-entry cannot replace it with raw Markdown.
                return true
            }
        }

        private func scheduleAnchorCorrection() {
            guard !menuState.occluded, currentProjection, readingAnchor?.restoring == true else { return }
            correctionTask?.cancel()
            let token = readingAnchor?.token
            correctionTask = Task { @MainActor in
                // Coalesce geometry updates before issuing a stable content-space target.
                await Task.yield()
                guard !Task.isCancelled, !menuState.occluded, currentProjection,
                    var anchor = readingAnchor, anchor.token == token,
                    let sample = turnFrames[anchor.turnID],
                    sample.firstMessageID == runtime.messages.first?.id,
                    sample.returnRevision == menuState.returnRevision,
                    scrollMetrics.returnRevision == menuState.returnRevision,
                    sample.contentFrame.minY.isFinite, anchor.viewportY.isFinite,
                    scrollMetrics.offset.isFinite, scrollMetrics.topInset.isFinite
                else { return }
                // Content coordinates keep this target independent of a separately
                // delivered scroll-offset update from another layout pass.
                let point = max(0, sample.contentFrame.minY - anchor.viewportY + scrollMetrics.topInset)
                guard point.isFinite, abs(point - (scrollMetrics.offset + scrollMetrics.topInset)) > 0.5 else {
                    return
                }
                guard anchor.corrections < 64 else { readingAnchor = nil; return }
                anchor.corrections += 1
                readingAnchor = anchor
                var transaction = Transaction()
                transaction.disablesAnimations = true
                withTransaction(transaction) { readingPosition.scrollTo(x: 0, y: point) }
            }
        }

        private func resumeReadingAfterMenu(reader: ScrollViewProxy) {
            guard !menuState.occluded, currentProjection else { return }
            if let message = pendingScrollMessage {
                acceptedTurnScrollTask?.cancel()
                let token = pendingScrollToken, owner = runtime.snapshot.storageOwner
                acceptedTurnScrollTask = Task { @MainActor in
                    await Task.yield()
                    guard !Task.isCancelled, currentProjection, !menuState.occluded,
                        runtime.snapshot.storageOwner == owner, pendingScrollToken == token,
                        pendingScrollMessage == message
                    else { return }
                    readingAnchor = nil; correctionTask?.cancel()
                    // Sending a new turn opts back into following newly appended
                    // reply items, just as Research's forced user-send scroll does.
                    followPolicy.readerScrolled(distanceFromBottom: 0)
                    reader.scrollTo(message, anchor: .top)
                    pendingScrollMessage = nil
                }
            } else if let anchor = readingAnchor, anchor.restoring {
                readingPosition.scrollTo(id: anchor.turnID, anchor: .top)
                scheduleAnchorCorrection()
            }
        }

        private struct Turn: Identifiable {
            let id: String
            let firstItemIndex: Int
            var items: [ConversationItem]
        }
        private var turns: [Turn] {
            var result: [Turn] = []
            for (index, item) in runtime.messages.enumerated() {
                if item.role == "user" || result.isEmpty {
                    result.append(Turn(id: item.id, firstItemIndex: index, items: [item]))
                } else {
                    result[result.count - 1].items.append(item)
                }
            }
            return result
        }

        var body: some View {
            GeometryReader { available in
                GeometryReader { geometry in
                    ScrollViewReader { reader in
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 24) {
                                if runtime.hasOlderMessages {
                                    Button(MapleStrings.text("Load earlier messages")) {
                                        guard !runtime.isLoadingOlder else { return }
                                        cancelFollowTasks()
                                        let previousFirst = runtime.messages.first?.id
                                        let returnRevision = menuState.returnRevision
                                        let visible = turnFrames.filter {
                                            let frame = $0.value.contentFrame.offsetBy(dx: 0, dy: -scrollMetrics.offset)
                                            return $0.value.firstMessageID == previousFirst
                                                && $0.value.returnRevision == returnRevision
                                                && scrollMetrics.returnRevision == returnRevision
                                                && frame.maxY > 0 && frame.minY < scrollMetrics.height
                                        }.min { $0.value.contentFrame.minY < $1.value.contentFrame.minY }
                                        readingAnchor = visible.map {
                                            ReadingAnchor(
                                                turnID: $0.key,
                                                viewportY: $0.value.contentFrame.minY - scrollMetrics.offset)
                                        }
                                        let token = readingAnchor?.token
                                        Task { @MainActor in
                                            await runtime.loadOlderMessages()
                                            guard currentProjection, readingAnchor?.token == token,
                                                let anchor = readingAnchor
                                            else { return }
                                            guard runtime.messages.first?.id != previousFirst else {
                                                readingAnchor = nil; return
                                            }
                                            readingAnchor?.restoring = true
                                            guard !menuState.occluded else { return }
                                            // A LazyVStack can evict the previously visible row while
                                            // prepending. Materialize it before restoring its exact offset.
                                            readingPosition.scrollTo(id: anchor.turnID, anchor: .top)
                                            await Task.yield()
                                            guard currentProjection, !menuState.occluded,
                                                readingAnchor?.token == token
                                            else { return }
                                            scheduleAnchorCorrection()
                                        }
                                    }
                                    .frame(maxWidth: .infinity)
                                }
                                if runtime.messages.isEmpty, runtime.conversationID == nil {
                                    VStack(alignment: .leading, spacing: 12) {
                                        Text(MapleStrings.text("Research anything..."))
                                            .font(MapleTypography.display)
                                            .foregroundStyle(MapleSurface.displayText)
                                    }
                                    .padding(.bottom, 24)
                                    .frame(
                                        maxWidth: .infinity,
                                        minHeight: max(0, geometry.size.height - 40), alignment: .bottomLeading
                                    )
                                    .accessibilityElement(children: .combine)
                                }
                                ForEach(turns) { turn in
                                    VStack(alignment: .leading, spacing: 24) {
                                        ForEach(ChatTranscriptRow.rows(turn.items)) { row in
                                            let item = row.item
                                            TranscriptItem(
                                                item: item, toolResult: row.toolResult,
                                                generation: runtime.scope.generation,
                                                revealDelay: min(Double(turn.firstItemIndex + row.order) * 0.024, 0.12),
                                                theme: preferences.value.theme.messageRenderTheme,
                                                font: preferences.value.chatFont.messageRenderFont,
                                                isReplyReady: item.status == "completed" || !runtime.hasUnresolvedRun,
                                                speechAccess: account.speechAccess,
                                                onReadAloud: { environment.readAloud(item, runtime: runtime) },
                                                onSpeechUnavailable: { mediaNotice = .speech }
                                            ) { url in
                                                externalLink = url
                                            }
                                            #if DEBUG
                                                .onGeometryChange(for: TranscriptItemHorizontalGeometry?.self) {
                                                    geometry in
                                                    guard fixtureItemHorizontalGeometryChanged != nil else {
                                                        return nil
                                                    }
                                                    let frame = geometry.frame(in: .global)
                                                    return TranscriptItemHorizontalGeometry(
                                                        minX: frame.minX, width: frame.width)
                                                } action: { sample in
                                                    guard let callback = fixtureItemHorizontalGeometryChanged,
                                                        let sample,
                                                        currentProjection, !menuState.occluded,
                                                        sample.minX.isFinite, sample.width.isFinite, sample.width > 0
                                                    else { return }
                                                    callback(item.id, sample)
                                                }
                                            #endif
                                        }
                                    }
                                    .frame(
                                        maxWidth: .infinity,
                                        minHeight: turn.id == turns.last?.id ? max(0, geometry.size.height - 40) : 0,
                                        alignment: .topLeading
                                    )
                                    .id(turn.id)
                                    .onGeometryChange(for: TranscriptTurnGeometry.self) {
                                        [
                                            transcriptSpace, firstMessageID = runtime.messages.first?.id,
                                            lastMessageID = runtime.messages.last?.id,
                                            returnRevision = menuState.returnRevision
                                        ] in
                                        TranscriptTurnGeometry(
                                            contentFrame: $0.frame(in: .named(transcriptSpace)),
                                            firstMessageID: firstMessageID, lastMessageID: lastMessageID,
                                            returnRevision: returnRevision)
                                    } action: { sample in
                                        guard !menuState.occluded, sample.firstMessageID == runtime.messages.first?.id,
                                            sample.returnRevision == menuState.returnRevision,
                                            turnFrames[turn.id] != sample
                                        else { return }
                                        turnFrames[turn.id] = sample
                                        scheduleAnchorCorrection()
                                        scheduleAppendFollow()
                                    }
                                    .onDisappear {
                                        if !menuState.occluded, turnFrames[turn.id] != nil {
                                            turnFrames.removeValue(forKey: turn.id)
                                        }
                                    }
                                }
                            }
                            .scrollTargetLayout()
                            // Keep the content proposal and its gutters independent of streamed block widths.
                            .frame(width: max(0, min(geometry.size.width, 850) - 40), alignment: .leading)
                            .padding(.horizontal, 20).padding(.vertical, 20)
                            .frame(maxWidth: .infinity)
                            .coordinateSpace(name: transcriptSpace)
                            .background {
                                TranscriptVerticalAxisGuard(active: currentProjection && !menuState.occluded)
                                    .frame(width: 0, height: 0)
                                    .allowsHitTesting(false)
                                    .accessibilityHidden(true)
                            }
                        }
                        .scrollDismissesKeyboard(.interactively)
                        .scrollPosition($readingPosition)
                        .onScrollGeometryChange(for: TranscriptScrollMetrics.self) {
                            [returnRevision = menuState.returnRevision] geometry in
                            TranscriptScrollMetrics(
                                offset: geometry.contentOffset.y, topInset: geometry.contentInsets.top,
                                bottomInset: geometry.contentInsets.bottom, contentHeight: geometry.contentSize.height,
                                height: geometry.containerSize.height,
                                visibleMinY: geometry.visibleRect.minY, visibleHeight: geometry.visibleRect.height,
                                boundsMinY: geometry.bounds.minY, boundsHeight: geometry.bounds.height,
                                returnRevision: returnRevision)
                        } action: { _, metrics in
                            guard !menuState.occluded, metrics.returnRevision == menuState.returnRevision else {
                                return
                            }
                            scrollMetrics = metrics
                            #if DEBUG
                                if metrics.distanceFromBottom.isFinite {
                                    fixtureScrollMetricsChanged?(
                                        .init(
                                            offset: metrics.offset + metrics.topInset,
                                            distanceFromBottom: metrics.distanceFromBottom,
                                            viewportHeight: metrics.height, rawOffset: metrics.offset,
                                            topInset: metrics.topInset, bottomInset: metrics.bottomInset,
                                            contentHeight: metrics.contentHeight, visibleMinY: metrics.visibleMinY,
                                            visibleHeight: metrics.visibleHeight,
                                            visibleMaxY: metrics.visibleMinY + metrics.visibleHeight,
                                            boundsMinY: metrics.boundsMinY, boundsHeight: metrics.boundsHeight))
                                }
                            #endif
                            if userScrollPhase {
                                followPolicy.readerScrolled(distanceFromBottom: metrics.distanceFromBottom)
                            }
                            scheduleAnchorCorrection()
                            scheduleAppendFollow()
                        }
                        #if DEBUG
                            .onScrollGeometryChange(for: [String: CGFloat]?.self) { geometry in
                                guard fixtureHorizontalScrollMetricsChanged != nil else { return nil }
                                return [
                                    "offsetX": geometry.contentOffset.x,
                                    "contentWidth": geometry.contentSize.width,
                                    "viewportWidth": geometry.containerSize.width,
                                    "boundsMinX": geometry.bounds.minX,
                                    "boundsWidth": geometry.bounds.width,
                                    "visibleMinX": geometry.visibleRect.minX,
                                    "visibleWidth": geometry.visibleRect.width,
                                    "leadingInset": geometry.contentInsets.leading,
                                    "trailingInset": geometry.contentInsets.trailing,
                                ]
                            } action: { _, metrics in
                                guard let callback = fixtureHorizontalScrollMetricsChanged, let metrics,
                                    currentProjection, !menuState.occluded, metrics.values.allSatisfy(\.isFinite)
                                else { return }
                                callback(metrics)
                            }
                        #endif
                        .onScrollPhaseChange { _, phase in
                            if !menuState.occluded, phase == .tracking || phase == .interacting {
                                readingAnchor = nil; correctionTask?.cancel()
                                scrollGestureRevision += 1; cancelFollowTasks()
                            }
                            if !menuState.occluded {
                                let wasUserScrolling = userScrollPhase
                                userScrollPhase = phase == .tracking || phase == .interacting || phase == .decelerating
                                if userScrollPhase || wasUserScrolling {
                                    followPolicy.readerScrolled(distanceFromBottom: scrollMetrics.distanceFromBottom)
                                }
                            }
                        }
                        .accessibilityIdentifier("chat.transcript")
                        .onChange(of: runtime.snapshot.pendingRun?.id) { _, id in
                            guard id != nil, let message = runtime.snapshot.pendingRun?.messageID else { return }
                            // One anchor change per accepted user turn, never per token.
                            // A hidden retained Chat defers it until the actual return.
                            pendingScrollMessage = message; pendingScrollToken = UUID()
                            resumeReadingAfterMenu(reader: reader)
                        }
                        .onChange(of: menuState.returnRevision) { _, _ in
                            resumeReadingAfterMenu(reader: reader)
                        }
                        .onChange(of: transcriptMutation) { _, mutation in
                            followAppendedReplies(mutation)
                        }
                        .onAppear { resumeReadingAfterMenu(reader: reader) }
                    }
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { composer(maximumHeight: available.size.height) }
            }
            .onDisappear {
                correctionTask?.cancel(); correctionTask = nil
                acceptedTurnScrollTask?.cancel(); acceptedTurnScrollTask = nil
                cancelFollowTasks()
                if !menuState.occluded { readingAnchor = nil; pendingScrollMessage = nil }
            }
            .environment(\.retainedMessageRendering, retainsMessageRendering)
            .tracksChatPresentation(queuePresented || modelPresented || mediaNotice != nil)
            .environment(\.chatPresentationChanged) { id, presented in
                environment.setPresentationBlocked(presented, id: id, scope: runtime.scope)
            }
            .onChange(of: composerFocused) { _, focused in
                #if DEBUG
                    ComposerFocusTrace.record("shipping.focusChanged=\(focused)")
                #endif
                if !focused { softwareKeyboardVisible = nil }
            }
            .onChange(of: menuState.occluded) { _, occluded in
                if occluded {
                    composerFocused = false; correctionTask?.cancel(); acceptedTurnScrollTask?.cancel()
                    cancelFollowTasks(); userScrollPhase = false
                }
            }
            .onChange(of: menuState.returnRevision) { _, _ in
                if menuState.focusNewProjection, runtime.conversationID == nil, currentProjection {
                    composerFocused = true
                }
            }
            .onAppear {
                environment.updatePresentationVisibility()
                if !menuState.occluded, runtime.conversationID == nil { composerFocused = true }
            }
            .externalLinks($externalLink) { presented in
                environment.setPresentationBlocked(presented, id: browserPresentationID, scope: runtime.scope)
            }
            .sheet(item: $mediaNotice) { feature in
                MediaFeatureNotice(feature: feature, account: account)
            }
            .sheet(isPresented: $modelPresented) {
                let scope = runtime.scope, owner = runtime.snapshot.storageOwner
                if let context = environment.libraryContext, context.scope == scope {
                    ChatOptionsSheet(
                        entitlements: account, preferences: preferences, library: context.root,
                        regularWidth: sizeClass == .regular,
                        selectedProjectID: environment.projectID(for: runtime),
                        makeInstructionEditor: context.makeInstructionEditor,
                        isCurrent: {
                            modelPresented && environment.session.account?.scope == scope && runtime.scope == scope
                                && runtime.snapshot.storageOwner == owner && currentProjection
                        },
                        onSelectProject: { id in await environment.selectProject(runtime: runtime, id: id) },
                        onSelectModel: { id in
                            guard modelPresented, environment.session.account?.scope == scope,
                                runtime.scope == scope, runtime.snapshot.storageOwner == owner, currentProjection
                            else { return }
                            preferences.setSelectedModelID(id); modelPresented = false
                        })
                }
                #if DEBUG
                    if environment.libraryContext == nil, let fixtureOptions, fixtureOptions.library.scope == scope,
                        fixtureProjectionIsCurrent != nil
                    {
                        ChatOptionsSheet(
                            entitlements: account, preferences: preferences, library: fixtureOptions.library,
                            regularWidth: sizeClass == .regular,
                            selectedProjectID: fixtureOptions.projectID(),
                            makeInstructionEditor: fixtureOptions.makeInstructionEditor,
                            isCurrent: {
                                modelPresented && runtime.scope == scope && account.scope == scope
                                    && runtime.snapshot.storageOwner == owner && currentProjection
                            },
                            onSelectProject: fixtureOptions.selectProject,
                            onSelectModel: { id in
                                guard modelPresented, runtime.scope == scope, account.scope == scope,
                                    runtime.snapshot.storageOwner == owner, currentProjection
                                else { return }
                                preferences.setSelectedModelID(id); modelPresented = false
                            })
                    }
                #endif
            }
            .onDisappear {
                // Outer navigation hides this exact retained projection temporarily.
                // Selection replacement and account teardown still retire it normally.
                guard !menuState.occluded else { return }
                environment.setPresentationBlocked(false, id: browserPresentationID, scope: runtime.scope)
                runtime.isVisible = false; Task { await runtime.saveDraft() }
            }
            .sheet(isPresented: $queuePresented) {
                QueueReview(runtime: runtime) { composerFocused = true }
                    .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
            }
        }

        private func composer(maximumHeight: CGFloat) -> some View {
            let owner = runtime.snapshot.draft.id
            let projectionOwner = runtime.snapshot.storageOwner, scope = runtime.scope
            let padding: CGFloat = 8, bottomPadding: CGFloat = 4
            return ComposerVerticalLayout(
                maximumHeight: max(0, maximumHeight - padding * 2 - bottomPadding),
                layoutDirection: layoutDirection, spacing: 6
            ) {
                if runtime.isEditingQueue {
                    HStack {
                        Label(MapleStrings.text("Editing queued message"), systemImage: "pencil").font(.footnote)
                        Spacer()
                        Button(MapleStrings.text("Cancel")) { runtime.cancelQueueEdit() }
                        Button(MapleStrings.text("Save changes")) { _ = runtime.saveQueueEdit() }
                            .disabled(runtime.busyDraftIDs.contains(runtime.snapshot.draft.id))
                            .accessibilityIdentifier("queue.save")
                    }
                }
                if !runtime.queuedTurns.isEmpty {
                    Button {
                        queuePresented = true
                    } label: {
                        Label(
                            MapleStrings.queuedMessageCount(runtime.queuedTurns.count),
                            systemImage: "text.line.first.and.arrowtriangle.forward"
                        )
                        .frame(minWidth: 44, minHeight: 44, alignment: .leading)
                        .contentShape(Rectangle())
                    }.font(.footnote).accessibilityIdentifier("queue.open")
                }
                if runtime.hasUnresolvedRun, runtime.canSend, !runtime.isEditingQueue {
                    Button(MapleStrings.text("Queue message"), systemImage: "text.line.first.and.arrowtriangle.forward")
                    {
                        send()
                    }
                    .font(.footnote).frame(minHeight: 44)
                    .accessibilityIdentifier("queue.enqueue")
                }
                if !runtime.isOnline {
                    Label(MapleStrings.text("Offline — your draft stays here"), systemImage: "wifi.slash")
                        .font(.footnote).foregroundStyle(.secondary)
                }
                if account.chatAccess != .allowed {
                    AccountAccessNotice(account: account)
                }
                ChatRecoveryControls(runtime: runtime)
                if let importer = environment.imageImporters[ObjectIdentifier(runtime)] {
                    DraftImageStrip(importer: importer)
                    if let issue = importer.issue { ImageImportIssueView(issue: issue) }
                }
                if let dictation = environment.dictation { DictationControls(coordinator: dictation, runtime: runtime) }
                ComposerInputLayout(expanded: composerExpanded, layoutDirection: layoutDirection) {
                    attachmentButton.layoutValue(key: ComposerInputRoleKey.self, value: .attachment)
                    Group {
                        if runtime.isLoading {
                            Color.clear.frame(height: 44).accessibilityIdentifier("chat.loading")
                        } else {
                            composerEditor(owner: owner)
                        }
                    }.layoutValue(key: ComposerInputRoleKey.self, value: .editor)
                    modelButton.layoutValue(key: ComposerInputRoleKey.self, value: .model)
                    microphoneButton.layoutValue(key: ComposerInputRoleKey.self, value: .microphone)
                    primaryAction.layoutValue(key: ComposerInputRoleKey.self, value: .action)
                }
                .padding(8)
                .glassEffect(.regular, in: RoundedRectangle(cornerRadius: composerExpanded ? 24 : 32))
                .background {
                    ComposerKeyboardVisibility(eligible: composerFocused && currentProjection && !menuState.occluded) {
                        visible in
                        guard runtime.scope == scope, runtime.snapshot.storageOwner == projectionOwner,
                            currentProjection, !menuState.occluded
                        else { return }
                        softwareKeyboardVisible = visible
                    }
                    .allowsHitTesting(false).accessibilityHidden(true)
                }
                .layoutValue(key: ComposerEditorLayoutKey.self, value: true)
            }
            // Keep the trailing controls clear of the keyboard's prediction strip.
            .padding(padding).padding(.bottom, bottomPadding).frame(maxWidth: 874).frame(maxWidth: .infinity)
        }

        private var composerExpanded: Bool {
            ChatComposerPresentation.expanded(
                newConversation: runtime.conversationID == nil, editingQueue: runtime.isEditingQueue,
                focused: composerFocused, keyboardVisible: softwareKeyboardVisible)
        }

        /// Capture a fresh draft owner for both SwiftUI updates and synchronous Send handoff.
        private func composerEditor(owner: UUID) -> ComposerEditor {
            ComposerEditor(
                text: Binding(
                    get: { runtime.ownedDraft(id: owner)?.text ?? "" },
                    set: { value in
                        guard var draft = runtime.ownedDraft(id: owner) else { return }
                        draft.text = value; _ = runtime.updateOwnedDraft(draft)
                    }), focused: $composerFocused, ownerID: owner, accountGeneration: runtime.scope.generation,
                isCurrentOwner: {
                    runtime.scope == account.scope && runtime.snapshot.draft.id == owner
                },
                isCurrentPresentation: { currentProjection && !menuState.occluded },
                placeholder: MapleStrings.text("Message Maple"),
                isCompact: !composerExpanded,
                externalSelection: environment.dictation?.editorState(runtime: runtime, draftID: owner),
                onChange: { state, edits in
                    environment.dictation?.updateEditor(
                        runtime: runtime, draftID: owner, state: state, edits: edits)
                },
                onEditingInteraction: {
                    guard runtime.scope == account.scope,
                        runtime.snapshot.draft.id == owner, currentProjection, !menuState.occluded
                    else {
                        #if DEBUG
                            ComposerFocusTrace.record("shipping.interactionRejected")
                        #endif
                        return false
                    }
                    softwareKeyboardVisible = nil
                    #if DEBUG
                        ComposerFocusTrace.record("shipping.interactionAdmitted; focused=\(composerFocused)")
                    #endif
                    return true
                },
                refreshAfterSubmit: {
                    guard environment.session.account?.scope == runtime.scope else { return nil }
                    return self.composerEditor(owner: runtime.snapshot.draft.id)
                },
                onSubmit: {
                    guard runtime.snapshot.draft.id == owner else { return }
                    send()
                }
            )
        }

        @ViewBuilder private var attachmentButton: some View {
            if let importer = environment.imageImporters[ObjectIdentifier(runtime)] {
                ImageAttachmentMenu(
                    importer: importer, access: account.imageAccess,
                    onUnavailable: { mediaNotice = .images })
            }
        }
        private var modelButton: some View {
            Button {
                environment.dictation?.stopCaptureForNavigation()
                composerFocused = false
                modelPresented = true
            } label: {
                Image(systemName: "slider.horizontal.3")
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .accessibilityLabel(Text("Chat options", tableName: "Settings", bundle: .module))
            .accessibilityIdentifier("chat.model")
        }
        @ViewBuilder private var microphoneButton: some View {
            if let dictation = environment.dictation {
                if account.transcriptionAccess == .allowed || dictation.status(for: runtime)?.phase == .recording {
                    DictationMicrophoneButton(coordinator: dictation, runtime: runtime, isEnabled: true)
                        .labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                } else {
                    Button {
                        mediaNotice = .dictation
                    } label: {
                        Label(MapleStrings.text("Dictate"), systemImage: "mic")
                            .frame(minWidth: 44, minHeight: 44)
                            .contentShape(Rectangle())
                    }
                    .labelStyle(.iconOnly)
                    .accessibilityValue(MapleStrings.text("Unavailable"))
                    .accessibilityHint(MapleStrings.text("Learn why this feature is unavailable."))
                    .accessibilityIdentifier("dictation.start")
                }
            }
        }
        @ViewBuilder private var primaryAction: some View {
            if !runtime.isEditingQueue || runtime.hasUnresolvedRun {
                Button {
                    if runtime.hasUnresolvedRun { runtime.stop() } else { send() }
                } label: {
                    Label(
                        runtime.hasUnresolvedRun ? MapleStrings.text("Stop") : MapleStrings.text("Send"),
                        systemImage: runtime.hasUnresolvedRun ? "stop.fill" : "arrow.up"
                    )
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
                }
                .labelStyle(.iconOnly).buttonStyle(MaplePrimaryButtonStyle(compact: true))
                .disabled(!runtime.hasUnresolvedRun && (!runtime.canSend || account.chatAccess != .allowed))
                .accessibilityIdentifier(runtime.hasUnresolvedRun ? "chat.stop" : "chat.send")
            }
        }

        private func send() {
            guard account.chatAccess == .allowed else { return }
            if environment.send(
                runtime,
                options: ChatSendOptions(
                    model: account.selectedModelID, webSearch: account.effectiveWebSearch,
                    projectID: environment.selectedProjectID))
            {
                composerFocused = false
            }
        }

    }

    private struct TranscriptItem: View {
        let item: ConversationItem
        let toolResult: ConversationItem?
        let generation: UUID
        let revealDelay: Double
        let theme: MessageRenderTheme
        let font: MessageRenderFont
        let isReplyReady: Bool
        let speechAccess: AccountAccess
        let onReadAloud: () -> Void
        let onSpeechUnavailable: () -> Void
        let onOpenLink: (URL) -> Void
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @State private var appeared = false
        private var text: String { (item.content ?? []).compactMap(\.text).joined(separator: "\n\n") }
        private var readableContent: Bool {
            !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                || (item.content ?? []).contains(where: { $0.imageUrl != nil })
        }
        private var revealsContent: Bool {
            ["user", "assistant"].contains(item.role ?? "")
                && !ChatTranscriptRow.isTool(item)
        }

        private var isUserMessage: Bool { item.role == "user" && item.type == "message" }

        var body: some View {
            HStack(alignment: .top, spacing: 0) {
                if isUserMessage { Spacer(minLength: 36) }
                VStack(alignment: isUserMessage ? .trailing : .leading, spacing: 8) {
                    if isUserMessage {
                        messageContent
                            .padding(12)
                            .background(MapleSurface.card, in: RoundedRectangle(cornerRadius: 18))
                            .overlay {
                                RoundedRectangle(cornerRadius: 18)
                                    .strokeBorder(MapleSurface.border.opacity(0.3), lineWidth: 0.5)
                            }
                            .accessibilityElement(children: .contain)
                            .accessibilityLabel(Text("You", bundle: .module))
                            .accessibilityIdentifier("message.bubble." + item.id)
                    } else {
                        messageContent
                    }
                    if item.type != "reasoning", !ChatTranscriptRow.isTool(item), !text.isEmpty {
                        messageActions
                    }
                }
                .frame(maxWidth: isUserMessage ? 650 : .infinity, alignment: isUserMessage ? .trailing : .leading)
            }
            .frame(maxWidth: .infinity, alignment: isUserMessage ? .trailing : .leading)
            .opacity(revealsContent && !appeared && !reduceMotion ? 0 : 1)
            .onAppear { revealContent() }
            .onChange(of: readableContent) { _, _ in revealContent() }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier((item.role == "assistant" ? "assistant.message." : "message.") + item.id)
        }

        private var messageContent: some View {
            VStack(alignment: .leading, spacing: 8) {
                if (item.content ?? []).contains(where: { $0.imageUrl != nil }) {
                    MessageImages(content: item.content ?? [])
                }
                if item.type == "reasoning" {
                    DisclosureGroup(MapleStrings.text("Thinking")) {
                        MessageView(
                            text: text, theme: theme, font: font, accountGeneration: generation, onOpenLink: onOpenLink)
                    }
                } else if ChatTranscriptRow.isTool(item) {
                    ToolActivityView(
                        item: item, result: toolResult, theme: theme, font: font, generation: generation,
                        onOpenLink: onOpenLink)
                } else if !text.isEmpty {
                    MessageView(
                        text: text, theme: theme, font: font, accountGeneration: generation, onOpenLink: onOpenLink)
                }
            }
        }

        private var messageActions: some View {
            HStack(spacing: 8) {
                Button(MapleStrings.text("Copy message"), systemImage: "doc.on.doc") {
                    UIPasteboard.general.setItems(
                        [[UTType.utf8PlainText.identifier: text]],
                        options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
                }.labelStyle(.iconOnly).buttonStyle(.borderless).frame(minWidth: 44, minHeight: 44)
                    .accessibilityIdentifier("message.copy." + item.id)
                if item.role == "assistant" {
                    Button(
                        MapleStrings.text("Read reply aloud"), systemImage: "speaker.wave.2",
                        action: speechAccess == .allowed ? onReadAloud : onSpeechUnavailable
                    )
                    .labelStyle(.iconOnly).buttonStyle(.borderless).frame(minWidth: 44, minHeight: 44)
                    .disabled(!isReplyReady)
                    .accessibilityValue(speechAccess == .allowed ? "" : MapleStrings.text("Unavailable"))
                    .accessibilityHint(
                        !isReplyReady
                            ? MapleStrings.text("Available after this reply finishes.")
                            : speechAccess == .allowed
                                ? ""
                                : MapleStrings.text("Learn why this feature is unavailable.")
                    )
                    .accessibilityIdentifier("message.speak." + item.id)
                }
            }
        }
        private func revealContent() {
            guard !appeared, readableContent else { return }
            if revealsContent, !reduceMotion {
                withAnimation(.easeOut(duration: 0.18).delay(revealDelay)) { appeared = true }
            } else {
                appeared = true
            }
        }
    }

    private struct ToolActivityView: View {
        let item: ConversationItem
        let result: ConversationItem?
        let theme: MessageRenderTheme
        let font: MessageRenderFont
        let generation: UUID
        let onOpenLink: (URL) -> Void
        @State private var expanded = false
        @State private var details = ""
        @State private var detailsRevision = UUID()
        private var failed: Bool {
            [item.status, result?.status].contains(where: { ["failed", "error"].contains($0 ?? "") })
        }
        var body: some View {
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    expanded.toggle()
                } label: {
                    HStack(spacing: 10) {
                        Image(
                            systemName: failed ? "exclamationmark.circle" : item.name == "open_urls" ? "link" : "globe"
                        )
                        .foregroundStyle(failed ? AnyShapeStyle(MapleRedText()) : AnyShapeStyle(MapleAccent.foreground))
                        Text(title).foregroundStyle(.primary)
                        Spacer(minLength: 8)
                        Image(systemName: expanded ? "chevron.down" : "chevron.right")
                            .font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    }
                    .font(.footnote.weight(.medium))
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityValue(MapleStrings.text(expanded ? "Expanded" : "Collapsed"))
                .accessibilityIdentifier("tool.activity." + item.id)
                if expanded {
                    Group {
                        if !details.isEmpty {
                            MessageView(
                                text: details, theme: theme, font: font, accountGeneration: generation,
                                onOpenLink: onOpenLink)
                        } else {
                            Text(MapleStrings.text("No tool details available.")).foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("tool.details." + item.id)
                }
            }
            .overlay(alignment: .bottom) {
                Rectangle().fill(MapleSurface.border.opacity(0.25)).frame(height: 0.5)
            }
            .onChange(of: failed, initial: true) { _, value in if value { expanded = true } }
            .onChange(of: item) { _, _ in detailsRevision = UUID() }
            .onChange(of: result) { _, _ in detailsRevision = UUID() }
            .task(id: detailsRevision) {
                let arguments = item.arguments, output = item.output, result = result
                let work = Task.detached(priority: .userInitiated) {
                    let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
                    var details = [arguments, output, result?.arguments, result?.output].compactMap {
                        value -> String? in
                        guard let value, let bytes = try? encoder.encode(value), bytes.count <= 1_048_576,
                            let text = String(data: bytes, encoding: .utf8)
                        else { return nil }
                        return text
                    }
                    if let content = result?.content, !content.isEmpty,
                        let bytes = try? encoder.encode(content), bytes.count <= 1_048_576,
                        let text = String(data: bytes, encoding: .utf8)
                    {
                        details.append(text)
                    }
                    return details.joined(separator: "\n\n")
                }
                let value = await work.value
                if !Task.isCancelled { details = value }
            }
        }
        private var title: String {
            let search = item.name == "web_search" || item.type == "web_search_call"
            if failed { return MapleStrings.text(search ? "Search failed" : "Tool failed") }
            if item.name == "open_urls" { return MapleStrings.text("Opened pages") }
            if search {
                return MapleStrings.text(result != nil || item.status == "completed" ? "Search results" : "Web search")
            }
            return MapleStrings.text("Tool results")
        }
    }

    /// Observe only this composer's window. Focus and account admission remain
    /// with ChatDetail; this adapter reports geometry without changing input.
    private struct ComposerKeyboardVisibility: UIViewRepresentable {
        let eligible: Bool
        let changed: (Bool) -> Void
        func makeUIView(context: Context) -> KeyboardVisibilityView { KeyboardVisibilityView() }
        func updateUIView(_ view: KeyboardVisibilityView, context: Context) {
            view.eligible = eligible; view.changed = changed
        }
        static func dismantleUIView(_ view: KeyboardVisibilityView, coordinator: ()) { view.invalidate() }
    }

    private final class KeyboardVisibilityView: UIView {
        var eligible = false
        var changed: ((Bool) -> Void)?
        override init(frame: CGRect) {
            super.init(frame: frame)
            isUserInteractionEnabled = false; isAccessibilityElement = false
            NotificationCenter.default.addObserver(
                self, selector: #selector(keyboardChanged), name: UIResponder.keyboardDidChangeFrameNotification,
                object: nil)
            NotificationCenter.default.addObserver(
                self, selector: #selector(keyboardHidden), name: UIResponder.keyboardDidHideNotification, object: nil)
        }
        convenience init() { self.init(frame: .zero) }
        required init?(coder: NSCoder) { nil }
        func invalidate() {
            eligible = false; changed = nil; NotificationCenter.default.removeObserver(self)
        }
        @objc private func keyboardChanged(_ notification: Notification) {
            guard eligible, let window, let scene = window.windowScene, scene.activationState == .foregroundActive,
                notification.userInfo?[UIResponder.keyboardIsLocalUserInfoKey] as? Bool != false,
                let frame = (notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? NSValue)?.cgRectValue,
                frame.origin.x.isFinite, frame.origin.y.isFinite, frame.width.isFinite, frame.height.isFinite
            else { return }
            let overlap = window.bounds.intersection(window.convert(frame, from: scene.screen.coordinateSpace))
            changed?(!overlap.isNull && overlap.width > 0 && overlap.height > 0)
        }
        @objc private func keyboardHidden(_ notification: Notification) {
            guard eligible, let window, window.windowScene?.activationState == .foregroundActive,
                notification.userInfo?[UIResponder.keyboardIsLocalUserInfoKey] as? Bool != false
            else { return }
            changed?(false)
        }
    }
#endif
