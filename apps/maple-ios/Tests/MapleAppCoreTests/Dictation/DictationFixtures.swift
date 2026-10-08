import Foundation
import Testing
@testable import MapleAppCore

@MainActor final class DictationFixtureCapture: DictationCapturing {
    var permission = true
    var permissionGate: ChatTestGate?
    var active: UUID?
    var starts = 0
    var stops = 0
    var limits: (() -> Void)?
    var pcm = Data([0, 0, 1, 0, 2, 0])
    func requestPermission() async -> Bool {
        if let permissionGate { await permissionGate.hold() }
        return permission
    }
    func start(id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void) throws {
        #expect(active == nil && maximumFrames == DictationAudio.maximumFrames)
        active = id; starts += 1; limits = onLimit
    }
    func stop(id: UUID) throws -> Data {
        #expect(active == id); active = nil; stops += 1; return pcm
    }
    func cancel(id: UUID) { if active == id { active = nil } }
}

actor DictationFixtureTranscriber: DictationTranscribing {
    var clips: [Data] = []
    var pending: [Int: CheckedContinuation<String, Error>] = [:]
    func transcribe(wav: Data) async throws -> String {
        let index = clips.count; clips.append(wav)
        return try await withCheckedThrowingContinuation { pending[index] = $0 }
    }
    func complete(_ index: Int = 0, text: String = "spoken words", fails: Bool = false) {
        if fails {
            pending.removeValue(forKey: index)?.resume(throwing: URLError(.notConnectedToInternet))
        } else {
            pending.removeValue(forKey: index)?.resume(returning: text)
        }
    }
    func waitForCalls(_ count: Int = 1) async {
        for _ in 0..<50_000 { if clips.count >= count { return }; await Task.yield() }
        Issue.record("Transcription did not start")
    }
}

@MainActor func dictationWait(_ condition: () -> Bool) async {
    for _ in 0..<50_000 { if condition() { return }; await Task.yield() }
    Issue.record("Dictation transition did not complete")
}

@MainActor func dictationFixture() -> (
    ChatRuntime, DictationCoordinator, DictationFixtureCapture, DictationFixtureTranscriber, ChatFixtureStore
) {
    let (runtime, _, store) = ChatRuntimeTests().fixture()
    let capture = DictationFixtureCapture(), service = DictationFixtureTranscriber()
    let coordinator = DictationCoordinator(scope: runtime.scope, capture: capture, transcriber: service)
    runtime.onDraftOwnerChanging = { [weak coordinator, weak runtime] id in
        guard let runtime else { return }; coordinator?.ownerWillChange(runtime: runtime, draftID: id)
    }
    runtime.onDraftOwnerRetired = { [weak coordinator, weak runtime] id in
        guard let runtime else { return }; coordinator?.ownerRetired(runtime: runtime, draftID: id)
    }
    return (runtime, coordinator, capture, service, store)
}

@MainActor func dictationEditor(
    _ runtime: ChatRuntime, _ coordinator: DictationCoordinator, offset: Int,
    marked: NSRange? = nil, draftID: UUID? = nil
) {
    let owner = draftID ?? runtime.snapshot.draft.id
    guard let draft = runtime.ownedDraft(id: owner) else { return }
    coordinator.updateEditor(
        runtime: runtime, draftID: owner,
        state: DictationEditorState(
            text: draft.text, selectionUTF16: NSRange(location: offset, length: 0), markedRangeUTF16: marked))
}
