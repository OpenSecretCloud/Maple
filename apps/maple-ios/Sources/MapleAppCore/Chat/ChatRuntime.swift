import Foundation
import MapleSDK
import Observation

/// Retain this owner in the account session when its screen is not visible.
/// Navigation only changes presentation eligibility; it never owns the run task.
@MainActor @Observable
public final class ChatRuntime {
    public let scope: AccountScope
    public private(set) var snapshot: ChatSnapshot
    public private(set) var messages: [ConversationItem] = []
    public private(set) var lastIssue: ChatIssue? {
        didSet {
            lastIssueID = lastIssue == nil ? nil : hooks.uuid()
            acknowledgedIssueID = nil
        }
    }
    public private(set) var lastIssueID: UUID?
    public private(set) var lastOutcome: ChatOutcome?
    public private(set) var isLoading = false
    public private(set) var isLoadingOlder = false
    public private(set) var hasOlderMessages = false
    public var isOnline = true { didSet { if !isOnline { pauseQueue() } } }
    public var isVisible = false
    public var isForeground = true
    public var onConversationAssigned: (@MainActor (String) -> Void)?
    public var onCompletion: (@MainActor (ChatCompletion) -> Void)?
    public var onTurnAccepted: (@MainActor (ChatSendAcceptance) -> Void)?
    public var onSuccessfulCompletion: (@MainActor (String) -> Void)?
    public var onSuccessfulVisibleCompletion: (@MainActor (String) -> Void)?
    public var canAdmitSnapshot: (@MainActor (ChatSnapshot) -> Bool)?
    public var onDraftOwnerChanging: (@MainActor (UUID) -> Void)?
    public var onDraftOwnerRetired: (@MainActor (UUID) -> Void)?
    public var onQuiesceForDeletion: (@MainActor () -> Void)?
    /// Recording/transcription blocks only submission of its own draft owner.
    public var busyDraftIDs: Set<UUID> = []
    public private(set) var deletionFenceIDs: Set<UUID> = []
    @ObservationIgnored private var quiescingFenceIDs: Set<UUID> = []
    var deletionAdmission: (@MainActor (ChatSendOptions) -> Bool)?

    @ObservationIgnored private var runOutputIDs: [UUID: Set<String>] = [:]
    @ObservationIgnored private let service: any ChatServing
    @ObservationIgnored private let persistence: any ChatPersistence
    @ObservationIgnored private let writes: ChatWriteLane
    @ObservationIgnored private let hooks: ChatRuntimeHooks
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var operationID = UUID()
    @ObservationIgnored private var runTask: Task<Void, Never>?
    @ObservationIgnored private var cancelTask: Task<Void, Never>?
    @ObservationIgnored private var autosaveTask: Task<Void, Never>?
    @ObservationIgnored private var oldestItemID: String?
    @ObservationIgnored private var paginationID: UUID?
    @ObservationIgnored private var pendingRekeyFrom: String?
    @ObservationIgnored private var waitingForCapacity = false
    @ObservationIgnored private var detachmentSource: ChatSnapshot?
    @ObservationIgnored private var detachmentDestination: ChatSnapshot?
    @ObservationIgnored private var detachmentTask: Task<Void, Error>?
    @ObservationIgnored private var detachmentID: UUID?
    @ObservationIgnored private var submissionTask: Task<Void, Never>?
    @ObservationIgnored private var submissionID: UUID?
    @ObservationIgnored private var submissionDraftID: UUID?
    @ObservationIgnored private var creationInFlight: UUID?
    @ObservationIgnored private var deletionRecoveryTask: Task<Void, Never>?
    @ObservationIgnored private var deletedStorageKey: String?
    @ObservationIgnored private var pendingAcceptances: [ChatSendAcceptance] = []
    @ObservationIgnored private var acknowledgedIssueID: UUID?

    public init(
        scope: AccountScope, service: any ChatServing, persistence: any ChatPersistence,
        conversationID: String? = nil, hooks: ChatRuntimeHooks = ChatRuntimeHooks()
    ) {
        self.scope = scope; self.service = service; self.persistence = persistence; self.hooks = hooks
        snapshot = ChatSnapshot(
            storageKey: conversationID ?? ChatSnapshot.newChatKey, conversationID: conversationID,
            draft: ChatDraft(id: hooks.uuid()))
        writes = ChatWriteLane(persistence: persistence, scope: scope)
    }

    public var conversationID: String? { snapshot.conversationID }
    public var isRetired: Bool { !valid }
    public var phase: ChatRunPhase? { snapshot.pendingRun?.phase ?? (submissionID == nil ? nil : .preparing) }
    public var hasUnresolvedRun: Bool { snapshot.pendingRun != nil || submissionID != nil }
    public var queuedTurns: [ChatQueuedTurn] { snapshot.queue?.items ?? [] }
    public var isEditingQueue: Bool { snapshot.queue?.edit != nil }
    public var isQueuePaused: Bool { snapshot.queue?.paused ?? false }
    public var canSend: Bool {
        valid && deletionFenceIDs.isEmpty && !isLoading && isOnline && !isEditingQueue && submissionID == nil
            && !busyDraftIDs.contains(snapshot.draft.id)
            && !snapshot.draft.isEmpty
    }
    public var draftText: String {
        get { snapshot.draft.text }
        set {
            guard valid, !isLoading else { return }
            snapshot.draft.text = newValue; changed(); scheduleAutosave()
        }
    }

    /// Presentation acknowledgment belongs to this account-owned runtime, so
    /// re-mounting its screen cannot present the same issue occurrence again.
    public func pendingIssuePresentationID(for issue: ChatIssue) -> UUID? {
        guard valid, lastIssue == issue, let id = lastIssueID, acknowledgedIssueID != id else { return nil }
        return id
    }

    @discardableResult
    public func acknowledgeIssuePresentation(id: UUID) -> Bool {
        guard valid, lastIssueID == id, lastIssue != nil, acknowledgedIssueID != id else { return false }
        acknowledgedIssueID = id
        return true
    }

    public func updateDraft(_ draft: ChatDraft) {
        guard valid, !isLoading, draft.id == snapshot.draft.id else { return }
        snapshot.draft = draft; changed(); scheduleAutosave()
    }

    public func ownedDraft(id: UUID) -> ChatDraft? {
        if snapshot.draft.id == id { return snapshot.draft }
        if snapshot.queue?.edit?.heldDraft.id == id { return snapshot.queue?.edit?.heldDraft }
        return nil
    }

    /// Pending dictation may update held B while A is in the editor, but never
    /// a committed queued entry, an outgoing turn, or a retired edit owner.
    @discardableResult
    public func updateOwnedDraft(_ draft: ChatDraft) -> Bool {
        guard valid, !isLoading else { return false }
        if draft.id == snapshot.draft.id { updateDraft(draft); return true }
        guard snapshot.queue?.edit?.heldDraft.id == draft.id else { return false }
        snapshot.queue?.edit?.heldDraft = draft; changed(); scheduleAutosave(); return true
    }

    @discardableResult
    public func enqueueDraft(options: ChatSendOptions = ChatSendOptions()) -> Bool {
        enqueueDraft(options: options, bodyValidated: false)
    }

