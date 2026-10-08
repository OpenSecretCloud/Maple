import Foundation

/// An allowlisted account route. No credential or private reset secret is
/// retained from a URL; reset confirmation requires the app-owned ceremony.
public enum IncomingAccountLink: Equatable, Sendable {
    case verify(code: String)
    case passwordReset(code: String?)

    public init?(url: URL, configuration: AppConfiguration) {
        guard url.absoluteString.utf8.count <= 4_096,
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let origin = URLComponents(url: configuration.authURL, resolvingAgainstBaseURL: false),
            parts.scheme == origin.scheme, parts.host == origin.host, parts.port == origin.port,
            parts.percentEncodedHost == parts.host,
            parts.user == nil, parts.password == nil, parts.fragment == nil,
            !parts.percentEncodedPath.contains("%"), !parts.path.contains("\\")
        else { return nil }
        if parts.path.hasPrefix("/verify/"), parts.query == nil {
            let code = String(parts.path.dropFirst("/verify/".count))
            guard UUID(uuidString: code) != nil else { return nil }
            self = .verify(code: code)
            return
        }
        guard parts.path == "/password-reset" || parts.path == "/password-reset/confirm" else { return nil }
        let items = parts.queryItems ?? []
        guard Set(items.map(\.name)).count == items.count,
            items.allSatisfy({ ["code", "email", "secret"].contains($0.name) && ($0.value?.utf8.count ?? 0) <= 1_024 })
        else { return nil }
        let code = items.first(where: { $0.name == "code" })?.value
        if let code {
            guard code.utf8.count == 8,
                code.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) })
            else { return nil }
        }
        self = .passwordReset(code: code)
    }
}
