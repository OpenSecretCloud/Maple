#if os(iOS)
    import AVFoundation
    import Foundation
    import MapleAppCore
    import MediaPlayer

    /// A single owner transfers these read-only handles from the file-loading
    /// actor to the main-actor engine. The loader retains no reference after return.
    private final class OpenSpeechFile: @unchecked Sendable {
        let file: AVAudioFile
        init(_ file: AVAudioFile) { self.file = file }
    }

    private actor NativeSpeechFileLoader {
        func load(_ chunks: [SpeechChunk]) throws -> [OpenSpeechFile] {
            guard !chunks.isEmpty, chunks.count <= SpeechLimits.maximumChunks else { throw SpeechIssue.invalidAudio }
            return try chunks.map { chunk in
                try Task.checkCancellation()
                let file = try AVAudioFile(forReading: chunk.file.url)
                let duration = Double(file.length) / file.processingFormat.sampleRate
                guard file.length > 0, file.length <= AVAudioFramePosition(UInt32.max),
                    file.processingFormat.sampleRate.isFinite,
                    (8_000...192_000).contains(file.processingFormat.sampleRate),
                    (1...2).contains(file.processingFormat.channelCount),
                    duration.isFinite, abs(duration - chunk.duration) < 0.01
                else { throw SpeechIssue.invalidAudio }
                return OpenSpeechFile(file)
            }
        }
    }

    public actor NativeSpeechInspector: SpeechMediaInspecting {
        public init() {}
        public func duration(of file: StoredSpeechFile) throws -> TimeInterval {
            let audio = try AVAudioFile(forReading: file.url)
            let rate = audio.processingFormat.sampleRate
            guard rate.isFinite, (8_000...192_000).contains(rate),
                (1...2).contains(audio.processingFormat.channelCount),
                audio.length > 0, audio.length <= AVAudioFramePosition(UInt32.max)
            else { throw SpeechIssue.invalidAudio }
            return Double(audio.length) / rate
        }
    }

    /// Completed files are scheduled before Play. The time-pitch unit applies the
    /// saved rate exactly once with pitch zero; synthesis always requested rate 1.
    @MainActor public final class NativeSpeechPlayer: SpeechPlaying {
        public var onEvent: (@MainActor (SpeechPlayerEvent) -> Void)?
        public private(set) var duration: TimeInterval = 0
        private let engine = AVAudioEngine()
        private let node = AVAudioPlayerNode()
        private let timePitch = AVAudioUnitTimePitch()
        private let loader = NativeSpeechFileLoader()
        private var files: [OpenSpeechFile] = []
        private var basePosition: TimeInterval = 0
        private var pausedPosition: TimeInterval = 0
        private var isPlaying = false
        private var needsReschedule = false
        private var speed: MaplePlaybackSpeed = .normal
        private var generation = UUID()
        private var playbackID: UUID?
        private var listeningID: UUID?
        private weak var listening: ReplyListeningSession?
        private var targets: [(MPRemoteCommand, Any)] = []
        private var notifications: [NSObjectProtocol] = []
        private var metadataTask: Task<Void, Never>?

        public init() {
            engine.attach(node); engine.attach(timePitch)
            timePitch.pitch = 0
        }
        public func bind(to session: ReplyListeningSession) { listening = session }

        public var position: TimeInterval {
            guard isPlaying, let render = node.lastRenderTime, let time = node.playerTime(forNodeTime: render),
                time.sampleRate > 0
            else { return pausedPosition }
            return min(duration, max(0, basePosition + Double(time.sampleTime) / time.sampleRate))
        }

        public func load(_ speech: PreparedSpeech, playbackID: UUID) async throws {
            stop()
            let ticket = generation
            let loaded = try await loader.load(speech.chunks)
            guard generation == ticket else { throw SpeechIssue.retired }
            guard let first = loaded.first?.file.processingFormat,
                loaded.allSatisfy({
                    $0.file.processingFormat.sampleRate == first.sampleRate
                        && $0.file.processingFormat.channelCount == first.channelCount
                })
            else { throw SpeechIssue.invalidAudio }
            files = loaded; duration = speech.duration; self.playbackID = playbackID;
            listeningID = speech.owner.sessionID
            engine.disconnectNodeOutput(node); engine.disconnectNodeOutput(timePitch)
            engine.connect(node, to: timePitch, format: first)
            engine.connect(timePitch, to: engine.mainMixerNode, format: first)
            timePitch.pitch = 0; timePitch.rate = Float(speed.rawValue)
            schedule(from: 0)
            installCommands(playbackID: playbackID, listeningID: speech.owner.sessionID)
            installNotifications(playbackID: playbackID)
        }

        public func play() throws {
            guard playbackID != nil, !files.isEmpty else { throw SpeechIssue.invalidAudio }
            if pausedPosition >= duration { if let playbackID { onEvent?(.ended(playbackID)) }; return }
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .spokenAudio, options: [])
            try session.setActive(true)
            if !engine.isRunning { try engine.start() }
            if needsReschedule { schedule(from: pausedPosition); needsReschedule = false }
            isPlaying = true; node.play(); publishMetadata()
            startMetadataUpdates()
        }
        public func pause() {
            pausedPosition = position; node.pause(); isPlaying = false
            publishMetadata()
        }
        public func stop() {
            generation = UUID(); playbackID = nil; listeningID = nil
            metadataTask?.cancel(); metadataTask = nil
            node.stop(); engine.stop(); engine.reset()
            files = []; duration = 0; basePosition = 0; pausedPosition = 0; isPlaying = false
            needsReschedule = false
            for (command, target) in targets { command.removeTarget(target) }; targets = []
            for token in notifications { NotificationCenter.default.removeObserver(token) }; notifications = []
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            MPNowPlayingInfoCenter.default().playbackState = .stopped
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        public func seek(to position: TimeInterval) throws {
            guard playbackID != nil, position.isFinite else { throw SpeechIssue.invalidAudio }
            let resume = isPlaying
            node.stop(); isPlaying = false
            schedule(from: min(duration, max(0, position)))
            needsReschedule = false
            if resume { try play() } else { publishMetadata() }
        }
        public func setSpeed(_ speed: MaplePlaybackSpeed) {
            self.speed = speed; timePitch.rate = Float(speed.rawValue); publishMetadata()
        }

        private func schedule(from position: TimeInterval) {
            basePosition = position; pausedPosition = position
            let scheduleID = UUID(); generation = scheduleID
            guard let playbackID else { return }
            var skipped: TimeInterval = 0
            for (index, opened) in files.enumerated() {
                let file = opened.file, rate = file.processingFormat.sampleRate
                let length = Double(file.length) / rate
                defer { skipped += length }
                guard position < skipped + length else { continue }
                let startingFrame = min(file.length, AVAudioFramePosition(max(0, position - skipped) * rate))
                let frames = AVAudioFrameCount(file.length - startingFrame)
                guard frames > 0 else { continue }
                node.scheduleSegment(
                    file, startingFrame: startingFrame, frameCount: frames, at: nil,
                    completionCallbackType: .dataPlayedBack
                ) { [weak self] _ in
                    Task { @MainActor [weak self] in
                        guard let self, self.generation == scheduleID, self.playbackID == playbackID,
                            index == self.files.count - 1
                        else { return }
                        self.pausedPosition = self.duration; self.isPlaying = false
                        self.publishMetadata(); self.onEvent?(.ended(playbackID))
                    }
                }
            }
        }

        private func startMetadataUpdates() {
            guard metadataTask == nil else { return }
            let id = playbackID
            metadataTask = Task { [weak self] in
                while !Task.isCancelled {
                    do { try await Task.sleep(for: .milliseconds(500)) } catch { return }
                    guard let self, self.playbackID == id else { return }
                    self.publishMetadata(); self.listening?.refreshPosition()
                }
            }
        }
        private func publishMetadata() {
            guard playbackID != nil else { return }
            MPNowPlayingInfoCenter.default().nowPlayingInfo = [
                MPMediaItemPropertyTitle: "Maple",
                MPMediaItemPropertyPlaybackDuration: duration,
                MPNowPlayingInfoPropertyElapsedPlaybackTime: position,
                MPNowPlayingInfoPropertyPlaybackRate: isPlaying ? speed.rawValue : 0,
                MPNowPlayingInfoPropertyDefaultPlaybackRate: speed.rawValue,
            ]
            MPNowPlayingInfoCenter.default().playbackState = isPlaying ? .playing : .paused
        }
        private func installCommands(playbackID: UUID, listeningID: UUID) {
            let center = MPRemoteCommandCenter.shared()
            center.skipBackwardCommand.preferredIntervals = [10]
            center.skipForwardCommand.preferredIntervals = [10]
            let commands: [(MPRemoteCommand, SpeechRemoteCommand)] = [
                (center.playCommand, .play), (center.pauseCommand, .pause), (center.togglePlayPauseCommand, .toggle),
                (center.skipBackwardCommand, .backwardTenSeconds), (center.skipForwardCommand, .forwardTenSeconds),
            ]
            for (command, action) in commands {
                command.isEnabled = true
                let target = command.addTarget { [weak self] _ in
                    Self.onMain {
                        guard let self, self.playbackID == playbackID else { return false }
                        return self.listening?.remote(action, sessionID: listeningID) ?? false
                    } ? .success : .commandFailed
                }
                targets.append((command, target))
            }
        }
        private nonisolated static func onMain(_ action: @MainActor @Sendable () -> Bool) -> Bool {
            if Thread.isMainThread { return MainActor.assumeIsolated { action() } }
            return DispatchQueue.main.sync { MainActor.assumeIsolated { action() } }
        }
        private func installNotifications(playbackID: UUID) {
            let center = NotificationCenter.default
            notifications.append(
                center.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) {
                    [weak self] _ in
                    Task { @MainActor [weak self] in
                        guard let self, self.playbackID == playbackID, !self.engine.isRunning else { return }
                        self.pausedPosition = self.position
                        self.generation = UUID(); self.isPlaying = false; self.needsReschedule = true
                        self.node.stop()
                        // A graph or route reconfiguration must not restart a
                        // previously playing reply without a new explicit Play.
                        if let listening = self.listening, listening.dictationHold == .none {
                            listening.routeDisconnected()
                        }
                    }
                })
            notifications.append(
                center.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) {
                    [weak self] note in
                    let raw = (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? NSNumber)?.uintValue
                    Task { @MainActor [weak self] in
                        guard let self, self.playbackID == playbackID else { return }
                        if raw == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue {
                            self.listening?.routeDisconnected()
                        }
                        // A new route alone deliberately never resumes playback.
                    }
                })
            notifications.append(
                center.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) {
                    [weak self] note in
                    let raw = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? NSNumber)?.uintValue
                    let options = (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? NSNumber)?.uintValue ?? 0
                    Task { @MainActor [weak self] in
                        guard let self, self.playbackID == playbackID else { return }
                        if raw == AVAudioSession.InterruptionType.began.rawValue { self.listening?.interruptionBegan() }
                        if raw == AVAudioSession.InterruptionType.ended.rawValue {
                            self.listening?.interruptionEnded(
                                shouldResume: AVAudioSession.InterruptionOptions(rawValue: options).contains(
                                    .shouldResume))
                        }
                    }
                })
        }
    }
#endif