    private func enqueueDraft(options: ChatSendOptions, bodyValidated: Bool) -> Bool {
        guard deletionFenceIDs.isEmpty, deletionAdmission?(options) ?? true else { lastIssue = .busy; return false }
        guard valid, !isLoading, !isEditingQueue else { lastIssue = .unfinishedQueueEdit; return false }
        guard submissionID == nil else { lastIssue = .busy; return false }
        guard !busyDraftIDs.contains(snapshot.draft.id) else { lastIssue = .draftInUse; return false }
        guard !snapshot.draft.isEmpty else { lastIssue = .emptyDraft; return false }
        guard snapshot.draft.text.utf8.count <= 32 * 1_024 else { lastIssue = .draftTooLarge; return false }
        guard Self.validImages(snapshot.draft.images) else { lastIssue = .invalidImages; return false }
        if !bodyValidated {
            let draft = snapshot.draft
            switch checkBody(
                draft, options: options, isCurrent: { [weak self] in self?.snapshot.draft == draft },
                onValid: { [weak self] in
                    guard let self, snapshot.draft == draft else { return }
                    _ = enqueueDraft(options: options, bodyValidated: true)
                })
            {
            case .fits: break
            case .rejected: return false
            case .reserved: return true
            }
        }
        var proposed = snapshot
        var queue = proposed.queue ?? ChatQueueState()
        guard queue.items.count < 16 else { lastIssue = .queueLimit; return false }
        let accepted = ChatQueuedTurn(
            id: hooks.uuid(), messageID: hooks.uuid().uuidString.lowercased(),
            draft: proposed.draft, options: options)
        queue.items.append(accepted)
        if !isOnline || phase == .recovering || snapshot.pendingRun?.stopRequested == true || !hasUnresolvedRun {
            queue.paused = true
        }
        proposed.queue = queue; proposed.draft = ChatDraft(id: hooks.uuid())
        guard admits(proposed) else { lastIssue = .queueLimit; return false }
        onDraftOwnerChanging?(snapshot.draft.id)
        snapshot = proposed; changed(); lastIssue = nil
        recordAcceptance(draftID: accepted.draft.id, messageID: accepted.messageID)
        scheduleAutosave(); return true
    }

    @discardableResult
    public func beginQueueEdit(id: UUID) -> Bool {
        guard valid, !isLoading, !isEditingQueue, var queue = snapshot.queue,
            let index = queue.items.firstIndex(where: { $0.id == id })
        else { return false }
        onDraftOwnerChanging?(snapshot.draft.id)
        let original = queue.items[index]
        queue.edit = ChatQueueEdit(original: original, originalIndex: index, heldDraft: snapshot.draft)
        snapshot.queue = queue
        snapshot.draft = ChatDraft(id: hooks.uuid(), text: original.draft.text, images: original.draft.images)
        changed(); lastIssue = nil; scheduleAutosave(); return true
    }

    @discardableResult
    public func saveQueueEdit(options: ChatSendOptions? = nil) -> Bool {
        saveQueueEdit(options: options, bodyValidated: false)
    }

    private func saveQueueEdit(options: ChatSendOptions?, bodyValidated: Bool) -> Bool {
        guard valid, let edit = snapshot.queue?.edit, var queue = snapshot.queue,
            let index = queue.items.firstIndex(where: { $0.id == edit.original.id })
        else { return false }
        guard !busyDraftIDs.contains(snapshot.draft.id) else { lastIssue = .draftInUse; return false }
        guard submissionID == nil else { lastIssue = .busy; return false }
        let options = options ?? edit.original.options
        guard !snapshot.draft.isEmpty else { lastIssue = .emptyDraft; return false }
        guard snapshot.draft.text.utf8.count <= 32 * 1_024, Self.validImages(snapshot.draft.images)
        else { lastIssue = .draftTooLarge; return false }
        if !bodyValidated {
            let draft = snapshot.draft
            switch checkBody(
                draft, options: options, isCurrent: { [weak self] in self?.snapshot.draft == draft },
                onValid: { [weak self] in
                    guard let self, snapshot.draft == draft, snapshot.queue?.edit?.original.id == edit.original.id
                    else { return }
                    _ = saveQueueEdit(options: options, bodyValidated: true)
                })
            {
            case .fits: break
            case .rejected: return false
            case .reserved: return true
            }
        }
        var item = edit.original
        item.draft = ChatDraft(id: hooks.uuid(), text: snapshot.draft.text, images: snapshot.draft.images)
        item.options = options; queue.items[index] = item; queue.edit = nil
        var proposed = snapshot; proposed.queue = queue; proposed.draft = edit.heldDraft
        guard admits(proposed) else { lastIssue = .queueLimit; return false }
        retireVisibleEdit()
        snapshot = proposed; changed(); lastIssue = nil; scheduleAutosave(); promoteIfEligible(); return true
    }

    public func cancelQueueEdit() {
        guard valid, let edit = snapshot.queue?.edit else { return }
        retireVisibleEdit()
        snapshot.draft = edit.heldDraft; snapshot.queue?.edit = nil
        changed(); lastIssue = nil; scheduleAutosave(); promoteIfEligible()
    }

    public func deleteQueuedTurn(id: UUID) {
        guard valid, var queue = snapshot.queue, let index = queue.items.firstIndex(where: { $0.id == id }) else {
            return
        }
        if let edit = queue.edit, edit.original.id == id {
            retireVisibleEdit(); snapshot.draft = edit.heldDraft; queue.edit = nil
        } else if let edit = queue.edit, index < edit.originalIndex {
            queue.edit?.originalIndex -= 1
        }
        queue.items.remove(at: index)
        if queue.items.count <= 16 || queue.recoveredHeadID == id { queue.recoveredHeadID = nil }
        snapshot.queue = queue
        changed(); scheduleAutosave(); promoteIfEligible()
    }

    public func pauseQueue() {
        guard valid, snapshot.queue != nil else { return }
        snapshot.queue?.paused = true; changed(); scheduleAutosave()
    }

    @discardableResult
    public func resumeQueue() -> Bool {
        guard valid, deletionFenceIDs.isEmpty, isOnline, !isEditingQueue, !hasUnresolvedRun, !queuedTurns.isEmpty else {
            return false
        }
        snapshot.queue?.paused = false; changed(); scheduleAutosave(); promoteIfEligible(); return true
    }

    private func retireVisibleEdit() {
        let owner = snapshot.draft.id
        onDraftOwnerChanging?(owner); busyDraftIDs.remove(owner); onDraftOwnerRetired?(owner)
    }

    private func admits(_ proposed: ChatSnapshot) -> Bool {
        ChatResourceUsage(snapshots: [proposed]).fitsAccount && (canAdmitSnapshot?(proposed) ?? true)
    }

    private func promoteIfEligible(bodyValidated: Bool = false) {
        guard valid, deletionFenceIDs.isEmpty, !isLoading, isOnline, !hasUnresolvedRun, !isEditingQueue,
            var queue = snapshot.queue, !queue.paused, let next = queue.items.first
        else { return }
        guard deletionAdmission?(next.options) ?? true else { return }
        guard !busyDraftIDs.contains(next.draft.id) else { return }
        guard !next.draft.isEmpty, next.draft.text.utf8.count <= 32 * 1_024, Self.validImages(next.draft.images) else {
            snapshot.queue?.paused = true; lastIssue = .storageUnavailable; changed(); scheduleAutosave(); return
        }
        guard let messageUUID = UUID(uuidString: next.messageID), messageUUID.uuidString.lowercased() == next.messageID
        else {
            snapshot.queue?.paused = true; lastIssue = .storageUnavailable; changed(); scheduleAutosave(); return
        }
        if !bodyValidated {
            switch checkBody(
                next.draft, options: next.options, isCurrent: { [weak self] in self?.queuedTurns.first == next },
                pauseQueueOnFailure: true,
                onValid: { [weak self] in
                    guard let self, queuedTurns.first == next else { return }
                    promoteIfEligible(bodyValidated: true)
                })
            {
            case .fits: break
            case .reserved: return
            case .rejected: pauseQueue(); return
            }
        }
        autosaveTask?.cancel()
        queue.items.removeFirst()
        if queue.items.count <= 16 || queue.recoveredHeadID == next.id { queue.recoveredHeadID = nil }
        var run = ChatPendingRun(
            id: hooks.uuid(), messageID: next.messageID, outgoingDraft: next.draft,
            options: next.options)
        run.queuedTurn = next
        snapshot.queue = queue; snapshot.pendingRun = run; changed()
        detachNewChatIfNeeded(runID: run.id)
        messages.append(Self.userItem(run)); lastIssue = nil; lastOutcome = nil
        let operation = hooks.uuid(); operationID = operation; waitingForCapacity = false
        runTask = Task { await execute(runID: run.id, operation: operation) }
    }

