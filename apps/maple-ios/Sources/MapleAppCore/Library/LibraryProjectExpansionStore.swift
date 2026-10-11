import Foundation

public enum LibraryProjectExpansionIssue: Error, Equatable, Sendable {
    case sessionChanged, invalidRecord, unsupportedVersion, protectedDataUnavailable, unavailable
}

public protocol LibraryProjectExpansionStoring: Sendable {
    func activate(_ scope: AccountScope) async throws -> Set<String>
    func save(_ digests: Set<String>, scope: AccountScope, revision: UInt64) async throws
    func prepareRemoval(_ scope: AccountScope) async throws
    func cancelRemoval(_ scope: AccountScope) async throws
    func retire(_ scope: AccountScope, removePersisted: Bool) async throws
}

/// Local presentation metadata only, never a server preference. The separate
/// protected directory is excluded from backups. Records contain project-ID
/// digests, not identifiers, names, account details, or project content. Hashing
/// is namespacing and data minimization, not encryption or authorization.
public actor LibraryProjectExpansionStore: LibraryProjectExpansionStoring {
    private struct Record: Codable {
        let version: Int
        let expanded: [String]
    }
    private struct Removal: Codable { let version: Int }
    static let maximumProjects = 4_096
    static let maximumBytes = 300 * 1_024
    private let files: ProtectedChatFiles
    private var protectionAvailable: Bool
    private var active: AccountScope?
    private var retired = Set<AccountScope>()
    private var latestGeneration: [String: UUID] = [:]
    private var latestRevision: UInt64 = 0
    private var pendingRemoval = Set<String>()
    private var preparedOwners: [String: UUID] = [:]
    private var pendingCancellations: [String: UUID] = [:]
    private var startupCleaned = false

    public init(root: URL, protectionAvailable: Bool = true) {
        files = ProtectedChatFiles(root: root.standardizedFileURL)
        self.protectionAvailable = protectionAvailable
    }

    public func setProtectedDataAvailable(_ available: Bool) { protectionAvailable = available }

    public func activate(_ scope: AccountScope) throws -> Set<String> {
        guard !retired.contains(scope), active == nil || active == scope else {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
        let account = try Self.accountName(scope)
        if active == nil { latestRevision = 0 }
        // A failed/locked read still owns a revocable session lease. Otherwise
        // retirement could mistake this generation for a stale former owner.
        active = scope; latestGeneration[account] = scope.generation
        try requireProtection()
        try finishStartupCleanup()
        try finishRemovals()
        try finishCancellations()
        try finishPreparedHandoff(scope)
        do {
            return try files.withDirectory { directory in
                for name in try files.entries(in: directory)
                where ProtectedChatFiles.temporaryOwner(of: name) == account {
                    try files.remove(name, in: directory)
                }
                guard let data = try files.read(account + ".json", maximumBytes: Self.maximumBytes, in: directory)
                else { return [] }
                struct Version: Decodable { let version: Int }
                let version = try JSONDecoder().decode(Version.self, from: data)
                guard version.version == 1 else { throw LibraryProjectExpansionIssue.unsupportedVersion }
                let record = try JSONDecoder().decode(Record.self, from: data)
                let expanded = Set(record.expanded)
                guard expanded.count == record.expanded.count else {
                    throw LibraryProjectExpansionIssue.invalidRecord
                }
                try Self.validate(expanded)
                return expanded
            }
        } catch { throw Self.safe(error) }
    }

    public func save(_ digests: Set<String>, scope: AccountScope, revision: UInt64) throws {
        guard active == scope, !retired.contains(scope), pendingRemoval.isEmpty else {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
        try requireProtection()
        try finishCancellations()
        try finishPreparedHandoff(scope)
        try Self.validate(digests)
        guard revision > latestRevision else { return }
        let account = try Self.accountName(scope)
        do {
            let data = try JSONEncoder().encode(Record(version: 1, expanded: digests.sorted()))
            guard data.count <= Self.maximumBytes else { throw LibraryProjectExpansionIssue.invalidRecord }
            try files.withDirectory { directory in
                try files.publish(data, name: account + ".json", account: account, directory: directory) {}
            }
            latestRevision = revision
        } catch { throw Self.safe(error) }
    }

    /// Called before the remote account-deletion request. A protected, tiny
    /// marker survives termination while the request/result is uncertain; on
    /// relaunch only these local presentation choices may be reset.
    public func prepareRemoval(_ scope: AccountScope) throws {
        guard active == scope, !retired.contains(scope), pendingRemoval.isEmpty else {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
        try requireProtection()
        try finishStartupCleanup()
        try finishCancellations()
        let account = try Self.accountName(scope)
        do {
            let data = try JSONEncoder().encode(Removal(version: 1))
            try files.withDirectory { directory in
                try files.publish(data, name: account + ".removal", account: account, directory: directory) {}
            }
            preparedOwners[account] = scope.generation
        } catch { throw Self.safe(error) }
    }

    public func cancelRemoval(_ scope: AccountScope) throws {
        let account = try Self.accountName(scope)
        try requireLatestGeneration(scope, account: account)
        guard latestGeneration[account] == scope.generation else {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
        guard !pendingRemoval.contains(account) else { throw LibraryProjectExpansionIssue.sessionChanged }
        // Record the already-authorized cancellation before a locked-device
        // preflight; unlock can then remove this marker without losing choices.
        pendingCancellations[account] = scope.generation
        try requireProtection()
        try finishCancellations()
    }

    /// Run before publishing account-owned UI. Do not consume a marker for an
    /// in-flight deletion in the current process. activate also performs this
    /// startup pass before its first protected read, including after unlock.
    public func startupCleanup() throws {
        try requireProtection()
        if active == nil { try finishStartupCleanup() }
        try finishRemovals()
        try finishCancellations()
        if let active { try finishPreparedHandoff(active) }
    }

    public func retire(_ scope: AccountScope, removePersisted: Bool = false) throws {
        let account = try Self.accountName(scope)
        try requireLatestGeneration(scope, account: account)
        retired.insert(scope)
        if active == scope { active = nil; latestRevision = 0 }
        if removePersisted { pendingRemoval.insert(account) }
        try finishRemovals()
    }

    static func accountName(_ scope: AccountScope) throws -> String {
        do { return String(try PreferencesStore.key(scope).suffix(64)) } catch {
            throw LibraryProjectExpansionIssue.invalidRecord
        }
    }

    static func projectDigest(_ id: String) -> String? {
        guard !id.isEmpty, id.utf8.count <= 1_024 else { return nil }
        return ProtectedChatFiles.digest(Data(id.utf8))
    }

    private static func validate(_ digests: Set<String>) throws {
        guard digests.count <= maximumProjects, digests.allSatisfy(ProtectedChatFiles.isDigest) else {
            throw LibraryProjectExpansionIssue.invalidRecord
        }
    }

    private func requireProtection() throws {
        guard protectionAvailable else { throw LibraryProjectExpansionIssue.protectedDataUnavailable }
    }

    private func requireLatestGeneration(_ scope: AccountScope, account: String) throws {
        if let active, active != scope, try Self.accountName(active) == account {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
        guard latestGeneration[account] == nil || latestGeneration[account] == scope.generation else {
            throw LibraryProjectExpansionIssue.sessionChanged
        }
    }

    private func finishStartupCleanup() throws {
        guard !startupCleaned else { return }
        do {
            try files.withDirectory { directory in
                for name in try files.entries(in: directory) {
                    guard name.hasSuffix(".removal"), name.utf8.count == 72 else { continue }
                    let account = String(name.prefix(64))
                    guard ProtectedChatFiles.isDigest(account),
                        let data = try files.read(name, maximumBytes: 128, in: directory)
                    else { continue }
                    let marker = try JSONDecoder().decode(Removal.self, from: data)
                    guard marker.version == 1 else { throw LibraryProjectExpansionIssue.unsupportedVersion }
                    try removeRecord(account, directory: directory)
                }
            }
            startupCleaned = true
        } catch { throw Self.safe(error) }
    }

    private func removeRecord(_ account: String, directory: Int32) throws {
        try files.remove(account + ".json", in: directory)
        for name in try files.entries(in: directory)
        where ProtectedChatFiles.temporaryOwner(of: name) == account {
            try files.remove(name, in: directory)
        }
        // The durable intent goes last, so interrupted cleanup retries safely.
        try files.remove(account + ".removal", in: directory)
        preparedOwners.removeValue(forKey: account)
        pendingCancellations.removeValue(forKey: account)
    }

    private func finishCancellations() throws {
        guard !pendingCancellations.isEmpty else { return }
        try requireProtection()
        do {
            try files.withDirectory { directory in
                for (account, generation) in pendingCancellations {
                    // A newer prepared marker cannot be removed by an earlier
                    // generation's delayed cancellation, even after logout.
                    if preparedOwners[account] == generation, !pendingRemoval.contains(account) {
                        try files.remove(account + ".removal", in: directory)
                        preparedOwners.removeValue(forKey: account)
                    }
                    pendingCancellations.removeValue(forKey: account)
                }
            }
        } catch { throw Self.safe(error) }
    }

    private func finishPreparedHandoff(_ scope: AccountScope) throws {
        let account = try Self.accountName(scope)
        guard let prepared = preparedOwners[account], prepared != scope.generation else { return }
        try requireProtection()
        do {
            // An uncertain old request may reset only the old generation's UI
            // choices. Consume its marker before the replacement can read or
            // publish, so a future restart cannot erase replacement choices.
            try files.withDirectory { directory in try removeRecord(account, directory: directory) }
        } catch { throw Self.safe(error) }
    }

    private func finishRemovals() throws {
        guard !pendingRemoval.isEmpty else { return }
        try requireProtection()
        do {
            try files.withDirectory { directory in
                for account in pendingRemoval {
                    try removeRecord(account, directory: directory)
                }
            }
            pendingRemoval.removeAll()
        } catch { throw Self.safe(error) }
    }

    private static func safe(_ error: any Error) -> LibraryProjectExpansionIssue {
        if let issue = error as? LibraryProjectExpansionIssue { return issue }
        if error is DecodingError { return .invalidRecord }
        if let issue = error as? ChatStorageError, issue == .capacityExceeded { return .invalidRecord }
        return .unavailable
    }
}
