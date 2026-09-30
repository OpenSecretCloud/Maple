import Foundation
import Testing
@testable import MapleSDK

private func lifecycle(_ type: String, status: String) -> String {
    "{\"type\":\"\(type)\",\"sequence_number\":1,\"response\":{\"id\":\"response-1\",\"object\":\"response\",\"created_at\":1,\"status\":\"\(status)\",\"model\":\"auto:quick\",\"output\":[]}}"
}
private func sse(_ type: String, _ json: String, newline: String = "\n") -> String {
    "event: \(type)\(newline)data: \(json)\(newline)\(newline)"
}
private let delta =
    #"{"type":"response.output_text.delta","item_id":"item-1","output_index":0,"content_index":0,"sequence_number":2,"delta":"Hello 🌍"}"#

@Test func sseHandlesEveryByteSplitAndWithholdsTerminalUntilEOF() throws {
    let fixture =
        sse("response.created", lifecycle("response.created", status: "in_progress"))
        + sse("response.output_text.delta", delta)
        + sse("response.completed", lifecycle("response.completed", status: "completed"))
        + "data: [DONE]\n\n"
    let bytes = Data(fixture.utf8)
    for split in 0...bytes.count {
        var decoder = ResponsesSSEDecoder()
        let first = try decoder.append(bytes.prefix(split))
        let second = try decoder.append(bytes.dropFirst(split))
        #expect((first + second).count == 2)
        #expect(!(first + second).contains(where: { $0.isTerminal }))
        let terminal = try decoder.finish()
        #expect(terminal.count == 1 && terminal[0].isTerminal)
    }
}

@Test(arguments: ["\n", "\r\n", "\r"])
func sseNewlineCommentsAndBytewiseUTF8(_ newline: String) throws {
    let fixture =
        ": heartbeat\(newline)\(newline)" + sse("response.output_text.delta", delta, newline: newline)
        + sse("response.completed", lifecycle("response.completed", status: "completed"), newline: newline)
    var decoder = ResponsesSSEDecoder()
    var events: [ResponseStreamEvent] = []
    for byte in fixture.utf8 { events += try decoder.append(Data([byte])) }
    events += try decoder.finish()
    #expect(events.count == 2)
    guard case let .outputTextDelta(text) = events[0] else { Issue.record("Expected delta"); return }
    #expect(text.delta == "Hello 🌍")
}

@Test func sseRejectsTruncationMalformedPayloadAndPostTerminalData() throws {
    let complete = sse("response.completed", lifecycle("response.completed", status: "completed"))
    for text in ["", sse("response.created", lifecycle("response.created", status: "in_progress")), "data: [DONE]\n\n"]
    {
        #expect(throws: ResearchAPIError.self) {
            var decoder = ResponsesSSEDecoder()
            _ = try decoder.append(Data(text.utf8)); _ = try decoder.finish()
        }
    }
    for text in [
        complete + sse("response.output_text.delta", delta), complete + complete,
        sse("response.completed", lifecycle("response.completed", status: "in_progress")),
        sse("response.completed", "not json"), sse("response.completed", delta),
        "data: {\"type\":\"response.output_text.delta\"}\n\n",
    ] {
        #expect(throws: ResearchAPIError.invalidStream) {
            var decoder = ResponsesSSEDecoder(); _ = try decoder.append(Data(text.utf8)); _ = try decoder.finish()
        }
    }
    #expect(throws: ResearchAPIError.responseTooLarge) {
        var decoder = ResponsesSSEDecoder(maximumEventBytes: 8)
        _ = try decoder.append(Data("data: 123456789".utf8))
    }
    #expect(throws: ResearchAPIError.invalidStream) {
        var decoder = ResponsesSSEDecoder()
        _ = try decoder.append(Data([100, 97, 116, 97, 58, 32, 0xFF, 10]))
    }
}

