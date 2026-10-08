import Foundation
import MapleSDK

public enum SpeechIssue: Error, Equatable, Sendable {
    case emptyText, capacity, invalidAudio, unavailable, storage, retired, dictationActive
    case paidPlanRequired, usageLimit
}

public struct SpeechOwner: Hashable, Sendable {
    public let account: AccountScope
    public let sessionID: UUID
    public init(account: AccountScope, sessionID: UUID = UUID()) { self.account = account; self.sessionID = sessionID }
}

public struct SpeechReply: Equatable, Sendable {
    public let chatID: String
    public let responseID: String
    public let text: String
    public let turnID: String?
    public init(chatID: String, responseID: String, text: String, turnID: String? = nil) {
        self.chatID = chatID; self.responseID = responseID; self.text = text; self.turnID = turnID
    }
}

public struct StoredSpeechFile: Hashable, Sendable {
    public let id: UUID
    public let url: URL
    public let byteCount: Int
    public init(id: UUID, url: URL, byteCount: Int) { self.id = id; self.url = url; self.byteCount = byteCount }
}

public struct SpeechChunk: Equatable, Sendable {
    public let file: StoredSpeechFile
    public let duration: TimeInterval
    public init(file: StoredSpeechFile, duration: TimeInterval) { self.file = file; self.duration = duration }
}

public struct PreparedSpeech: Equatable, Sendable {
    public let owner: SpeechOwner
    public let responseID: String
    public let chunks: [SpeechChunk]
    public var duration: TimeInterval { chunks.reduce(0) { $0 + $1.duration } }
    public init(owner: SpeechOwner, responseID: String, chunks: [SpeechChunk]) {
        self.owner = owner; self.responseID = responseID; self.chunks = chunks
    }
}

public protocol SpeechFileStoring: Sendable {
    func startupCleanup() async throws
    func activate(_ owner: SpeechOwner) async throws
    func write(_ body: AsyncThrowingStream<Data, Error>, owner: SpeechOwner) async throws -> StoredSpeechFile
    func remove(_ files: [StoredSpeechFile], owner: SpeechOwner) async throws
    func retire(_ owner: SpeechOwner) async throws
}

public protocol SpeechMediaInspecting: Sendable {
    func duration(of file: StoredSpeechFile) async throws -> TimeInterval
}

public protocol SpeechSynthesizing: Sendable {
    func synthesize(text: String, voice: MapleVoice) async throws -> MapleResponse
}

public struct SDKSpeechSynthesizer: SpeechSynthesizing {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func synthesize(text: String, voice: MapleVoice) async throws -> MapleResponse {
        try await api.synthesizeSpeech(
            input: text, voice: voice.rawValue, speed: 1, model: "voxtral-tts", format: "wav")
    }
}

public protocol SpeechPreparing: Sendable {
    func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) async throws -> PreparedSpeech
    func discard(_ speech: PreparedSpeech) async throws
    func retire(_ owner: SpeechOwner) async throws
}

public enum SpeechPlayerEvent: Sendable { case ended(UUID), failed(UUID) }
public enum SpeechRemoteCommand: Sendable { case play, pause, toggle, backwardTenSeconds, forwardTenSeconds }

@MainActor public protocol SpeechPlaying: AnyObject {
    var onEvent: (@MainActor (SpeechPlayerEvent) -> Void)? { get set }
    var position: TimeInterval { get }
    var duration: TimeInterval { get }
    func load(_ speech: PreparedSpeech, playbackID: UUID) async throws
    func play() throws
    func pause()
    func stop()
    func seek(to position: TimeInterval) throws
    func setSpeed(_ speed: MaplePlaybackSpeed)
}

public enum SpeechLimits {
    public static let maximumSourceBytes = 1_024 * 1_024
    public static let maximumChunks = 256
    public static let maximumPendingReplies = 16
    public static let maximumPendingTextBytes = 4 * 1_024 * 1_024
    public static let maximumFileBytes = 32 * 1_024 * 1_024
    public static let maximumSessionBytes = 256 * 1_024 * 1_024
    public static let maximumStreamBlockBytes = 128 * 1_024
}
