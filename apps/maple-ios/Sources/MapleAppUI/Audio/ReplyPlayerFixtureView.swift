#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MediaPlayer
    import Observation
    import SwiftUI

    /// No SDK client or network. Only synthesis is replaced with a bounded local
    /// tone; the native engine, protected files, preferences and player UI are real.
    struct ReplyPlayerFixtureView: View {
        @State private var model = ReplyPlayerFixtureModel()

        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Synthetic native reply player").font(.headline)
                    Text(model.ready ? "ready" : "preparing").accessibilityIdentifier("player.fixture.ready")
                    Button("Play synthetic reply") { model.play() }
                        .accessibilityIdentifier("player.fixture.play").disabled(!model.ready)
                    Button("Change visible chat") { model.changeChat() }
                        .accessibilityIdentifier("player.fixture.change-chat")
                    Button("Complete eligible synthetic reply") { model.completeEligible() }
                        .accessibilityIdentifier("player.fixture.complete")
                    Text(model.visibleChat).accessibilityIdentifier("player.fixture.visible-chat")
                    Text(model.session.current?.responseID ?? "none").accessibilityIdentifier("player.fixture.current")
                    Text(model.session.phase == .ended ? "ended" : "other").accessibilityIdentifier(
                        "player.fixture.ended")
                    Button("Commit preference checkpoint") { Task { await model.checkpoint() } }
                        .accessibilityIdentifier("player.fixture.checkpoint")
                    Text(model.checkpointState).accessibilityIdentifier("player.fixture.saved")
                    Text(model.metadataPolicy).accessibilityIdentifier("player.fixture.metadata-policy")
                    Text(String(model.rate)).accessibilityIdentifier("player.fixture.rate")
                    Text(String(model.defaultRate)).accessibilityIdentifier("player.fixture.default-rate")
                    Text(String(Int(model.elapsed))).accessibilityIdentifier("player.fixture.elapsed")
                    Text(String(model.bytes)).accessibilityIdentifier("player.fixture.bytes")
                    Text(String(model.preferences.value.playbackSpeed.rawValue))
                        .accessibilityIdentifier("player.fixture.saved-speed")
                    Text(model.issue).accessibilityIdentifier("player.fixture.issue")
                }.padding()
            }
            .safeAreaInset(edge: .bottom) { CompactReplyPlayer(session: model.session).padding(8) }
            .task {
                await model.start()
                while !Task.isCancelled {
                    await model.sample()
                    do { try await Task<Never, Never>.sleep(nanoseconds: 200_000_000) } catch { return }
                }
            }
        }
    }

    @MainActor @Observable private final class ReplyPlayerFixtureModel {
        private static let suite = "cloud.opensecret.maple.native.dev.synthetic-player-proof"
        private(set) var visibleChat = "synthetic-chat"
        private var completionCount = 0
        let session: ReplyListeningSession
        let preferences: PreferencesController
        private let files: TemporarySpeechStore
        var ready = false
        var checkpointState = "not saved"
        var metadataPolicy = "absent"
        var rate = -1.0
        var defaultRate = -1.0
        var elapsed = 0.0
        var bytes = 0
        var issue = "none"
        private var started = false

        init() {
            let scope = AccountScope(apiOrigin: "https://synthetic-player.invalid", principalID: "player-fixture")
            files = TemporarySpeechStore(
                root: FileManager.default.temporaryDirectory.appendingPathComponent(
                    "MapleSyntheticPlayer", isDirectory: true))
            preferences = PreferencesController(scope: scope, store: PreferencesStore(suiteName: Self.suite))
            let player = NativeSpeechPlayer()
            session = ReplyListeningSession(scope: scope, preparer: ReplyTonePreparer(files: files), player: player)
            player.bind(to: session)
            session.setVisibility(chatID: "synthetic-chat", foreground: true)
            preferences.onChange = { [weak session = self.session] value in
                session?.updatePreferences(voice: value.voice, speed: value.playbackSpeed)
            }
            session.onSpeedChanged = { [weak preferences = self.preferences] speed in
                preferences?.setPlaybackSpeed(speed)
            }
        }

        func start() async {
            guard !started else { return }; started = true
            if ProcessInfo.processInfo.arguments.contains("--maple-player-fixture-reset") {
                UserDefaults.standard.removePersistentDomain(forName: Self.suite)
            }
            do {
                try await files.startupCleanup()
                await preferences.load()
                guard preferences.state == .ready else { issue = "preferences unavailable"; return }
                session.updatePreferences(voice: preferences.value.voice, speed: preferences.value.playbackSpeed)
                ready = true
            } catch { issue = "storage unavailable" }
        }
        func play() {
            guard ready else { return }
            checkpointState = "not saved"
            session.playReply(
                SpeechReply(chatID: visibleChat, responseID: UUID().uuidString, text: "Synthetic reply only."))
        }
        func changeChat() {
            visibleChat = visibleChat == "synthetic-chat" ? "synthetic-other-chat" : "synthetic-chat"
            session.setVisibility(chatID: visibleChat, foreground: true)
        }
        func completeEligible() {
            guard ready else { return }; completionCount += 1
            let response = "automatic-reply-\(completionCount)"
            session.completed(
                SpeechReply(
                    chatID: visibleChat, responseID: response, text: "Synthetic eligible completion.",
                    turnID: UUID().uuidString.lowercased()),
                wasVisible: true, wasForeground: true)
        }

        func checkpoint() async {
            await preferences.flush()
            checkpointState = preferences.saveIssue == nil ? "saved" : "failed"
        }
        func sample() async {
            bytes = await files.allocatedBytes
            let values = MPNowPlayingInfoCenter.default().nowPlayingInfo
            let allowed: Set<String> = [
                MPMediaItemPropertyTitle, MPMediaItemPropertyPlaybackDuration,
                MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
                MPNowPlayingInfoPropertyDefaultPlaybackRate,
            ]
            if let values {
                let duration = (values[MPMediaItemPropertyPlaybackDuration] as? NSNumber)?.doubleValue
                metadataPolicy =
                    Set(values.keys) == allowed && values[MPMediaItemPropertyTitle] as? String == "Maple"
                        && duration == 30 ? "Maple-only" : "unexpected"
                rate = (values[MPNowPlayingInfoPropertyPlaybackRate] as? NSNumber)?.doubleValue ?? -1
                defaultRate = (values[MPNowPlayingInfoPropertyDefaultPlaybackRate] as? NSNumber)?.doubleValue ?? -1
                elapsed = (values[MPNowPlayingInfoPropertyElapsedPlaybackTime] as? NSNumber)?.doubleValue ?? 0
            } else {
                metadataPolicy = "absent"; rate = -1; defaultRate = -1; elapsed = 0
            }
        }
    }

    private actor ReplyTonePreparer: SpeechPreparing {
        let files: TemporarySpeechStore
        init(files: TemporarySpeechStore) { self.files = files }
        func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) async throws -> PreparedSpeech {
            try Task.checkCancellation()
            // Thirty seconds of quiet PCM16 at 16 kHz. It contains no microphone
            // samples or user text; generation runs on this synthesis actor.
            let count = 30 * 16_000
            var pcm = Data(count: count * 2)
            pcm.withUnsafeMutableBytes { buffer in
                let samples = buffer.bindMemory(to: Int16.self)
                for index in 0..<count {
                    samples[index] = Int16(sin(Double(index) * 2 * .pi * 440 / 16_000) * 512).littleEndian
                }
            }
            let wav = try DictationAudio.wav(pcm16Mono: pcm)
            try Task.checkCancellation()
            try await files.activate(owner)
            let body = AsyncThrowingStream<Data, Error> { continuation in
                for offset in stride(from: 0, to: wav.count, by: 64 * 1_024) {
                    continuation.yield(wav.subdata(in: offset..<min(wav.count, offset + 64 * 1_024)))
                }
                continuation.finish()
            }
            let file = try await files.write(body, owner: owner)
            return PreparedSpeech(
                owner: owner, responseID: reply.responseID, chunks: [SpeechChunk(file: file, duration: 30)])
        }
        func discard(_ speech: PreparedSpeech) async throws {
            try await files.remove(speech.chunks.map(\.file), owner: speech.owner)
        }
        func retire(_ owner: SpeechOwner) async throws { try await files.retire(owner) }
    }
#endif
