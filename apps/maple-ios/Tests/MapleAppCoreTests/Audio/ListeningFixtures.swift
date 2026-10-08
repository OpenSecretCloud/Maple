import Foundation
import Testing
@testable import MapleAppCore

actor ListeningPreparer: SpeechPreparing {
    struct Request: Sendable { let reply: SpeechReply; let voice: MapleVoice; let owner: SpeechOwner }
    var requests: [Request] = []
    var discarded: [PreparedSpeech] = []
    var retired: [SpeechOwner] = []
    var gate: ChatTestGate?
    var failRetirement = false
    private var discardWaiters: [CheckedContinuation<Void, Never>] = []
    func hold(_ gate: ChatTestGate?) { self.gate = gate }
    func failRetire(_ value: Bool) { failRetirement = value }
    func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) async throws -> PreparedSpeech {
        requests.append(Request(reply: reply, voice: voice, owner: owner))
        if let gate { await gate.hold() }
        return PreparedSpeech(
            owner: owner, responseID: reply.responseID,
            chunks: [
                SpeechChunk(
                    file: StoredSpeechFile(
                        id: UUID(), url: URL(fileURLWithPath: "/synthetic/speech.wav"), byteCount: 40), duration: 30)
            ])
    }
    func discard(_ speech: PreparedSpeech) {
        discarded.append(speech); discardWaiters.forEach { $0.resume() }; discardWaiters = []
    }
    func waitForDiscard() async {
        if discarded.isEmpty { await withCheckedContinuation { discardWaiters.append($0) } }
    }
    func retire(_ owner: SpeechOwner) throws {
        if failRetirement { throw SpeechIssue.storage }
        retired.append(owner)
    }
}

@MainActor final class ListeningPlayer: SpeechPlaying {
    var onEvent: (@MainActor (SpeechPlayerEvent) -> Void)?
    var position: TimeInterval = 0
    var duration: TimeInterval = 0
    var speed: MaplePlaybackSpeed = .normal
    var playCount = 0
    var pauseCount = 0
    var loads: [String] = []
    var currentID: UUID?
    var isPlaying = false
    func load(_ speech: PreparedSpeech, playbackID: UUID) {
        duration = speech.duration; position = 0; currentID = playbackID; loads.append(speech.responseID)
    }
    func play() { playCount += 1; isPlaying = true }
    func pause() { pauseCount += 1; isPlaying = false }
    func stop() { isPlaying = false; position = 0; duration = 0; currentID = nil }
    func seek(to position: TimeInterval) { self.position = min(duration, max(0, position)) }
    func setSpeed(_ speed: MaplePlaybackSpeed) { self.speed = speed }
    func finish() {
        guard let currentID else { return }; isPlaying = false; position = duration; onEvent?(.ended(currentID))
    }
}

@MainActor final class ListeningAdmission {
    var issue: SpeechIssue?
}

func speechReply(_ id: String, chat: String = "chat", turn: String? = nil) -> SpeechReply {
    SpeechReply(chatID: chat, responseID: id, text: "Readable reply \(id)", turnID: turn)
}

@MainActor func listeningFixture() -> (ReplyListeningSession, ListeningPreparer, ListeningPlayer) {
    let preparer = ListeningPreparer(), player = ListeningPlayer()
    let session = ReplyListeningSession(scope: accountScope(), preparer: preparer, player: player)
    session.setVisibility(chatID: "chat", foreground: true)
    return (session, preparer, player)
}
