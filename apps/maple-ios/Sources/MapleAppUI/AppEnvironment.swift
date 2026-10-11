#if os(iOS)
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Network
    import Observation
    import UIKit

    @MainActor @Observable final class AppEnvironment {
        let configuration: AppConfiguration
        @ObservationIgnored private let accountLinkInbox: AccountLinkInbox
        let client: MapleClient
        let storage: ProtectedChatStore
        let projectExpansionStore: LibraryProjectExpansionStore
        let speechFiles: TemporarySpeechStore
        let session: SessionController
        let signIn: NativeSignInCoordinator
        let preferenceStore = PreferencesStore()
        private(set) var chats: [String: ChatRuntime] = [:]
        private(set) var selectedKey = ChatSnapshot.newChatKey { didSet { updatePresentationVisibility() } }
        private(set) var accountAPI: ResearchAPI?
        private(set) var entitlements: AccountEntitlements?
        private(set) var deletion: ChatDeletionCoordinator?
        private(set) var preferences: PreferencesController?
        private(set) var projectExpansion: LibraryProjectExpansionController?
        private(set) var library: LibraryModel?
        private(set) var libraryContext: LibraryContext?
        private(set) var readyScope: AccountScope?
        private(set) var conversationDetails: [String: Conversation] = [:]
        private var conversationDetailTickets: [String: UUID] = [:]
        var accountFlow: AccountFlow? { didSet { updatePresentationVisibility() } }
        var menuPresented = false {
            didSet {
                if menuPresented { dictation?.stopCaptureForNavigation() }
                updatePresentationVisibility()
            }
        }
        var settingsPresented = false { didSet { updatePresentationVisibility() } }
        private(set) var listening: ReplyListeningSession?
        private(set) var dictation: DictationCoordinator?
        private let imageProcessor = ImageImportProcessor()
        private let imageBudget = ImageImportBudget()
        private(set) var imageImporters: [ObjectIdentifier: ImageImportController] = [:]
        private var audioRetirements: [ReplyListeningSession] = []
        private var dictationSend = DictationSendAssociation()
        private var pendingAccountFlow: AccountFlow?
        private var presentationBlocks: Set<UUID> = []
        var chatNavigationBlocked: Bool {
            !foreground || accountFlow != nil || settingsPresented || !presentationBlocks.isEmpty
        }
        private var projectSelections: [UUID: String] = [:]
        private(set) var isOnline = true
        private(set) var issue: ChatIssue?
        private var retainedSnapshots: [ChatSnapshot] = []
        private var navigation = UUID()
        private var activatingScope: AccountScope?
        private(set) var activationFailedScope: AccountScope?
        private var accountCeremonies: [AuthCeremonies] = []
        var foreground = true {
            didSet {
                if !foreground {
                    dictation?.stopCaptureForNavigation()
                    let projectExpansion = self.projectExpansion
                    Task { await projectExpansion?.flush() }
                }
                for runtime in chats.values { runtime.isForeground = foreground }
                updatePresentationVisibility()
                if foreground && !oldValue { reconcileAfterForeground() }
            }
        }
        #if DEBUG
            private(set) var lifecycleFixture: LocalLifecycleFixtureDriver?
        #endif
        @ObservationIgnored private let network = NWPathMonitor()

        init(configuration: AppConfiguration, accountLinkInbox: AccountLinkInbox = AccountLinkInbox()) throws {
            self.configuration = configuration
            self.accountLinkInbox = accountLinkInbox
            client = try MapleClient(
                apiURL: configuration.apiURL.absoluteString, clientID: configuration.clientID,
                policy: configuration.trustPolicy,
                store: KeychainSecureStore(service: "cloud.opensecret.maple.native.dev.sdk"))
            let support = try FileManager.default.url(
                for: .applicationSupportDirectory, in: .userDomainMask,
                appropriateFor: nil, create: true)
            storage = ProtectedChatStore(
                root: support.appendingPathComponent("ProtectedDrafts", isDirectory: true),
                protectionAvailable: UIApplication.shared.isProtectedDataAvailable)
            projectExpansionStore = LibraryProjectExpansionStore(
                root: support.appendingPathComponent("LibraryPresentation", isDirectory: true),
                protectionAvailable: UIApplication.shared.isProtectedDataAvailable)
            speechFiles = TemporarySpeechStore(
                root: support.appendingPathComponent("TemporarySpeech", isDirectory: true))
            session = SessionController(
                service: SDKSessionService(client: client),
                storage: NativeAccountDataLifecycle(
                    drafts: storage, speech: speechFiles, projectExpansion: projectExpansionStore))
            signIn = NativeSignInCoordinator(
                session: session,
                ceremonies: AuthCeremonies(
                    service: SDKAuthCeremonyService(client: client), configuration: configuration))
            session.onCancelExternal = { [weak self] in
                self?.accountLinkInbox.retire()
                await self?.signIn.cancelPresentation()
            }
            session.onRetire = { [weak self] scope in
                guard let self else { return }
                for ceremony in self.accountCeremonies { ceremony.invalidate() }
                self.accountCeremonies.removeAll()
                self.accountFlow?.ceremonies.invalidate(); self.accountFlow = nil
                self.pendingAccountFlow?.ceremonies.invalidate(); self.pendingAccountFlow = nil
                self.presentationBlocks.removeAll()
                self.menuPresented = false; self.settingsPresented = false; self.projectSelections = [:]
                self.deletion?.invalidate(); self.deletion = nil
                self.libraryContext?.invalidate(); self.libraryContext = nil; self.library = nil
                self.readyScope = nil
                let projectExpansion = self.projectExpansion; self.projectExpansion = nil
                projectExpansion?.invalidate()
                self.conversationDetails = [:]
                self.conversationDetailTickets = [:]
                self.dictation?.invalidate(); self.dictation = nil
                self.dictationSend.reset()
                #if DEBUG
                    self.lifecycleFixture?.listeningProof?.retire()
                #endif
                for importer in self.imageImporters.values { importer.invalidate() }
                self.imageImporters.removeAll()
                for runtime in self.chats.values where runtime.scope == scope { runtime.invalidate() }
                self.chats.removeAll(); self.accountAPI = nil; self.selectedKey = ChatSnapshot.newChatKey
                self.retainedSnapshots = []; self.navigation = UUID()
                self.activatingScope = nil
                self.activationFailedScope = nil
                let details = self.entitlements; self.entitlements = nil
                let preferences = self.preferences; self.preferences = nil
                if let listening = self.listening { self.audioRetirements.append(listening) }
                self.listening = nil
                var audioFailed = false
                for listening in self.audioRetirements {
                    do {
                        try await listening.invalidate()
                        self.audioRetirements.removeAll { $0 === listening }
                    } catch { audioFailed = true }
                }
                try? await preferences?.retire()
                try? await projectExpansion?.retire()
                await details?.retire()
                if audioFailed { throw ChatStorageError.cleanupRequired }
            }
            #if DEBUG
                if configuration.profile == .local,
                    (ProcessInfo.processInfo.arguments.contains("--maple-local-lifecycle-fixture")
                        || ProcessInfo.processInfo.arguments.contains("--maple-local-listening-fixture"))
                {
                    let fixture = LocalLifecycleFixtureDriver()
                    lifecycleFixture = fixture
                    fixture.onConnectivity = { [weak self] online in self?.applyConnectivity(online) }
                }
            #endif
            network.pathUpdateHandler = { [weak self] path in
                let online = path.status == .satisfied
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    #if DEBUG
                        guard self.lifecycleFixture == nil else { return }
                    #endif
                    self.applyConnectivity(online)
                }
            }
            network.start(queue: DispatchQueue(label: "cloud.opensecret.maple.connectivity"))
        }

        private func applyConnectivity(_ online: Bool) {
            isOnline = online
            for runtime in chats.values { runtime.isOnline = online }
        }

        var selectedChat: ChatRuntime? { chats[selectedKey] }

        func activate(_ account: AccountSession) async {
            guard session.phase == .signedIn, session.account?.scope == account.scope,
                accountAPI == nil, activatingScope != account.scope
            else { return }
            activatingScope = account.scope
            activationFailedScope = nil
            issue = nil
            defer { if activatingScope == account.scope { activatingScope = nil } }
            do {
                let api = try await client.accountAPI()
                guard session.account?.scope == account.scope else { return }
                let retained = try await storage.loadAll(scope: account.scope)
                guard session.account?.scope == account.scope else { return }
                retainedSnapshots = retained
                accountAPI = api
                let coordinator = ChatDeletionCoordinator(
                    scope: account.scope, retained: retained,
                    service: SDKChatService(api: api), persistence: storage)
                deletion = coordinator
                coordinator.onRemoved = { [weak self] removed in
                    guard let self, self.session.account?.scope == account.scope else { return }
                    let wasSelected = self.selectedChat === removed
                    self.imageImporters.removeValue(forKey: ObjectIdentifier(removed))?.invalidate()
                    for key in Array(self.chats.keys) where self.chats[key] === removed {
                        self.chats.removeValue(forKey: key)
                    }
                    if wasSelected {
                        self.selectedKey = ChatSnapshot.newChatKey
                        Task { await self.selectChat(nil) }
                    }
                }
                let preferences = PreferencesController(scope: account.scope, store: preferenceStore)
                self.preferences = preferences
                await preferences.load()
                guard session.account?.scope == account.scope else { return }
                let projectExpansion = LibraryProjectExpansionController(
                    scope: account.scope, store: projectExpansionStore)
                self.projectExpansion = projectExpansion
                await projectExpansion.load()
                guard session.account?.scope == account.scope else { return }
                let details = AccountEntitlements(
                    scope: account.scope, isGuest: account.user.loginMethod == "guest",
                    billing: BillingService(
                        scope: account.scope, configuration: configuration,
                        tokenSource: SDKBillingTokenSource(api: api)),
                    catalog: SDKModelCatalogSource(api: api),
                    selectedModelID: preferences.value.selectedModelID,
                    webSearchPreferred: preferences.value.webSearchPreferred)
                entitlements = details
                preferences.onChange = { [weak self, weak details] value in
                    guard self?.session.account?.scope == account.scope else { return }
                    details?.webSearchPreferred = value.webSearchPreferred
                    details?.selectModel(value.selectedModelID)
                    self?.listening?.updatePreferences(voice: value.voice, speed: value.playbackSpeed)
                }
                let admission: @MainActor @Sendable () -> SpeechIssue? = { [weak self, weak details] in
                    guard self?.session.account?.scope == account.scope else { return .retired }
                    switch details?.speechAccess {
                    case .allowed: return nil
                    case .paidPlanRequired, .freeGuest: return .paidPlanRequired
                    case .usageLimit: return .usageLimit
                    case .retired: return .retired
                    default: return .unavailable
                    }
                }
                let player = NativeSpeechPlayer()
                let preparer = SpeechPreparer(
                    synthesis: SDKSpeechSynthesizer(api: api), files: speechFiles,
                    inspector: NativeSpeechInspector(), admission: { await admission() })
                let listening = ReplyListeningSession(
                    scope: account.scope, preparer: preparer, player: player,
                    voice: preferences.value.voice, speed: preferences.value.playbackSpeed, admission: admission)
                player.bind(to: listening)
                listening.onSpeedChanged = { [weak preferences] speed in preferences?.setPlaybackSpeed(speed) }
                self.listening = listening
                #if DEBUG
                    if ProcessInfo.processInfo.arguments.contains("--maple-local-listening-fixture") {
                        lifecycleFixture?.listeningProof = LocalListeningProof(scope: account.scope, session: listening)
                    }
                #endif
                let capture: any DictationCapturing
                let transcription: any DictationTranscribing
                #if DEBUG
                    if let fixture = lifecycleFixture {
                        capture = fixture.capture; transcription = fixture.transcriber
                    } else {
                        capture = AVDictationCapture(); transcription = SDKDictationService(api: api)
                    }
                #else
                    capture = AVDictationCapture(); transcription = SDKDictationService(api: api)
                #endif
                let dictation = DictationCoordinator(
                    scope: account.scope, capture: capture, transcriber: transcription)
                dictation.onCaptureAdmitted = { [weak self] owner in
                    guard self?.session.account?.scope == owner.scope else { return }
                    self?.dictationSend.captureAdmitted(owner)
                }
                dictation.onActivityChange = { [weak self, weak listening] activity in
                    guard self?.session.account?.scope == account.scope else { return }
                    switch activity {
                    case .capturing:
                        listening?.startDictation()
                    case .transcribing: listening?.transcriptionStarted()
                    case .idle: listening?.dictationFinishedOrCancelled()
                    }
                }
                self.dictation = dictation
                let context = LibraryContext(
                    scope: account.scope, api: api, deletions: coordinator,
                    projectExpansion: projectExpansion)
                self.libraryContext = context; self.library = context.root
                context.onConversationsDeleted = { [weak self] ids, all in
                    guard let self, self.session.account?.scope == account.scope else { return }
                    self.retainedSnapshots.removeAll { all || $0.conversationID.map(ids.contains) == true }
                    self.conversationDetails = self.conversationDetails.filter { !all && !ids.contains($0.key) }
                    self.conversationDetailTickets = self.conversationDetailTickets.filter {
                        !all && !ids.contains($0.key)
                    }
                }
                context.onConversationUpdated = { [weak self] conversation in
                    guard let self, self.session.account?.scope == account.scope else { return }
                    self.conversationDetails[conversation.id] = conversation
                    self.conversationDetailTickets[conversation.id] = UUID()
                }
                context.onConversationProjectUpdated = { [weak self] ids, projectID in
                    guard let self, self.session.account?.scope == account.scope else { return }
                    for id in ids {
                        self.conversationDetailTickets[id] = UUID()
                        self.conversationDetails[id]?.projectId = projectID
                    }
                }
                context.onProjectDeleted = { [weak self] id in
                    guard let self, self.session.account?.scope == account.scope else { return }
                    self.projectSelections = self.projectSelections.filter { $0.value != id }
                }
                for snapshot in retained
                where snapshot.conversationID == nil && snapshot.storageKey.hasPrefix("pending:") {
                    let runtime = makeRuntime(account: account, api: api, id: nil)
                    if runtime.loadRecovered(snapshot) { chats[snapshot.storageKey] = runtime }
                }
                // Publish the interactive scene only after its first menu pages
                // and account presentation are ready. Later refreshes retain
                // these account-owned snapshots rather than rebuilding the menu.
                async let menuReady: Void = context.prepareMenu()
                async let detailsReady: Void = details.refresh()
                await selectChat(nil)
                _ = await (menuReady, detailsReady)
                guard session.account?.scope == account.scope, libraryContext === context,
                    !Task.isCancelled
                else { return }
                readyScope = account.scope
            } catch {
                guard session.account?.scope == account.scope else { return }
                issue = .signedOut
                activationFailedScope = account.scope
            }
        }

        @discardableResult
        func selectChat(_ id: String?, selectionTicket ticket: UUID = UUID()) async -> Bool {
            dictation?.stopCaptureForNavigation()
            guard let account = session.account, let api = accountAPI else { return false }
            if let id, deletion?.isConversationDeleted(id) == true {
                return await selectChat(nil, selectionTicket: ticket)
            }
            navigation = ticket
            let previous = selectedChat
            let key = id ?? ChatSnapshot.newChatKey
            if id == nil, let prior = chats[key], prior.snapshot.storageKey != key || prior.hasUnresolvedRun {
                guard await prior.prepareForNewChat(), navigation == ticket,
                    session.account?.scope == account.scope
                else { return false }
                if chats[key] === prior { chats.removeValue(forKey: key) }
                chats[prior.snapshot.storageKey] = prior
            }
            previous?.isVisible = false
            selectedKey = key
            if let existing = chats[key] {
                existing.isVisible = true
                updatePresentationVisibility()
                await existing.reconcileOnLifecycle(foreground: false)
                guard session.account?.scope == account.scope, navigation == ticket else { return false }
                if let id { await refreshConversationDetails(id, account: account, api: api) }
                return session.account?.scope == account.scope && navigation == ticket && selectedChat === existing
            }
            let runtime = makeRuntime(account: account, api: api, id: id)
            runtime.isVisible = true
            chats[key] = runtime
            updatePresentationVisibility()
            await runtime.load(chatID: id)
            if let id { await refreshConversationDetails(id, account: account, api: api) }
            return session.account?.scope == account.scope && navigation == ticket && selectedChat === runtime
        }

        var selectedProjectID: String? {
            get {
                guard let runtime = selectedChat else { return nil }
                return projectID(for: runtime)
            }
            set {
                guard let runtime = selectedChat, runtime.conversationID == nil else { return }
                projectSelections[runtime.snapshot.storageOwner] = newValue
            }
        }

        func projectID(for runtime: ChatRuntime) -> String? {
            guard session.account?.scope == runtime.scope, !runtime.isRetired else { return nil }
            if let id = runtime.conversationID {
                if let details = conversationDetails[id] { return details.projectId }
                if let context = libraryContext, context.scope == runtime.scope {
                    let cached =
                        [context.root, context.menu.pinned, context.menu.recent]
                        + Array(context.menu.projects.values)
                    for model in cached {
                        if let conversation = model.conversations.first(where: { $0.id == id }) {
                            return conversation.projectId
                        }
                    }
                }
            }
            return projectSelections[runtime.snapshot.storageOwner]
        }

        @discardableResult
        func selectProject(runtime: ChatRuntime, id: String?) async -> Bool {
            guard let scope = session.account?.scope, runtime.scope == scope,
                selectedChat === runtime, !runtime.isRetired,
                let context = libraryContext, context.scope == scope
            else { return false }
            let owner = runtime.snapshot.storageOwner
            if let conversationID = runtime.conversationID {
                guard deletion?.isConversationDeleted(conversationID) != true else { return false }
                guard await context.root.moveConversations(ids: [conversationID], projectID: id) else { return false }
                guard session.account?.scope == scope, libraryContext === context,
                    selectedChat === runtime, !runtime.isRetired,
                    runtime.snapshot.storageOwner == owner, runtime.conversationID == conversationID,
                    deletion?.isConversationDeleted(conversationID) != true, !Task.isCancelled
                else { return false }
                // The context callback updates every menu snapshot. A metadata
                // fetch may still be absent after offline navigation, so keep
                // this runtime's acknowledged destination available as well.
                projectSelections[owner] = id
                conversationDetailTickets[conversationID] = UUID()
                conversationDetails[conversationID]?.projectId = id
                return true
            }
            // Draft assignment has no suspension and belongs to this exact
            // runtime, independently of another chat's project selection.
            projectSelections[owner] = id
            return true
        }

        func selectNewChat(projectID: String? = nil) async {
            guard let scope = session.account?.scope else { return }
            await NewChatProjectSelection.perform(
                scope: scope, projectID: projectID,
                navigate: { ticket in await self.selectChat(nil, selectionTicket: ticket) },
                current: {
                    .init(
                        accountScope: self.session.account?.scope, navigation: self.navigation,
                        selectedKey: self.selectedKey, runtime: self.selectedChat)
                },
                assign: { runtime, project in self.projectSelections[runtime.snapshot.storageOwner] = project })
        }

        private func refreshConversationDetails(_ id: String, account: AccountSession, api: ResearchAPI) async {
            guard session.account?.scope == account.scope, deletion?.isConversationDeleted(id) != true else { return }
            let ticket = UUID()
            conversationDetailTickets[id] = ticket
            do {
                let conversation = try await api.getConversation(id: id)
                guard session.account?.scope == account.scope, conversation.id == id,
                    deletion?.isConversationDeleted(id) != true, conversationDetailTickets[id] == ticket
                else { return }
                conversationDetails[id] = conversation
                deletion?.updateProject(conversationID: id, projectID: conversation.projectId)
            } catch { /* History and drafts remain available when metadata cannot refresh. */  }
        }

        var unfinishedChats: [ChatRuntime] {
            chats.values.filter { $0.snapshot.storageKey.hasPrefix("pending:") && $0.conversationID == nil }
                .sorted { $0.snapshot.storageKey < $1.snapshot.storageKey }
        }

        func selectUnfinished(_ runtime: ChatRuntime) {
            guard session.account?.scope == runtime.scope else { return }
            dictation?.stopCaptureForNavigation()
            navigation = UUID(); selectedChat?.isVisible = false
            selectedKey = runtime.snapshot.storageKey; runtime.isVisible = true
            Task { await runtime.reconcileOnLifecycle(foreground: false) }
        }

        private func reconcileAfterForeground() {
            guard let scope = session.account?.scope else { return }
            let owners = Array(chats.values)
            Task { [weak self] in
                for runtime in owners {
                    guard let self, self.foreground, self.session.account?.scope == scope else { return }
                    guard runtime.scope == scope, self.chats.values.contains(where: { $0 === runtime }) else {
                        continue
                    }
                    await runtime.reconcileOnLifecycle(foreground: true)
                }
            }
        }

        private func makeRuntime(account: AccountSession, api: ResearchAPI, id: String?) -> ChatRuntime {
            let service: any ChatServing
            #if DEBUG
                if let proof = lifecycleFixture?.listeningProof, proof.scope == account.scope {
                    service = LocalListeningChatService(base: SDKChatService(api: api), gate: proof.requestGate)
                } else {
                    service = SDKChatService(api: api)
                }
            #else
                service = SDKChatService(api: api)
            #endif
            let candidate = ChatRuntime(
                scope: account.scope, service: service, persistence: storage,
                conversationID: id)
            let runtime = deletion?.register(candidate) ?? candidate
            if imageImporters[ObjectIdentifier(runtime)] == nil {
                imageImporters[ObjectIdentifier(runtime)] = ImageImportController(
                    runtime: runtime, processor: imageProcessor, budget: imageBudget,
                    admission: { [weak self] in
                        self?.session.account?.scope == account.scope && self?.entitlements?.imageAccess == .allowed
                    })
            }
            runtime.onDraftOwnerChanging = { [weak self, weak runtime] owner in
                guard let runtime else { return }
                self?.dictation?.ownerWillChange(runtime: runtime, draftID: owner)
            }
            runtime.onDraftOwnerRetired = { [weak self, weak runtime] owner in
                guard let runtime else { return }
                self?.dictation?.ownerRetired(runtime: runtime, draftID: owner)
                self?.dictationSend.retire(
                    DictationDraftOwner(
                        scope: runtime.scope, storageOwnerID: runtime.snapshot.storageOwner, draftID: owner))
                self?.imageImporters[ObjectIdentifier(runtime)]?.cancel(draftID: owner)
            }
            runtime.onQuiesceForDeletion = { [weak self, weak runtime] in
                guard let runtime else { return }
                for draft in runtime.snapshot.ownedDrafts {
                    self?.dictation?.ownerRetired(runtime: runtime, draftID: draft.id)
                    self?.dictationSend.retire(
                        DictationDraftOwner(
                            scope: runtime.scope, storageOwnerID: runtime.snapshot.storageOwner, draftID: draft.id))
                    self?.imageImporters[ObjectIdentifier(runtime)]?.cancel(draftID: draft.id)
                }
            }
            runtime.isOnline = isOnline; runtime.isForeground = foreground
            runtime.onConversationAssigned = { [weak self, weak runtime] assignedID in
                guard let self, let runtime, self.session.account?.scope == runtime.scope else { return }
                let wasSelected = self.selectedChat === runtime
                for key in Array(self.chats.keys) where self.chats[key] === runtime {
                    self.chats.removeValue(forKey: key)
                }
                self.chats[assignedID] = runtime
                if wasSelected { self.selectedKey = assignedID }
                let arm = self.dictationSend.conversationAssigned(
                    scope: runtime.scope, storageOwnerID: runtime.snapshot.storageOwner, chatID: assignedID,
                    reviewActive: self.listening?.dictationHold == .review)
                if let arm { self.listening?.sentAfterDictation(chatID: arm.chatID, turnID: arm.messageID) }
                #if DEBUG
                    self.lifecycleFixture?.listeningProof?.assigned(
                        scope: runtime.scope, chatID: assignedID, armedMessageID: arm?.messageID)
                #endif
                Task { await self.refreshConversationDetails(assignedID, account: account, api: api) }
            }
            runtime.onTurnAccepted = { [weak self] accepted in
                guard let self, self.session.account?.scope == accepted.scope else { return }
                let arm = self.dictationSend.accepted(
                    accepted, reviewActive: self.listening?.dictationHold == .review)
                if let arm { self.listening?.sentAfterDictation(chatID: arm.chatID, turnID: arm.messageID) }
                #if DEBUG
                    self.lifecycleFixture?.listeningProof?.accepted(accepted, armedMessageID: arm?.messageID)
                #endif
            }
            runtime.onCompletion = { [weak self] completion in
                guard let self, self.session.account?.scope == completion.scope else { return }
                self.listening?.completed(completion)
                #if DEBUG
                    self.lifecycleFixture?.listeningProof?.completed(completion)
                #endif
            }
            runtime.onSuccessfulVisibleCompletion = { [weak self] _ in
                guard let self, self.session.account?.scope == account.scope else { return }
                UINotificationFeedbackGenerator().notificationOccurred(.success)
            }
            runtime.onSuccessfulCompletion = { [weak self] chatID in
                guard let self, self.session.account?.scope == account.scope else { return }
                let details = self.entitlements
                Task {
                    guard self.session.account?.scope == account.scope else { return }
                    await details?.refreshBilling()
                    await self.refreshConversationDetails(chatID, account: account, api: api)
                }
            }
            runtime.canAdmitSnapshot = { [weak self, weak runtime] proposed in
                guard let self, let runtime, self.session.account?.scope == runtime.scope else { return false }
                let loadedOwners = Set(self.chats.values.map { $0.snapshot.storageOwner })
                let retained = self.retainedSnapshots.filter { !loadedOwners.contains($0.storageOwner) }
                let others = self.chats.values.filter { $0 !== runtime }.map(\.snapshot)
                return ChatResourceUsage(snapshots: retained + others + [proposed]).fitsAccount
            }
            return runtime
        }

        /// A browser presentation belongs to its originating account and view. Late
        /// dismissal from a retired account cannot expose a replacement chat.
        func setPresentationBlocked(_ blocked: Bool, id: UUID, scope: AccountScope) {
            guard session.account?.scope == scope,
                presentationBlocks.contains(id) != blocked
            else { return }
            if blocked {
                presentationBlocks.insert(id); dictation?.stopCaptureForNavigation()
            } else {
                presentationBlocks.remove(id)
            }
            updatePresentationVisibility()
        }

        func updatePresentationVisibility() {
            let visible =
                session.account != nil && !menuPresented && !settingsPresented && accountFlow == nil
                && presentationBlocks.isEmpty
            let selected = selectedChat
            for runtime in chats.values {
                let isVisible = visible && runtime === selected
                if runtime.isVisible != isVisible { runtime.isVisible = isVisible }
            }
            listening?.setVisibility(chatID: visible ? selectedChat?.conversationID : nil, foreground: foreground)
        }

        func send(_ runtime: ChatRuntime, options: ChatSendOptions) -> Bool {
            guard session.account?.scope == runtime.scope else { return false }
            let owner = DictationDraftOwner(
                scope: runtime.scope, storageOwnerID: runtime.snapshot.storageOwner, draftID: runtime.snapshot.draft.id)
            let ticket = dictationSend.ticket(for: owner, reviewActive: listening?.dictationHold == .review)
            guard runtime.send(options: options) else { return false }
            if let ticket { dictationSend.didSubmit(ticket) }
            return true
        }

        func readAloud(_ item: ConversationItem, runtime: ChatRuntime) {
            guard session.account?.scope == runtime.scope, let chatID = runtime.conversationID,
                item.role == "assistant", entitlements?.speechAvailable == true
            else { return }
            let text = (item.content ?? []).compactMap { part in
                ["output_text", "text"].contains(part.type) ? part.text : nil
            }.joined(separator: "\n")
            listening?.playReply(SpeechReply(chatID: chatID, responseID: item.responseId ?? item.id, text: text))
        }

        func makeCeremony() -> AuthCeremonies {
            let ceremony = AuthCeremonies(
                service: SDKAuthCeremonyService(client: client), configuration: configuration,
                scope: session.account?.scope, accountEmail: session.account?.user.email)
            if let scope = session.account?.scope {
                let preferences = self.preferences
                let projectExpansion = self.projectExpansion
                for previous in accountCeremonies { previous.invalidate() }
                accountCeremonies.removeAll()
                accountCeremonies.append(ceremony)
                ceremony.onPasswordResetResult = { [weak self] resetScope in
                    guard resetScope == scope, self?.session.account?.scope == scope else { return }
                    await self?.session.signOut()
                }
                guard let coordinator = deletion else { return ceremony }
                var lease: ChatDeletionLease?
                ceremony.beforeAccountDeletion = { [weak self] requested in
                    guard requested == scope, self?.session.account?.scope == scope else {
                        throw AuthCeremonyIssue.changedAccount
                    }
                    let owned = try coordinator.begin(.all); lease = owned
                    do {
                        try await coordinator.quiesce(owned)
                        try await projectExpansion?.prepareAccountDeletion()
                    } catch {
                        coordinator.release(owned); lease = nil; throw error
                    }
                }
                ceremony.onAccountDeletionResult = { [weak self] requested, deleted in
                    guard requested == scope else { return }
                    if let owned = lease {
                        lease = nil
                        if deleted { try? await coordinator.finish(owned, deleteScope: true) }
                        coordinator.release(owned)
                    }
                    // The SDK already revoked confirmed-deleted authority. The
                    // session owns retryable file cleanup, including late success.
                    if deleted {
                        try? await projectExpansion?.retire(removePersisted: true)
                    } else {
                        try? await projectExpansion?.cancelAccountDeletion()
                    }
                    if deleted, self?.session.account?.scope == scope {
                        try? await preferences?.retire(removePersisted: true)
                        await self?.session.signOut()
                    }
                }
            }
            return ceremony
        }

        func protectedDataChanged(_ available: Bool) async {
            if !available { dictation?.stopCaptureForNavigation() }
            await storage.setProtectedDataAvailable(available)
            await projectExpansionStore.setProtectedDataAvailable(available)
            if available {
                try? await projectExpansionStore.startupCleanup()
                await projectExpansion?.load()
                projectExpansion?.retrySave()
                for runtime in chats.values { await runtime.saveDraft() }
            }
        }

        func presentAccount(_ screen: AccountScreen, code: String? = nil) {
            guard !signIn.isRunning, session.phase == .signedIn || session.phase == .signedOut else { return }
            if let accountFlow, accountFlow.screen == screen {
                if let code { accountFlow.code = code }
                return
            }
            if let pendingAccountFlow, pendingAccountFlow.screen == screen {
                if let code { pendingAccountFlow.code = code }
                return
            }
            accountFlow?.ceremonies.invalidate(); pendingAccountFlow?.ceremonies.invalidate()
            let flow = AccountFlow(screen: screen, ceremonies: makeCeremony(), code: code)
            if menuPresented || settingsPresented {
                pendingAccountFlow = flow
                menuPresented = false; settingsPresented = false
            } else {
                accountFlow = flow
            }
        }

        func finishNavigationDismissal() {
            guard !menuPresented, !settingsPresented, let flow = pendingAccountFlow else { return }
            pendingAccountFlow = nil
            guard flow.ceremonies.scope == session.account?.scope else { flow.ceremonies.invalidate(); return }
            accountFlow = flow
        }

        // Admission is synchronous with onOpenURL: account routes cannot adopt
        // a replacement scope while waiting for a scheduled task to start.
        func receiveLink(_ url: URL) {
            if let link = IncomingAccountLink(url: url, configuration: configuration) {
                if let ready = accountLinkInbox.offer(link, phase: session.phase, scope: session.account?.scope) {
                    presentAccountLink(ready)
                }
            } else {
                // Native callbacks still require the live coordinator's attempt;
                // stale grants never enter the restoration inbox.
                Task { await signIn.receiveCallback(url) }
            }
        }

        func resumeAccountLinks() {
            guard let ready = accountLinkInbox.take(phase: session.phase, scope: session.account?.scope) else { return }
            presentAccountLink(ready)
        }

        private func presentAccountLink(_ link: IncomingAccountLink) {
            switch link {
            case .verify(let code): presentAccount(.verification, code: code)
            case .passwordReset(let code): presentAccount(.reset, code: code)
            }
        }
    }
#endif