    /// Exchanges two complete drafts rather than losing edits made during a send.
    public func restoreUnsentTurn() {
        guard valid, !hasUnresolvedRun, !isEditingQueue, !busyDraftIDs.contains(snapshot.draft.id),
            let unsent = snapshot.unsentTurn
        else { return }
        onDraftOwnerChanging?(snapshot.draft.id)
        snapshot.unsentTurn = snapshot.draft.isEmpty ? nil : snapshot.draft
        snapshot.draft = unsent; changed(); scheduleAutosave()
    }

    /// Releases the shared New Chat slot only after its outgoing owner has a
    /// durable unique key. A registry must await this before loading a fresh slot.
    public func prepareForNewChat() async -> Bool {
        await submissionTask?.value
        guard valid, snapshot.storageKey != ChatSnapshot.newChatKey else { return false }
        do {
            try await ensureDetached()
            return valid
        } catch {
            if valid { lastIssue = .storageUnavailable }
            return false
        }
    }

    /// Restores a discovered pre-conversation owner without creating a chat or
    /// sending anything. Explicit reconciliation can return its unsent draft.
    @discardableResult
    public func loadRecovered(_ restored: ChatSnapshot) -> Bool {
        guard valid, !isLoading, !hasUnresolvedRun,
            restored.conversationID == nil, restored.storageKey.hasPrefix("pending:")
        else { return false }
        return loadRetainedSnapshot(restored)
    }

    /// Restores an already validated protected snapshot for account-wide
    /// ownership/deletion without fetching history or dispatching network work.
    @discardableResult public func loadRetainedSnapshot(_ restored: ChatSnapshot) -> Bool {
        guard valid, !isLoading, !hasUnresolvedRun,
            restored.conversationID.map({ $0 == restored.storageKey })
                ?? (restored.storageKey == ChatSnapshot.newChatKey || restored.storageKey.hasPrefix("pending:"))
        else { return false }
        autosaveTask?.cancel(); operationID = hooks.uuid()
        snapshot = restored
        snapshot.storageOwnerID = restored.storageOwner
        snapshot.pendingRun?.phase = .recovering; changed()
        snapshot.queue?.paused = true
        messages = restored.pendingRun.map { [Self.userItem($0)] } ?? []
        lastIssue = nil; lastOutcome = nil
        return true
    }

    /// A cold account launch calls load(nil). Existing interrupted work is loaded
    /// only when its conversation is opened, and is never automatically replayed.
    public func load(chatID: String?) async {
        guard valid, deletionFenceIDs.isEmpty, !hasUnresolvedRun else { return }
        autosaveTask?.cancel()
        paginationID = nil; isLoadingOlder = false
        let operation = hooks.uuid(); operationID = operation; waitingForCapacity = false; isLoading = true
        let key = chatID ?? ChatSnapshot.newChatKey
        var loadingStorage = true
        snapshot = ChatSnapshot(storageKey: key, conversationID: chatID, draft: ChatDraft(id: hooks.uuid()))
        messages = []; oldestItemID = nil; hasOlderMessages = false; lastIssue = nil; lastOutcome = nil
        do {
            let restored = try await persistence.load(chatID: key, scope: scope)
            guard owns(operation) else { return }
            if var restored {
                guard restored.storageKey == key, restored.conversationID == chatID else {
                    throw ChatIssue.storageUnavailable
                }
                if restored.storageOwnerID == nil {
                    restored.storageOwnerID = restored.storageOwner; restored.revision &+= 1
                }
                if restored.pendingRun != nil {
                    restored.pendingRun?.phase = .recovering
                    restored.revision &+= 1
                }
                if restored.queue != nil { restored.queue?.paused = true; restored.revision &+= 1 }
                snapshot = restored
                if let run = restored.pendingRun { messages = [Self.userItem(run)] }
            }
            loadingStorage = false
            if let chatID {
                let page = try await service.listConversationItems(
                    conversationId: chatID, parameters: ListParameters(limit: 20, order: .desc))
                guard owns(operation) else { return }
                let optimistic = messages
                messages = Array(page.data.reversed())
                for item in optimistic where !messages.contains(where: { $0.id == item.id }) { messages.append(item) }
                oldestItemID = page.lastId; hasOlderMessages = page.hasMore
            }
        } catch {
            if owns(operation) { lastIssue = loadingStorage ? .storageUnavailable : .historyUnavailable }
        }
        guard owns(operation) else { return }
        isLoading = false
        if hasUnresolvedRun { await reconcile() }
    }

    public func loadOlderMessages() async {
        guard valid, !isLoading, !isLoadingOlder, hasOlderMessages, let chatID = conversationID,
            let cursor = oldestItemID
        else {
            return
        }
        let operation = operationID, request = hooks.uuid()
        paginationID = request; isLoadingOlder = true
        defer {
            if paginationID == request { paginationID = nil; isLoadingOlder = false }
        }
        do {
            let page = try await service.listConversationItems(
                conversationId: chatID, parameters: ListParameters(limit: 20, after: cursor, order: .desc))
            guard owns(operation), conversationID == chatID, paginationID == request else { return }
            let existing = Set(messages.map(\.id))
            messages.insert(contentsOf: page.data.reversed().filter { !existing.contains($0.id) }, at: 0)
            oldestItemID = page.lastId; hasOlderMessages = page.hasMore
        } catch { if owns(operation), paginationID == request { lastIssue = .historyUnavailable } }
    }

    /// Admission is synchronous so two taps cannot detach the same draft. The
    /// account-owned task survives a view disappearing or its button task ending.
    @discardableResult
    public func send(options: ChatSendOptions = ChatSendOptions()) -> Bool {
        send(options: options, bodyValidated: false)
    }

    private func send(options: ChatSendOptions, bodyValidated: Bool) -> Bool {
        guard valid else { return false }
        guard deletionFenceIDs.isEmpty, deletionAdmission?(options) ?? true else { lastIssue = .busy; return false }
        guard submissionID == nil else { lastIssue = .busy; return false }
        guard !isLoading else { lastIssue = .busy; return false }
        guard !isEditingQueue else { lastIssue = .unfinishedQueueEdit; return false }
        guard !busyDraftIDs.contains(snapshot.draft.id) else { lastIssue = .draftInUse; return false }
        guard isOnline else { lastIssue = .offline; return false }
        if hasUnresolvedRun || !queuedTurns.isEmpty { return enqueueDraft(options: options) }
        guard !snapshot.draft.isEmpty else { lastIssue = .emptyDraft; return false }
        guard Self.validImages(snapshot.draft.images) else { lastIssue = .invalidImages; return false }
        if !bodyValidated {
            let draft = snapshot.draft
            switch checkBody(
                draft, options: options, isCurrent: { [weak self] in self?.snapshot.draft == draft },
                onValid: { [weak self] in
                    guard let self, snapshot.draft == draft else { return }
                    _ = send(options: options, bodyValidated: true)
                })
            {
            case .fits: break
            case .rejected: return false
            case .reserved: return true
            }
        }
        autosaveTask?.cancel()
        let run = ChatPendingRun(
            id: hooks.uuid(), messageID: hooks.uuid().uuidString.lowercased(), outgoingDraft: snapshot.draft,
            options: options)
        var proposed = snapshot; proposed.pendingRun = run; proposed.draft = ChatDraft(id: hooks.uuid())
        guard admits(proposed) else { lastIssue = .queueLimit; return false }
        onDraftOwnerChanging?(snapshot.draft.id)
        snapshot = proposed; changed()
        recordAcceptance(draftID: run.outgoingDraft.id, messageID: run.messageID)
        detachNewChatIfNeeded(runID: run.id)
        messages.append(Self.userItem(run)); lastIssue = nil; lastOutcome = nil
        let operation = hooks.uuid(); operationID = operation; waitingForCapacity = false
        runTask = Task { await execute(runID: run.id, operation: operation) }
        return true
    }

