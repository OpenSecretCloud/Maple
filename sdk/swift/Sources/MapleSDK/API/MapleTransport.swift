import Foundation

/// A logical HTTP request. Implementations must attest and encrypt before dispatch;
/// this protocol is not permission to use a plaintext HTTP adapter in production.
public protocol MapleTransport: Sendable {
    func send(_ request: MapleRequest) async throws -> MapleResponse
}

public enum MapleHTTPMethod: String, Sendable, Codable {
    case get = "GET", post = "POST", delete = "DELETE"
}

public enum MapleRequestAuthentication: Sendable, Equatable {
    case none
    case required
}

public struct MapleRequest: Sendable {
    public var method: MapleHTTPMethod
    public var target: String
    public var headers: [String: String]
    public var body: Data?
    public var auth: MapleRequestAuthentication
    /// Capture the owning account at the caller. The transport must reject a
    /// different account before dispatch and while consuming the response.
    public var expectedPrincipalId: String?
    /// Includes an eligible Transport V2 recovery send; never forwarded upstream.
    public var maximumInferenceSends: Int
    public var allowSessionRecovery: Bool

    public init(
        method: MapleHTTPMethod, target: String, headers: [String: String] = [:],
        body: Data? = nil, auth: MapleRequestAuthentication = .required,
        expectedPrincipalId: String? = nil, maximumInferenceSends: Int = 2,
        allowSessionRecovery: Bool = true
    ) {
        self.method = method
        self.target = target
        self.headers = headers
        self.body = body
        self.auth = auth
        self.expectedPrincipalId = expectedPrincipalId
        self.maximumInferenceSends = maximumInferenceSends
        self.allowSessionRecovery = allowSessionRecovery
    }
}

public struct MapleResponse: Sendable {
    public let status: Int
    public let headers: [String: String]
    public let body: AsyncThrowingStream<Data, Error>
    public let inferenceSendCount: Int

    public init(
        status: Int, headers: [String: String] = [:],
        body: AsyncThrowingStream<Data, Error>, inferenceSendCount: Int = 1
    ) {
        self.status = status
        self.headers = headers
        self.body = body
        self.inferenceSendCount = inferenceSendCount
    }

    public func header(_ name: String) -> String? {
        headers.first { $0.key.caseInsensitiveCompare(name) == .orderedSame }?.value
    }
}

/// Errors deliberately exclude untrusted response bodies, tokens, and URLs.
public enum ResearchAPIError: Error, Sendable, Equatable, LocalizedError {
    case invalidArgument
    case invalidResponse
    case responseTooLarge
    case http(status: Int, reason: RejectionReason)
    case truncatedStream
    case invalidStream
    case invalidCallback
    case expiredAuthenticationAttempt

    public enum RejectionReason: String, Sendable, Codable {
        case unspecified, contextLimit, freeTierTokenLimit, usageLimit
        case imageDescriptionUnavailable, authenticationRequired, forbidden, notFound
        case invalidLogin, emailAlreadyRegistered, oauthNotLinked, invalidInviteCode
        case userNotFound, oauthEmailUnavailable, alreadyVerified, modelUnavailable
        case billingCleanupUnavailable, payloadTooLarge, rateLimited, unsupportedValue
    }

    public var errorDescription: String? {
        switch self {
        case .invalidArgument: "The request is invalid."
        case .invalidResponse: "The service returned an invalid response."
        case .responseTooLarge: "The service response exceeded the supported size."
        case .http: "The service could not complete the request."
        case .truncatedStream: "The response was interrupted before its final status arrived."
        case .invalidStream: "The service returned an invalid response stream."
        case .invalidCallback: "The authentication callback is invalid. Restart sign-in."
        case .expiredAuthenticationAttempt: "Sign-in expired. Please try again."
        }
    }
}

public let openSecretInferenceSendLimitHeader = "x-opensecret-client-inference-send-limit"

public struct OpenSecretInferenceCapacityError: Error, Sendable, Equatable, LocalizedError {
    public let status: Int
    public let retryDelayMilliseconds: Int?
    public let inferenceSendCount: Int
    public var errorDescription: String? { "Inference capacity is temporarily unavailable." }

    public init?(response: MapleResponse) {
        guard [429, 503].contains(response.status),
            response.header("x-opensecret-error-contract") == "1",
            response.header("x-opensecret-error-code") == "inference_capacity",
            response.header("x-opensecret-client-replay") == "safe"
        else { return nil }
        status = response.status
        inferenceSendCount = response.inferenceSendCount
        let value = response.header("retry-after")
        if let value, !value.isEmpty,
            value.utf8.allSatisfy({ (48...57).contains($0) }),
            (value == "0" || !value.hasPrefix("0"))
        {
            if let seconds = Int(value), seconds <= 60 {
                retryDelayMilliseconds = seconds * 1_000
            } else {
                retryDelayMilliseconds = nil
            }
        } else {
            retryDelayMilliseconds = 1_000
        }
    }
}

/// Used only for extensible metadata, tool arguments, and future event payloads.
public enum JSONValue: Codable, Sendable, Equatable {
    case null, bool(Bool), integer(Int64), number(Double), string(String)
    case array([JSONValue]), object([String: JSONValue])

    public init(from decoder: any Decoder) throws {
        let value = try decoder.singleValueContainer()
        if value.decodeNil() {
            self = .null
        } else if let decoded = try? value.decode(Bool.self) {
            self = .bool(decoded)
        } else if let decoded = try? value.decode(Int64.self) {
            self = .integer(decoded)
        } else if let decoded = try? value.decode(Double.self) {
            self = .number(decoded)
        } else if let decoded = try? value.decode(String.self) {
            self = .string(decoded)
        } else if let decoded = try? value.decode([JSONValue].self) {
            self = .array(decoded)
        } else {
            self = .object(try value.decode([String: JSONValue].self))
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var value = encoder.singleValueContainer()
        switch self {
        case .null: try value.encodeNil()
        case let .bool(v): try value.encode(v)
        case let .integer(v): try value.encode(v)
        case let .number(v): try value.encode(v)
        case let .string(v): try value.encode(v)
        case let .array(v): try value.encode(v)
        case let .object(v): try value.encode(v)
        }
    }
}

/// Update fields distinguish omission (unchanged) from null (clear).
public enum NullableUpdate<Value: Encodable & Sendable>: Sendable {
    case unchanged, clear, set(Value)
}

internal enum APIJSON {
    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    static func encode<T: Encodable>(_ value: T) throws -> Data {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return try encoder.encode(value)
    }
}

/// Finds the capacity contract through Foundation error wrappers without exposing
/// their messages. The depth bound also contains cyclic wrapper chains.
public func findOpenSecretInferenceCapacityError(_ error: any Error) -> OpenSecretInferenceCapacityError? {
    var current: (any Error)? = error
    var visited: Set<ObjectIdentifier> = []
    for _ in 0..<8 {
        guard let value = current else { return nil }
        if let capacity = value as? OpenSecretInferenceCapacityError { return capacity }
        let wrapped = value as NSError
        guard visited.insert(ObjectIdentifier(wrapped)).inserted else { return nil }
        current = wrapped.userInfo[NSUnderlyingErrorKey] as? any Error
    }
    return nil
}
