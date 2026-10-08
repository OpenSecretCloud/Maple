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
                    chat: NavigationStack {
                        detail.safeAreaInset(edge: .bottom) { audioPlayer }
                    }
                    .environment(\.layoutDirection, layoutDirection)
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
                    chatShown: {
                        guard environment.session.account?.scope == scope,
                            environment.libraryContext?.scope == scope, !environment.menuPresented
                        else { return }
                        environment.finishNavigationDismissal()
                        menuButtonFocused = environment.accountFlow == nil && environment.session.account != nil
                    }
                )
                .id(scope)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .ignoresSafeArea(.container)
            } else {
                ProgressView(MapleStrings.text("Opening chat…"))
            }
        }

        @ViewBuilder private var audioPlayer: some View {
            if let listening = environment.listening { CompactReplyPlayer(session: listening).padding(.horizontal, 8) }
        }

        private var detail: some View {
            Group {
                if let chat = environment.selectedChat, let details = environment.entitlements,
                    let preferences = environment.preferences
                {
                    ChatDetail(environment: environment, runtime: chat, account: details, preferences: preferences)
                        .id(chat.snapshot.storageOwner)
                } else {
                    ProgressView(MapleStrings.text("Opening chat…"))
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
            .navigationTitle(chatTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar(removing: .sidebarToggle)
            .toolbar {
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
                    Button {
                        guard let scope = environment.session.account?.scope else { return }
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
            }
        }

        @ViewBuilder private func library(wide: Bool) -> some View {
            if let context = environment.libraryContext {
                LibraryView(
                    library: context.root, makeLibrary: context.makeLibrary,
                    makeInstructionEditor: context.makeInstructionEditor,
                    onSelectChat: { id in
                        guard context.scope == environment.session.account?.scope else { return }
                        environment.menuPresented = false
                        Task {
                            guard context.scope == environment.session.account?.scope else { return }
                            await environment.selectChat(id)
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
                ProgressView(MapleStrings.text("Opening menu…"))
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
                !isCurrent() || lease.chatAccessibilityHidden(scope: scope, storageOwner: storageOwner))
        }
    }

    struct ChatDetail: View {
        @Bindable var environment: AppEnvironment
        @Environment(\.layoutDirection) private var layoutDirection
        @Environment(\.compactMenuChatState) private var compactMenuState
        @Bindable var runtime: ChatRuntime
        @Bindable var account: AccountEntitlements
        @Bindable var preferences: PreferencesController
        #if DEBUG
            var fixtureProjectionIsCurrent: (@MainActor @Sendable (AccountScope, UUID) -> Bool)? = nil
        #endif
        @State private var queuePresented = false
        @State private var modelPresented = false
        @State private var projectPresented = false
        @State private var externalLink: URL?
        @State private var browserPresentationID = UUID()
        @State private var mediaNotice: MediaFeature?
        @State private var composerFocused = false
        @State private var readingPosition = ScrollPosition(idType: String.self)
        @State private var turnFrames: [String: TranscriptTurnGeometry] = [:]
        @State private var readingAnchor: ReadingAnchor?
        @State private var scrollMetrics = TranscriptScrollMetrics(returnRevision: 0)
        @State private var correctionTask: Task<Void, Never>?
        @State private var acceptedTurnScrollTask: Task<Void, Never>?
        @State private var pendingScrollMessage: String?
        @State private var pendingScrollToken = UUID()
        @State private var transcriptSpace = UUID()

        private struct ReadingAnchor {
            let token = UUID()
            let turnID: String
            let viewportY: CGFloat
            var restoring = false
            var corrections = 0
        }
        private struct TranscriptScrollMetrics: Equatable {
            var offset: CGFloat = 0
            var topInset: CGFloat = 0
            var height: CGFloat = 0
            let returnRevision: Int
        }
        private struct TranscriptTurnGeometry: Equatable {
            let contentFrame: CGRect
            let firstMessageID: String?
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
                withTransaction(transaction) { readingPosition.scrollTo(y: point) }
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
            var items: [ConversationItem]
        }
        private var turns: [Turn] {
            var result: [Turn] = []
            for item in runtime.messages {
                if item.role == "user" || result.isEmpty {
                    result.append(Turn(id: item.id, items: [item]))
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
                                if runtime.messages.isEmpty {
                                    VStack(alignment: .leading, spacing: 12) {
                                        Text(MapleStrings.text("Research anything..."))
                                            .font(MapleTypography.display)
                                            .foregroundStyle(MapleSurface.displayText)
                                        Text(MapleStrings.text("Encrypted and private at every step")).foregroundStyle(
                                            .secondary)
                                    }
                                    .accessibilityElement(children: .combine)
                                }
                                ForEach(turns) { turn in
                                    VStack(alignment: .leading, spacing: 24) {
                                        ForEach(turn.items, id: \.id) { item in
                                            TranscriptItem(
                                                item: item, generation: runtime.scope.generation,
                                                theme: preferences.value.theme.messageRenderTheme,
                                                font: preferences.value.chatFont.messageRenderFont,
                                                isReplyReady: item.status == "completed" || !runtime.hasUnresolvedRun,
                                                speechAccess: account.speechAccess,
                                                onReadAloud: { environment.readAloud(item, runtime: runtime) },
                                                onSpeechUnavailable: { mediaNotice = .speech }
                                            ) { url in
                                                externalLink = url
                                            }
                                        }
                                        if turn.id == turns.last?.id, runtime.hasUnresolvedRun {
                                            HStack(spacing: 8) {
                                                ProgressView()
                                                Text(runLabel).foregroundStyle(.secondary)
                                            }.font(.footnote).accessibilityIdentifier("chat.progress")
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
                                            returnRevision = menuState.returnRevision
                                        ] in
                                        TranscriptTurnGeometry(
                                            contentFrame: $0.frame(in: .named(transcriptSpace)),
                                            firstMessageID: firstMessageID, returnRevision: returnRevision)
                                    } action: { sample in
                                        guard !menuState.occluded, sample.firstMessageID == runtime.messages.first?.id,
                                            sample.returnRevision == menuState.returnRevision,
                                            turnFrames[turn.id] != sample
                                        else { return }
                                        turnFrames[turn.id] = sample
                                        scheduleAnchorCorrection()
                                    }
                                    .onDisappear {
                                        if !menuState.occluded, turnFrames[turn.id] != nil {
                                            turnFrames.removeValue(forKey: turn.id)
                                        }
                                    }
                                }
                            }
                            .scrollTargetLayout()
                            .padding(.horizontal, 20).padding(.vertical, 20)
                            .frame(maxWidth: 850).frame(maxWidth: .infinity)
                            .coordinateSpace(name: transcriptSpace)
                        }
                        .scrollDismissesKeyboard(.interactively)
                        .scrollPosition($readingPosition)
                        .onScrollGeometryChange(for: TranscriptScrollMetrics.self) {
                            [returnRevision = menuState.returnRevision] geometry in
                            TranscriptScrollMetrics(
                                offset: geometry.contentOffset.y, topInset: geometry.contentInsets.top,
                                height: geometry.containerSize.height, returnRevision: returnRevision)
                        } action: { _, metrics in
                            guard !menuState.occluded, metrics.returnRevision == menuState.returnRevision else {
                                return
                            }
                            scrollMetrics = metrics
                            scheduleAnchorCorrection()
                        }
                        .onScrollPhaseChange { _, phase in
                            if !menuState.occluded, phase == .tracking || phase == .interacting {
                                readingAnchor = nil; correctionTask?.cancel()
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
                        .onAppear { resumeReadingAfterMenu(reader: reader) }
                    }
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { composer(maximumHeight: available.size.height) }
            }
            .onDisappear {
                correctionTask?.cancel(); correctionTask = nil
                acceptedTurnScrollTask?.cancel(); acceptedTurnScrollTask = nil
                if !menuState.occluded { readingAnchor = nil; pendingScrollMessage = nil }
            }
            .environment(\.retainedMessageRendering) {
                [
                    weak environment, weak runtime, weak account, scope = runtime.scope,
                    owner = runtime.snapshot.storageOwner, state = compactMenuState
                ] in
                guard let environment, let runtime, let account,
                    environment.session.account?.scope == scope, account.scope == scope,
                    runtime.scope == scope, runtime.snapshot.storageOwner == owner,
                    environment.selectedChat?.snapshot.storageOwner == owner
                else { return false }
                return state(scope, owner).occluded
            }
            .tracksChatPresentation(queuePresented || modelPresented || projectPresented || mediaNotice != nil)
            .environment(\.chatPresentationChanged) { id, presented in
                environment.setPresentationBlocked(presented, id: id, scope: runtime.scope)
            }
            .onChange(of: environment.menuPresented) { _, presented in
                if presented { composerFocused = false }
            }
            .onChange(of: menuState.occluded) { _, occluded in
                if occluded {
                    composerFocused = false; correctionTask?.cancel(); acceptedTurnScrollTask?.cancel()
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
                NavigationStack {
                    SettingsModelPicker(
                        entitlements: account,
                        webSearchPreference: Binding(
                            get: { preferences.value.webSearchPreferred },
                            set: { preferences.setWebSearchPreferred($0) }),
                        webSearchPreferences: preferences
                    ) { id in
                        preferences.setSelectedModelID(id); modelPresented = false
                    }
                    .navigationTitle(MapleStrings.text("Model"))
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button(MapleStrings.text("Done")) { modelPresented = false }
                        }
                    }
                }.presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $projectPresented) {
                NavigationStack {
                    List {
                        Button(MapleStrings.text("No project")) {
                            environment.selectedProjectID = nil; projectPresented = false
                        }
                        ForEach(environment.library?.projects ?? [], id: \.id) { project in
                            Button(project.name) {
                                environment.selectedProjectID = project.id; projectPresented = false
                            }
                        }
                        if environment.library?.isLoadingProjects == true { ProgressView() }
                        if let issue = environment.library?.issue { LibraryIssueView(issue: issue) }
                    }
                    .navigationTitle(MapleStrings.text("Project"))
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button(MapleStrings.text("Done")) { projectPresented = false }
                        }
                    }
                    .task { await environment.library?.loadProjects() }
                }.presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
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
            let padding: CGFloat = 12, bottomPadding: CGFloat = 8
            return ComposerVerticalLayout(
                maximumHeight: max(0, maximumHeight - padding * 2 - bottomPadding),
                layoutDirection: layoutDirection
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
                    if importer.importingDrafts.contains(owner) { ProgressView(MapleStrings.text("Preparing images…")) }
                    if let issue = importer.issue { ImageImportIssueView(issue: issue) }
                }
                if let dictation = environment.dictation { DictationControls(coordinator: dictation, runtime: runtime) }
                if runtime.isLoading {
                    ProgressView(MapleStrings.text("Opening chat…"))
                        .frame(maxWidth: .infinity).padding(12)
                        .accessibilityIdentifier("chat.loading")
                } else {
                    composerEditor(owner: owner)
                        .padding(12).background(MapleSurface.canvas, in: RoundedRectangle(cornerRadius: 16))
                        .overlay {
                            RoundedRectangle(cornerRadius: 16)
                                .stroke(composerFocused ? MapleAccent.foreground : MapleSurface.border, lineWidth: 1)
                                .allowsHitTesting(false)
                        }
                        .layoutValue(key: ComposerEditorLayoutKey.self, value: true)
                }
                composerControls
            }
            // Keep the trailing controls clear of the keyboard's prediction strip.
            .padding(padding).padding(.bottom, bottomPadding).frame(maxWidth: 874).frame(maxWidth: .infinity)
            .background(MapleSurface.card)
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
                placeholder: MapleStrings.text("Message Maple"),
                externalSelection: environment.dictation?.editorState(runtime: runtime, draftID: owner),
                onChange: { state, edits in
                    environment.dictation?.updateEditor(
                        runtime: runtime, draftID: owner, state: state, edits: edits)
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

        /// Layout moves the existing controls without creating alternative owners.
        private var composerControls: some View {
            ComposerControlFlow(layoutDirection: layoutDirection) {
                attachmentButton
                modelButton
                projectButton
                microphoneButton
                stopButton
                sendButton.layoutValue(key: ComposerTrailingControlKey.self, value: true)
            }.frame(maxWidth: .infinity, alignment: .leading)
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
                modelPresented = true
            } label: {
                Text(account.selectedModel?.shortName ?? MapleStrings.text("Model"))
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .accessibilityIdentifier("chat.model")
        }
        @ViewBuilder private var projectButton: some View {
            if runtime.conversationID == nil {
                Button(
                    MapleStrings.text("Project"),
                    systemImage: environment.selectedProjectID == nil ? "folder" : "folder.fill"
                ) {
                    projectPresented = true
                }
                .labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                .accessibilityIdentifier("chat.project")
            }
        }
        @ViewBuilder private var microphoneButton: some View {
            if let dictation = environment.dictation {
                if account.transcriptionAccess == .allowed {
                    DictationMicrophoneButton(coordinator: dictation, runtime: runtime, isEnabled: true)
                        .labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                } else {
                    Button {
                        mediaNotice = .dictation
                    } label: {
                        Label(MapleStrings.text("Dictate"), systemImage: "mic")
                    }
                    .labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                    .accessibilityValue(MapleStrings.text("Unavailable"))
                    .accessibilityHint(MapleStrings.text("Learn why this feature is unavailable."))
                    .accessibilityIdentifier("dictation.start")
                }
            }
        }
        @ViewBuilder private var stopButton: some View {
            if runtime.hasUnresolvedRun {
                Button(MapleStrings.text("Stop"), systemImage: "stop.fill") { runtime.stop() }
                    .buttonStyle(.bordered).frame(minWidth: 44, minHeight: 44)
                    .accessibilityIdentifier("chat.stop")
            }
        }
        @ViewBuilder private var sendButton: some View {
            if !runtime.isEditingQueue {
                Button {
                    send()
                } label: {
                    Label(
                        runtime.hasUnresolvedRun ? MapleStrings.text("Queue message") : MapleStrings.text("Send"),
                        systemImage: "arrow.up"
                    )
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
                }
                .labelStyle(.iconOnly).buttonStyle(MaplePrimaryButtonStyle(compact: true))
                .disabled(!runtime.canSend || account.chatAccess != .allowed)
                .accessibilityIdentifier("chat.send")
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
        private var runLabel: String {
            switch runtime.phase {
            case .preparing: MapleStrings.text("Preparing…")
            case .sending: MapleStrings.text("Sending…")
            case .streaming: MapleStrings.text("Responding…")
            case .recovering: MapleStrings.text("Checking response…")
            case .stopping: MapleStrings.text("Stopping…")
            case nil: ""
            }
        }

    }

    private struct TranscriptItem: View {
        let item: ConversationItem
        let generation: UUID
        let theme: MessageRenderTheme
        let font: MessageRenderFont
        let isReplyReady: Bool
        let speechAccess: AccountAccess
        let onReadAloud: () -> Void
        let onSpeechUnavailable: () -> Void
        let onOpenLink: (URL) -> Void
        private var text: String { (item.content ?? []).compactMap(\.text).joined(separator: "\n\n") }

        var body: some View {
            VStack(alignment: .leading, spacing: 8) {
                if item.role == "user" {
                    Text(MapleStrings.text("You")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                }
                if (item.content ?? []).contains(where: { $0.imageUrl != nil }) {
                    MessageImages(content: item.content ?? [])
                }
                if item.type == "reasoning" {
                    DisclosureGroup(MapleStrings.text("Thinking")) {
                        MessageView(
                            text: text, theme: theme, font: font, accountGeneration: generation, onOpenLink: onOpenLink)
                    }
                } else if item.type == "function_call" || item.type == "web_search_call"
                    || item.type == "function_call_output"
                {
                    ToolActivityView(
                        item: item, theme: theme, font: font, generation: generation, onOpenLink: onOpenLink)
                } else if !text.isEmpty {
                    MessageView(
                        text: text, theme: theme, font: font, accountGeneration: generation, onOpenLink: onOpenLink)
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
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier((item.role == "assistant" ? "assistant.message." : "message.") + item.id)
        }
    }

    private struct ToolActivityView: View {
        let item: ConversationItem
        let theme: MessageRenderTheme
        let font: MessageRenderFont
        let generation: UUID
        let onOpenLink: (URL) -> Void
        @State private var expanded = false
        @State private var details = ""
        @State private var detailsRevision = UUID()
        private var failed: Bool { ["failed", "error"].contains(item.status ?? "") }
        var body: some View {
            DisclosureGroup(isExpanded: $expanded) {
                if !details.isEmpty {
                    MessageView(
                        text: details, theme: theme, font: font, accountGeneration: generation, onOpenLink: onOpenLink)
                } else {
                    Text(MapleStrings.text("No tool details available.")).foregroundStyle(.secondary)
                }
            } label: {
                HStack {
                    Label(
                        item.name ?? MapleStrings.text("Web tool"),
                        systemImage: failed ? "exclamationmark.circle" : "globe")
                    Spacer()
                    Text(statusLabel).font(.caption).foregroundStyle(failed ? .red : .secondary)
                }
            }
            .padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
            .onChange(of: failed, initial: true) { _, value in if value { expanded = true } }
            .onChange(of: item) { _, _ in detailsRevision = UUID() }
            .task(id: detailsRevision) {
                let arguments = item.arguments, output = item.output
                let work = Task.detached(priority: .userInitiated) {
                    let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
                    return [arguments, output].compactMap { value -> String? in
                        guard let value, let bytes = try? encoder.encode(value), bytes.count <= 1_048_576,
                            let text = String(data: bytes, encoding: .utf8)
                        else { return nil }
                        return text
                    }.joined(separator: "\n\n")
                }
                let value = await work.value
                if !Task.isCancelled { details = value }
            }
        }
        private var statusLabel: String {
            switch item.status {
            case "completed": MapleStrings.text("Completed")
            case "failed", "error": MapleStrings.text("Failed")
            case "incomplete", "cancelled": MapleStrings.text("Incomplete")
            default: MapleStrings.text("Working…")
            }
        }
    }
#endif
