import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

private actor SynthesisFixture: SpeechSynthesizing {
    var requests: [(String, MapleVoice)] = []
    var mime = "audio/wav"
    var failAt: Int?
    var gate: ChatTestGate?
    func configure(mime: String = "audio/wav", failAt: Int? = nil, gate: ChatTestGate? = nil) {
        self.mime = mime; self.failAt = failAt; self.gate = gate
    }
    func synthesize(text: String, voice: MapleVoice) async throws -> MapleResponse {
        requests.append((text, voice))
        if let gate { await gate.hold() }
        if requests.count == failAt { throw ResearchAPIError.http(status: 503, reason: .unspecified) }
        return MapleResponse(
            status: 200, headers: ["Content-Type": mime],
            body: AsyncThrowingStream {
                $0.yield(Data("synthetic-wave".utf8)); $0.finish()
            })
    }
}
private actor SpeechInspectorFixture: SpeechMediaInspecting {
    let seconds: Double
    init(seconds: Double = 1) { self.seconds = seconds }
    func duration(of file: StoredSpeechFile) -> TimeInterval { seconds }
}
private actor SpeechAdmissionFixture {
    var requests = 0
    func check() -> SpeechIssue? {
        requests += 1
        return requests > 1 ? .paidPlanRequired : nil
    }
}

@Suite struct SpeechPreparerTests {
    private func directory() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("speech-prepare-\(UUID().uuidString)")
    }

    @Test func entireReplyIsPreparedSequentiallyWithExplicitVoiceBeforePlay() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture()
        try await files.startupCleanup()
        let preparer = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture())
        let owner = SpeechOwner(account: accountScope())
        let result = try await preparer.prepare(
            SpeechReply(chatID: "chat", responseID: "reply", text: String(repeating: "word ", count: 601)),
            voice: .casualFemale, owner: owner)
        #expect(result.chunks.count == 3 && result.duration == 3)
        #expect(await synth.requests.map { $0.0.split(separator: " ").count } == [300, 300, 1])
        #expect(await synth.requests.allSatisfy { $0.1 == .casualFemale })
        try await preparer.retire(owner)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
    @Test func providerJSONAndUndecodableContentAreSanitizedAndRemoved() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture()
        try await files.startupCleanup(); await synth.configure(mime: "audio/error+json")
        let preparer = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture())
        let owner = SpeechOwner(account: accountScope())
        await #expect(throws: SpeechIssue.invalidAudio) {
            try await preparer.prepare(speechReply("one"), voice: .neutralMale, owner: owner)
        }
        #expect(await files.allocatedBytes == 0)
        await synth.configure()
        let invalid = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture(seconds: .nan))
        await #expect(throws: SpeechIssue.invalidAudio) {
            try await invalid.prepare(speechReply("two"), voice: .neutralMale, owner: owner)
        }
        #expect(await files.allocatedBytes == 0)
    }
    @Test func partialProviderFailureCleansCompletedChunksWithoutReturningTruncatedReply() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture()
        try await files.startupCleanup(); await synth.configure(failAt: 2)
        let preparer = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture())
        let owner = SpeechOwner(account: accountScope())
        await #expect(throws: SpeechIssue.unavailable) {
            try await preparer.prepare(
                SpeechReply(chatID: "chat", responseID: "reply", text: String(repeating: "word ", count: 350)),
                voice: .casualFemale, owner: owner)
        }
        #expect(await files.allocatedBytes == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
    @Test func retiredSessionRejectsLateSynthesisBeforeAnyFileWrite() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture(), gate = ChatTestGate()
        try await files.startupCleanup(); await synth.configure(gate: gate)
        let preparer = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture())
        let owner = SpeechOwner(account: accountScope())
        let work = Task { try await preparer.prepare(speechReply("one"), voice: .casualFemale, owner: owner) }
        await gate.waitForArrival(); try await preparer.retire(owner); await gate.release()
        await #expect(throws: SpeechIssue.retired) { try await work.value }
        #expect(await files.allocatedBytes == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
    @Test func emptySpeechDoesNotContactProvider() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture()
        try await files.startupCleanup()
        let preparer = SpeechPreparer(synthesis: synth, files: files, inspector: SpeechInspectorFixture())
        await #expect(throws: SpeechIssue.emptyText) {
            try await preparer.prepare(
                SpeechReply(chatID: "chat", responseID: "empty", text: "```\ncode\n```"), voice: .casualFemale,
                owner: SpeechOwner(account: accountScope()))
        }
        #expect(await synth.requests.isEmpty)
    }
    @Test func everyChunkRechecksAdmissionAndFailureRemovesAlreadyPreparedFiles() async throws {
        let root = directory(); defer { try? FileManager.default.removeItem(at: root) }
        let files = TemporarySpeechStore(root: root), synth = SynthesisFixture(), admission = SpeechAdmissionFixture()
        try await files.startupCleanup()
        let preparer = SpeechPreparer(
            synthesis: synth, files: files, inspector: SpeechInspectorFixture(),
            admission: { await admission.check() })
        await #expect(throws: SpeechIssue.paidPlanRequired) {
            try await preparer.prepare(
                SpeechReply(chatID: "chat", responseID: "reply", text: String(repeating: "word ", count: 350)),
                voice: .casualFemale, owner: SpeechOwner(account: accountScope()))
        }
        #expect(await synth.requests.count == 1)
        #expect(await files.allocatedBytes == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
}
