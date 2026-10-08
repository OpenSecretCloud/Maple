import Foundation
import MapleSDK

/// An account generation owns this entire cache, including in-flight minting.
/// Create a new instance on every sign-in, even for the same principal.
public actor BillingService: BillingServing {
    public nonisolated let scope: AccountScope
    private let baseURL: URL
    private let source: any BillingTokenSource
    private let http: any BillingHTTP
    private var active = true
    private var token: String?
    private var mint: (id: UUID, task: Task<String, Error>)?

    public init(
        scope: AccountScope, configuration: AppConfiguration,
        tokenSource: any BillingTokenSource, http: any BillingHTTP = URLSessionBillingHTTP()
    ) {
        self.scope = scope
        self.baseURL = configuration.billingURL
        self.source = tokenSource
        self.http = http
    }

    public func subscriptionStatus() async throws -> BillingStatus {
        let value: BillingStatus = try await fetch(path: "v1/maple/subscription/status")
        try value.validate()
        return value
    }

    public func teamStatus() async throws -> TeamStatus {
        let value: TeamStatus = try await fetch(path: "v1/maple/team/status")
        try value.validate()
        return value
    }

    public func retire() {
        active = false
        token = nil
        mint?.task.cancel()
        mint = nil
    }

    private func checkOwner() throws {
        guard active else { throw AccountServiceError.sessionChanged }
        if Task.isCancelled { throw AccountServiceError.cancelled }
    }

    private func currentToken() async throws -> String {
        try checkOwner()
        if let token { return token }
        let request: (id: UUID, task: Task<String, Error>)
        if let mint {
            request = mint
        } else {
            let audience = baseURL.absoluteString
            let source = source
            request = (UUID(), Task { try await source.mintToken(audience: audience) })
            mint = request
        }
        do {
            let result = try await request.task.value
            try checkOwner()
            guard !result.isEmpty, result.utf8.count <= 65_536,
                result.utf8.allSatisfy({ (33...126).contains($0) })
            else { throw AccountServiceError.invalidResponse }
            // A waiter for an older mint must never replace a newer cache entry.
            if mint?.id == request.id {
                token = result
                mint = nil
            }
            return token ?? result
        } catch {
            if mint?.id == request.id { mint = nil }
            try checkOwner()
            throw Self.safe(error)
        }
    }

    private func fetch<Value: Decodable & Sendable>(path: String) async throws -> Value {
        do {
            var usedToken = try await currentToken()
            for attempt in 0..<2 {
                try checkOwner()
                let response = try await http.get(url: baseURL.appendingPathComponent(path), bearerToken: usedToken)
                try checkOwner()
                if response.status == 401 {
                    guard attempt == 0 else { throw AccountServiceError.unauthorized }
                    // Another request may have already replaced this exact token.
                    if token == usedToken { token = nil }
                    usedToken = try await currentToken()
                    continue
                }
                guard (200..<300).contains(response.status) else { throw AccountServiceError.unavailable }
                guard response.body.count <= URLSessionBillingHTTP.maximumBody else {
                    throw AccountServiceError.invalidResponse
                }
                let decoder = JSONDecoder()
                decoder.keyDecodingStrategy = .convertFromSnakeCase
                return try decoder.decode(Value.self, from: response.body)
            }
            throw AccountServiceError.unauthorized
        } catch {
            try checkOwner()
            throw Self.safe(error)
        }
    }

    static func safe(_ error: any Error) -> AccountServiceError {
        if let error = error as? AccountServiceError { return error }
        if error is CancellationError { return .cancelled }
        if error is DecodingError { return .invalidResponse }
        return .unavailable
    }
}

public struct SDKBillingTokenSource: BillingTokenSource {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func mintToken(audience: String) async throws -> String {
        try await api.generateThirdPartyToken(audience: audience).token
    }
}

public protocol ModelCatalogServing: Sendable {
    func fetchCatalog() async throws -> ModelCatalog
}

public struct SDKModelCatalogSource: ModelCatalogServing {
    private let api: ResearchAPI
    public init(api: ResearchAPI) { self.api = api }
    public func fetchCatalog() async throws -> ModelCatalog { try await api.fetchModelCatalog() }
}

/// No redirects, cookies, persistence or credential storage at this boundary.
/// The bounded streaming read prevents a billing error body consuming unbounded memory.
public final class URLSessionBillingHTTP: BillingHTTP, Sendable {
    static let maximumBody = 1_048_576

    public init() {}

    public func get(url: URL, bearerToken: String) async throws -> BillingHTTPResponse {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.urlCredentialStorage = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 15
        configuration.timeoutIntervalForResource = 30
        let delegate = RejectBillingRedirects()
        let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
        // In particular, cancel an unread error or oversized body without
        // interrupting any concurrent account request's session.
        defer { session.invalidateAndCancel() }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (bytes, response) = try await session.bytes(for: request)
        guard let response = response as? HTTPURLResponse, response.url == url else {
            throw AccountServiceError.invalidResponse
        }
        guard (200..<300).contains(response.statusCode) else {
            return BillingHTTPResponse(status: response.statusCode, body: Data())
        }
        guard response.expectedContentLength <= Self.maximumBody else { throw AccountServiceError.invalidResponse }
        var body = Data()
        for try await byte in bytes {
            guard body.count < Self.maximumBody else { throw AccountServiceError.invalidResponse }
            body.append(byte)
        }
        return BillingHTTPResponse(status: response.statusCode, body: body)
    }
}

private final class RejectBillingRedirects: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}
