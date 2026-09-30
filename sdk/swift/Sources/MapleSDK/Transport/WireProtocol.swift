import Foundation

struct APIEndpoint: Sendable, Hashable {
    let url: URL
    let origin: String
    let isLoopback: Bool

    init(_ string: String) throws {
        guard var components = URLComponents(string: string), let scheme = components.scheme?.lowercased(),
            let host = components.host?.lowercased(), !host.isEmpty,
            components.user == nil, components.password == nil, components.query == nil,
            components.fragment == nil, !string.contains("\\"),
            components.port.map({ (1...65535).contains($0) }) ?? true
        else { throw TransportError.invalidEndpoint }
        isLoopback = scheme == "http" && ["localhost", "127.0.0.1", "[::1]", "::1"].contains(host)
        guard scheme == "https" || (scheme == "http" && isLoopback) else { throw TransportError.invalidEndpoint }
        components.scheme = scheme
        components.host = host
        if (scheme == "https" && components.port == 443) || (scheme == "http" && components.port == 80) {
            components.port = nil
        }
        var originComponents = components
        originComponents.path = ""
        guard let origin = originComponents.string else { throw TransportError.invalidEndpoint }
        self.origin = origin
        while components.path.hasSuffix("/") { components.path.removeLast() }
        guard let url = components.url else { throw TransportError.invalidEndpoint }
        self.url = url
    }

    func outerURL(_ path: String) -> URL { url.appendingPathComponent(path) }
}

struct WireCredential: Sendable, Encodable {
    enum Kind: String, Sendable, Encodable { case bearer, resumption }
    let kind: Kind
    let value: String
}

struct WireHeader: Sendable, Codable, Equatable {
    let name: String
    let value: String
}

enum WireProtocol {
    static let maximumBody = 50 * 1024 * 1024
    static let maximumCiphertext = 1 + 64 * 1024 + 16
    static let forbiddenHeaders: Set<String> = [
        "authorization", "proxy-authorization", "cookie", "set-cookie", "host", "content-length",
        "transfer-encoding", "connection", "keep-alive", "te", "trailer", "upgrade", "forwarded",
        "via", "x-forwarded-for", "x-forwarded-host", "x-forwarded-proto", "x-opensecret-routing-key", "x-session-id",
    ]
    static let tokenCharacters = Set(
        "!#$%&'*+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".utf8)

    static func visibleASCII(_ value: String, tabs: Bool = false) -> Bool {
        value.utf8.allSatisfy { (32...126).contains($0) || (tabs && $0 == 9) }
    }

    static func validateHeaders(_ headers: [WireHeader], maximum: Int) throws {
        guard headers.count <= maximum else { throw TransportError.invalidEnvelope }
        for header in headers {
            guard !header.name.isEmpty, header.name.utf8.allSatisfy({ tokenCharacters.contains($0) }),
                header.name == header.name.lowercased(), !forbiddenHeaders.contains(header.name),
                visibleASCII(header.value, tabs: true)
            else { throw TransportError.invalidEnvelope }
        }
    }

    static func envelope(
        method: String, target: String, headers: [WireHeader], body: Data?,
        credential: WireCredential?, cacheRoot: Data?
    ) throws -> Data {
        guard !method.isEmpty, method.utf8.count <= 32, method.utf8.allSatisfy({ tokenCharacters.contains($0) }),
            target.hasPrefix("/"), !target.hasPrefix("//"), !target.contains("#"), !target.contains("\\"),
            target.utf8.count <= 16 * 1024, visibleASCII(target), (body?.count ?? 0) <= maximumBody,
            cacheRoot == nil || cacheRoot?.count == 32
        else { throw TransportError.invalidEnvelope }
        try validateHeaders(headers, maximum: 64)
        if let credential {
            guard !credential.value.isEmpty, credential.value.utf8.count <= 16 * 1024,
                visibleASCII(credential.value), !credential.value.contains(" ")
            else { throw TransportError.invalidCredentials }
        }
        // Explicit nulls and body presence are wire-significant.
        let metadata: [String: Any] = [
            "version": 2,
            "credential": credential.map { ["kind": $0.kind.rawValue, "value": $0.value] as Any } ?? NSNull(),
            "cache_namespace_root": cacheRoot.map { $0.base64EncodedString() as Any } ?? NSNull(),
            "method": method, "target": target,
            "headers": headers.map { ["name": $0.name, "value": $0.value] }, "body_present": body != nil,
        ]
        let encoded = try JSONSerialization.data(
            withJSONObject: metadata, options: [.sortedKeys, .withoutEscapingSlashes])
        guard encoded.count <= 128 * 1024 else { throw TransportError.invalidEnvelope }
        return UInt32(encoded.count).bigEndianData + encoded + (body ?? Data())
    }

    static func exactObject(_ data: Data, keys: Set<String>, maximum: Int = 64 * 1024) throws -> [String: Any] {
        guard data.count <= maximum, String(data: data, encoding: .utf8) != nil,
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            Set(object.keys) == keys
        else { throw TransportError.invalidEnvelope }
        return object
    }

    static func integer(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
            number.doubleValue.isFinite, number.doubleValue.rounded() == number.doubleValue,
            number.doubleValue >= 0, number.doubleValue <= Double(Int.max - 1024)
        else { return nil }
        return number.intValue
    }

    enum Record: Sendable, Equatable {
        case start(Int, [WireHeader]), chunk(Data), end, error(String)
    }

    static func decode(_ plaintext: Data) throws -> Record {
        guard !plaintext.isEmpty, plaintext.count <= 1 + 64 * 1024 else { throw TransportError.invalidFrame }
        let payload = Data(plaintext.dropFirst())
        switch plaintext.first! {
        case 1:
            let object = try exactObject(payload, keys: ["status", "headers"])
            guard let status = integer(object["status"]), (200...599).contains(status),
                let values = object["headers"] as? [[String: Any]]
            else { throw TransportError.invalidFrame }
            let headers = try values.map { value -> WireHeader in
                guard Set(value.keys) == ["name", "value"], let name = value["name"] as? String,
                    let text = value["value"] as? String
                else { throw TransportError.invalidFrame }
                return WireHeader(name: name, value: text)
            }
            try validateHeaders(headers, maximum: 32)
            return .start(status, headers)
        case 2: return .chunk(payload)
        case 3:
            guard payload.isEmpty else { throw TransportError.invalidFrame }
            return .end
        case 4:
            let object = try exactObject(payload, keys: ["code"])
            guard let code = object["code"] as? String, !code.isEmpty, code.utf8.count <= 64,
                code.utf8.allSatisfy({ (97...122).contains($0) || (48...57).contains($0) || $0 == 95 })
            else { throw TransportError.invalidFrame }
            return .error(code)
        default: throw TransportError.invalidFrame
        }
    }
}