    public func stop() {
        cancelSubmission()
        pauseQueue()
        guard valid, var run = snapshot.pendingRun else { return }
        run.stopRequested = true; run.phase = .stopping; snapshot.pendingRun = run; changed()
        scheduleAutosave()
        if waitingForCapacity { runTask?.cancel(); return }
        if run.dispatched, run.responseID == nil { lastIssue = .stoppingWhenResponseReady }
        if run.dispatched, run.responseID != nil {
            startCancellation(runID: run.id, operation: operationID)
        }
        // A pre-ID request retains its exact message owner. Its late created
        // event or explicit reconciliation will carry Stop to the server.
        else if run.dispatched, runTask == nil {
            Task { await reconcile() }
        }
    }

    /// Lifecycle events reconcile already-owned uncertain work. Reopening a
    /// healthy live stream leaves its owner running; foreground recovery may
    /// replace a suspended stream with one bounded authoritative lookup.
    public func reconcileOnLifecycle(foreground: Bool) async {
        guard valid, isOnline, !Task.isCancelled, deletionFenceIDs.isEmpty,
            let pending = snapshot.pendingRun, submissionID == nil, creationInFlight == nil,
            runTask == nil || (foreground && pending.dispatched)
        else { return }
        await reconcile()
    }

    /// One bounded reconciliation pass. Foreground/visible polling may call it
    /// again for a pending result; it never calls createResponse.
    public func reconcile() async {
        guard valid, let run = snapshot.pendingRun, !isLoading, creationInFlight == nil else { return }
        pauseQueue()
        let operation = hooks.uuid(); operationID = operation; waitingForCapacity = false
        runTask?.cancel(); runTask = nil; cancelTask?.cancel(); cancelTask = nil
        let task = Task { await recover(runID: run.id, operation: operation) }
        runTask = task
        await task.value
        if owns(operation) { runTask = nil }
    }

    private func recover(runID: UUID, operation: UUID) async {
        guard owns(runID, operation), let run = snapshot.pendingRun else { return }
        snapshot.pendingRun?.phase = run.stopRequested ? .stopping : .recovering; changed()
        if conversationID == nil, run.creationDispatched == true {
            do {
                guard let creationID = run.creationID,
                    let conversation = try await findCreatedConversation(creationID, projectID: run.options.projectID)
                else { lastIssue = .uncertainResponse; return }
                guard owns(runID, operation) else { return }
                try await adoptConversation(conversation.id, runID: runID, operation: operation)
                guard owns(runID, operation) else { return }
            } catch { if owns(runID, operation) { lastIssue = .uncertainResponse }; return }
        }
        guard run.dispatched else {
            await restoreRejected(runID: run.id, operation: operation, issue: .requestRejected)
            return
        }
        guard let chatID = conversationID else { lastIssue = .uncertainResponse; return }
        do {
            var responseID = run.responseID
            if responseID == nil {
                let item = try await service.getConversationItem(conversationId: chatID, itemId: run.messageID)
                guard owns(run.id, operation) else { return }
                guard item.id == run.messageID, item.role == "user", let linked = item.responseId, !linked.isEmpty
                else {
                    throw ChatIssue.uncertainResponse
                }
                responseID = linked; snapshot.pendingRun?.responseID = linked; changed()
                try await persistCurrent()
                guard owns(run.id, operation) else { return }
            }
            guard let responseID else { throw ChatIssue.uncertainResponse }
            if snapshot.pendingRun?.stopRequested == true {
                await cancel(runID: run.id, operation: operation, responseID: responseID, recovered: true)
                return
            }
            let response = try await service.retrieveResponse(id: responseID)
            guard owns(run.id, operation) else { return }
            guard response.id == responseID else { throw ChatIssue.uncertainResponse }
            await applyRecovery(response, runID: run.id, operation: operation)
        } catch {
            guard owns(run.id, operation) else { return }
            // In particular, an exact-item 404 does not establish rejection.
            lastIssue = .uncertainResponse
            snapshot.pendingRun?.phase = .recovering; changed(); scheduleAutosave()
        }
    }

    /// The account owner revokes the persistence lease separately. In-memory
    /// invalidation comes first and rejects even same-account late callbacks.
    public func invalidate() {
        valid = false; operationID = hooks.uuid()
        for owner in Set(snapshot.ownedDrafts.map(\.id)) { onDraftOwnerRetired?(owner) }
        cancelSubmission()
        waitingForCapacity = false
        runTask?.cancel(); cancelTask?.cancel(); autosaveTask?.cancel()
        deletionRecoveryTask?.cancel(); deletionRecoveryTask = nil
        detachmentTask?.cancel(); detachmentTask = nil; detachmentID = nil
        detachmentSource = nil; detachmentDestination = nil; pendingRekeyFrom = nil
        runTask = nil; cancelTask = nil; autosaveTask = nil
        messages = []; runOutputIDs.removeAll(); snapshot = ChatSnapshot(draft: ChatDraft(id: hooks.uuid()))
        lastIssue = .signedOut; lastOutcome = nil; isLoading = false
        paginationID = nil; isLoadingOlder = false
        busyDraftIDs.removeAll()
        pendingAcceptances.removeAll()
    }

    public func saveDraft() async {
        guard valid else { return }
        autosaveTask?.cancel()
        do { try await persistCurrent() } catch { if valid { lastIssue = .storageUnavailable } }
    }

    func waitForCurrentWork() async {
        let admission = submissionTask, running = runTask
        await admission?.value
        if admission != nil { await runTask?.value } else { await running?.value }
        await cancelTask?.value
    }

    private func execute(runID: UUID, operation: UUID) async {
        defer { if owns(runID, operation) || owns(operation) { runTask = nil } }
        do {
            do { try await persistCurrent() } catch {
                await restoreRejected(runID: runID, operation: operation, issue: .storageUnavailable); return
            }
            guard owns(runID, operation) else { return }
            if snapshot.pendingRun?.stopRequested == true {
                await restoreRejected(runID: runID, operation: operation, issue: nil); return
            }
            if conversationID == nil {
                let project = snapshot.pendingRun?.options.projectID
                let creationID = try generateSecureSecret()
                snapshot.pendingRun?.creationID = creationID
                snapshot.pendingRun?.creationDispatched = true; changed()
                do { try await persistCurrent() } catch {
                    guard owns(runID, operation) else { return }
                    snapshot.pendingRun?.creationDispatched = false
                    throw error
                }
                guard owns(runID, operation) else { return }
                if snapshot.pendingRun?.stopRequested == true {
                    snapshot.pendingRun?.creationDispatched = false
                    await restoreRejected(runID: runID, operation: operation, issue: nil); return
                }
                creationInFlight = runID
                let conversation: Conversation
                do {
                    conversation = try await service.createConversation(
                        metadata: ["internal_native_creation_id": .string(creationID)], projectId: project, pinned: nil)
                } catch { if creationInFlight == runID { creationInFlight = nil }; throw error }
                if creationInFlight == runID { creationInFlight = nil }
                guard owns(runID, operation) else { return }
                try await adoptConversation(conversation.id, runID: runID, operation: operation)
            }
            guard let conversationID, let run = snapshot.pendingRun else { return }
            if run.stopRequested {
                await restoreRejected(runID: runID, operation: operation, issue: nil); return
            }
            let input = await Self.responseInput(for: run.outgoingDraft)
            guard owns(runID, operation) else { return }
            if case let .messages(inputMessages) = input,
                let index = messages.firstIndex(where: { $0.id == run.messageID }),
                let userMessage = inputMessages.first
            {
                messages[index].content = userMessage.content.map {
                    switch $0 {
                    case let .text(text): MessageContent(type: "input_text", text: text)
                    case let .image(url, detail):
                        MessageContent(type: "input_image", imageUrl: .string(url), detail: detail)
                    }
                }
            }
            snapshot.pendingRun?.phase = .sending
            // Persist the exact UUID and conservative potentially-dispatched
            // marker before entering the SDK, including the crash-before-created gap.
            snapshot.pendingRun?.dispatched = true; changed()
            do { try await persistCurrent() } catch {
                guard owns(runID, operation) else { return }
                snapshot.pendingRun?.dispatched = false
                await restoreRejected(runID: runID, operation: operation, issue: .storageUnavailable); return
            }
            guard owns(runID, operation) else { return }
            if snapshot.pendingRun?.stopRequested == true {
                snapshot.pendingRun?.dispatched = false
                await restoreRejected(runID: runID, operation: operation, issue: nil); return
            }
            let source = try await createResponse(
                ResponsesCreateRequest(
                    model: run.options.model, input: input, conversation: conversationID,
                    metadata: ["internal_message_id": .string(run.messageID)], webSearch: run.options.webSearch),
                runID: runID, operation: operation)
            guard owns(runID, operation) else { return }
            for try await batch in ChatStreamBatcher.batches(from: source, sleep: hooks.sleep) {
                guard owns(runID, operation) else { return }
                try await consume(batch, runID: runID, operation: operation)
            }
            if owns(runID, operation) {
                snapshot.pendingRun?.phase = .recovering; changed(); lastIssue = .uncertainResponse
                snapshot.queue?.paused = true
                try? await persistCurrent()
            }
        } catch {
            guard owns(runID, operation) else { return }
            if conversationID == nil, snapshot.pendingRun?.creationDispatched == true, !Self.definitelyRejected(error) {
                snapshot.pendingRun?.phase = .recovering; changed(); lastIssue = .uncertainResponse
                snapshot.queue?.paused = true; try? await persistCurrent()
            } else if snapshot.pendingRun?.dispatched != true || Self.definitelyRejected(error) {
                await restoreRejected(runID: runID, operation: operation, issue: Self.issue(error))
            } else {
                snapshot.pendingRun?.phase = .recovering; changed(); lastIssue = .uncertainResponse
                snapshot.queue?.paused = true
                try? await persistCurrent()
            }
        }
    }

