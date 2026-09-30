import Foundation
import Testing
@testable import MapleSDK

struct PCRHistoryHTTPTests {
    private func fetch(_ path: String) async throws -> Data {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [PCRHistoryProtocol.self]
        return try await SignedPCRHistory.fetch(
            URL(string: "https://history.test/\(path)")!, configuration: configuration)
    }

    @Test func successfulHTTPBodyStillRequiresPinnedSignature() async throws {
        let body = try await fetch("valid")
        #expect(try SignedPCRHistory.verify(hash: PCRTrustTests.signedPCR0, data: body))
        let unsigned = try await fetch("unsigned")
        #expect(try !SignedPCRHistory.verify(hash: PCRTrustTests.signedPCR0, data: unsigned))
    }

    @Test(arguments: ["status", "redirect", "content-length", "chunk-limit", "network-failure"])
    func untrustedHTTPFailuresAreSanitized(_ path: String) async {
        await #expect(throws: TrustError.historyUnavailable) { try await fetch(path) }
    }
}

private final class PCRHistoryProtocol: URLProtocol, @unchecked Sendable {
    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "history.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url else { return }
        let path = url.lastPathComponent
        if path == "network-failure" {
            client?.urlProtocol(self, didFailWithError: URLError(.timedOut))
            return
        }
        if path == "redirect" {
            let response = HTTPURLResponse(
                url: url, statusCode: 302, httpVersion: "HTTP/1.1",
                headerFields: ["Location": "https://history.test/valid"])!
            client?.urlProtocol(
                self, wasRedirectedTo: URLRequest(url: URL(string: "https://history.test/valid")!),
                redirectResponse: response)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        let response = HTTPURLResponse(
            url: url, statusCode: path == "status" ? 503 : 200,
            httpVersion: "HTTP/1.1", headerFields: path == "content-length" ? ["Content-Length": "1048577"] : [:])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        do {
            let body =
                path == "chunk-limit"
                ? Data(repeating: 0, count: 1_048_577)
                : try PCRTrustTests.history(
                    signature: path == "unsigned"
                        ? Data(repeating: 0, count: 96).base64EncodedString() : PCRTrustTests.signature)
            client?.urlProtocol(self, didLoad: body)
            client?.urlProtocolDidFinishLoading(self)
        } catch { client?.urlProtocol(self, didFailWithError: error) }
    }

    override func stopLoading() {}
}
