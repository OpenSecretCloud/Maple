import Foundation
import Observation

@MainActor @Observable public final class PreferencesController {
    public enum State: Equatable, Sendable { case idle, loading, ready, failed(PreferencesIssue), retired }
    public let scope: AccountScope
    public private(set) var value = LocalPreferences()
    public private(set) var state: State = .idle
    public private(set) var saveIssue: PreferencesIssue?
    public var onChange: (@MainActor (LocalPreferences) -> Void)?
    private let store: any PreferencesStoring
    private var active = true
    private var operation = UUID()
    private var revision: UInt64 = 0
    private var saveTask: Task<Void, Never>?

    public init(scope: AccountScope, store: any PreferencesStoring) { self.scope = scope; self.store = store }

    public func load() async {
        guard active, state != .loading, state != .ready else { return }
        let ticket = UUID(); operation = ticket; state = .loading
        do {
            let loaded = try await store.activate(scope)
            guard active, operation == ticket else { return }
            value = loaded; state = .ready; onChange?(loaded)
        } catch {
            guard active, operation == ticket else { return }
            state = .failed(Self.safe(error))
        }
    }

    @discardableResult public func setTheme(_ theme: MapleTheme) -> Bool { update { $0.theme = theme } }
    @discardableResult public func setChatFont(_ font: MapleChatFont) -> Bool { update { $0.chatFont = font } }
    @discardableResult public func setVoice(_ voice: MapleVoice) -> Bool { update { $0.voice = voice } }
    @discardableResult public func setPlaybackSpeed(_ speed: MaplePlaybackSpeed) -> Bool {
        update { $0.playbackSpeed = speed }
    }
    @discardableResult public func setWebSearchPreferred(_ enabled: Bool) -> Bool {
        update { $0.webSearchPreferred = enabled }
    }
    @discardableResult public func setSelectedModelID(_ id: String) -> Bool {
        guard LocalPreferences.isValidModelID(id) else { return false }
        return update { $0.selectedModelID = id }
    }

    /// Await before controlled lifecycle transitions. UserDefaults itself owns
    /// asynchronous disk flushing; this drains application writes, not an fsync.
    public func flush() async { await saveTask?.value }

    public func retrySave() {
        guard active, state == .ready, saveIssue != nil else { return }
        enqueueSave()
    }

    public func retire(removePersisted: Bool = false) async throws {
        active = false; operation = UUID(); state = .retired; onChange = nil
        await saveTask?.value
        // Store retirement rejects even uncancellable work that arrives later.
        try await store.retire(scope, removePersisted: removePersisted)
        value = LocalPreferences(); saveIssue = nil
    }

    private func update(_ mutation: (inout LocalPreferences) -> Void) -> Bool {
        guard active, state == .ready else { return false }
        var next = value; mutation(&next)
        guard next != value else { return true }
        value = next; onChange?(next); enqueueSave(); return true
    }

    private func enqueueSave() {
        revision += 1
        let nextRevision = revision, next = value, prior = saveTask
        saveIssue = nil
        saveTask = Task { [weak self, store, scope] in
            await prior?.value
            do {
                try await store.save(next, scope: scope, revision: nextRevision)
                guard let self, self.active, self.revision == nextRevision else { return }
                self.saveIssue = nil
            } catch {
                guard let self, self.active, self.revision == nextRevision else { return }
                self.saveIssue = Self.safe(error)
            }
        }
    }

    private static func safe(_ error: any Error) -> PreferencesIssue {
        error as? PreferencesIssue ?? .unavailable
    }
}
