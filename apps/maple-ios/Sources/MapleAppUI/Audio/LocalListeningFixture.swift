#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI

    /// A local-only request timing seam. Requests and terminal events still come
    /// from the captured SDK service; no response or ownership callback is forged.
    @MainActor @Observable final class LocalListeningRequestGate {
        private(set) var armed = false
        private(set) var waiting = false
        private var valid = true
        private var waiter: (UUID, CheckedContinuation<Void, Error>)?

        func arm() { guard valid, !armed, waiter == nil else { return }; armed = true }
        func release() {
            guard valid else { return }
            armed = false; waiting = false
            let old = waiter; waiter = nil; old?.1.resume()
        }
        func retire() {
            valid = false; armed = false; waiting = false
            let old = waiter; waiter = nil; old?.1.resume(throwing: CancellationError())
        }
        func pass() async throws {
            try Task.checkCancellation()
            guard valid else { throw CancellationError() }
            guard armed else { return }
            guard waiter == nil else { throw ChatIssue.busy }
            let id = UUID()
            try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { continuation in
                    waiter = (id, continuation); waiting = true
                }
            } onCancel: {
                Task { @MainActor [weak self] in self?.cancel(id) }
            }
            try Task.checkCancellation()
            guard valid else { throw CancellationError() }
        }
        private func cancel(_ id: UUID) {
            guard waiter?.0 == id else { return }
            let old = waiter; waiter = nil; armed = false; waiting = false
            old?.1.resume(throwing: CancellationError())
        }
    }

    struct LocalListeningChatService: ChatServing {
        let base: any ChatServing
        let gate: LocalListeningRequestGate
        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
            -> Conversation
        { try await base.createConversation(metadata: metadata, projectId: projectId, pinned: pinned) }
        func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
            try await base.listConversations(parameters)
        }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        {
            try await gate.pass()
            return try await base.createResponse(request, maximumInferenceSends: maximumInferenceSends)
        }
        func retrieveResponse(id: String) async throws -> ResearchResponse { try await base.retrieveResponse(id: id) }
        func cancelResponse(id: String) async throws -> ResearchResponse { try await base.cancelResponse(id: id) }
        func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
            try await base.getConversationItem(conversationId: conversationId, itemId: itemId)
        }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws
            -> APIPage<ConversationItem>
        { try await base.listConversationItems(conversationId: conversationId, parameters: parameters) }
    }

    /// Read-only snapshots at the actual root callbacks. Retains bounded opaque
    /// IDs and state only, never the account principal, prompt, response or audio.
    @MainActor @Observable final class LocalListeningProof {
        let scope: AccountScope
        let requestGate = LocalListeningRequestGate()
        private weak var session: ReplyListeningSession?
        private var pendingTranscripts = 0
        private var finishedTranscripts = 0
        private var acceptedCount = 0
        private var acceptedID = "none"
        private var acceptedChat = "none"
        private var acceptedArm = "none"
        private var acceptedCurrent = "none"
        private var acceptedPhase = "closed"
        private var acceptedHold = "none"
        private var assignedChat = "none"
        private var assignedArm = "none"
        private var completions = 0
        private var completionID = "none"
        private var completionMessage = "none"
        private var completionChat = "none"
        private var completionVisible = false
        private var completionForeground = false

        init(scope: AccountScope, session: ReplyListeningSession) { self.scope = scope; self.session = session }
        func accepted(_ value: ChatSendAcceptance, armedMessageID: String?) {
            guard value.scope == scope, let session else { return }
            acceptedCount += 1; acceptedID = Self.id(value.messageID)
            acceptedChat = Self.id(value.conversationID); acceptedArm = Self.id(armedMessageID)
            acceptedCurrent = Self.id(session.current?.responseID)
            acceptedPhase = Self.phase(session.phase); acceptedHold = Self.hold(session.dictationHold)
        }
        func assigned(scope: AccountScope, chatID: String, armedMessageID: String?) {
            guard scope == self.scope, session != nil else { return }
            assignedChat = Self.id(chatID); assignedArm = Self.id(armedMessageID)
        }
        func completed(_ value: ChatCompletion) {
            guard value.scope == scope, session != nil else { return }
            completions += 1; completionID = Self.id(value.responseID ?? value.runID.uuidString)
            completionMessage = Self.id(value.messageID); completionChat = Self.id(value.conversationID)
            completionVisible = value.wasVisible; completionForeground = value.wasForeground
        }
        func inspectTranscripts(_ count: Int) {
            guard session != nil else { return }; pendingTranscripts = count
        }
        func transcriptFinished(_ count: Int) {
            guard session != nil else { return }; finishedTranscripts += count
        }
        func retire() { requestGate.retire(); session = nil }

        var diagnostics: String {
            let value: [String: Any] = [
                "phase": session.map { Self.phase($0.phase) } ?? "closed",
                "hold": session.map { Self.hold($0.dictationHold) } ?? "none",
                "manual": session?.manualPaused ?? false,
                "current": Self.id(session?.current?.responseID),
                "currentTurn": Self.id(session?.current?.turnID),
                "pending": session?.pending.map { Self.id($0.responseID) } ?? [],
                "pendingTurns": session?.pending.map { Self.id($0.turnID) } ?? [],
                "duration": session?.duration ?? 0, "speed": session?.speed.rawValue ?? 1,
                "position": session?.position ?? 0,
                "autoplay": session?.autoplayChats.isEmpty == false,
                "armed": requestGate.armed, "waiting": requestGate.waiting,
                "pendingTranscripts": pendingTranscripts, "finishedTranscripts": finishedTranscripts,
                "acceptedCount": acceptedCount, "acceptedID": acceptedID,
                "acceptedChat": acceptedChat, "acceptedArm": acceptedArm,
                "acceptedCurrent": acceptedCurrent, "acceptedPhase": acceptedPhase, "acceptedHold": acceptedHold,
                "assignedChat": assignedChat, "assignedArm": assignedArm,
                "completions": completions, "completionID": completionID,
                "completionMessage": completionMessage, "completionChat": completionChat,
                "completionVisible": completionVisible, "completionForeground": completionForeground,
            ]
            guard let data = try? JSONSerialization.data(withJSONObject: value, options: .sortedKeys),
                data.count <= 8_192, let text = String(data: data, encoding: .utf8)
            else { return "unavailable" }
            return text
        }
        private static func id(_ value: String?) -> String { value.map { String($0.prefix(128)) } ?? "none" }
        private static func phase(_ value: ReplyListeningSession.Phase) -> String {
            switch value {
            case .closed: "closed"
            case .preparing: "preparing"
            case .playing: "playing"
            case .paused: "paused"
            case .ended: "ended"
            case .failed: "failed"
            }
        }
        private static func hold(_ value: ReplyListeningSession.DictationHold) -> String {
            switch value {
            case .none: "none"
            case .capture: "capture"
            case .transcription: "transcription"
            case .review: "review"
            }
        }
    }

    struct LocalListeningFixtureControls: View {
        let proof: LocalListeningProof
        let transcriber: LocalLifecycleTranscriber
        var body: some View {
            VStack(spacing: 0) {
                HStack {
                    Button("Transcript") {
                        Task { proof.transcriptFinished(await transcriber.finish()) }
                    }.accessibilityIdentifier("lifecycle.transcript")
                    Button("Inspect") {
                        Task { proof.inspectTranscripts(await transcriber.pendingCount()) }
                    }.accessibilityIdentifier("listening.inspect-transcripts")
                    Button("Hold request") { proof.requestGate.arm() }
                        .disabled(proof.requestGate.armed).accessibilityIdentifier("listening.hold-request")
                    Button("Release") { proof.requestGate.release() }
                        .disabled(!proof.requestGate.waiting).accessibilityIdentifier("listening.release-request")
                }.font(.caption).buttonStyle(.bordered).frame(minHeight: 44)
                Text("Local listening diagnostics").font(.caption2)
                    .accessibilityIdentifier("listening.diagnostics").accessibilityValue(proof.diagnostics)
            }.padding(.horizontal, 4).background(.background)
        }
    }
#endif