    private func createResponse(
        _ request: ResponsesCreateRequest, runID: UUID, operation: UUID
    ) async throws -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        var remaining = 2
        while true {
            do { return try await service.createResponse(request, maximumInferenceSends: remaining) } catch {
                guard let capacity = findOpenSecretInferenceCapacityError(error),
                    (1...remaining).contains(capacity.inferenceSendCount)
                else { throw error }
                remaining -= capacity.inferenceSendCount
                guard remaining > 0, let delay = capacity.retryDelayMilliseconds,
                    owns(runID, operation), snapshot.pendingRun?.stopRequested != true
                else { throw error }
                // Only the authenticated replay-safe capacity contract can enter
                // this path. A shared budget includes the SDK transport resend.
                snapshot.pendingRun?.dispatched = false; snapshot.pendingRun?.phase = .preparing; changed()
                try await persistCurrent()
                guard owns(runID, operation) else { throw CancellationError() }
                waitingForCapacity = true
                do { try await hooks.sleep(.milliseconds(delay)) } catch {
                    if owns(runID, operation) { waitingForCapacity = false }
                    throw error
                }
                if owns(runID, operation) { waitingForCapacity = false }
                guard owns(runID, operation), snapshot.pendingRun?.stopRequested != true, isOnline else {
                    throw CancellationError()
                }
                snapshot.pendingRun?.dispatched = true; snapshot.pendingRun?.phase = .sending; changed()
                do { try await persistCurrent() } catch {
                    if owns(runID, operation) { snapshot.pendingRun?.dispatched = false }
                    throw error
                }
                guard owns(runID, operation), snapshot.pendingRun?.stopRequested != true else {
                    if owns(runID, operation) { snapshot.pendingRun?.dispatched = false }
                    throw CancellationError()
                }
            }
        }
    }

    private func consume(_ batch: [ResponseStreamEvent], runID: UUID, operation: UUID) async throws {
        var next = messages
        for event in batch {
            guard owns(runID, operation) else { return }
            switch event {
            case let .outputItemAdded(value), let .outputItemDone(value):
                runOutputIDs[runID, default: []].insert(value.item.id)
            case let .contentPartAdded(value), let .contentPartDone(value):
                runOutputIDs[runID, default: []].insert(value.itemId)
            case let .outputTextDelta(value), let .outputTextDone(value), let .reasoningTextDelta(value),
                let .reasoningTextDone(value):
                runOutputIDs[runID, default: []].insert(value.itemId)
            case let .toolCallCreated(value):
                runOutputIDs[runID, default: []].insert(value.toolCallId)
            case let .toolOutputCreated(value):
                runOutputIDs[runID, default: []].insert(value.toolOutputId)
            case let .webSearchInProgress(value), let .webSearchSearching(value), let .webSearchCompleted(value):
                runOutputIDs[runID, default: []].insert(value.itemId)
            default: break
            }
            switch event {
            case let .created(value), let .inProgress(value):
                if let existing = snapshot.pendingRun?.responseID, existing != value.response.id {
                    throw ChatIssue.uncertainResponse
                }
                snapshot.pendingRun?.responseID = value.response.id
                let nextPhase: ChatRunPhase = snapshot.pendingRun?.stopRequested == true ? .stopping : .streaming
                snapshot.pendingRun?.phase = nextPhase
                messages = next
                changed(); try await persistCurrent()
                guard owns(runID, operation) else { return }
                // Pagination may have prepended history during protected I/O.
                // Continue from its current value rather than a pre-await copy.
                next = messages
                if snapshot.pendingRun?.stopRequested == true { startCancellation(runID: runID, operation: operation) }
            case let .outputItemAdded(value), let .outputItemDone(value):
                Self.merge(
                    value.item, into: &next, authoritative: false, responseID: snapshot.pendingRun?.responseID)
            case let .contentPartAdded(value), let .contentPartDone(value):
                try Self.part(value, into: &next)
            case let .outputTextDelta(value): try Self.text(value, reasoning: false, final: false, into: &next)
            case let .outputTextDone(value): try Self.text(value, reasoning: false, final: true, into: &next)
            case let .reasoningTextDelta(value): try Self.text(value, reasoning: true, final: false, into: &next)
            case let .reasoningTextDone(value): try Self.text(value, reasoning: true, final: true, into: &next)
            case let .toolCallCreated(value):
                let status = next.first { $0.id == value.toolCallId }?.status ?? "in_progress"
                Self.merge(
                    ConversationItem(
                        id: value.toolCallId, type: "function_call", status: status, callId: value.toolCallId,
                        name: value.name, arguments: value.arguments), into: &next, authoritative: false)
            case let .toolOutputCreated(value):
                Self.merge(
                    ConversationItem(
                        id: value.toolOutputId, type: "function_call_output", status: "completed",
                        callId: value.toolCallId,
                        output: .string(value.output)), into: &next, authoritative: false)
                if let index = next.firstIndex(where: { $0.id == value.toolCallId && $0.type == "function_call" }) {
                    next[index].status = "completed"
                }
            case let .webSearchInProgress(value), let .webSearchSearching(value):
                Self.merge(
                    ConversationItem(id: value.itemId, type: "web_search_call", status: "in_progress"),
                    into: &next, authoritative: false)
            case let .webSearchCompleted(value):
                Self.merge(
                    ConversationItem(id: value.itemId, type: "web_search_call", status: "completed"),
                    into: &next, authoritative: false)
            case let .completed(value), let .failed(value):
                if let known = snapshot.pendingRun?.responseID, known != value.response.id {
                    throw ChatIssue.uncertainResponse
                }
                guard let outcome = Self.outcome(value.response.status) else { throw ChatIssue.uncertainResponse }
                for item in value.response.output ?? [] {
                    Self.merge(item, into: &next, authoritative: true, responseID: value.response.id)
                }
                messages = next
                await settle(
                    runID: runID, operation: operation,
                    outcome: outcome, recovered: false, response: value.response)
                return
            case let .cancelled(value):
                let returnedID = value.response?.id ?? value.id ?? value.data?.id
                if let known = snapshot.pendingRun?.responseID, let returnedID, known != returnedID {
                    throw ChatIssue.uncertainResponse
                }
                let outcome: ChatOutcome
                if let response = value.response {
                    guard let terminal = Self.outcome(response.status) else { throw ChatIssue.uncertainResponse }
                    outcome = terminal
                } else {
                    outcome = .cancelled
                }
                messages = next
                await settle(
                    runID: runID, operation: operation, outcome: outcome, recovered: false, response: value.response);
                return
            case .error:
                messages = next
                await settle(runID: runID, operation: operation, outcome: .failed, recovered: false); return
            case .unknown: break
            }
        }
        if owns(runID, operation) { messages = next }
    }

    private func startCancellation(runID: UUID, operation: UUID) {
        guard cancelTask == nil, let responseID = snapshot.pendingRun?.responseID else { return }
        cancelTask = Task {
            await cancel(runID: runID, operation: operation, responseID: responseID, recovered: false)
            if owns(operation) { cancelTask = nil }
        }
    }

    private func cancel(runID: UUID, operation: UUID, responseID: String, recovered: Bool) async {
        do {
            let response: ResearchResponse
            do { response = try await service.cancelResponse(id: responseID) } catch ResearchAPIError.http(
                status: 400, reason: _)
            {
                response = try await service.retrieveResponse(id: responseID)
            }
            guard owns(runID, operation) else { return }
            guard response.id == responseID, Self.outcome(response.status) != nil else {
                throw ChatIssue.cancellationUnavailable
            }
            await applyRecovery(response, runID: runID, operation: operation, recovered: recovered)
        } catch {
            guard owns(runID, operation) else { return }
            // A generic cancellation failure does not certify quiescence, even
            // if a separately fetched database row would look terminal.
            lastIssue = .cancellationUnavailable; snapshot.pendingRun?.phase = .recovering
            changed(); try? await persistCurrent()
        }
    }

    private func applyRecovery(_ response: ResearchResponse, runID: UUID, operation: UUID, recovered: Bool = true) async
    {
        guard owns(runID, operation), let chatID = conversationID else { return }
        let outcome: ChatOutcome? =
            response.status == .failed && (recovered || snapshot.pendingRun?.stopRequested == true)
            ? .cancelled : Self.outcome(response.status)
        for item in response.output ?? [] {
            Self.merge(item, into: &messages, authoritative: outcome != nil, responseID: response.id)
        }
        let priorHistoryIssueID = lastIssue == .historyUnavailable ? lastIssueID : nil
        do {
            let page = try await service.listConversationItems(
                conversationId: chatID, parameters: ListParameters(limit: 100, order: .desc))
            guard owns(runID, operation) else { return }
            mergeHistory(page.data.reversed(), authoritative: outcome != nil)
            if let priorHistoryIssueID, lastIssue == .historyUnavailable, lastIssueID == priorHistoryIssueID {
                lastIssue = nil
            }
        } catch { if owns(runID, operation) { lastIssue = .historyUnavailable } }
        guard owns(runID, operation) else { return }
        if let outcome {
            await settle(runID: runID, operation: operation, outcome: outcome, recovered: recovered, response: response)
        } else {
            snapshot.pendingRun?.phase = .recovering; changed(); try? await persistCurrent()
        }
    }

    private func settle(
        runID: UUID, operation: UUID, outcome: ChatOutcome, recovered: Bool, response: ResearchResponse? = nil
    ) async {
        guard owns(runID, operation), let finishedRun = snapshot.pendingRun else { return }
        let chatID = conversationID
        let wasStopped = finishedRun.stopRequested
        // Sparse terminal responses can omit output already received as text deltas.
        // Associate only this run's output; older history and explicit links retain their owners.
        let responseID = response?.id ?? finishedRun.responseID
        let outputIDs = (runOutputIDs.removeValue(forKey: runID) ?? [])
            .union(response?.output?.map(\.id) ?? [])
            .union(messages.filter { responseID != nil && $0.responseId == responseID && $0.role != "user" }.map(\.id))
        for index in messages.indices where outputIDs.contains(messages[index].id) {
            if messages[index].responseId == nil { messages[index].responseId = responseID }
            if outcome == .cancelled, ["in_progress", "streaming", "searching"].contains(messages[index].status ?? "") {
                messages[index].status = "incomplete"
            }
        }
        let completion = chatID.map { id in
            ChatCompletion(
                scope: scope, runID: runID, conversationID: id, messageID: finishedRun.messageID,
                responseID: responseID,
                assistantItems: messages.filter { $0.role == "assistant" && outputIDs.contains($0.id) },
                recovered: recovered, wasStopped: wasStopped, wasVisible: isVisible, wasForeground: isForeground)
        }
        snapshot.pendingRun = nil; changed(); lastOutcome = outcome
        let settlementIssue: ChatIssue?
        if outcome == .completed, wasStopped {
            settlementIssue = .responseFinishedBeforeStop
        } else if recovered {
            settlementIssue = outcome == .completed ? .recoveredCompletion : .recoveredStop
        } else if outcome == .failed, !wasStopped {
            settlementIssue = .generationFailed
        } else {
            settlementIssue = nil
        }
        // Settlement owns transient run feedback. History, storage and current
        // composer/queue failures remain visible until their own recovery.
        if lastIssue == nil
            || [
                ChatIssue.uncertainResponse, .cancellationUnavailable, .generationFailed,
                .stoppedBeforeSending, .stoppingWhenResponseReady, .responseFinishedBeforeStop,
                .recoveredCompletion, .recoveredStop,
            ].contains(where: { $0 == lastIssue })
        {
            lastIssue = settlementIssue
        }
        runTask?.cancel(); cancelTask?.cancel()
        runTask = nil; cancelTask = nil
        if outcome != .completed || recovered || wasStopped { snapshot.queue?.paused = true }
        do { try await persistCurrent() } catch {
            if owns(operation) { lastIssue = .storageUnavailable; snapshot.queue?.paused = true; changed() }
            return
        }
        guard owns(operation) else { return }
        if outcome == .completed, let chatID {
            if let completion { onCompletion?(completion) }
            onSuccessfulCompletion?(chatID)
            if !recovered, !wasStopped, completion?.wasVisible == true, completion?.wasForeground == true,
                isVisible, isForeground
            {
                onSuccessfulVisibleCompletion?(chatID)
            }
        }
        promoteIfEligible()
    }

    private func restoreRejected(runID: UUID, operation: UUID, issue: ChatIssue?) async {
        guard owns(runID, operation), let run = snapshot.pendingRun else { return }
        runOutputIDs.removeValue(forKey: runID)
        if let queued = run.queuedTurn {
            var queue = snapshot.queue ?? ChatQueueState()
            if !queue.items.contains(where: { $0.id == queued.id }) {
                queue.items.insert(queued, at: 0)
                if queue.edit != nil { queue.edit?.originalIndex += 1 }
                if queue.items.count > 16 { queue.recoveredHeadID = queued.id }
            }
            queue.paused = true; snapshot.queue = queue
        } else if snapshot.draft.isEmpty && !isEditingQueue && !busyDraftIDs.contains(snapshot.draft.id)
            && submissionDraftID != snapshot.draft.id
        {
            onDraftOwnerChanging?(snapshot.draft.id)
            snapshot.draft = run.outgoingDraft
        } else {
            snapshot.unsentTurn = run.outgoingDraft
        }
        messages.removeAll { $0.id == run.messageID }
        snapshot.pendingRun = nil; changed(); lastOutcome = run.stopRequested ? .cancelled : .rejected;
        snapshot.queue?.paused = true
        lastIssue = run.stopRequested && (issue == nil || issue == .requestRejected) ? .stoppedBeforeSending : issue
        do { try await persistCurrent() } catch { if owns(operation) { lastIssue = .storageUnavailable } }
    }

    private func persistCurrent() async throws {
        guard valid else { throw ChatIssue.signedOut }
        try await ensureDetached()
        guard valid else { throw ChatIssue.signedOut }
        let value = snapshot
        if let from = pendingRekeyFrom {
            try await writes.rekey(from: from, to: value)
            if valid, pendingRekeyFrom == from { pendingRekeyFrom = nil }
        } else {
            try await writes.save(value)
        }
        publishAcceptances(persisted: value)
    }

    private func recordAcceptance(draftID: UUID, messageID: String) {
        pendingAcceptances.removeAll { !Self.containsAcceptance($0, in: snapshot) }
        pendingAcceptances.append(
            ChatSendAcceptance(
                scope: scope, storageOwnerID: snapshot.storageOwner, draftID: draftID,
                messageID: messageID, conversationID: conversationID))
    }

    private func publishAcceptances(persisted: ChatSnapshot) {
        guard valid else { return }
        let ready = pendingAcceptances.filter {
            Self.containsAcceptance($0, in: persisted) && Self.containsAcceptance($0, in: snapshot)
        }
        pendingAcceptances.removeAll { ready.contains($0) || !Self.containsAcceptance($0, in: snapshot) }
        for value in ready {
            guard valid, Self.containsAcceptance(value, in: snapshot) else { continue }
            onTurnAccepted?(
                ChatSendAcceptance(
                    scope: scope, storageOwnerID: value.storageOwnerID, draftID: value.draftID,
                    messageID: value.messageID, conversationID: conversationID))
        }
    }

    private static func containsAcceptance(_ value: ChatSendAcceptance, in snapshot: ChatSnapshot) -> Bool {
        if let run = snapshot.pendingRun,
            run.messageID == value.messageID, run.outgoingDraft.id == value.draftID, !run.stopRequested
        {
            return true
        }
        return snapshot.queue?.items.contains { $0.messageID == value.messageID && $0.draft.id == value.draftID }
            ?? false
    }

    public var isDeletionQuiescent: Bool {
        valid && !isLoading && !hasUnresolvedRun && creationInFlight == nil && deletionRecoveryTask == nil
    }

    public func beginDeletionFence(_ id: UUID) {
        guard valid else { return }
        deletionFenceIDs.insert(id)
        // Discovery fences admission without interrupting unrelated inference.
        // The coordinator requests Stop only after the exact affected set is known.
    }

    public func releaseDeletionFence(_ id: UUID) {
        deletionFenceIDs.remove(id); quiescingFenceIDs.remove(id)
        // Quiescence calls Stop, so affected queues remain explicitly paused.
        // A merely fenced unrelated queue may continue after discovery narrows.
        promoteIfEligible()
    }

    /// Starts one owned, nonblocking recovery attempt. A deletion coordinator
    /// polls boundedly; it never waits indefinitely for an uncooperative request.
    public func requestDeletionQuiescence(_ fenceID: UUID) {
        guard valid, deletionFenceIDs.contains(fenceID) else { return }
        if quiescingFenceIDs.insert(fenceID).inserted { onQuiesceForDeletion?(); stop() }
        guard !isLoading, creationInFlight == nil, deletionRecoveryTask == nil, snapshot.pendingRun != nil else {
            return
        }
        if snapshot.pendingRun?.stopRequested != true {
            snapshot.pendingRun?.stopRequested = true; snapshot.pendingRun?.phase = .stopping; changed()
        }
        guard cancelTask == nil else { return }
        deletionRecoveryTask = Task { [weak self] in
            guard let self else { return }
            await reconcile()
            deletionRecoveryTask = nil
        }
    }

    /// Call only after the server has confirmed deletion of this conversation.
    /// The serialized removal runs behind all previously admitted writes.
    public func clearAfterDeletion(_ fenceID: UUID) async throws {
        if let deletedStorageKey { try await writes.remove(deletedStorageKey); return }
        guard deletionFenceIDs.contains(fenceID), isDeletionQuiescent else { throw ChatIssue.cancellationUnavailable }
        let key = snapshot.storageKey
        deletedStorageKey = key
        invalidate()
        try await writes.remove(key)
    }

    private func adoptConversation(_ id: String, runID: UUID, operation: UUID) async throws {
        guard owns(runID, operation), !id.isEmpty, id.utf8.count <= 256 else { throw ChatIssue.uncertainResponse }
        autosaveTask?.cancel()
        let oldKey = snapshot.storageKey
        snapshot.conversationID = id; snapshot.storageKey = id; changed()
        pendingRekeyFrom = oldKey
        try await persistCurrent()
        guard owns(runID, operation) else { return }
        onConversationAssigned?(id)
    }

    private func findCreatedConversation(_ creationID: String, projectID: String?) async throws -> Conversation? {
        guard creationID.utf8.count == 64,
            creationID.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
        else {
            throw ChatIssue.uncertainResponse
        }
        var after: String?, seenCursors = Set<String>(), match: Conversation?
        for _ in 0..<100 {
            try Task.checkCancellation()
            let page = try await service.listConversations(
                ConversationsListParameters(page: ListParameters(limit: 100, after: after, order: .desc)))
            guard page.data.count <= 100 else { throw ChatIssue.uncertainResponse }
            for conversation in page.data
            where conversation.metadata?["internal_native_creation_id"] == .string(creationID) {
                guard conversation.projectId == projectID, match == nil || match?.id == conversation.id else {
                    throw ChatIssue.uncertainResponse
                }
                match = conversation
            }
            if !page.hasMore { return match }
            guard let cursor = page.lastId, !page.data.isEmpty, seenCursors.insert(cursor).inserted else {
                throw ChatIssue.uncertainResponse
            }
            after = cursor
        }
        throw ChatIssue.uncertainResponse
    }

    private func detachNewChatIfNeeded(runID: UUID) {
        guard snapshot.storageKey == ChatSnapshot.newChatKey else { return }
        detachmentSource = snapshot
        snapshot.storageKey = "pending:" + runID.uuidString.lowercased(); changed()
        detachmentDestination = snapshot
    }

    private func ensureDetached() async throws {
        guard let source = detachmentSource, let destination = detachmentDestination else { return }
        let task: Task<Void, Error>
        let identifier: UUID
        if let existing = detachmentTask, let existingID = detachmentID {
            task = existing; identifier = existingID
        } else {
            identifier = hooks.uuid(); detachmentID = identifier
            task = Task { try await writes.detach(source: source, destination: destination) }
            detachmentTask = task
        }
        do {
            try await task.value
            guard valid else { throw ChatIssue.signedOut }
            if detachmentID == identifier {
                detachmentSource = nil; detachmentDestination = nil; detachmentTask = nil; detachmentID = nil
            }
        } catch {
            if detachmentID == identifier { detachmentTask = nil; detachmentID = nil }
            throw error
        }
    }

    private func changed() { snapshot.revision &+= 1 }
    private func owns(_ operation: UUID) -> Bool { valid && operationID == operation }
    private func owns(_ run: UUID, _ operation: UUID) -> Bool { owns(operation) && snapshot.pendingRun?.id == run }

    private func scheduleAutosave() {
        autosaveTask?.cancel()
        autosaveTask = Task {
            do {
                try await hooks.sleep(.milliseconds(250)); try Task.checkCancellation()
                guard valid else { return }
                try await persistCurrent()
            } catch is CancellationError {} catch { if valid { lastIssue = .storageUnavailable } }
        }
    }

    private func mergeHistory<S: Sequence>(_ items: S, authoritative: Bool) where S.Element == ConversationItem {
        for item in items { Self.merge(item, into: &messages, authoritative: authoritative) }
    }

    static func outcome(_ status: ResponseStatus) -> ChatOutcome? {
        switch status {
        case .completed: .completed
        case .cancelled, .incomplete: .cancelled
        case .failed: .failed
        case .queued, .inProgress: nil
        }
    }

    private static func definitelyRejected(_ error: any Error) -> Bool {
        if findOpenSecretInferenceCapacityError(error) != nil { return true }
        guard case let ResearchAPIError.http(status, reason) = error else { return false }
        return (400..<500).contains(status) && status != 408 || status == 503 && reason == .imageDescriptionUnavailable
    }

    private static func issue(_ error: any Error) -> ChatIssue {
        if findOpenSecretInferenceCapacityError(error) != nil { return .inferenceCapacityUnavailable }
        guard case let ResearchAPIError.http(_, reason) = error else {
            return error is ChatIssue ? (error as? ChatIssue ?? .requestRejected) : .requestRejected
        }
        return switch reason {
        case .contextLimit: .contextLimit
        case .freeTierTokenLimit: .freeTierTokenLimit
        case .usageLimit: .usageLimit
        case .modelUnavailable: .modelUnavailable
        case .unsupportedValue: .unsupportedValue
        case .imageDescriptionUnavailable: .imageDescriptionUnavailable
        default: .requestRejected
        }
    }

    private static func validImages(_ images: [ChatImageAttachment]) -> Bool {
        images.count <= 10 && Set(images.map(\.id)).count == images.count
            && images.allSatisfy {
                !$0.data.isEmpty && $0.data.count <= 20 * 1_024 * 1_024
                    && ["image/jpeg", "image/png", "image/webp"].contains($0.contentType)
            }
    }

    private enum BodyAdmission { case fits, rejected, reserved }

    private func checkBody(
        _ draft: ChatDraft, options: ChatSendOptions, isCurrent: @escaping @MainActor () -> Bool,
        pauseQueueOnFailure: Bool = false,
        onValid: @escaping @MainActor () -> Void
    ) -> BodyAdmission {
        switch ChatRequestBudget.assess(draft, options: options, conversationID: conversationID) {
        case .fits: return .fits
        case .exceeds: lastIssue = .draftTooLarge; return .rejected
        case .needsEncoding:
            guard submissionID == nil else { lastIssue = .busy; return .rejected }
            let identifier = hooks.uuid(), conversationID = conversationID
            submissionID = identifier; submissionDraftID = draft.id
            submissionTask = Task {
                do {
                    let fits = try await ChatRequestBudget.exactFits(
                        draft, options: options, conversationID: conversationID)
                    guard valid, submissionID == identifier else { return }
                    submissionID = nil; submissionDraftID = nil; submissionTask = nil
                    guard isCurrent() else { return }
                    if fits { onValid() } else { lastIssue = .draftTooLarge; if pauseQueueOnFailure { pauseQueue() } }
                } catch {
                    guard valid, submissionID == identifier else { return }
                    submissionID = nil; submissionDraftID = nil; submissionTask = nil
                    guard isCurrent() else { return }
                    lastIssue = error is CancellationError ? nil : .draftTooLarge
                    if pauseQueueOnFailure { pauseQueue() }
                }
            }
            return .reserved
        }
    }

    private func cancelSubmission() {
        submissionID = nil; submissionDraftID = nil; submissionTask?.cancel(); submissionTask = nil
    }

    private static func userItem(_ run: ChatPendingRun) -> ConversationItem {
        var content: [MessageContent] = []
        if !run.outgoingDraft.text.isEmpty {
            content.append(MessageContent(type: "input_text", text: run.outgoingDraft.text))
        }
        for image in run.outgoingDraft.images {
            // UI resolves local image IDs from the owned draft without base64 work
            // on the main actor or a network load from untrusted Markdown.
            content.append(MessageContent(type: "input_image", id: image.id.uuidString))
        }
        return ConversationItem(id: run.messageID, type: "message", role: "user", content: content)
    }

    private nonisolated static func responseInput(for draft: ChatDraft) async -> ResponseInput {
        await Task.detached {
            var content: [ResponseInputContent] = []
            if !draft.text.isEmpty { content.append(.text(draft.text)) }
            content += draft.images.map {
                .image(url: "data:\($0.contentType);base64," + $0.data.base64EncodedString())
            }
            return .messages([ResponseInputMessage(content: content)])
        }.value
    }

    private static func merge(
        _ item: ConversationItem, into items: inout [ConversationItem], authoritative: Bool, responseID: String? = nil
    ) {
        var replacement = item
        guard let index = items.firstIndex(where: { $0.id == item.id }) else {
            replacement.responseId = replacement.responseId ?? responseID
            items.append(replacement); return
        }
        let old = items[index]
        if let oldParts = old.content {
            if replacement.content?.isEmpty != false {
                replacement.content = oldParts
            } else if !authoritative {
                for (partIndex, part) in oldParts.enumerated() where partIndex < replacement.content!.count {
                    if (part.text?.count ?? 0) > (replacement.content![partIndex].text?.count ?? 0) {
                        replacement.content![partIndex] = part
                    }
                }
            }
        }
        replacement.responseId = replacement.responseId ?? old.responseId ?? responseID
        replacement.role = replacement.role ?? old.role
        items[index] = replacement
    }

    private static func part(_ value: ResponseContentEvent, into items: inout [ConversationItem]) throws {
        guard (0..<128).contains(value.contentIndex) else { throw ChatIssue.responseUnavailable }
        if !items.contains(where: { $0.id == value.itemId }) {
            items.append(ConversationItem(id: value.itemId, type: "message", role: "assistant", content: []))
        }
        let index = items.firstIndex { $0.id == value.itemId }!
        var parts = items[index].content ?? []
        while parts.count <= value.contentIndex { parts.append(MessageContent(type: "output_text", text: "")) }
        if (value.part.text?.count ?? 0) >= (parts[value.contentIndex].text?.count ?? 0) {
            parts[value.contentIndex] = value.part
        }
        items[index].content = parts
    }

    private static func text(
        _ value: ResponseTextEvent, reasoning: Bool, final: Bool, into items: inout [ConversationItem]
    ) throws {
        guard (0..<128).contains(value.contentIndex) else { throw ChatIssue.responseUnavailable }
        if !items.contains(where: { $0.id == value.itemId }) {
            items.append(
                ConversationItem(
                    id: value.itemId, type: reasoning ? "reasoning" : "message",
                    role: reasoning ? nil : "assistant", content: []))
        }
        let index = items.firstIndex { $0.id == value.itemId }!
        var parts = items[index].content ?? []
        while parts.count <= value.contentIndex {
            parts.append(MessageContent(type: reasoning ? "text" : "output_text", text: ""))
        }
        if final, let text = value.text {
            parts[value.contentIndex].text = text
        } else {
            parts[value.contentIndex].text = (parts[value.contentIndex].text ?? "") + (value.delta ?? "")
        }
        items[index].content = parts
        if items[index].status != "incomplete" {
            items[index].status = final ? "completed" : "in_progress"
        }
    }
}

