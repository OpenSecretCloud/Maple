import Foundation

public enum ResponseInput: Encodable, Sendable {
    case text(String)
    case messages([ResponseInputMessage])
    public func encode(to encoder: any Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case let .text(text): try value.encode(text)
        case let .messages(messages): try value.encode(messages)
        }
    }
}

public struct ResponseInputMessage: Encodable, Sendable {
    public let role: String
    public let content: [ResponseInputContent]
    public init(role: String = "user", content: [ResponseInputContent]) {
        self.role = role; self.content = content
    }
}

public enum ResponseInputContent: Encodable, Sendable {
    case text(String)
    case image(url: String, detail: String = "auto")
    private enum CodingKeys: String, CodingKey { case type, text, imageUrl, detail }
    public func encode(to encoder: any Encoder) throws {
        var value = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .text(text):
            try value.encode("input_text", forKey: .type)
            try value.encode(text, forKey: .text)
        case let .image(url, detail):
            try value.encode("input_image", forKey: .type)
            try value.encode(url, forKey: .imageUrl)
            try value.encode(detail, forKey: .detail)
        }
    }
}

public struct ResponsesCreateRequest: Encodable, Sendable {
    public var model: String
    public var input: ResponseInput
    public var conversation: String
    public var metadata: [String: JSONValue]?
    public var stream = true
    public var store = true
    public var tools: [ResponseTool]?

    public init(
        model: String = "auto:quick", input: ResponseInput, conversation: String,
        metadata: [String: JSONValue]? = nil, webSearch: Bool = false
    ) {
        self.model = model; self.input = input; self.conversation = conversation
        self.metadata = metadata; self.tools = webSearch ? [ResponseTool()] : nil
    }
}

public struct ResponseTool: Encodable, Sendable {
    public let type = "web_search"
    public init() {}
}

public struct ResponseLifecycleEvent: Decodable, Sendable, Equatable {
    public let response: ResearchResponse
    public let sequenceNumber: Int?
    public let opensecret: ResponseErrorContract?
}

public struct ResponseErrorContract: Decodable, Sendable, Equatable {
    public let errorContract: String
    public let errorCode: String
}

public struct ResponseTextEvent: Decodable, Sendable, Equatable {
    public let itemId: String
    public let outputIndex: Int
    public let contentIndex: Int
    public let sequenceNumber: Int?
    public let delta: String?
    public let text: String?
}

public struct ResponseItemEvent: Decodable, Sendable, Equatable {
    public let outputIndex: Int
    public let item: ConversationItem
    public let sequenceNumber: Int?
}

public struct ResponseContentEvent: Decodable, Sendable, Equatable {
    public let itemId: String
    public let outputIndex: Int
    public let contentIndex: Int
    public let part: MessageContent
    public let sequenceNumber: Int?
}

public struct ResponseWebSearchEvent: Decodable, Sendable, Equatable {
    public let itemId: String
    public let outputIndex: Int
    public let sequenceNumber: Int?
}

public struct ToolCallEvent: Decodable, Sendable, Equatable {
    public let toolCallId: String
    public let outputIndex: Int
    public let name: String
    public let arguments: JSONValue
    public let sequenceNumber: Int?
}

public struct ToolOutputEvent: Decodable, Sendable, Equatable {
    public let toolOutputId: String
    public let toolCallId: String
    public let outputIndex: Int
    public let output: String
    public let sequenceNumber: Int?
}

public struct ResponseCancelledEvent: Decodable, Sendable, Equatable {
    public struct Payload: Decodable, Sendable, Equatable { public let id: String }
    public let id: String?
    public let data: Payload?
    public let response: ResearchResponse?
}

public struct ResponseStreamFailure: Decodable, Sendable, Equatable {
    public let code: String?
    public let message: String?
    public let error: ResponseFailure?
}

