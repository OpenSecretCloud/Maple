import Foundation
import Observation

public enum DictationActivity: Sendable { case capturing, transcribing, idle }

@MainActor @Observable public final class DictationCoordinator {
    public let scope: AccountScope
    public private(set) var statuses: [UUID: DictationStatus] = [:]
    public private(set) var lastIssue: DictationIssue?
    /// Fires once at admitted permission request, before its playback hold. A
    /// denied/cancelled request still owns its later review; navigation cannot
    /// redirect a Send from another draft into that review.
    public var onCaptureAdmitted: (@MainActor (DictationDraftOwner) -> Void)?
    public var onActivityChange: (@MainActor (DictationActivity) -> Void)?

    private let capture: any DictationCapturing
    private let transcriber: any DictationTranscribing
    private let hooks: ChatRuntimeHooks
    private var jobs: [UUID: Job] = [:]
    private var editors: [UUID: DictationEditorState] = [:]
    private var editorRuntimes: [UUID: ObjectIdentifier] = [:]
    private var capturingID: UUID?
    private weak var issueRuntime: ChatRuntime?
    private var issueDraftID: UUID?
    private var valid = true

    private final class Job {
        let id = UUID()
        let runtime: ChatRuntime
        let draftID: UUID
        var anchor: DictationInsertionAnchor?
        var wav: Data?
        var result: String?
        var anchorReliable = true
        var task: Task<Void, Never>?
        var timer: Task<Void, Never>?
        init(runtime: ChatRuntime, draftID: UUID) { self.runtime = runtime; self.draftID = draftID }
    }

    public init(
        scope: AccountScope, capture: any DictationCapturing,
        transcriber: any DictationTranscribing, hooks: ChatRuntimeHooks = ChatRuntimeHooks()
    ) {
        self.scope = scope; self.capture = capture; self.transcriber = transcriber; self.hooks = hooks
    }

    public func status(for runtime: ChatRuntime, draftID: UUID? = nil) -> DictationStatus? {
        let id = draftID ?? runtime.snapshot.draft.id
        guard let job = jobs[id], job.runtime === runtime else { return nil }
        return statuses[id]
    }

    public func issue(for runtime: ChatRuntime) -> DictationIssue? {
        issueRuntime === runtime && issueDraftID == runtime.snapshot.draft.id ? lastIssue : nil
    }

    public func dismissIssue() { lastIssue = nil; issueRuntime = nil; issueDraftID = nil }

    public func editorState(runtime: ChatRuntime, draftID: UUID) -> DictationEditorState? {
        guard valid, runtime.scope == scope, editorRuntimes[draftID] == ObjectIdentifier(runtime),
            let state = editors[draftID], runtime.ownedDraft(id: draftID)?.text == state.text
        else { return nil }
        return state
    }

    /// Called after a text edit or selection/composition change, using the exact
    /// owner that produced it. A held draft has no current selection changes.
    public func updateEditor(
        runtime: ChatRuntime, draftID: UUID, state: DictationEditorState, edit: DictationTextEdit? = nil
    ) {
        guard valid, runtime.scope == scope, runtime.ownedDraft(id: draftID)?.text == state.text,
            Self.validRange(state.selectionUTF16, in: state.text),
            state.markedRangeUTF16.map({ Self.validRange($0, in: state.text) }) ?? true
        else { return }
        if let job = jobs[draftID], job.runtime !== runtime { return }
        editors[draftID] = state; editorRuntimes[draftID] = ObjectIdentifier(runtime)
        guard let job = jobs[draftID], job.runtime === runtime else { return }
        if let edit, edit.resultingText == state.text, job.anchor?.text == edit.beforeText {
            job.anchor?.rebase(using: edit)
        } else {
            job.anchor?.rebase(to: state.text)
        }
        if job.result != nil, state.markedRangeUTF16 == nil { insertResult(job) }
    }

    /// Interactive editors supply every actual replacement in order. Invalid or
    /// missing transactions never guess among repeated text: a completed result
    /// is retained for the user's explicit insertion-point choice instead.
    public func updateEditor(
        runtime: ChatRuntime, draftID: UUID, state: DictationEditorState, edits: [DictationTextEdit]
    ) {
        guard valid, runtime.scope == scope, runtime.ownedDraft(id: draftID)?.text == state.text,
            Self.validRange(state.selectionUTF16, in: state.text),
            state.markedRangeUTF16.map({ Self.validRange($0, in: state.text) }) ?? true
        else { return }
        if let job = jobs[draftID], job.runtime !== runtime { return }
        editors[draftID] = state; editorRuntimes[draftID] = ObjectIdentifier(runtime)
        guard let job = jobs[draftID], job.runtime === runtime, var anchor = job.anchor else { return }
        var matches = job.anchorReliable
        for edit in edits {
            guard matches else { break }
            matches = anchor.rebase(using: edit)
        }
        matches = matches && anchor.text == state.text
        if matches { job.anchor = anchor } else { job.anchorReliable = false }
        if job.result != nil { insertResult(job) }
    }

