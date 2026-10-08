import Foundation
import Observation

@MainActor @Observable public final class ReplyListeningSession {
    public enum Phase: Equatable, Sendable { case closed, preparing, playing, paused, ended, failed(SpeechIssue) }
    public enum DictationHold: Equatable, Sendable { case none, capture, transcription, review }
    public let scope: AccountScope
    public private(set) var phase: Phase = .closed
    public private(set) var current: SpeechReply?
    public private(set) var pending: [SpeechReply] = []
    public private(set) var speed: MaplePlaybackSpeed
    public private(set) var voice: MapleVoice
    public private(set) var manualPaused = false
    public private(set) var routePaused = false
    public private(set) var dictationHold: DictationHold = .none
    public private(set) var issue: SpeechIssue?
    public private(set) var cleanupRequired = false
    public private(set) var position: TimeInterval = 0
    public private(set) var duration: TimeInterval = 0
    public private(set) var autoplayChats = Set<String>()
    public var onSpeedChanged: (@MainActor (MaplePlaybackSpeed) -> Void)?
    @ObservationIgnored private let preparer: any SpeechPreparing
    @ObservationIgnored private let player: any SpeechPlaying
    @ObservationIgnored private let admission: @MainActor () -> SpeechIssue?
    @ObservationIgnored private let makeID: @Sendable () -> UUID
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var visibleChat: String?
    @ObservationIgnored private var foreground = true
    // Presentation and transport controls observe this owner even while closed.
    private var owner: SpeechOwner?
    @ObservationIgnored private var prepared: PreparedSpeech?
    @ObservationIgnored private var operation: UUID
    @ObservationIgnored private var playbackID: UUID
    @ObservationIgnored private var preparing: Task<Void, Never>?
    @ObservationIgnored private var cleanup: Task<Void, Error>?
    @ObservationIgnored private var retiredOwners: [SpeechOwner] = []
    @ObservationIgnored private var seenResponses: [String] = []
    @ObservationIgnored private var explicitStart = false
    @ObservationIgnored private var dictationSend: (chatID: String, turnID: String)?
    @ObservationIgnored private var suppressOldAdvance = false
    @ObservationIgnored private var interruptionMayResume = false

    public init(
        scope: AccountScope, preparer: any SpeechPreparing, player: any SpeechPlaying,
        voice: MapleVoice = .casualFemale, speed: MaplePlaybackSpeed = .normal,
        admission: @escaping @MainActor () -> SpeechIssue? = { nil },
        makeID: @escaping @Sendable () -> UUID = UUID.init
    ) {
        self.scope = scope; self.preparer = preparer; self.player = player; self.voice = voice; self.speed = speed
        self.admission = admission; self.makeID = makeID; operation = makeID(); playbackID = makeID()
        player.onEvent = { [weak self] event in self?.receive(event) }
        player.setSpeed(speed)
    }

    public var isPresented: Bool { owner != nil }
    public var sessionID: UUID? { owner?.sessionID }
    public var canPlay: Bool { valid && owner != nil && dictationHold != .capture && dictationHold != .transcription }
    public var pendingCount: Int { pending.count }
    public func autoplayEnabled(chatID: String) -> Bool { autoplayChats.contains(chatID) }

    /// Visibility changes never start audio or collect completions missed elsewhere.
    public func setVisibility(chatID: String?, foreground: Bool) {
        visibleChat = chatID; self.foreground = foreground
    }
    public func setAutoplay(_ enabled: Bool, chatID: String) {
        guard valid else { return }
        if enabled, let issue = admission() { self.issue = issue; return }
        if enabled { autoplayChats.insert(chatID) } else { autoplayChats.remove(chatID) }
    }
    public func updatePreferences(voice: MapleVoice, speed: MaplePlaybackSpeed) {
        guard valid else { return }; self.voice = voice; self.speed = speed; player.setSpeed(speed)
    }
    public func setSpeed(_ speed: MaplePlaybackSpeed) {
        guard valid else { return }; self.speed = speed; player.setSpeed(speed); onSpeedChanged?(speed)
    }