public enum ResponseStreamEvent: Sendable, Equatable {
    case created(ResponseLifecycleEvent), inProgress(ResponseLifecycleEvent)
    case outputItemAdded(ResponseItemEvent), outputItemDone(ResponseItemEvent)
    case contentPartAdded(ResponseContentEvent), contentPartDone(ResponseContentEvent)
    case outputTextDelta(ResponseTextEvent), outputTextDone(ResponseTextEvent)
    case reasoningTextDelta(ResponseTextEvent), reasoningTextDone(ResponseTextEvent)
    case webSearchInProgress(ResponseWebSearchEvent), webSearchSearching(ResponseWebSearchEvent), webSearchCompleted(
        ResponseWebSearchEvent)
    case toolCallCreated(ToolCallEvent), toolOutputCreated(ToolOutputEvent)
    case completed(ResponseLifecycleEvent), failed(ResponseLifecycleEvent)
    case cancelled(ResponseCancelledEvent), error(ResponseStreamFailure)
    case unknown(type: String, payload: JSONValue)

    public var isTerminal: Bool {
        switch self {
        case .completed, .failed, .cancelled, .error: true
        default: false
        }
    }
}

extension ResearchAPI {
    /// Event buffering preserves every event across consumer stalls; SSE event
    /// sizes remain bounded and cancellation cancels the network consumer. A
    /// terminal event is withheld until authenticated transport EOF is verified.
    public func createResponse(
        _ request: ResponsesCreateRequest,
        maximumInferenceSends: Int = 2
    ) async throws -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        guard request.stream, maximumInferenceSends == 1 || maximumInferenceSends == 2 else {
            throw ResearchAPIError.invalidArgument
        }
        let response = try await checked(
            try await transport.send(
                MapleRequest(
                    method: .post, target: "/v1/responses",
                    headers: ["content-type": "application/json", "accept": "text/event-stream"],
                    body: try APIJSON.encode(request), expectedPrincipalId: expectedPrincipalId,
                    maximumInferenceSends: maximumInferenceSends)))
        guard
            response.header("content-type")?.split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                .lowercased() == "text/event-stream"
        else {
            throw ResearchAPIError.invalidResponse
        }
        return AsyncThrowingStream(bufferingPolicy: .unbounded) { continuation in
            let task = Task {
                do {
                    var decoder = ResponsesSSEDecoder()
                    for try await chunk in response.body {
                        try Task.checkCancellation()
                        for event in try decoder.append(chunk) { continuation.yield(event) }
                    }
                    try Task.checkCancellation()
                    for event in try decoder.finish() { continuation.yield(event) }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
            continuation.onTermination = { @Sendable _ in task.cancel() }
        }
    }
}

/// Incremental SSE parser, independent of network chunk boundaries and UTF-8
/// boundaries. Unknown event types remain forward-compatible but cannot finish
/// a response. A bare [DONE], malformed JSON, or EOF cannot manufacture success.
public struct ResponsesSSEDecoder: Sendable {
    private var buffer = Data()
    private var eventName: String?
    private var dataLines: [String] = []
    private var eventBytes = 0
    private var terminal: ResponseStreamEvent?
    private var sawDone = false
    private var finished = false
    public let maximumEventBytes: Int

    public init(maximumEventBytes: Int = 16 * 1_024 * 1_024) {
        self.maximumEventBytes = maximumEventBytes
    }

    public mutating func append(_ chunk: Data) throws -> [ResponseStreamEvent] {
        guard !finished else { throw ResearchAPIError.invalidStream }
        var events: [ResponseStreamEvent] = []
        // Process one byte at a time so a large transport chunk containing many
        // small events does not count as one over-sized SSE event.
        for byte in chunk {
            if buffer.last == 13 {
                buffer.removeLast()
                if let event = try consumeLine(buffer) { events.append(event) }
                buffer.removeAll(keepingCapacity: true)
                if byte == 10 { continue }
            }
            if byte == 10 {
                if let event = try consumeLine(buffer) { events.append(event) }
                buffer.removeAll(keepingCapacity: true)
            } else {
                guard buffer.count < maximumEventBytes - eventBytes else { throw ResearchAPIError.responseTooLarge }
                buffer.append(byte)
            }
        }
        return events
    }

    public mutating func finish() throws -> [ResponseStreamEvent] {
        guard !finished else { throw ResearchAPIError.invalidStream }
        finished = true
        if buffer.last == 13 {
            buffer.removeLast()
            _ = try consumeLine(buffer)
            buffer.removeAll()
        }
        guard buffer.isEmpty, dataLines.isEmpty, eventName == nil, let terminal else {
            throw ResearchAPIError.truncatedStream
        }
        return [terminal]
    }