    public func insertAtCurrentPosition(runtime: ChatRuntime) {
        let owner = runtime.snapshot.draft.id
        guard let job = jobs[owner], job.runtime === runtime, owns(job), job.result != nil,
            let draft = runtime.ownedDraft(id: owner), let editor = editors[owner], editor.text == draft.text
        else { return }
        job.anchor = DictationInsertionAnchor(
            text: draft.text, offsetUTF16: editor.selectionUTF16.location + editor.selectionUTF16.length)
        job.anchorReliable = true
        insertResult(job)
    }

    @discardableResult public func start(runtime: ChatRuntime) -> Bool {
        let owner = runtime.snapshot.draft.id
        guard valid, !runtime.isRetired, !runtime.isLoading, runtime.deletionFenceIDs.isEmpty, runtime.scope == scope,
            runtime.ownedDraft(id: owner) != nil,
            !runtime.busyDraftIDs.contains(owner), jobs[owner] == nil, capturingID == nil
        else { setIssue(.draftUnavailable, runtime: runtime, owner: owner); return false }
        guard jobs.count < 4 else { setIssue(.tooManyOperations, runtime: runtime, owner: owner); return false }
        let job = Job(runtime: runtime, draftID: owner)
        jobs[owner] = job; capturingID = job.id; runtime.busyDraftIDs.insert(owner)
        statuses[owner] = DictationStatus(draftID: owner, phase: .requestingPermission, elapsedSeconds: 0)
        onCaptureAdmitted?(
            DictationDraftOwner(scope: scope, storageOwnerID: runtime.snapshot.storageOwner, draftID: owner))
        dismissIssue(); publishActivity()
        job.task = Task { [self] in
            let granted = await capture.requestPermission()
            guard owns(job), !Task.isCancelled else { remove(job); return }
            guard granted else { fail(job, issue: .permissionDenied, retainingClip: false); return }
            do {
                try capture.start(id: job.id, maximumFrames: DictationAudio.maximumFrames) { [weak self] in
                    guard let self, self.owns(job) else { return }
                    self.stop(runtime: job.runtime, draftID: job.draftID)
                }
                guard owns(job), !Task.isCancelled else { capture.cancel(id: job.id); return }
                statuses[owner]?.phase = .recording; job.task = nil
                startTimer(job)
            } catch { fail(job, issue: .captureUnavailable, retainingClip: false) }
        }
        return true
    }

    /// This method stops the actual capture before returning, allowing callers
    /// to change visible composer ownership safely in the same actor turn.
    public func stop(runtime: ChatRuntime, draftID: UUID? = nil) {
        let owner = draftID ?? runtime.snapshot.draft.id
        guard let job = jobs[owner], job.runtime === runtime, owns(job) else { return }
        guard statuses[owner]?.phase != .requestingPermission else { cancel(runtime: runtime, draftID: owner); return }
        guard statuses[owner]?.phase == .recording else { return }
        job.timer?.cancel(); job.timer = nil
        guard let draft = runtime.ownedDraft(id: owner) else { remove(job); return }
        let editor = editors[owner].flatMap { $0.text == draft.text ? $0 : nil }
        let offset = editor.map { $0.selectionUTF16.location + $0.selectionUTF16.length } ?? draft.text.utf16.count
        job.anchor = DictationInsertionAnchor(text: draft.text, offsetUTF16: offset)
        do {
            let pcm = try capture.stop(id: job.id)
            capturingID = nil; statuses[owner]?.phase = .transcribing; publishActivity()
            let encoding = Task.detached { try DictationAudio.wav(pcm16Mono: pcm) }
            job.task = Task { [self] in
                do {
                    let wav = try await encoding.value
                    guard owns(job), !Task.isCancelled else { remove(job); return }
                    job.wav = wav
                    await transcribe(job)
                } catch {
                    if owns(job) {
                        fail(job, issue: error as? DictationIssue ?? .captureUnavailable, retainingClip: false)
                    }
                }
            }
        } catch { fail(job, issue: .captureUnavailable, retainingClip: false) }
    }

    public func stopCaptureForNavigation() {
        for job in Array(jobs.values) where job.id == capturingID {
            stop(runtime: job.runtime, draftID: job.draftID)
        }
    }

    public func ownerWillChange(runtime: ChatRuntime, draftID: UUID) {
        stop(runtime: runtime, draftID: draftID)
    }

    public func ownerRetired(runtime: ChatRuntime, draftID: UUID) {
        cancel(runtime: runtime, draftID: draftID)
        if editorRuntimes[draftID] == ObjectIdentifier(runtime) {
            editors[draftID] = nil; editorRuntimes[draftID] = nil
        }
    }

    public func retry(runtime: ChatRuntime, draftID: UUID? = nil) {
        let owner = draftID ?? runtime.snapshot.draft.id
        guard let job = jobs[owner], job.runtime === runtime, owns(job),
            statuses[owner]?.phase == .failed, job.wav != nil
        else { return }
        statuses[owner]?.phase = .transcribing; statuses[owner]?.issue = nil; lastIssue = nil
        publishActivity()
        job.task = Task { [self] in await transcribe(job) }
    }

