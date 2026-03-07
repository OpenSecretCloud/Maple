import Foundation
import Security
import Observation

// MARK: - Keychain Helper

private enum KeychainHelper {
    private static let service = "cloud.opensecret.maple.ios"

    static func save(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    static func load(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data,
              let str = String(data: data, encoding: .utf8) else { return nil }
        return str
    }

    static func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
    }
}

// MARK: - AppManager

@MainActor
@Observable
final class AppManager: AppReconciler {
    let rust: FfiApp
    var state: AppState
    private var lastRevApplied: UInt64

    private static func configuredApiUrl() -> String {
        if let apiUrl = ProcessInfo.processInfo.environment["OPEN_SECRET_API_URL"]?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           !apiUrl.isEmpty {
            return apiUrl
        }

        if let apiUrl = (Bundle.main.object(forInfoDictionaryKey: "OPEN_SECRET_API_URL") as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines),
           !apiUrl.isEmpty,
           !apiUrl.contains("$(") {
            return apiUrl
        }

        return defaultApiUrl()
    }

    init() {
        let fm = FileManager.default
        let dataDirUrl = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dataDir = dataDirUrl.path
        try? fm.createDirectory(at: dataDirUrl, withIntermediateDirectories: true)

        let apiUrl = Self.configuredApiUrl()
        let clientId = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6"

        let rust = FfiApp(apiUrl: apiUrl, clientId: clientId, dataDir: dataDir)
        self.rust = rust

        let initial = rust.state()
        self.state = initial
        self.lastRevApplied = initial.rev

        rust.listenForUpdates(reconciler: self)

        // Attempt session restore from Keychain
        if let access = KeychainHelper.load(key: "access_token"),
           let refresh = KeychainHelper.load(key: "refresh_token") {
            rust.dispatch(action: .restoreSession(accessToken: access, refreshToken: refresh))
        }
    }

    nonisolated func reconcile(update: AppUpdate) {
        Task { @MainActor [weak self] in
            self?.apply(update: update)
        }
    }

    private func apply(update: AppUpdate) {
        switch update {
        case .sessionTokens(let rev, let accessToken, let refreshToken):
            // Persist side-effect BEFORE rev guard (per bible 6.6)
            if accessToken.isEmpty {
                KeychainHelper.delete(key: "access_token")
                KeychainHelper.delete(key: "refresh_token")
            } else {
                KeychainHelper.save(key: "access_token", value: accessToken)
                KeychainHelper.save(key: "refresh_token", value: refreshToken)
            }
            // Still update rev tracking
            if rev > lastRevApplied {
                lastRevApplied = rev
            }
        case .fullState(let s):
            if s.rev <= lastRevApplied { return }
            lastRevApplied = s.rev
            state = s
        }
    }

    func dispatch(_ action: AppAction) {
        rust.dispatch(action: action)
    }
}
