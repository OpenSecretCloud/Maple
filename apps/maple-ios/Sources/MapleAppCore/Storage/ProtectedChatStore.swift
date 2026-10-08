import Foundation
import Synchronization

public enum ChatStorageError: Error, Equatable, Sendable {
    case retiredOwner, protectedDataUnavailable, unsafePath, corruptData, unsupportedVersion
    case staleRevision, destinationOccupied, capacityExceeded, writeFailed, cleanupRequired
}

/// One atomic account manifest keeps draft-to-conversation moves and queue edits
/// coherent. Images are immutable account-owned blobs; this is never a transcript
/// cache. Version 1 inline data is read and migrated on the next successful save.
public actor ProtectedChatStore: ChatPersistence {
    struct Limits: Sendable {
        var metadataBytes = 16 * 1_024 * 1_024
        var imageBytes = 20 * 1_024 * 1_024
        var accountImageBytes = 256 * 1_024 * 1_024
        var retainedImages = 640
        var imagesPerDraft = 10
        var queuedPerChat = 16
        var accountQueuedTurns = 64
        var queuedTextBytes = 32 * 1_024
    }

    enum Publication: Equatable, Sendable { case image, manifest }

    private struct Move: Codable {
        let destination: String
    }

    private struct Envelope: Codable {
        var version = 2
        let origin: String
        let principal: String
        var chats: [String: ChatSnapshot]
        // Optional for the pre-release version 1 envelope. Persisted tombstones
        // fence old New Chat storage owners after relaunch, while allowing a fresh draft.
        var moves: [String: [String: Move]]?
    }

    private struct DecodedImages: Sendable {
        var referenced: Set<String> = []
        var bytes = 0
        var images: [String: Data] = [:]
    }

    private struct Blob: Codable {
        let sha256: String
        let count: Int
    }

    private let files: ProtectedChatFiles
    private let limits: Limits
    private let beforePublication: @Sendable (Publication) throws -> Void
    private var active: AccountScope?
    private var retired: Set<AccountScope> = []
    private var pendingCleanup: String?
    private var pendingSignedOutCleanup = false
    private var protectionAvailable: Bool
    private var envelope: Envelope?
    private var referencedImages: Set<String> = []

    public init(root: URL, maximumBytes: Int = 16 * 1_024 * 1_024, protectionAvailable: Bool = true) {
        files = ProtectedChatFiles(root: root.standardizedFileURL)
        limits = Limits(metadataBytes: maximumBytes)
        beforePublication = { _ in }
        self.protectionAvailable = protectionAvailable
    }

    init(
        root: URL, limits: Limits, protectionAvailable: Bool = true,
        beforePublication: @escaping @Sendable (Publication) throws -> Void
    ) {
        files = ProtectedChatFiles(root: root.standardizedFileURL)
        self.limits = limits
        self.beforePublication = beforePublication
        self.protectionAvailable = protectionAvailable
    }

    public func setProtectedDataAvailable(_ available: Bool) { protectionAvailable = available }

    public func activate(_ scope: AccountScope) throws {
        guard !retired.contains(scope), active == nil || active == scope else { throw ChatStorageError.retiredOwner }
        try requireProtection()
        try finishCleanup()
        try finishSignedOutCleanup()
        if active == scope { return }
        let account = ProtectedChatFiles.accountName(scope)
        let limits = self.limits
        let files = self.files
        let loaded = try files.withDirectory { directory in
            try removeTemporaries(account: account, directory: directory)
            guard let data = try files.read(account + ".json", maximumBytes: limits.metadataBytes, in: directory) else {
                try pruneImages(account: account, keeping: [], directory: directory)
                return (Envelope(origin: scope.apiOrigin, principal: scope.principalID, chats: [:]), Set<String>())
            }
            struct Version: Decodable { let version: Int }
            let version: Int
            do { version = try JSONDecoder().decode(Version.self, from: data).version } catch {
                throw ChatStorageError.corruptData
            }
            guard version == 1 || version == 2 else { throw ChatStorageError.unsupportedVersion }
            let decoder = JSONDecoder()
            let decoded = Mutex(DecodedImages())
            if version == 2 {
                decoder.dataDecodingStrategy = .custom { decoder in
                    let blob = try Blob(from: decoder)
                    return try decoded.withLock { state in
                        guard ProtectedChatFiles.isDigest(blob.sha256), blob.count >= 0,
                            blob.count <= limits.imageBytes
                        else {
                            throw ChatStorageError.capacityExceeded
                        }
                        guard
                            let bytes = try files.read(
                                account + "-" + blob.sha256 + ".blob", maximumBytes: limits.imageBytes, in: directory),
                            bytes.count == blob.count, ProtectedChatFiles.digest(bytes) == blob.sha256
                        else {
                            throw ChatStorageError.corruptData
                        }
                        state.images[blob.sha256] = bytes
                        state.bytes += bytes.count
                        state.referenced.insert(blob.sha256)
                        return bytes
                    }
                }
            }
            let loaded: Envelope
            do { loaded = try decoder.decode(Envelope.self, from: data) } catch let error as ChatStorageError {
                throw error
            } catch { throw ChatStorageError.corruptData }
            guard loaded.origin == scope.apiOrigin, loaded.principal == scope.principalID,
                loaded.chats.allSatisfy({ $0.key == $0.value.storageKey })
            else { throw ChatStorageError.corruptData }
            try validateDrafts(loaded)
            let referenced = decoded.withLock { $0.referenced }
            try pruneImages(account: account, keeping: referenced, directory: directory)
            return (loaded, referenced)
        }
        active = scope
        envelope = loaded.0
        referencedImages = loaded.1
    }

    public func load(chatID: String, scope: AccountScope) throws -> ChatSnapshot? {
        try requireOwner(scope)
        try requireProtection()
        return envelope?.chats[chatID]
    }

    public func loadAll(scope: AccountScope) throws -> [ChatSnapshot] {
        try requireOwner(scope)
        try requireProtection()
        return (envelope?.chats.values.map { $0 } ?? []).sorted { $0.storageKey < $1.storageKey }
    }

    public func save(_ snapshot: ChatSnapshot, scope: AccountScope) throws {
        try requireOwner(scope)
        try requireProtection()
        guard var next = envelope else { throw ChatStorageError.retiredOwner }
        guard next.moves?[snapshot.storageKey]?[snapshot.storageOwner.uuidString] == nil else {
            throw ChatStorageError.staleRevision
        }
        if let prior = next.chats[snapshot.storageKey] {
            guard prior.storageOwner == snapshot.storageOwner else { throw ChatStorageError.staleRevision }
            guard snapshot.revision > prior.revision || snapshot == prior else { throw ChatStorageError.staleRevision }
        }
        next.chats[snapshot.storageKey] = snapshot
        try commit(next, scope: scope)
    }

    public func rekey(from source: String, to snapshot: ChatSnapshot, scope: AccountScope) throws {
        try requireOwner(scope)
        try requireProtection()
        guard var next = envelope else { throw ChatStorageError.retiredOwner }
        guard source != snapshot.storageKey else { try save(snapshot, scope: scope); return }
        let ownerID = snapshot.storageOwner.uuidString
        if let move = next.moves?[source]?[ownerID] {
            guard move.destination == snapshot.storageKey, let destination = next.chats[snapshot.storageKey] else {
                throw ChatStorageError.staleRevision
            }
            guard destination.storageOwner == snapshot.storageOwner else { throw ChatStorageError.destinationOccupied }
            // A retried move may arrive after destination edits or after a fresh
            // New Chat draft exists. Never rewind either of those owners.
            if destination.revision == snapshot.revision {
                guard destination == snapshot else { throw ChatStorageError.staleRevision }
                return
            }
            if destination.revision > snapshot.revision { return }
            next.chats[snapshot.storageKey] = snapshot
            try commit(next, scope: scope)
            return
        }
        guard next.chats[snapshot.storageKey] == nil else { throw ChatStorageError.destinationOccupied }
        guard let original = next.chats[source],
            original.storageOwner == snapshot.storageOwner,
            snapshot.revision > original.revision
        else { throw ChatStorageError.staleRevision }
        next.chats.removeValue(forKey: source)
        next.chats[snapshot.storageKey] = snapshot
        var moves = next.moves ?? [:]
        moves[source, default: [:]][ownerID] = Move(destination: snapshot.storageKey)
        next.moves = moves
        try commit(next, scope: scope)
    }

    public func remove(chatID: String, scope: AccountScope) throws {
        try requireOwner(scope)
        try requireProtection()
        guard var next = envelope else { throw ChatStorageError.retiredOwner }
        next.chats.removeValue(forKey: chatID)
        try commit(next, scope: scope)
    }

    /// Revoke before fallible deletion. Failed cleanup blocks any replacement
    /// account lease until every recognized file of the retired owner is removed.
    public func retire(_ scope: AccountScope) throws {
        guard active == nil || active == scope else { throw ChatStorageError.retiredOwner }
        retired.insert(scope)
        if active == scope {
            active = nil
            envelope = nil
            referencedImages = []
            pendingCleanup = ProtectedChatFiles.accountName(scope)
        }
        try finishCleanup()
    }

    /// No saved credential authority exists. Unknown names remain untouched;
    /// recognized but unsafe entries fail closed instead of following links.
    public func clearSignedOutData() throws {
        guard active == nil else { throw ChatStorageError.retiredOwner }
        pendingSignedOutCleanup = true
        try requireProtection()
        try finishCleanup()
        try finishSignedOutCleanup()
    }

    private func finishSignedOutCleanup() throws {
        guard pendingSignedOutCleanup else { return }
        try requireProtection()
        do {
            try files.withDirectory { directory in
                for name in try files.entries(in: directory)
                where ProtectedChatFiles.owner(of: name) != nil || ProtectedChatFiles.temporaryOwner(of: name) != nil {
                    try files.remove(name, in: directory)
                }
            }
        } catch { throw ChatStorageError.cleanupRequired }
        pendingSignedOutCleanup = false
    }

    private func requireOwner(_ scope: AccountScope) throws {
        guard active == scope, !retired.contains(scope), pendingCleanup == nil, !pendingSignedOutCleanup else {
            throw ChatStorageError.retiredOwner
        }
    }

    private func requireProtection() throws {
        guard protectionAvailable else { throw ChatStorageError.protectedDataUnavailable }
    }

    private func validateDrafts(_ value: Envelope) throws {
        var bytes = 0
        var count = 0
        try validateQueues(value)
        var seen: [UUID: [ChatImageAttachment]] = [:]
        for snapshot in value.chats.values {
            for draft in snapshot.ownedDrafts {
                guard draft.images.count <= limits.imagesPerDraft else { throw ChatStorageError.capacityExceeded }
                for image in draft.images {
                    // Queue original/working/held values can share immutable
                    // images. Reusing an ID for different bytes does not make
                    // that second retained version free or replace the first.
                    if seen[image.id]?.contains(image) == true { continue }
                    guard image.data.count <= limits.imageBytes, count < limits.retainedImages,
                        image.data.count <= limits.accountImageBytes - bytes
                    else { throw ChatStorageError.capacityExceeded }
                    seen[image.id, default: []].append(image)
                    bytes += image.data.count
                    count += 1
                }
            }
        }
    }

    private func validateQueues(_ value: Envelope) throws {
        var retained: [UUID: (chat: String, turn: ChatQueuedTurn)] = [:]
        for snapshot in value.chats.values {
            let queued = snapshot.queue?.items ?? []
            let recoveredHead = snapshot.queue?.recoveredHeadID
            let waitingLimit = limits.queuedPerChat + (recoveredHead == nil ? 0 : 1)
            guard queued.count <= waitingLimit else { throw ChatStorageError.capacityExceeded }
            if let recoveredHead {
                guard queued.count == limits.queuedPerChat + 1, queued.first?.id == recoveredHead,
                    snapshot.pendingRun == nil
                else { throw ChatStorageError.corruptData }
            }
            if let run = snapshot.pendingRun {
                guard Self.isMessageID(run.messageID),
                    run.queuedTurn == nil || run.queuedTurn?.messageID == run.messageID
                else {
                    throw ChatStorageError.corruptData
                }
            }
            guard Set(queued.map(\.id)).count == queued.count else { throw ChatStorageError.corruptData }
            var turns = queued
            if let edit = snapshot.queue?.edit {
                guard queued.indices.contains(edit.originalIndex), queued[edit.originalIndex] == edit.original else {
                    throw ChatStorageError.corruptData
                }
                turns.append(edit.original)
            }
            // The current popped FIFO item still occupies account capacity. The
            // per-chat limit applies to the waiting list, matching Research.
            if let active = snapshot.pendingRun?.queuedTurn { turns.append(active) }
            for turn in turns {
                guard Self.isMessageID(turn.messageID) else { throw ChatStorageError.corruptData }
                guard turn.draft.text.utf8.count <= limits.queuedTextBytes else {
                    throw ChatStorageError.capacityExceeded
                }
                if let prior = retained[turn.id] {
                    guard prior.chat == snapshot.storageKey, prior.turn == turn else {
                        throw ChatStorageError.corruptData
                    }
                } else {
                    guard retained.count < limits.accountQueuedTurns else { throw ChatStorageError.capacityExceeded }
                    retained[turn.id] = (snapshot.storageKey, turn)
                }
            }
        }
    }

    private static func isMessageID(_ value: String) -> Bool {
        UUID(uuidString: value)?.uuidString.lowercased() == value
    }

    private func commit(_ value: Envelope, scope: AccountScope) throws {
        try requireOwner(scope)
        try requireProtection()
        try validateDrafts(value)
        var next = value
        next.version = 2
        let encodedImages = Mutex<[String: Data]>([:])
        let encoder = JSONEncoder()
        encoder.dataEncodingStrategy = .custom { data, encoder in
            // Freeze bytes before hashing; an attachment UUID does not authorize
            // mutating an older snapshot's blob or aliasing another account.
            let frozen = data.withUnsafeBytes { Data($0) }
            let digest = ProtectedChatFiles.digest(frozen)
            encodedImages.withLock { $0[digest] = frozen }
            try Blob(sha256: digest, count: frozen.count).encode(to: encoder)
        }
        let data: Data
        do { data = try encoder.encode(next) } catch { throw ChatStorageError.writeFailed }
        guard data.count <= limits.metadataBytes else { throw ChatStorageError.capacityExceeded }
        let images = encodedImages.withLock { $0 }
        let account = ProtectedChatFiles.accountName(scope)
        try files.withDirectory { directory in
            try removeTemporaries(account: account, directory: directory)
            try pruneImages(account: account, keeping: referencedImages, directory: directory)
            for (digest, bytes) in images {
                let name = account + "-" + digest + ".blob"
                if let existing = try files.read(name, maximumBytes: limits.imageBytes, in: directory) {
                    guard existing == bytes else { throw ChatStorageError.corruptData }
                } else {
                    try files.publish(bytes, name: name, account: account, directory: directory) {
                        try beforePublication(.image)
                    }
                }
            }
            try files.publish(data, name: account + ".json", account: account, directory: directory) {
                try beforePublication(.manifest)
            }
            envelope = next
            referencedImages = Set(images.keys)
            // A failed collection cannot undo a successful manifest publication.
            // The next activation retries orphan collection; retirement requires
            // complete cleanup and reports any failure.
            try? pruneImages(account: account, keeping: Set(images.keys), directory: directory)
        }
    }

    private func removeTemporaries(account: String, directory: Int32) throws {
        for name in try files.entries(in: directory) where ProtectedChatFiles.temporaryOwner(of: name) == account {
            try files.remove(name, in: directory)
        }
    }

    private func pruneImages(account: String, keeping: Set<String>, directory: Int32) throws {
        for name in try files.entries(in: directory)
        where ProtectedChatFiles.owner(of: name) == account && name.hasSuffix(".blob") {
            let digest = String(name.dropFirst(65).dropLast(5))
            if !keeping.contains(digest) { try files.remove(name, in: directory) }
        }
    }

    private func finishCleanup() throws {
        guard let account = pendingCleanup else { return }
        try requireProtection()
        do {
            try files.withDirectory { directory in
                for name in try files.entries(in: directory)
                where ProtectedChatFiles.owner(of: name) == account
                    || ProtectedChatFiles.temporaryOwner(of: name) == account
                {
                    try files.remove(name, in: directory)
                }
            }
        } catch { throw ChatStorageError.cleanupRequired }
        pendingCleanup = nil
    }
}