    public func completed(_ completion: ChatCompletion) {
        guard completion.scope == scope else { return }
        completed(
            SpeechReply(
                chatID: completion.conversationID,
                responseID: completion.responseID ?? completion.runID.uuidString,
                text: completion.assistantText, turnID: completion.messageID),
            wasVisible: completion.wasVisible, wasForeground: completion.wasForeground,
            recovered: completion.recovered, wasStopped: completion.wasStopped)
    }

    /// Call only for a completed assistant reply, carrying its captured completion
    /// visibility. Recovered/stopped replies are never eligible for automatic speech.
    public func completed(
        _ reply: SpeechReply, wasVisible: Bool, wasForeground: Bool,
        recovered: Bool = false, wasStopped: Bool = false
    ) {
        guard valid, !recovered, !wasStopped, wasVisible, wasForeground,
            foreground, visibleChat == reply.chatID, autoplayChats.contains(reply.chatID),
            !seenResponses.contains(reply.responseID)
        else { return }
        if let issue = admission() { self.issue = issue; return }
        guard admissible(reply) else { issue = .capacity; return }
        if let arm = dictationSend, arm.chatID == reply.chatID, arm.turnID == reply.turnID,
            !manualPaused, !routePaused, dictationHold == .none
        {
            dictationSend = nil; suppressOldAdvance = false
            replace(with: reply, explicit: false); return
        }
        remember(reply)
        if owner == nil {
            if !manualPaused && !routePaused && dictationHold == .none {
                replace(with: reply, explicit: false)
            } else {
                owner = SpeechOwner(account: scope, sessionID: makeID())
                pending.append(reply); phase = .ended
            }
            return
        }
        guard pending.count < SpeechLimits.maximumPendingReplies,
            pending.reduce(0, { $0 + $1.text.utf8.count }) + reply.text.utf8.count
                <= SpeechLimits.maximumPendingTextBytes
        else { issue = .capacity; return }
        pending.append(reply)
        if phase == .ended { advanceAutomatically() }
    }

    /// Speaker actions are explicit replacement, and do not enable autoplay.
    public func playReply(_ reply: SpeechReply) {
        guard valid else { return }
        guard dictationHold != .capture && dictationHold != .transcription else { issue = .dictationActive; return }
        if let issue = admission() { self.issue = issue; return }
        guard admissible(reply) else { issue = .capacity; return }
        manualPaused = false; routePaused = false; dictationHold = .none
        dictationSend = nil; suppressOldAdvance = false
        replace(with: reply, explicit: true)
    }

    @discardableResult public func play() -> Bool {
        guard canPlay else { return false }
        manualPaused = false; routePaused = false; dictationHold = .none
        dictationSend = nil; suppressOldAdvance = false; issue = nil
        if phase == .failed(.storage), let current { replace(with: current, explicit: true); return true }
        if phase == .preparing { explicitStart = true; return true }
        if phase == .ended {
            guard !pending.isEmpty else { return false }
            if let issue = admission() { self.issue = issue; return false }
            prepare(pending.removeFirst(), explicit: true); return true
        }
        if let prepared, duration > 0 {
            _ = prepared
            do { phase = .playing; try player.play(); return true } catch {
                phase = .failed(.invalidAudio); issue = .invalidAudio; return false
            }
        }
        if let current { prepare(current, explicit: true); return true }
        return false
    }

