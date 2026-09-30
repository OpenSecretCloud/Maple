import CryptoKit
import Foundation

public enum PCREnvironment: String, Codable, Sendable {
    case production, development
}

/// Immutable environment-scoped PCR0 policy. Additional roots belong to this
/// selected environment only; remote history never falls back to another one.
public struct PCRTrustPolicy: Sendable {
    public let environment: PCREnvironment
    public let remoteHistoryURL: URL?
    let trustedPCR0s: Set<String>

    public init(
        environment: PCREnvironment = .production,
        additionalPCR0s: [String] = [],
        remoteHistoryEnabled: Bool = true,
        remoteHistoryURL: URL? = nil
    ) throws {
        let additional = additionalPCR0s.map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
        guard additional.allSatisfy({ Self.validHex($0, rejectZero: true) }) else { throw TrustError.invalidPolicy }
        self.environment = environment
        trustedPCR0s = Set((environment == .development ? Self.developmentRoots : Self.productionRoots) + additional)
        let url = remoteHistoryURL ?? Self.historyURL(environment)
        try Self.validateURL(url)
        self.remoteHistoryURL = remoteHistoryEnabled ? url : nil
    }

    /// Cache binding includes every trust-relevant input and the embedded key.
    /// Changing policy or SDK roots invalidates persisted attested sessions.
    public var cacheIdentity: String {
        let components = [
            "swift-pcr0-environment-v1", environment.rawValue, Self.verificationKeyBase64,
            trustedPCR0s.sorted().joined(separator: ","), remoteHistoryURL?.absoluteString ?? "disabled",
        ]
        return SHA256.hash(data: Data(components.joined(separator: "\n").utf8)).map { String(format: "%02x", $0) }
            .joined()
    }

    /// Must only receive PCR0 from a Nitro-authenticated document.
    @discardableResult
    public func requireTrustedPCR0(_ bytes: Data?) async throws -> String {
        try await requireTrustedPCR0(bytes, fetch: SignedPCRHistory.fetch)
    }

    @discardableResult
    func requireTrustedPCR0(_ bytes: Data?, fetch: @Sendable (URL) async throws -> Data) async throws -> String {
        guard let bytes, bytes.count == 48, bytes.contains(where: { $0 != 0 }) else { throw TrustError.invalidPCR0 }
        let hash = bytes.map { String(format: "%02x", $0) }.joined()
        if trustedPCR0s.contains(hash) { return hash }
        guard let url = remoteHistoryURL else { throw TrustError.untrustedPCR0 }
        do {
            let history = try await fetch(url)
            try Task.checkCancellation()
            if try SignedPCRHistory.verify(hash: hash, data: history) { return hash }
        } catch is CancellationError { throw CancellationError() } catch
        { /* An unavailable or malformed history cannot expand trust. */  }
        throw TrustError.untrustedPCR0
    }

    static func validHex(_ value: String, rejectZero: Bool = false) -> Bool {
        value.utf8.count == 96 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
            && (!rejectZero || value.utf8.contains { $0 != 48 })
    }

    static func validateURL(_ url: URL) throws {
        guard url.absoluteString.utf8.count <= 2_048,
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let host = parts.host, !host.isEmpty, parts.user == nil, parts.password == nil, parts.fragment == nil,
            parts.scheme == "https"
                || (parts.scheme == "http" && ["localhost", "127.0.0.1", "[::1]", "::1"].contains(host.lowercased()))
        else { throw TrustError.invalidPolicy }
    }

    static func historyURL(_ environment: PCREnvironment) -> URL {
        URL(
            string: "https://raw.githubusercontent.com/MaplePrivacyLabs/Maple/master/services/opensecret/"
                + (environment == .development ? "pcrDevHistory.json" : "pcrProdHistory.json"))!
    }

    static let verificationKeyBase64 =
        "MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAEHiUY9kFWK1GqBGzczohhwEwElXzgWLDZa9R6wBx3JOBocgSt9+UIzZlJbPDjYeGBfDUXh7Z62BG2vVsh2NgclLB5S7A2ucBBtb1wd8vSQHP8jpdPhZX1slauPgbnROIP"
}

enum SignedPCRHistory {
    static let maximumBytes = 1_024 * 1_024

    struct Entry: Decodable {
        let PCR0: String
        let PCR1: String
        let PCR2: String
        let timestamp: UInt64
        let signature: String
    }

    static func verify(hash: String, data: Data) throws -> Bool {
        guard data.count <= maximumBytes else { throw TrustError.historyUnavailable }
        let entries: [Entry]
        do { entries = try JSONDecoder().decode([Entry].self, from: data) } catch {
            throw TrustError.historyUnavailable
        }
        guard (1...2_048).contains(entries.count),
            entries.allSatisfy({ entry in
                PCRTrustPolicy.validHex(entry.PCR0) && PCRTrustPolicy.validHex(entry.PCR1)
                    && PCRTrustPolicy.validHex(entry.PCR2) && entry.timestamp > 0
                    && entry.timestamp <= 9_007_199_254_740_991 && entry.signature.utf8.count <= 256
                    && Data(base64Encoded: entry.signature)?.count == 96
            }), let der = Data(base64Encoded: PCRTrustPolicy.verificationKeyBase64),
            let key = try? P384.Signing.PublicKey(derRepresentation: der)
        else { throw TrustError.historyUnavailable }
        // The existing protocol signs PCR0 only. Timestamp/PCR1/PCR2 are schema
        // validated but never used as authenticated identity or freshness claims.
        return entries.contains { entry in
            guard entry.PCR0 == hash, let raw = Data(base64Encoded: entry.signature),
                let signature = try? P384.Signing.ECDSASignature(rawRepresentation: raw)
            else { return false }
            return key.isValidSignature(signature, for: Data(entry.PCR0.utf8))
        }
    }

    static func fetch(_ url: URL) async throws -> Data {
        try await fetch(url, configuration: .ephemeral)
    }

    static func fetch(_ url: URL, configuration: URLSessionConfiguration) async throws -> Data {
        try PCRTrustPolicy.validateURL(url)
        configuration.timeoutIntervalForRequest = 5
        configuration.timeoutIntervalForResource = 5
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        let session = URLSession(configuration: configuration, delegate: RejectHistoryRedirects(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        do {
            let (bytes, response) = try await session.bytes(from: url)
            guard let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode),
                response.url == url, response.expectedContentLength <= Int64(maximumBytes)
            else { throw TrustError.historyUnavailable }
            var body = Data()
            for try await byte in bytes {
                guard body.count < maximumBytes else { throw TrustError.historyUnavailable }
                body.append(byte)
            }
            return body
        } catch is CancellationError { throw CancellationError() } catch { throw TrustError.historyUnavailable }
    }
}

private final class RejectHistoryRedirects: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}