    public func cancel(runtime: ChatRuntime, draftID: UUID? = nil) {
        let owner = draftID ?? runtime.snapshot.draft.id
        guard let job = jobs[owner], job.runtime === runtime else { return }
        remove(job)
    }

    public func invalidate() {
        guard valid else { return }
        valid = false
        for job in Array(jobs.values) { remove(job) }
        editors.removeAll(); editorRuntimes.removeAll(); dismissIssue()
    }

    private func transcribe(_ job: Job) async {
        guard owns(job), let wav = job.wav else { return }
        do {
            let text = try await transcriber.transcribe(wav: wav)
            guard owns(job), !Task.isCancelled else { remove(job); return }
            guard text.utf8.count <= 256 * 1024 else {
                fail(job, issue: .invalidTranscription, retainingClip: true); return
            }
            job.result = text; job.wav = nil; job.task = nil
            insertResult(job)
        } catch {
            guard owns(job), !Task.isCancelled else { return }
            fail(job, issue: .transcriptionUnavailable, retainingClip: true)
        }
    }

    private func insertResult(_ job: Job) {
        guard owns(job), let result = job.result, var draft = job.runtime.ownedDraft(id: job.draftID) else { return }
        guard job.anchorReliable else {
            statuses[job.draftID]?.phase = .waitingForInsertionPoint; publishActivity(); return
        }
        job.anchor?.rebase(to: draft.text)
        if editors[job.draftID]?.markedRangeUTF16 != nil {
            statuses[job.draftID]?.phase = .waitingForComposition; publishActivity(); return
        }
        guard let anchor = job.anchor else { remove(job); return }
        let before = draft.text
        draft.text = anchor.inserting(result)
        guard job.runtime.updateOwnedDraft(draft) else { remove(job); return }
        if var editor = editors[job.draftID], editor.text == before {
            let added = draft.text.utf16.count - before.utf16.count
            let start = editor.selectionUTF16.location
            let end = start + editor.selectionUTF16.length
            let shiftedStart = start >= anchor.offsetUTF16 ? start + added : start
            let shiftedEnd = end >= anchor.offsetUTF16 ? end + added : end
            editor.text = draft.text
            editor.selectionUTF16 = NSRange(location: shiftedStart, length: shiftedEnd - shiftedStart)
            editor.markedRangeUTF16 = nil
            editors[job.draftID] = editor
        }
        // updateOwnedDraft schedules the same protected, owner-fenced autosave as
        // typing; protection failures leave the inserted text visibly unsaved.
        remove(job)
    }

    private func owns(_ job: Job) -> Bool {
        valid && jobs[job.draftID] === job && !job.runtime.isRetired && job.runtime.scope == scope
            && job.runtime.ownedDraft(id: job.draftID) != nil
    }

    private func fail(_ job: Job, issue: DictationIssue, retainingClip: Bool) {
        guard owns(job) else { return }
        setIssue(issue, runtime: job.runtime, owner: job.draftID)
        if retainingClip {
            statuses[job.draftID]?.phase = .failed; statuses[job.draftID]?.issue = issue
            job.task = nil; publishActivity()
        } else {
            remove(job)
        }
    }

    private func remove(_ job: Job) {
        guard jobs[job.draftID] === job else { return }
        // Retire the identity before cancelling work that may complete late.
        jobs[job.draftID] = nil; statuses[job.draftID] = nil
        if capturingID == job.id { capturingID = nil }
        job.task?.cancel(); job.timer?.cancel(); capture.cancel(id: job.id)
        job.task = nil; job.timer = nil; job.wav = nil; job.result = nil
        job.runtime.busyDraftIDs.remove(job.draftID)
        publishActivity()
    }

    private func setIssue(_ issue: DictationIssue, runtime: ChatRuntime, owner: UUID) {
        issueRuntime = runtime; issueDraftID = owner; lastIssue = issue
    }

    private func startTimer(_ job: Job) {
        job.timer = Task { [self] in
            do {
                for elapsed in 1...DictationAudio.maximumSeconds {
                    try await hooks.sleep(.seconds(1))
                    guard owns(job), !Task.isCancelled, statuses[job.draftID]?.phase == .recording else { return }
                    statuses[job.draftID]?.elapsedSeconds = elapsed
                    if elapsed == DictationAudio.maximumSeconds { stop(runtime: job.runtime, draftID: job.draftID) }
                }
            } catch {}
        }
    }

    private func publishActivity() {
        if statuses.values.contains(where: { $0.phase == .recording || $0.phase == .requestingPermission }) {
            onActivityChange?(.capturing)
        } else if statuses.values.contains(where: { $0.phase == .transcribing || $0.phase == .waitingForComposition }) {
            onActivityChange?(.transcribing)
        } else {
            onActivityChange?(.idle)
        }
    }

    private static func validRange(_ range: NSRange, in text: String) -> Bool {
        range.location != NSNotFound && range.location >= 0 && range.length >= 0
            && range.location <= text.utf16.count && range.length <= text.utf16.count - range.location
    }
}