    public func pause() {
        guard valid, owner != nil else { return }
        manualPaused = true; explicitStart = false; player.pause(); refreshPosition()
        interruptionMayResume = false
        if phase != .preparing && phase != .ended { phase = .paused }
    }
    public func routeDisconnected() {
        guard valid else { return }
        routePaused = true; explicitStart = false; player.pause(); refreshPosition()
        interruptionMayResume = false
        if owner != nil && phase != .preparing && phase != .ended { phase = .paused }
    }
    /// A route appearing never clears route pause; only a genuine explicit Play does.
    public func routeReconnected() {}
    public func interruptionBegan() {
        let wasPlaying = phase == .playing && !manualPaused
        routeDisconnected(); interruptionMayResume = wasPlaying
    }
    public func interruptionEnded(shouldResume: Bool) {
        let resume = interruptionMayResume
        interruptionMayResume = false
        guard valid, resume, shouldResume, !manualPaused, dictationHold == .none,
            phase == .paused, prepared != nil
        else { return }
        routePaused = false
        do { phase = .playing; try player.play() } catch { phase = .failed(.invalidAudio); issue = .invalidAudio }
    }

    public func startDictation() {
        guard valid else { return }
        dictationHold = .capture; dictationSend = nil; suppressOldAdvance = true
        interruptionMayResume = false
        explicitStart = false; player.pause(); refreshPosition()
        if owner != nil && phase != .preparing && phase != .ended { phase = .paused }
    }
    public func transcriptionStarted() {
        guard valid else { return }
        startDictation(); dictationHold = .transcription
    }
    public func dictationFinishedOrCancelled() {
        guard valid, dictationHold == .capture || dictationHold == .transcription else { return }
        dictationHold = .review
    }
    /// Sending arms only this exact turn. It does not resume old audio. If review
    /// was the sole pause, its new eligible completion may replace the old session.
    /// Manual/route pause instead preserve the existing pending FIFO.
    public func sentAfterDictation(chatID: String, turnID: String) {
        guard valid, dictationHold == .review else { return }
        dictationHold = .none
        if !manualPaused && !routePaused { dictationSend = (chatID, turnID) }
    }

    public func skip(seconds: TimeInterval) {
        guard valid, prepared != nil, seconds.isFinite else { return }
        let ticket = playbackID
        do {
            try player.seek(to: min(duration, max(0, player.position + seconds)))
            guard playbackID == ticket else { return }
            refreshPosition()
            if position >= duration { phase = .ended } else if phase == .ended { phase = .paused }
        } catch { issue = .invalidAudio }
    }
    public func refreshPosition() {
        guard valid else { return }
        position = min(duration, max(0, player.position.isFinite ? player.position : 0))
    }
    @discardableResult public func remote(_ command: SpeechRemoteCommand, sessionID: UUID) -> Bool {
        guard valid, owner?.sessionID == sessionID else { return false }
        switch command {
        case .play: return play()
        case .pause: pause(); return true
        case .toggle: if phase == .playing { pause(); return true }; return play()
        case .backwardTenSeconds: skip(seconds: -10); return prepared != nil
        case .forwardTenSeconds: skip(seconds: 10); return prepared != nil
        }
    }

    public func close() {
        guard valid else { return }
        if let chat = current?.chatID ?? pending.first?.chatID { autoplayChats.remove(chat) }
        endSession()
        manualPaused = false; routePaused = false
        // Closing the player does not stop a separately owned microphone operation.
    }
    public func invalidate() async throws {
        guard valid else { try await retryCleanup(); return }
        valid = false; autoplayChats = []; onSpeedChanged = nil
        endSession(); player.onEvent = nil
        try await cleanup?.value
    }
    public func waitForPreparation() async { await preparing?.value }
    public func waitForCleanup() async throws { try await cleanup?.value }
    public func retryCleanup() async throws {
        scheduleCleanup()
        try await cleanup?.value
    }

