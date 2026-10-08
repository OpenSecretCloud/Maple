import Foundation
import MapleSDK

public enum ConfigurationError: Error, Equatable, Sendable {
    case missingValue, invalidProfile, invalidOrigin, invalidClient, invalidCallback
}

public struct AppConfiguration: Equatable, Sendable {
    public enum Profile: String, Sendable { case local, development, production }

    public let profile: Profile
    public let apiURL: URL
    public let billingURL: URL
    public let flagsURL: URL
    public let authURL: URL
    public let clientID: String
    public let callbackScheme: String

    public let trustPolicy: PCRTrustPolicy

    public static func == (lhs: Self, rhs: Self) -> Bool {
        // The trust policy is derived exclusively from this immutable profile.
        lhs.profile == rhs.profile && lhs.apiURL == rhs.apiURL && lhs.billingURL == rhs.billingURL
            && lhs.flagsURL == rhs.flagsURL && lhs.authURL == rhs.authURL
            && lhs.clientID == rhs.clientID && lhs.callbackScheme == rhs.callbackScheme
    }

    public init(
        profile: Profile, apiURL: String, billingURL: String, flagsURL: String,
        authURL: String, clientID: String, callbackScheme: String
    ) throws {
        self.profile = profile
        self.trustPolicy = try PCRTrustPolicy(
            environment: profile == .production ? .production : .development,
            remoteHistoryEnabled: profile != .local)
        self.apiURL = try Self.origin(apiURL, profile: profile)
        self.billingURL = try Self.origin(billingURL, profile: profile)
        self.flagsURL = try Self.origin(flagsURL, profile: profile)
        self.authURL = try Self.origin(authURL, profile: profile)
        guard UUID(uuidString: clientID) != nil else { throw ConfigurationError.invalidClient }
        self.clientID = clientID.lowercased()
        guard callbackScheme.count <= 128, callbackScheme.hasPrefix("cloud.opensecret.maple"),
            callbackScheme.first?.isASCII == true,
            callbackScheme.unicodeScalars.allSatisfy({
                CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789+.-").contains($0)
            }), callbackScheme != "http", callbackScheme != "https"
        else { throw ConfigurationError.invalidCallback }
        self.callbackScheme = callbackScheme
    }

    public static func load(bundle: Bundle = .main) throws -> AppConfiguration {
        func value(_ key: String) throws -> String {
            guard let value = bundle.object(forInfoDictionaryKey: key) as? String,
                !value.isEmpty, !value.contains("$(")
            else { throw ConfigurationError.missingValue }
            return value
        }
        guard let profile = Profile(rawValue: try value("MAPLE_PROFILE")) else {
            throw ConfigurationError.invalidProfile
        }
        return try AppConfiguration(
            profile: profile, apiURL: value("MAPLE_API_URL"), billingURL: value("MAPLE_BILLING_URL"),
            flagsURL: value("MAPLE_FLAGS_URL"), authURL: value("MAPLE_AUTH_URL"),
            clientID: value("MAPLE_CLIENT_ID"), callbackScheme: value("MAPLE_CALLBACK_SCHEME"))
    }

    private static func origin(_ value: String, profile: Profile) throws -> URL {
        guard value.utf8.count <= 2_048, !value.contains("\\"),
            value == value.trimmingCharacters(in: .whitespacesAndNewlines),
            var parts = URLComponents(string: value), let host = parts.host, !host.isEmpty,
            parts.user == nil, parts.password == nil, parts.query == nil, parts.fragment == nil,
            parts.path.isEmpty || parts.path == "/", parts.percentEncodedHost == host,
            parts.port.map({ 1...65_535 ~= $0 }) ?? true
        else { throw ConfigurationError.invalidOrigin }
        if profile == .local {
            guard parts.scheme == "http", ["127.0.0.1", "localhost", "[::1]", "::1"].contains(host),
                parts.port != nil
            else { throw ConfigurationError.invalidOrigin }
        } else {
            guard parts.scheme == "https" else { throw ConfigurationError.invalidOrigin }
        }
        parts.scheme = parts.scheme?.lowercased()
        parts.host = host.lowercased()
        parts.path = ""
        guard let result = parts.url else { throw ConfigurationError.invalidOrigin }
        return result
    }
}
