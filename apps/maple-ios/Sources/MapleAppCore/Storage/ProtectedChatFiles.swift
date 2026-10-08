import CryptoKit
import Darwin
import Foundation

/// Descriptor-relative I/O keeps a checked directory or file from being swapped
/// for a link between validation and access. All names are generated here.
struct ProtectedChatFiles {
    let root: URL
    static let maximumDirectoryEntries = 8_192

    static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    static func isDigest(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }

    static func accountName(_ scope: AccountScope) -> String {
        digest(Data((scope.apiOrigin + "\u{0}" + scope.principalID).utf8))
    }

    static func owner(of name: String) -> String? {
        let account = String(name.prefix(64))
        guard isDigest(account) else { return nil }
        if name == account + ".json" { return account }
        if name.hasPrefix(account + "-"), name.hasSuffix(".blob"), name.count == 134,
            isDigest(String(name.dropFirst(65).dropLast(5)))
        {
            return account
        }
        return nil
    }

    static func temporaryOwner(of name: String) -> String? {
        guard name.hasPrefix(".pending-"), name.count == 110 else { return nil }
        let tail = name.dropFirst(9)
        let account = String(tail.prefix(64))
        guard isDigest(account), tail.dropFirst(64).first == "-",
            UUID(uuidString: String(tail.dropFirst(65))) != nil
        else { return nil }
        return account
    }

    func withDirectory<T>(_ body: (Int32) throws -> T) throws -> T {
        var existing = stat()
        if lstat(root.path, &existing) != 0 {
            guard errno == ENOENT else { throw ChatStorageError.unsafePath }
            try FileManager.default.createDirectory(
                at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        }
        let descriptor = open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard descriptor >= 0 else { throw ChatStorageError.unsafePath }
        defer { close(descriptor) }
        var info = stat()
        guard fstat(descriptor, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
            info.st_uid == geteuid(), fchmod(descriptor, 0o700) == 0
        else {
            throw ChatStorageError.unsafePath
        }
        // The app sandbox owns the ancestors. Verify the leaf still identifies
        // the opened directory before applying URL-only platform attributes.
        guard lstat(root.path, &existing) == 0, existing.st_dev == info.st_dev,
            existing.st_ino == info.st_ino
        else { throw ChatStorageError.unsafePath }
        try protect(root)
        return try body(descriptor)
    }

    func entries(in directory: Int32) throws -> [String] {
        let duplicate = dup(directory)
        guard duplicate >= 0 else { throw ChatStorageError.cleanupRequired }
        guard let stream = fdopendir(duplicate) else {
            close(duplicate)
            throw ChatStorageError.cleanupRequired
        }
        defer { closedir(stream) }
        rewinddir(stream)
        var names: [String] = []
        while true {
            errno = 0
            guard let entry = readdir(stream) else {
                guard errno == 0 else { throw ChatStorageError.cleanupRequired }
                return names
            }
            let name = withUnsafePointer(to: &entry.pointee.d_name) {
                $0.withMemoryRebound(to: CChar.self, capacity: Int(NAME_MAX) + 1) { String(cString: $0) }
            }
            guard name != ".", name != ".." else { continue }
            guard names.count < Self.maximumDirectoryEntries else { throw ChatStorageError.capacityExceeded }
            names.append(name)
        }
    }

    func exists(_ name: String, in directory: Int32) throws -> Bool {
        var info = stat()
        if fstatat(directory, name, &info, AT_SYMLINK_NOFOLLOW) == 0 {
            try validate(info)
            return true
        }
        guard errno == ENOENT else { throw ChatStorageError.unsafePath }
        return false
    }

    func read(_ name: String, maximumBytes: Int, in directory: Int32) throws -> Data? {
        let descriptor = openat(directory, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        if descriptor < 0 {
            guard errno == ENOENT else { throw ChatStorageError.unsafePath }
            return nil
        }
        defer { close(descriptor) }
        var before = stat()
        guard fstat(descriptor, &before) == 0 else { throw ChatStorageError.corruptData }
        try validate(before)
        guard fchmod(descriptor, 0o600) == 0 else { throw ChatStorageError.unsafePath }
        guard before.st_size >= 0, before.st_size <= maximumBytes else { throw ChatStorageError.capacityExceeded }
        guard fstat(descriptor, &before) == 0 else { throw ChatStorageError.corruptData }
        var data = Data(count: Int(before.st_size))
        try data.withUnsafeMutableBytes { bytes in
            guard let start = bytes.baseAddress else { return }
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.read(descriptor, start.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw ChatStorageError.corruptData }
                offset += count
            }
        }
        var after = stat()
        guard fstat(descriptor, &after) == 0, before.st_size == after.st_size,
            before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
            before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
            before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
            before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec
        else { throw ChatStorageError.corruptData }
        try validate(after)
        return data
    }

    func remove(_ name: String, in directory: Int32) throws {
        guard try exists(name, in: directory) else { return }
        // unlinkat removes this directory entry; it never follows a replacement
        // symlink or hardlink to delete or write its target.
        guard unlinkat(directory, name, 0) == 0 || errno == ENOENT else { throw ChatStorageError.cleanupRequired }
    }

    func publish(
        _ data: Data, name: String, account: String, directory: Int32,
        beforePublication: () throws -> Void
    ) throws {
        _ = try exists(name, in: directory)
        let temporary = ".pending-" + account + "-" + UUID().uuidString
        let descriptor = openat(
            directory, temporary, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else { throw ChatStorageError.writeFailed }
        defer {
            close(descriptor)
            unlinkat(directory, temporary, 0)
        }
        try protect(root.appendingPathComponent(temporary))
        try data.withUnsafeBytes { bytes in
            guard let start = bytes.baseAddress else { return }
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(descriptor, start.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw ChatStorageError.writeFailed }
                offset += count
            }
        }
        guard fsync(descriptor) == 0 else { throw ChatStorageError.writeFailed }
        try beforePublication()
        guard renameat(directory, temporary, directory, name) == 0 else { throw ChatStorageError.writeFailed }
        // Publication has succeeded. A directory flush has platform-dependent
        // support and cannot turn an already committed snapshot into a failure.
        _ = fsync(directory)
    }

    private func validate(_ info: stat) throws {
        guard info.st_mode & S_IFMT == S_IFREG, info.st_nlink == 1, info.st_uid == geteuid() else {
            throw ChatStorageError.unsafePath
        }
    }

    private func protect(_ url: URL) throws {
        var target = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try target.setResourceValues(values)
        #if os(iOS)
            try FileManager.default.setAttributes([.protectionKey: FileProtectionType.complete], ofItemAtPath: url.path)
        #endif
    }
}
