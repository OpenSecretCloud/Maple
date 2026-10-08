import Darwin
import Foundation

/// Temporary generated speech only. Complete-until-first-authentication is the
/// initial candidate because playback may reopen later chunks while locked.
/// Actual lock/reopen/protection behavior requires the final physical-device gate.
public actor TemporarySpeechStore: SpeechFileStoring {
    private struct Entry { let owner: SpeechOwner; let name: String; var bytes: Int }
    private let root: URL
    private let maximumFileBytes: Int
    private let maximumSessionBytes: Int
    private var initialized = false
    private var active: SpeechOwner?
    private var retired = Set<SpeechOwner>()
    private var entries: [UUID: Entry] = [:]
    private var allocated = 0
    private var failedCleanup = Set<SpeechOwner>()

    public init(
        root: URL, maximumFileBytes: Int = SpeechLimits.maximumFileBytes,
        maximumSessionBytes: Int = SpeechLimits.maximumSessionBytes
    ) {
        self.root = root; self.maximumFileBytes = max(0, maximumFileBytes)
        self.maximumSessionBytes = max(0, maximumSessionBytes)
    }

    public func startupCleanup() throws {
        guard active == nil, entries.isEmpty else { throw SpeechIssue.storage }
        let directory = try openDirectory(); defer { close(directory) }
        for name in try names(directory) where Self.ownedName(name) { try unlinkOwned(name, directory: directory) }
        initialized = true
    }

    public func activate(_ owner: SpeechOwner) throws {
        guard initialized, failedCleanup.isEmpty else { throw SpeechIssue.storage }
        guard !retired.contains(owner), active == nil || active == owner else { throw SpeechIssue.retired }
        active = owner
    }

    public func write(_ body: AsyncThrowingStream<Data, Error>, owner: SpeechOwner) async throws -> StoredSpeechFile {
        try require(owner)
        let directory = try openDirectory(); defer { close(directory) }
        let id = UUID(),
            name = "speech-" + owner.sessionID.uuidString.lowercased() + "-" + id.uuidString.lowercased() + ".wav"
        let descriptor = openat(
            directory, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else { throw SpeechIssue.storage }
        defer { close(descriptor) }
        entries[id] = Entry(owner: owner, name: name, bytes: 0)
        do {
            try validate(descriptor, name: name, directory: directory, expectedBytes: 0)
            try protect(root.appendingPathComponent(name))
            for try await bytes in body {
                try Task.checkCancellation(); try require(owner)
                guard var entry = entries[id], entry.owner == owner else { throw SpeechIssue.retired }
                guard bytes.count <= SpeechLimits.maximumStreamBlockBytes,
                    bytes.count <= maximumFileBytes - entry.bytes,
                    bytes.count <= maximumSessionBytes - allocated
                else { throw SpeechIssue.capacity }
                try validate(descriptor, name: name, directory: directory, expectedBytes: entry.bytes)
                // Reservation and descriptor writes are synchronous on this actor.
                entry.bytes += bytes.count; allocated += bytes.count; entries[id] = entry
                try bytes.withUnsafeBytes { buffer in
                    guard let pointer = buffer.baseAddress else { return }
                    var offset = 0
                    while offset < buffer.count {
                        let written = Darwin.write(descriptor, pointer.advanced(by: offset), buffer.count - offset)
                        if written < 0 && errno == EINTR { continue }
                        guard written > 0 else { throw SpeechIssue.storage }
                        offset += written
                    }
                }
            }
            try Task.checkCancellation(); try require(owner)
            guard let entry = entries[id], entry.bytes > 0 else { throw SpeechIssue.invalidAudio }
            guard fsync(descriptor) == 0 else { throw SpeechIssue.storage }
            try validate(descriptor, name: name, directory: directory, expectedBytes: entry.bytes)
            try validateRoot(directory)
            return StoredSpeechFile(id: id, url: root.appendingPathComponent(name), byteCount: entry.bytes)
        } catch {
            do { try removeEntry(id, directory: directory) } catch {
                failedCleanup.insert(owner); throw SpeechIssue.storage
            }
            throw error
        }
    }

    public func remove(_ files: [StoredSpeechFile], owner: SpeechOwner) throws {
        guard !files.isEmpty else { return }
        let directory = try openDirectory(); defer { close(directory) }
        for file in files {
            guard let entry = entries[file.id] else { continue }
            guard entry.owner == owner else { throw SpeechIssue.retired }
            do { try removeEntry(file.id, directory: directory) } catch {
                failedCleanup.insert(owner); throw SpeechIssue.storage
            }
        }
    }

    public func retire(_ owner: SpeechOwner) throws {
        retired.insert(owner)
        if active == owner { active = nil }
        failedCleanup.insert(owner)
        let directory = try openDirectory(); defer { close(directory) }
        do {
            for id in entries.keys.filter({ entries[$0]?.owner == owner }) { try removeEntry(id, directory: directory) }
            failedCleanup.remove(owner)
        } catch { failedCleanup.insert(owner); throw SpeechIssue.storage }
    }

    public var allocatedBytes: Int { allocated }

    private func require(_ owner: SpeechOwner) throws {
        guard initialized, active == owner, !retired.contains(owner), failedCleanup.isEmpty else {
            throw SpeechIssue.retired
        }
    }
    private func removeEntry(_ id: UUID, directory: Int32) throws {
        guard let entry = entries[id] else { return }
        try unlinkOwned(entry.name, directory: directory)
        entries.removeValue(forKey: id); allocated -= entry.bytes
    }
    private static func ownedName(_ name: String) -> Bool {
        guard name.hasPrefix("speech-"), name.hasSuffix(".wav"), name.utf8.count == 84 else { return false }
        let body = name.dropFirst(7).dropLast(4)
        return body.dropFirst(36).first == "-" && UUID(uuidString: String(body.prefix(36))) != nil
            && UUID(uuidString: String(body.dropFirst(37))) != nil
    }
    private func openDirectory() throws -> Int32 {
        var info = stat()
        if lstat(root.path, &info) != 0 {
            guard errno == ENOENT else { throw SpeechIssue.storage }
            do {
                try FileManager.default.createDirectory(
                    at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            } catch { throw SpeechIssue.storage }
        }
        let directory = open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard directory >= 0 else { throw SpeechIssue.storage }
        do {
            guard fstat(directory, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
                info.st_uid == geteuid(), fchmod(directory, 0o700) == 0
            else { throw SpeechIssue.storage }
            try validateRoot(directory); try protect(root)
            return directory
        } catch { close(directory); throw SpeechIssue.storage }
    }
    private func validateRoot(_ descriptor: Int32) throws {
        var opened = stat(), current = stat()
        guard fstat(descriptor, &opened) == 0, lstat(root.path, &current) == 0,
            current.st_mode & S_IFMT == S_IFDIR, current.st_ino == opened.st_ino, current.st_dev == opened.st_dev
        else { throw SpeechIssue.storage }
    }
    private func validate(_ descriptor: Int32, name: String, directory: Int32, expectedBytes: Int) throws {
        var opened = stat(), current = stat()
        guard fstat(descriptor, &opened) == 0, fstatat(directory, name, &current, AT_SYMLINK_NOFOLLOW) == 0,
            opened.st_mode & S_IFMT == S_IFREG, opened.st_nlink == 1, opened.st_uid == geteuid(),
            opened.st_size == expectedBytes, opened.st_ino == current.st_ino, opened.st_dev == current.st_dev
        else { throw SpeechIssue.storage }
    }
    private func unlinkOwned(_ name: String, directory: Int32) throws {
        guard Self.ownedName(name) else { throw SpeechIssue.storage }
        var info = stat()
        if fstatat(directory, name, &info, AT_SYMLINK_NOFOLLOW) != 0 {
            guard errno == ENOENT else { throw SpeechIssue.storage }; return
        }
        // Unlink only this exact generated entry; never follow links or recurse.
        guard info.st_uid == geteuid(), info.st_mode & S_IFMT != S_IFDIR,
            unlinkat(directory, name, 0) == 0
        else { throw SpeechIssue.storage }
    }
    private func protect(_ url: URL) throws {
        var target = url, values = URLResourceValues(); values.isExcludedFromBackup = true
        do {
            try target.setResourceValues(values)
            #if os(iOS)
                try FileManager.default.setAttributes(
                    [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: url.path)
            #endif
        } catch { throw SpeechIssue.storage }
    }
    private func names(_ directory: Int32) throws -> [String] {
        let duplicate = dup(directory)
        guard duplicate >= 0 else { throw SpeechIssue.storage }
        guard let stream = fdopendir(duplicate) else { close(duplicate); throw SpeechIssue.storage }
        defer { closedir(stream) }; rewinddir(stream)
        var result: [String] = []
        while true {
            errno = 0
            guard let entry = readdir(stream) else {
                guard errno == 0 else { throw SpeechIssue.storage }; return result
            }
            let name = withUnsafePointer(to: &entry.pointee.d_name) {
                $0.withMemoryRebound(to: CChar.self, capacity: Int(NAME_MAX) + 1) { String(cString: $0) }
            }
            if name == "." || name == ".." { continue }
            guard result.count < 1_024 else { throw SpeechIssue.capacity }
            result.append(name)
        }
    }
}
