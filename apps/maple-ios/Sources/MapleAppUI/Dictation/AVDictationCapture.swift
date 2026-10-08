#if os(iOS)
    import AVFoundation
    import Foundation
    import MapleAppCore
    import Synchronization

    /// Uses an audio-engine tap and bounded memory. It never creates a recording
    /// URL or writes microphone samples to a file or protected draft store.
    @MainActor public final class AVDictationCapture: DictationCapturing {
        private var active: Capture?

        private struct BufferState {
            let converter: AVAudioConverter
            let output: AVAudioFormat
            let maximumBytes: Int
            var pcm = Data()
            var reachedLimit = false
            var failed = false
        }

        private struct Capture {
            let id: UUID
            let engine: AVAudioEngine
            let buffer: CaptureBuffer
            let interruption: NSObjectProtocol
            let routeChange: NSObjectProtocol
        }

        private final class CaptureBuffer: Sendable {
            let state: Mutex<BufferState>
            init(_ state: sending BufferState) { self.state = Mutex(state) }
        }

        /// AVAudioNode lends this buffer to the tap. The unchecked bridge retains
        /// it read-only, solely for synchronous conversion before the tap returns;
        /// neither samples nor this wrapper enter a Task or the active capture.
        /// AVAudioConverter requires stable bytes until its next input callback
        /// or conversion completion. Only one-shot availability is mutable.
        private final class ConversionInput: @unchecked Sendable {
            private let samples: AVAudioPCMBuffer
            private let available = Mutex(true)
            init(_ samples: AVAudioPCMBuffer) { self.samples = samples }
            func take() -> AVAudioPCMBuffer? {
                let first = available.withLock { value in
                    guard value else { return false }
                    value = false; return true
                }
                return first ? samples : nil
            }
        }

        public init() {}

        public func requestPermission() async -> Bool {
            await AVAudioApplication.requestRecordPermission()
        }

        public func start(
            id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void
        ) throws {
            guard active == nil, maximumFrames > 0, maximumFrames <= DictationAudio.maximumFrames else {
                throw DictationIssue.captureUnavailable
            }
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playAndRecord, mode: .measurement, options: [.defaultToSpeaker])
            try session.setActive(true)
            let engine = AVAudioEngine()
            let input = engine.inputNode
            let inputFormat = input.outputFormat(forBus: 0)
            guard inputFormat.sampleRate > 0, inputFormat.channelCount > 0,
                let output = AVAudioFormat(
                    commonFormat: .pcmFormatInt16, sampleRate: Double(DictationAudio.sampleRate),
                    channels: 1, interleaved: false),
                let converter = AVAudioConverter(from: inputFormat, to: output)
            else {
                try? session.setActive(false, options: .notifyOthersOnDeactivation)
                throw DictationIssue.captureUnavailable
            }
            let buffer = CaptureBuffer(
                BufferState(converter: converter, output: output, maximumBytes: maximumFrames * 2))
            input.installTap(onBus: 0, bufferSize: 2_048, format: inputFormat) { @Sendable samples, _ in
                let shouldStop = buffer.state.withLock { state -> Bool in
                    guard !state.reachedLimit, !state.failed else { return false }
                    let capacity = AVAudioFrameCount(
                        ceil(
                            Double(samples.frameLength) * Double(DictationAudio.sampleRate) / samples.format.sampleRate)
                            + 32)
                    guard let converted = AVAudioPCMBuffer(pcmFormat: state.output, frameCapacity: capacity) else {
                        state.failed = true; return true
                    }
                    let input = ConversionInput(samples)
                    var error: NSError?
                    let status = state.converter.convert(to: converted, error: &error) { _, inputStatus in
                        guard let samples = input.take() else { inputStatus.pointee = .noDataNow; return nil }
                        inputStatus.pointee = .haveData; return samples
                    }
                    guard status != .error, error == nil, let channel = converted.int16ChannelData?.pointee else {
                        state.failed = true; return true
                    }
                    let count = min(Int(converted.frameLength) * 2, state.maximumBytes - state.pcm.count)
                    if count > 0 {
                        state.pcm.append(
                            contentsOf: UnsafeBufferPointer(
                                start: UnsafeRawPointer(channel).assumingMemoryBound(to: UInt8.self), count: count))
                    }
                    if state.pcm.count == state.maximumBytes { state.reachedLimit = true; return true }
                    return false
                }
                if shouldStop { Task { @MainActor in onLimit() } }
            }
            let interruption = NotificationCenter.default.addObserver(
                forName: AVAudioSession.interruptionNotification, object: session, queue: nil
            ) { _ in Task { @MainActor in onLimit() } }
            let routeChange = NotificationCenter.default.addObserver(
                forName: AVAudioSession.routeChangeNotification, object: session, queue: nil
            ) { _ in Task { @MainActor in onLimit() } }
            active = Capture(
                id: id, engine: engine, buffer: buffer, interruption: interruption, routeChange: routeChange)
            do { engine.prepare(); try engine.start() } catch {
                cancel(id: id); throw DictationIssue.captureUnavailable
            }
        }

        public func stop(id: UUID) throws -> Data {
            guard let capture = detach(id: id) else { throw DictationIssue.captureUnavailable }
            return try capture.buffer.state.withLock { state in
                guard !state.failed else { throw DictationIssue.captureUnavailable }
                let pcm = state.pcm; state.pcm = Data(); return pcm
            }
        }

        public func cancel(id: UUID) {
            guard let capture = detach(id: id) else { return }
            capture.buffer.state.withLock { $0.pcm = Data() }
        }

        private func detach(id: UUID) -> Capture? {
            guard let capture = active, capture.id == id else { return nil }
            active = nil
            NotificationCenter.default.removeObserver(capture.interruption)
            NotificationCenter.default.removeObserver(capture.routeChange)
            capture.engine.inputNode.removeTap(onBus: 0)
            capture.engine.stop()
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
            return capture
        }
    }
#endif
