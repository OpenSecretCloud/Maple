#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore

    /// Finite delivery inputs for local root onOpenURL tests where an unsigned
    /// loopback build has no associated-domain entitlement. It accepts no caller
    /// destination, code, secret or account value, and is never a release route.
    enum LocalAccountLinkFixture {
        static func forward(_ incoming: URL, configuration: AppConfiguration) -> URL? {
            guard configuration.profile == .local, incoming.absoluteString.utf8.count <= 4_096,
                let input = URLComponents(url: incoming, resolvingAgainstBaseURL: false),
                input.scheme == configuration.callbackScheme, input.host == "ui-test-account-link",
                input.user == nil, input.password == nil, input.port == nil,
                input.query == nil, input.fragment == nil, input.percentEncodedHost == input.host,
                input.percentEncodedPath == input.path,
                var result = URLComponents(url: configuration.authURL, resolvingAgainstBaseURL: false)
            else { return nil }
            switch input.path {
            case "/verify":
                result.path = "/verify/00000000-0000-4000-8000-000000000042"
                result.queryItems = nil
            case "/reset":
                result.path = "/password-reset/confirm"
                result.queryItems = [
                    URLQueryItem(name: "code", value: "ABCD1234"),
                    URLQueryItem(name: "email", value: "synthetic-other@example.test"),
                    URLQueryItem(name: "secret", value: "synthetic-external-secret"),
                ]
            case "/foreign-reset":
                result.port = (result.port ?? 80) == 1 ? 2 : 1
                result.path = "/password-reset"; result.queryItems = nil
            case "/duplicate-reset":
                result.path = "/password-reset"
                result.queryItems = [
                    URLQueryItem(name: "code", value: "ABCD1234"), URLQueryItem(name: "code", value: "EFGH5678"),
                ]
            default: return nil
            }
            return result.url
        }
    }
#endif
