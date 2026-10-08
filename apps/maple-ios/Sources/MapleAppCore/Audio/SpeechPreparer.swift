import Foundation
import MapleSDK

/// Prepares every chunk before exposing Play. No provider request or file is
/// reused across a retired listening session, including same-principal relogin.
public actor SpeechPreparer: SpeechPreparing {
    private let synthesis: any SpeechSynthesizing
    private let files: any SpeechFileStoring
    private let inspector: any SpeechMediaInspecting
    private let admission: @Sendable () async -> SpeechIssue?
    private var retired = Set<SpeechOwner>()

    public init(
        synthesis: any SpeechSynthesizing, files: any SpeechFileStoring, inspector: any SpeechMediaInspecting,
        admission: @escaping @Sendable () async -> SpeechIssue? = { nil }
    ) {
        self.synthesis = synthesis; self.files = files; self.inspector = inspector; self.admission = admission
    }

    public func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) async throws -> PreparedSpeech {
        try require(owner)
        let planning = Task.detached(priority: .userInitiated) { try SpeechTextPlanner.chunks(reply.text) }
        let text = try await withTaskCancellationHandler {
            try await planning.value
        } onCancel: {
            planning.cancel()
        }
        try require(owner)
        guard !text.isEmpty else { throw SpeechIssue.emptyText }
        try await files.activate(owner)
        var created: [StoredSpeechFile] = [], chunks: [SpeechChunk] = []
        do {
            for part in text {
                try require(owner)
                if let issue = await admission() { throw issue }
                try require(owner)
                let response = try await synthesis.synthesize(text: part, voice: voice)
                try require(owner)
                let mime =
                    response.header("content-type")?.split(separator: ";", maxSplits: 1).first?
                    .trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
                guard (200...299).contains(response.status), !mime.contains("json"),
                    mime.hasPrefix("audio/") || mime == "application/octet-stream"
                else { throw SpeechIssue.invalidAudio }
                let file = try await files.write(response.body, owner: owner)
                created.append(file)
                try require(owner)
                let duration = try await inspector.duration(of: file)
                try require(owner)
                guard duration.isFinite, duration > 0 else { throw SpeechIssue.invalidAudio }
                chunks.append(SpeechChunk(file: file, duration: duration))
            }
            try require(owner)
            let prepared = PreparedSpeech(owner: owner, responseID: reply.responseID, chunks: chunks)
            guard prepared.duration.isFinite, prepared.duration > 0 else { throw SpeechIssue.invalidAudio }
            return prepared
        } catch {
            do { try await files.remove(created, owner: owner) } catch { throw SpeechIssue.storage }
            throw Self.present(error)
        }
    }

    public func discard(_ speech: PreparedSpeech) async throws {
        try await files.remove(speech.chunks.map(\.file), owner: speech.owner)
    }
    public func retire(_ owner: SpeechOwner) async throws {
        retired.insert(owner)
        try await files.retire(owner)
    }
    private func require(_ owner: SpeechOwner) throws {
        guard !retired.contains(owner) else { throw SpeechIssue.retired }
        try Task.checkCancellation()
    }
    private static func present(_ error: any Error) -> SpeechIssue {
        if let issue = error as? SpeechIssue { return issue }
        if case let ResearchAPIError.http(_, reason) = error, reason == .usageLimit { return .usageLimit }
        return .unavailable
    }
}