@Test func sseDecodesResearchEventSurfaceAndRetainsUnknownEvents() throws {
    let textBody = #""item_id":"i","output_index":0,"content_index":0,"delta":"D","text":"T""#
    let itemBody =
        #""output_index":0,"item":{"id":"i","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"T"}]}"#
    let partBody = #""item_id":"i","output_index":0,"content_index":0,"part":{"type":"output_text","text":"T"}"#
    let fixtures: [(String, String)] = [
        ("response.output_item.added", itemBody), ("response.output_item.done", itemBody),
        ("response.content_part.added", partBody), ("response.content_part.done", partBody),
        ("response.output_text.delta", textBody), ("response.output_text.done", textBody),
        ("response.reasoning_text.delta", textBody), ("response.reasoning_text.done", textBody),
        ("response.web_search_call.in_progress", #""item_id":"i","output_index":0"#),
        ("response.web_search_call.searching", #""item_id":"i","output_index":0"#),
        ("response.web_search_call.completed", #""item_id":"i","output_index":0"#),
        ("tool_call.created", #""tool_call_id":"t","output_index":0,"name":"web_search","arguments":{"query":"q"}"#),
        ("tool_output.created", #""tool_output_id":"o","tool_call_id":"t","output_index":0,"output":"result""#),
        ("future.event", #""new_field":true"#),
    ]
    var decoder = ResponsesSSEDecoder()
    var events: [ResponseStreamEvent] = []
    for (type, fields) in fixtures {
        events += try decoder.append(Data(sse(type, "{\"type\":\"\(type)\",\(fields)}").utf8))
    }
    #expect(events.count == fixtures.count)
    guard case let .unknown(type, payload) = events.last else { Issue.record("Expected unknown extension"); return }
    #expect(type == "future.event")
    #expect(payload == .object(["type": .string("future.event"), "new_field": .bool(true)]))
}

@Test(arguments: ["response.failed", "response.cancelled", "error", "response.error"])
func sseDistinguishesTerminalFailureAndCancellation(_ type: String) throws {
    let payload: String
    if type == "response.failed" {
        payload = lifecycle(type, status: "failed")
    } else if type == "response.cancelled" {
        payload = #"{"type":"response.cancelled","id":"event","created_at":1,"data":{"id":"response-1"}}"#
    } else {
        payload = "{\"type\":\"\(type)\",\"code\":\"failure\",\"message\":\"safe\"}"
    }
    var decoder = ResponsesSSEDecoder()
    #expect(try decoder.append(Data(sse(type, payload).utf8)).isEmpty)
    #expect(try decoder.finish().first?.isTerminal == true)
}

@Test func responsesCreateRetrieveCancelWireAndSSEIntegration() async throws {
    let completion = sse("response.completed", lifecycle("response.completed", status: "completed"))
    let retrieved =
        #"{"id":"response-1","object":"response","created_at":1,"status":"completed","model":"auto:quick","output":[],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}"#
    let cancelled = #"{"id":"response-1","object":"response","created_at":1,"status":"cancelled","model":"auto:quick"}"#
    let transport = APIRecorder([
        .init(body: completion, headers: ["content-type": "text/event-stream; charset=utf-8"]), .init(body: retrieved),
        .init(body: cancelled),
    ])
    let api = ResearchAPI(transport: transport, expectedPrincipalId: "owner")
    let input = ResponseInput.messages([.init(content: [.text("Hello"), .image(url: "data:image/png;base64,AA==")])])
    let events = try await api.createResponse(
        .init(
            input: input, conversation: "conversation-1", metadata: ["internal_message_id": .string("correlation")],
            webSearch: true), maximumInferenceSends: 1)
    var received: [ResponseStreamEvent] = []
    for try await event in events { received.append(event) }
    #expect(received.count == 1 && received[0].isTerminal)
    #expect(try await api.retrieveResponse(id: "response-1").usage?.totalTokens == 3)
    #expect(try await api.cancelResponse(id: "response-1").status == .cancelled)
    let requests = await transport.snapshot()
    #expect(requests.map(\.target) == ["/v1/responses", "/v1/responses/response-1", "/v1/responses/response-1/cancel"])
    #expect(requests[0].maximumInferenceSends == 1)
    #expect(requests.allSatisfy { $0.expectedPrincipalId == "owner" })
    let body = try JSONDecoder().decode([String: JSONValue].self, from: #require(requests[0].body))
    #expect(body["stream"] == .bool(true) && body["store"] == .bool(true))
    #expect(body["metadata"] == .object(["internal_message_id": .string("correlation")]))
    #expect(body["tools"] == .array([.object(["type": .string("web_search")])]))
    #expect(
        body["input"]
            == .array([
                .object([
                    "role": .string("user"),
                    "content": .array([
                        .object(["type": .string("input_text"), "text": .string("Hello")]),
                        .object([
                            "type": .string("input_image"), "image_url": .string("data:image/png;base64,AA=="),
                            "detail": .string("auto"),
                        ]),
                    ]),
                ])
            ]))
}

private struct FixtureStreamTransport: MapleTransport {
    let response: MapleResponse
    func send(_ request: MapleRequest) async throws -> MapleResponse { response }
}

@Test func authenticatedTunnelFailureAfterTerminalIsNotSuccess() async throws {
    let fixture = sse("response.completed", lifecycle("response.completed", status: "completed"))
    let body = AsyncThrowingStream<Data, Error> { continuation in
        continuation.yield(Data(fixture.utf8))
        continuation.finish(throwing: ResearchAPIError.invalidStream)
    }
    let api = ResearchAPI(
        transport: FixtureStreamTransport(
            response: MapleResponse(
                status: 200,
                headers: ["content-type": "text/event-stream"], body: body)))
    let events = try await api.createResponse(.init(input: .text("Synthetic"), conversation: "c"))
    var received: [ResponseStreamEvent] = []
    do {
        for try await event in events { received.append(event) }
        Issue.record("Authentication failure must propagate")
    } catch { #expect(error as? ResearchAPIError == .invalidStream) }
    #expect(received.isEmpty)
}

private actor StreamCancellationProbe {
    private var wasCancelled = false
    private var waiter: CheckedContinuation<Void, Never>?
    func cancel() {
        wasCancelled = true
        waiter?.resume()
        waiter = nil
    }
    func wait() async {
        if wasCancelled { return }
        await withCheckedContinuation { waiter = $0 }
    }
}

@Test func responseConsumerCancellationCancelsUpstream() async throws {
    let probe = StreamCancellationProbe()
    let body = AsyncThrowingStream<Data, Error> { continuation in
        continuation.onTermination = { @Sendable _ in Task { await probe.cancel() } }
        continuation.yield(Data(sse("response.output_text.delta", delta).utf8))
    }
    let api = ResearchAPI(
        transport: FixtureStreamTransport(
            response: MapleResponse(
                status: 200,
                headers: ["content-type": "text/event-stream"], body: body)))
    let consumer = Task {
        let events = try await api.createResponse(.init(input: .text("Synthetic"), conversation: "c"))
        for try await _ in events { try Task.checkCancellation() }
    }
    consumer.cancel()
    _ = await consumer.result
    await probe.wait()
}