/// Queue storage operations in invocation order, even when the underlying actor
/// suspends for protected-data availability or disk I/O. The store still owns
/// revision checks and retirement across separate runtimes/account sessions.
private actor ChatWriteLane {
    let persistence: any ChatPersistence
    let scope: AccountScope
    var tail: Task<Void, Error>?
    init(persistence: any ChatPersistence, scope: AccountScope) { self.persistence = persistence; self.scope = scope }
    func save(_ snapshot: ChatSnapshot) async throws {
        let prior = tail
        let next = Task { [persistence, scope] in
            _ = await prior?.result
            try await persistence.save(snapshot, scope: scope)
        }
        tail = next; try await next.value
    }
    func remove(_ key: String) async throws {
        let prior = tail
        let next = Task { [persistence, scope] in
            _ = await prior?.result
            try await persistence.remove(chatID: key, scope: scope)
        }
        tail = next; try await next.value
    }
    func rekey(from: String, to snapshot: ChatSnapshot) async throws {
        let prior = tail
        let next = Task { [persistence, scope] in
            _ = await prior?.result
            try await persistence.rekey(from: from, to: snapshot, scope: scope)
        }
        tail = next; try await next.value
    }
    func detach(source: ChatSnapshot, destination: ChatSnapshot) async throws {
        let prior = tail
        let next = Task { [persistence, scope] in
            _ = await prior?.result
            // A disk write can commit before reporting an error. Never recreate
            // the old shared slot when an earlier move already committed.
            if try await persistence.load(chatID: destination.storageKey, scope: scope) == nil {
                try await persistence.save(source, scope: scope)
            }
            try await persistence.rekey(from: source.storageKey, to: destination, scope: scope)
        }
        tail = next; try await next.value
    }
}
