import Foundation
import Security
import Observation

// MARK: - Keychain Helper

private enum KeychainHelper {
    private static let service = "cloud.opensecret.maple.ios"

    static func saveData(key: String, data: Data) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]

        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]

        var item = query
        for (key, value) in attributes {
            item[key] = value
        }

        let addStatus = SecItemAdd(item as CFDictionary, nil)
        if addStatus == errSecDuplicateItem {
            SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        }
    }

    static func loadData(key: String) -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return data
    }

    static func loadString(key: String) -> String? {
        guard let data = loadData(key: key) else { return nil }
        return String(data: data, encoding: .utf8)
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

private struct StoredSessionTokens: Codable {
    let accessToken: String
    let refreshToken: String
}

private enum SessionTokenStore {
    private static let combinedKey = "session_tokens"
    private static let legacyAccessKey = "access_token"
    private static let legacyRefreshKey = "refresh_token"

    static func load() -> StoredSessionTokens? {
        if let data = KeychainHelper.loadData(key: combinedKey) {
            if let tokens = try? JSONDecoder().decode(StoredSessionTokens.self, from: data),
               !tokens.accessToken.isEmpty,
               !tokens.refreshToken.isEmpty {
                return tokens
            }

            KeychainHelper.delete(key: combinedKey)
        }

        guard let accessToken = KeychainHelper.loadString(key: legacyAccessKey),
              let refreshToken = KeychainHelper.loadString(key: legacyRefreshKey),
              !accessToken.isEmpty,
              !refreshToken.isEmpty else {
            return nil
        }

        let tokens = StoredSessionTokens(accessToken: accessToken, refreshToken: refreshToken)
        save(accessToken: accessToken, refreshToken: refreshToken)
        KeychainHelper.delete(key: legacyAccessKey)
        KeychainHelper.delete(key: legacyRefreshKey)
        return tokens
    }

    static func save(accessToken: String, refreshToken: String) {
        guard !accessToken.isEmpty, !refreshToken.isEmpty else {
            clear()
            return
        }

        let tokens = StoredSessionTokens(accessToken: accessToken, refreshToken: refreshToken)
        guard let data = try? JSONEncoder().encode(tokens) else { return }

        KeychainHelper.saveData(key: combinedKey, data: data)
        KeychainHelper.delete(key: legacyAccessKey)
        KeychainHelper.delete(key: legacyRefreshKey)
    }

    static func clear() {
        KeychainHelper.delete(key: combinedKey)
        KeychainHelper.delete(key: legacyAccessKey)
        KeychainHelper.delete(key: legacyRefreshKey)
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

        if let tokens = SessionTokenStore.load() {
            rust.dispatch(
                action: .restoreSession(
                    accessToken: tokens.accessToken,
                    refreshToken: tokens.refreshToken
                )
            )
        } else {
            SessionTokenStore.clear()
            rust.dispatch(action: .completeStartup)
        }
    }

    nonisolated func reconcile(update: AppUpdate) {
        if case .sessionTokens(_, let accessToken, let refreshToken) = update {
            SessionTokenStore.save(accessToken: accessToken, refreshToken: refreshToken)
        }

        Task { @MainActor [weak self] in
            self?.apply(update: update)
        }
    }

    private func apply(update: AppUpdate) {
        switch update {
        case .sessionTokens(let rev, _, _):
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