    private mutating func consumeLine(_ data: Data) throws -> ResponseStreamEvent? {
        guard let line = String(data: data, encoding: .utf8) else { throw ResearchAPIError.invalidStream }
        if line.isEmpty {
            defer { eventName = nil; dataLines.removeAll(keepingCapacity: true); eventBytes = 0 }
            guard !dataLines.isEmpty else { return nil }
            let text = dataLines.joined(separator: "\n")
            if text == "[DONE]" {
                guard terminal != nil, !sawDone else { throw ResearchAPIError.invalidStream }
                sawDone = true
                return nil
            }
            guard terminal == nil, !sawDone else { throw ResearchAPIError.invalidStream }
            let event = try Self.decode(name: eventName, data: Data(text.utf8))
            if event.isTerminal { terminal = event; return nil }
            return event
        }
        if line.hasPrefix(":") { return nil }
        let parts = line.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
        let name = String(parts[0])
        var value = parts.count > 1 ? String(parts[1]) : ""
        if value.hasPrefix(" ") { value.removeFirst() }
        eventBytes += data.count + 1
        guard eventBytes <= maximumEventBytes else { throw ResearchAPIError.responseTooLarge }
        switch name {
        case "event": eventName = value
        case "data": dataLines.append(value)
        default: break  // id/retry are irrelevant; reconnecting would replay inference.
        }
        return nil
    }

    private static func decode(name: String?, data: Data) throws -> ResponseStreamEvent {
        struct Envelope: Decodable { let type: String }
        let decoder = APIJSON.decoder()
        do {
            let type = try decoder.decode(Envelope.self, from: data).type
            guard name == nil || name == type else { throw ResearchAPIError.invalidStream }
            func parse<T: Decodable>(_ type: T.Type) throws -> T { try decoder.decode(type, from: data) }
            switch type {
            case "response.created": return .created(try parse(ResponseLifecycleEvent.self))
            case "response.in_progress": return .inProgress(try parse(ResponseLifecycleEvent.self))
            case "response.output_item.added": return .outputItemAdded(try parse(ResponseItemEvent.self))
            case "response.output_item.done": return .outputItemDone(try parse(ResponseItemEvent.self))
            case "response.content_part.added": return .contentPartAdded(try parse(ResponseContentEvent.self))
            case "response.content_part.done": return .contentPartDone(try parse(ResponseContentEvent.self))
            case "response.output_text.delta", "response.reasoning_text.delta":
                let event = try parse(ResponseTextEvent.self)
                guard event.delta != nil else { throw ResearchAPIError.invalidStream }
                return type == "response.output_text.delta" ? .outputTextDelta(event) : .reasoningTextDelta(event)
            case "response.output_text.done", "response.reasoning_text.done":
                let event = try parse(ResponseTextEvent.self)
                guard event.text != nil else { throw ResearchAPIError.invalidStream }
                return type == "response.output_text.done" ? .outputTextDone(event) : .reasoningTextDone(event)
            case "response.web_search_call.in_progress":
                return .webSearchInProgress(try parse(ResponseWebSearchEvent.self))
            case "response.web_search_call.searching":
                return .webSearchSearching(try parse(ResponseWebSearchEvent.self))
            case "response.web_search_call.completed":
                return .webSearchCompleted(try parse(ResponseWebSearchEvent.self))
            case "tool_call.created": return .toolCallCreated(try parse(ToolCallEvent.self))
            case "tool_output.created": return .toolOutputCreated(try parse(ToolOutputEvent.self))
            case "response.completed":
                let event = try parse(ResponseLifecycleEvent.self)
                guard event.response.status == .completed else { throw ResearchAPIError.invalidStream }
                return .completed(event)
            case "response.failed":
                let event = try parse(ResponseLifecycleEvent.self)
                guard event.response.status == .failed else { throw ResearchAPIError.invalidStream }
                return .failed(event)
            case "response.cancelled":
                let event = try parse(ResponseCancelledEvent.self)
                guard event.data?.id != nil || event.response?.status == .cancelled else {
                    throw ResearchAPIError.invalidStream
                }
                return .cancelled(event)
            case "error", "response.error": return .error(try parse(ResponseStreamFailure.self))
            default: return .unknown(type: type, payload: try JSONDecoder().decode(JSONValue.self, from: data))
            }
        } catch { throw ResearchAPIError.invalidStream }
    }
}
