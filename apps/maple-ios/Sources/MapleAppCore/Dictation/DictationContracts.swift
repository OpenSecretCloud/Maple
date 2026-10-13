import Foundation
import MapleSDK

public enum DictationIssue: String, Error, Sendable {
    case permissionDenied, captureUnavailable, emptyRecording, recordingTooLarge
    case transcriptionUnavailable, emptyTranscription, invalidTranscription, tooManyOperations, draftUnavailable
}

public enum DictationPhase: String, Sendable {
    case requestingPermission, recording, transcribing, waitingForComposition, waitingForInsertionPoint, failed
}

public struct DictationStatus: Sendable, Equatable {
    public let draftID: UUID
    public var phase: DictationPhase
    public var elapsedSeconds: Int
    public var issue: DictationIssue?
}

/// A view must obtain all ranges from this exact text. A stale editor snapshot
/// cannot move the marker for a newer draft revision.
public struct DictationEditorState: Sendable, Equatable {
    public var text: String
    public var selectionUTF16: NSRange
    public var markedRangeUTF16: NSRange?

    public init(text: String, selectionUTF16: NSRange, markedRangeUTF16: NSRange? = nil) {
        self.text = text; self.selectionUTF16 = selectionUTF16; self.markedRangeUTF16 = markedRangeUTF16
    }
}

/// The actual text-input transaction, needed to distinguish identical-looking
/// edits at different positions (for example deleting one of repeated letters).
public struct DictationTextEdit: Sendable, Equatable {
    public var beforeText: String
    public var rangeUTF16: NSRange
    public var replacement: String
    public init(beforeText: String, rangeUTF16: NSRange, replacement: String) {
        self.beforeText = beforeText; self.rangeUTF16 = rangeUTF16; self.replacement = replacement
    }
    public var resultingText: String? {
        guard rangeUTF16.location != NSNotFound, rangeUTF16.location >= 0, rangeUTF16.length >= 0,
            rangeUTF16.location <= beforeText.utf16.count,
            rangeUTF16.length <= beforeText.utf16.count - rangeUTF16.location,
            let range = Range(rangeUTF16, in: beforeText)
        else { return nil }
        return beforeText.replacingCharacters(in: range, with: replacement)
    }
}

/// The adapter owns one microphone capture. Stop synchronously detaches its
/// buffers before returning; only in-memory little-endian 16 kHz mono PCM16
/// crosses this boundary. Every operation checks its unique capture ID.
@MainActor public protocol DictationCapturing: AnyObject {
    func requestPermission() async -> Bool
    func start(id: UUID, maximumFrames: Int, onLimit: @escaping @MainActor @Sendable () -> Void) throws
    func stop(id: UUID) throws -> Data
    func cancel(id: UUID)
}

public protocol DictationTranscribing: Sendable {
    func transcribe(wav: Data) async throws -> String
}

public struct SDKDictationService: DictationTranscribing {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func transcribe(wav: Data) async throws -> String {
        try await api.transcribeAudio(
            data: wav, filename: "dictation.wav", contentType: "audio/wav", model: "whisper-large-v3"
        ).text
    }
}

public enum DictationAudio {
    public static let sampleRate = 16_000
    public static let maximumSeconds = 300
    public static let maximumFrames = sampleRate * maximumSeconds
    public static let maximumPCMBytes = maximumFrames * 2

    /// Call away from the UI actor. Neither PCM nor its WAV container is saved.
    public static func wav(pcm16Mono data: Data) throws -> Data {
        guard !data.isEmpty else { throw DictationIssue.emptyRecording }
        guard data.count <= maximumPCMBytes, data.count.isMultiple(of: 2) else {
            throw DictationIssue.recordingTooLarge
        }
        var result = Data(); result.reserveCapacity(data.count + 44)
        func append(_ value: UInt32) {
            var little = value.littleEndian
            withUnsafeBytes(of: &little) { result.append(contentsOf: $0) }
        }
        func short(_ value: UInt16) {
            var little = value.littleEndian
            withUnsafeBytes(of: &little) { result.append(contentsOf: $0) }
        }
        result.append(contentsOf: "RIFF".utf8); append(UInt32(data.count + 36))
        result.append(contentsOf: "WAVEfmt ".utf8); append(16); short(1); short(1)
        append(UInt32(sampleRate)); append(UInt32(sampleRate * 2)); short(2); short(16)
        result.append(contentsOf: "data".utf8); append(UInt32(data.count)); result.append(data)
        return result
    }
}
