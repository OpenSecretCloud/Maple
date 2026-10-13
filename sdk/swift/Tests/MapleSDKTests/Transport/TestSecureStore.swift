import Foundation
@testable import MapleSDK

final class TestSecureStore: SecureStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: Data] = [:]
    private var failWrites = false
    private var failReads = false
    func setWriteFailure(_ fail: Bool) { lock.withLock { failWrites = fail } }
    func setReadFailure(_ fail: Bool) { lock.withLock { failReads = fail } }
    func read(key: String) throws -> Data? {
        try lock.withLock {
            if failReads { throw TransportError.persistence }
            return values[key]
        }
    }
    func write(_ data: Data, key: String) throws {
        try lock.withLock {
            if failWrites { throw TransportError.persistence }
            values[key] = data
        }
    }
    func remove(key: String) throws {
        try lock.withLock {
            if failWrites { throw TransportError.persistence }
            values.removeValue(forKey: key)
        }
    }
}

func fixtureJWT(principal: String, kind: String, expiry: Int = 4_000_000_000, format: Int = 2) throws -> String {
    func base64URL(_ value: Data) -> String {
        value.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
    let claims: [String: Any] = [
        "sub": principal, "exp": expiry, "tf": format,
        "aud": "urn:opensecret:internal:transport-v2:user:\(kind)-token",
    ]
    return base64URL(Data("{\"alg\":\"HS256\"}".utf8)) + "."
        + base64URL(try JSONSerialization.data(withJSONObject: claims)) + "."
        + base64URL(Data("synthetic-signature".utf8))
}
