import CryptoKit
import Foundation

public enum PreferencesIssue: Error, Equatable, Sendable {
    case sessionChanged, invalidRecord, unsupportedVersion, unavailable
}

public protocol PreferencesStoring: Sendable {
    func activate(_ scope: AccountScope) async throws -> LocalPreferences
    func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) async throws
    func retire(_ scope: AccountScope, removePersisted: Bool) async throws
}

/// Own one instance for the application lifetime. UserDefaults stays inside the
/// application's sandbox, without cloud synchronization. Ordinary logout retains
/// these nonsensitive choices; confirmed account deletion removes its exact key.
/// Hashing is namespacing, not encryption. No principal or origin is in the record.
public actor PreferencesStore: PreferencesStoring {
    private struct Record: Codable { let version: Int; let preferences: LocalPreferences }
    private let defaults: UserDefaults
    private var active: AccountScope?
    private var retired = Set<AccountScope>()
    private var latestGeneration: [String: UUID] = [:]
    private var latestRevision: UInt64 = 0

    public init(suiteName: String? = nil) {
        defaults = suiteName.flatMap(UserDefaults.init(suiteName:)) ?? .standard
    }

    public func activate(_ scope: AccountScope) throws -> LocalPreferences {
        guard !retired.contains(scope), active == nil || active == scope else {
            throw PreferencesIssue.sessionChanged
        }
        let key = try Self.key(scope)
        if active == nil { latestRevision = 0 }
        active = scope
        latestGeneration[key] = scope.generation
        guard let object = defaults.object(forKey: key) else { return LocalPreferences() }
        guard let data = object as? Data, data.count <= 4_096 else { throw PreferencesIssue.invalidRecord }
        do {
            let record = try JSONDecoder().decode(Record.self, from: data)
            guard record.version == 1 else { throw PreferencesIssue.unsupportedVersion }
            guard LocalPreferences.isValidModelID(record.preferences.selectedModelID) else {
                throw PreferencesIssue.invalidRecord
            }
            return record.preferences
        } catch let issue as PreferencesIssue { throw issue } catch { throw PreferencesIssue.invalidRecord }
    }

    public func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) throws {
        guard active == scope, !retired.contains(scope) else { throw PreferencesIssue.sessionChanged }
        guard LocalPreferences.isValidModelID(value.selectedModelID) else { throw PreferencesIssue.invalidRecord }
        // An older scheduled save cannot replace a newer explicit choice.
        guard revision > latestRevision else { return }
        let data = try JSONEncoder().encode(Record(version: 1, preferences: value))
        guard data.count <= 4_096 else { throw PreferencesIssue.invalidRecord }
        defaults.set(data, forKey: try Self.key(scope))
        latestRevision = revision
    }

    public func retire(_ scope: AccountScope, removePersisted: Bool = false) throws {
        // Late cleanup must never remove a replacement generation's preferences.
        let key = try Self.key(scope)
        if let active, active != scope, try Self.key(active) == key {
            throw PreferencesIssue.sessionChanged
        }
        guard latestGeneration[key] == nil || latestGeneration[key] == scope.generation else {
            throw PreferencesIssue.sessionChanged
        }
        retired.insert(scope)
        if active == scope { active = nil; latestRevision = 0 }
        if removePersisted { defaults.removeObject(forKey: key) }
    }

    static func key(_ scope: AccountScope) throws -> String {
        guard !scope.apiOrigin.isEmpty, scope.apiOrigin.utf8.count <= 2_048,
            !scope.principalID.isEmpty, scope.principalID.utf8.count <= 1_024
        else { throw PreferencesIssue.invalidRecord }
        var input = Data()
        for field in [scope.apiOrigin, scope.principalID] {
            var count = UInt64(field.utf8.count).bigEndian
            withUnsafeBytes(of: &count) { input.append(contentsOf: $0) }
            input.append(contentsOf: field.utf8)
        }
        return "maple.native.preferences.v1." + SHA256.hash(data: input).map { String(format: "%02x", $0) }.joined()
    }
}
