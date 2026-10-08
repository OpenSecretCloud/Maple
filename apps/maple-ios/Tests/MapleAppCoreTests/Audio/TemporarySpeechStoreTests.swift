import Darwin
import Foundation
import Testing
@testable import MapleAppCore

@Suite struct TemporarySpeechStoreTests {
    private func root() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent(
            "speech-store-\(UUID().uuidString)", isDirectory: true)
    }
    private func body(_ data: Data) -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream {
            $0.yield(data); $0.finish()
        }
    }
    @Test func protectedOpaqueFilesPersistUntilExplicitSessionCleanup() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        let store = TemporarySpeechStore(root: root), owner = SpeechOwner(account: accountScope())
        try await store.startupCleanup(); try await store.activate(owner)
        let file = try await store.write(body(Data("wave".utf8)), owner: owner)
        #expect(file.byteCount == 4)
        #expect(await store.allocatedBytes == 4)
        #expect(!file.url.lastPathComponent.contains(owner.account.principalID))
        var info = stat(); #expect(lstat(file.url.path, &info) == 0 && info.st_mode & 0o777 == 0o600)
        #expect(try file.url.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)
        #expect(try Data(contentsOf: file.url) == Data("wave".utf8))
        try await store.retire(owner)
        #expect(!FileManager.default.fileExists(atPath: file.url.path))
        #expect(await store.allocatedBytes == 0)
    }
    @Test func perFileAndSessionCapsReclaimOnlyFailedWrites() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        let store = TemporarySpeechStore(root: root, maximumFileBytes: 4, maximumSessionBytes: 6)
        let owner = SpeechOwner(account: accountScope())
        try await store.startupCleanup(); try await store.activate(owner)
        await #expect(throws: SpeechIssue.capacity) {
            try await store.write(body(Data(repeating: 1, count: 5)), owner: owner)
        }
        #expect(await store.allocatedBytes == 0)
        let file = try await store.write(body(Data(repeating: 2, count: 4)), owner: owner)
        await #expect(throws: SpeechIssue.capacity) {
            try await store.write(body(Data(repeating: 3, count: 3)), owner: owner)
        }
        #expect(await store.allocatedBytes == 4)
        #expect(try Data(contentsOf: file.url) == Data(repeating: 2, count: 4))
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).count == 1)
    }
    @Test func retirementRejectsUncooperativeLateStreamAndSameAccountLease() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        let store = TemporarySpeechStore(root: root), owner = SpeechOwner(account: accountScope())
        try await store.startupCleanup(); try await store.activate(owner)
        let stream = AsyncThrowingStream<Data, Error>.makeStream()
        let writing = Task { try await store.write(stream.stream, owner: owner) }
        stream.continuation.yield(Data([1, 2, 3]))
        for _ in 0..<10_000 {
            if await store.allocatedBytes == 3 { break }; await Task.yield()
        }
        #expect(await store.allocatedBytes == 3)
        try await store.retire(owner)
        let nextAccount = AccountScope(apiOrigin: owner.account.apiOrigin, principalID: owner.account.principalID)
        let next = SpeechOwner(account: nextAccount)
        try await store.activate(next)
        stream.continuation.yield(Data([4, 5])); stream.continuation.finish()
        await #expect(throws: SpeechIssue.retired) { try await writing.value }
        await #expect(throws: SpeechIssue.retired) { try await store.activate(owner) }
        #expect(await store.allocatedBytes == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
        let current = try await store.write(body(Data([9])), owner: next)
        try await store.retire(owner)
        #expect(try Data(contentsOf: current.url) == Data([9]))
    }
    @Test func startupRemovesOnlyRecognizedOwnedFilesAndNeverFollowsLinks() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let keep = root.appendingPathComponent("keep.txt")
        try Data("unrelated".utf8).write(to: keep)
        let prefix = "speech-\(UUID().uuidString)-"
        let orphan = root.appendingPathComponent(prefix + UUID().uuidString + ".wav")
        let alias = root.appendingPathComponent(prefix + UUID().uuidString + ".wav")
        try Data("old audio".utf8).write(to: orphan)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: keep)
        let store = TemporarySpeechStore(root: root)
        try await store.startupCleanup()
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path) == ["keep.txt"])
        #expect(try Data(contentsOf: keep) == Data("unrelated".utf8))
    }
    @Test func rejectsSymlinkRootAndRequiresStartupCleanup() async throws {
        let base = root(); defer { try? FileManager.default.removeItem(at: base) }
        let real = base.appendingPathComponent("real"), alias = base.appendingPathComponent("alias")
        try FileManager.default.createDirectory(at: real, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: real)
        let unsafe = TemporarySpeechStore(root: alias)
        await #expect(throws: SpeechIssue.storage) { try await unsafe.startupCleanup() }
        let uninitialized = TemporarySpeechStore(root: real)
        await #expect(throws: SpeechIssue.storage) {
            try await uninitialized.activate(SpeechOwner(account: accountScope()))
        }
    }
    @Test func emptyAndOversizedFramesLeaveNoFile() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        let store = TemporarySpeechStore(root: root), owner = SpeechOwner(account: accountScope())
        try await store.startupCleanup(); try await store.activate(owner)
        await #expect(throws: SpeechIssue.invalidAudio) { try await store.write(body(Data()), owner: owner) }
        await #expect(throws: SpeechIssue.capacity) {
            try await store.write(
                body(Data(repeating: 0, count: SpeechLimits.maximumStreamBlockBytes + 1)), owner: owner)
        }
        #expect(await store.allocatedBytes == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }
    @Test func failedRetirementRevokesImmediatelyAndBlocksNewOwnerUntilExactCleanup() async throws {
        let root = root(); defer { try? FileManager.default.removeItem(at: root) }
        let store = TemporarySpeechStore(root: root), owner = SpeechOwner(account: accountScope())
        try await store.startupCleanup(); try await store.activate(owner)
        let file = try await store.write(body(Data([1, 2, 3])), owner: owner)
        try FileManager.default.removeItem(at: file.url)
        try FileManager.default.createDirectory(at: file.url, withIntermediateDirectories: false)
        await #expect(throws: SpeechIssue.storage) { try await store.retire(owner) }
        let next = SpeechOwner(account: accountScope())
        await #expect(throws: SpeechIssue.storage) { try await store.activate(next) }
        await #expect(throws: SpeechIssue.retired) { try await store.write(body(Data([4])), owner: owner) }
        try FileManager.default.removeItem(at: file.url)
        try await store.retire(owner); try await store.activate(next)
        #expect(await store.allocatedBytes == 0)
        let current = try await store.write(body(Data([9])), owner: next)
        try await store.retire(owner)
        #expect(try Data(contentsOf: current.url) == Data([9]))
    }
}