    private func admissible(_ reply: SpeechReply) -> Bool {
        !reply.chatID.isEmpty && !reply.responseID.isEmpty && reply.text.utf8.count <= SpeechLimits.maximumSourceBytes
    }
    private func remember(_ reply: SpeechReply) {
        seenResponses.append(reply.responseID)
        if seenResponses.count > 256 { seenResponses.removeFirst(seenResponses.count - 256) }
    }
    private var mayAutomaticallyStart: Bool {
        valid && !manualPaused && !routePaused && dictationHold == .none && !suppressOldAdvance
    }
    private func advanceAutomatically() {
        guard mayAutomaticallyStart, let next = pending.first, foreground,
            visibleChat == next.chatID, autoplayChats.contains(next.chatID)
        else { return }
        if let issue = admission() { self.issue = issue; return }
        prepare(pending.removeFirst(), explicit: false)
    }
    private func replace(with reply: SpeechReply, explicit: Bool) {
        endSession()
        owner = SpeechOwner(account: scope, sessionID: makeID()); remember(reply)
        prepare(reply, explicit: explicit)
    }
    private func prepare(_ reply: SpeechReply, explicit: Bool) {
        guard let owner, valid else { return }
        if let issue = admission() { self.issue = issue; if prepared == nil { phase = .failed(issue) }; return }
        let oldPrepared = prepared
        preparing?.cancel(); player.stop(); prepared = nil
        let ticket = makeID(); operation = ticket; playbackID = makeID(); let playback = playbackID
        current = reply; phase = .preparing; issue = nil; position = 0; duration = 0; explicitStart = explicit
        let previousCleanup = cleanup, chosenVoice = voice
        preparing = Task { [weak self, preparer] in
            do {
                try await previousCleanup?.value
                if let oldPrepared { try await preparer.discard(oldPrepared) }
                try Task.checkCancellation()
                let audio = try await preparer.prepare(reply, voice: chosenVoice, owner: owner)
                guard let self, self.valid, self.operation == ticket else {
                    try await preparer.discard(audio); return
                }
                self.prepared = audio
                try await self.player.load(audio, playbackID: playback)
                guard self.valid, self.operation == ticket else { return }
                self.duration = audio.duration
                self.player.setSpeed(self.speed)
                let permitted =
                    !self.manualPaused && !self.routePaused && self.dictationHold == .none
                    && (self.explicitStart
                        || (self.mayAutomaticallyStart && self.foreground
                            && self.visibleChat == reply.chatID && self.autoplayChats.contains(reply.chatID)))
                if permitted { self.phase = .playing; try self.player.play() } else { self.phase = .paused }
            } catch {
                guard let self, self.valid, self.operation == ticket else { return }
                let issue = (error as? SpeechIssue) ?? .unavailable
                self.issue = issue; self.phase = .failed(issue)
            }
        }
    }
    private func receive(_ event: SpeechPlayerEvent) {
        guard valid, owner != nil else { return }
        switch event {
        case .ended(let id):
            guard id == playbackID else { return }
            position = duration; phase = .ended; advanceAutomatically()
        case .failed(let id):
            guard id == playbackID else { return }
            issue = .invalidAudio; phase = .failed(.invalidAudio)
        }
    }
    private func endSession() {
        operation = makeID(); playbackID = makeID(); preparing?.cancel(); preparing = nil; player.stop()
        if let owner {
            retiredOwners.append(owner)
            scheduleCleanup()
        }
        owner = nil; prepared = nil; current = nil; pending = []; seenResponses = []
        phase = .closed; position = 0; duration = 0; issue = nil
        dictationSend = nil; suppressOldAdvance = false; explicitStart = false
        interruptionMayResume = false
    }
    private func scheduleCleanup() {
        guard !retiredOwners.isEmpty else { return }
        let oldCleanup = cleanup, retiring = retiredOwners
        cleanup = Task { [weak self, preparer] in
            // Retry exact retained owners if prior cleanup failed. No new
            // preparation is admitted until the resulting task succeeds.
            _ = try? await oldCleanup?.value
            do {
                for owner in retiring { try await preparer.retire(owner) }
                self?.retiredOwners.removeAll { retiring.contains($0) }
                if let self { self.cleanupRequired = !self.retiredOwners.isEmpty }
            } catch {
                self?.cleanupRequired = true
                throw SpeechIssue.storage
            }
        }
    }
}
