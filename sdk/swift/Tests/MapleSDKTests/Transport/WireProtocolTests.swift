import Foundation
import Testing
@testable import MapleSDK

@Suite struct WireProtocolTests {
    @Test func absentAndEmptyBodiesAreDistinct() throws {
        func metadata(_ body: Data?) throws -> [String: Any] {
            let bytes = try WireProtocol.envelope(
                method: "POST", target: "/test", headers: [], body: body, credential: nil, cacheRoot: nil)
            return try JSONSerialization.jsonObject(with: Data(bytes.dropFirst(4))) as! [String: Any]
        }
        #expect(try metadata(nil)["body_present"] as? Bool == false)
        #expect(try metadata(Data())["body_present"] as? Bool == true)
        #expect(try metadata(nil)["credential"] is NSNull)
    }

    @Test(arguments: ["", "//evil.test/path", "https://evil.test/path", "/x#fragment", "/x\\y", "/x\ny", "/💬"])
    func rejectsMalformedTargets(_ target: String) {
        #expect(throws: TransportError.invalidEnvelope) {
            try WireProtocol.envelope(
                method: "GET", target: target, headers: [], body: nil, credential: nil, cacheRoot: nil)
        }
    }

    @Test func deniesEveryGatewayHeaderAndControlCharacters() {
        for name in WireProtocol.forbiddenHeaders.union(["Content-Type", "", "bad name"]) {
            #expect(throws: TransportError.invalidEnvelope) {
                try WireProtocol.envelope(
                    method: "POST", target: "/test", headers: [WireHeader(name: name, value: "x")], body: nil,
                    credential: nil, cacheRoot: nil)
            }
        }
        #expect(throws: TransportError.invalidEnvelope) {
            try WireProtocol.validateHeaders([WireHeader(name: "x-test", value: "a\r\nb")], maximum: 32)
        }
        #expect(throws: TransportError.invalidEnvelope) {
            try WireProtocol.validateHeaders(
                Array(repeating: WireHeader(name: "x-test", value: "ok"), count: 65), maximum: 64)
        }
    }

    @Test func recordsHaveStrictMetadataAndTerminalShape() throws {
        func start(_ object: [String: Any]) throws -> WireProtocol.Record {
            try WireProtocol.decode(Data([1]) + JSONSerialization.data(withJSONObject: object))
        }
        #expect(try start(["status": 200, "headers": []]) == .start(200, []))
        for status in [true as Any, 199, 600, 200.5, "200"] {
            #expect(throws: (any Error).self) { try start(["status": status, "headers": []]) }
        }
        #expect(throws: TransportError.invalidEnvelope) { try start(["status": 200, "headers": [], "extra": 1]) }
        #expect(throws: TransportError.invalidFrame) { try WireProtocol.decode(Data([3, 0])) }
        #expect(throws: TransportError.invalidFrame) { try WireProtocol.decode(Data([9])) }
        #expect(throws: TransportError.invalidFrame) {
            try WireProtocol.decode(Data([4]) + Data("{\"code\":\"private details\"}".utf8))
        }
        #expect(
            try WireProtocol.decode(Data([4]) + Data("{\"code\":\"stream_failed\"}".utf8)) == .error("stream_failed"))
    }
}
