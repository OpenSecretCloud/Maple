import Foundation
import Testing
@testable import MapleAppCore

@Suite struct PreferencesStoreTests {
    private func scope(origin: String = "https://preferences.invalid", principal: String = "fixture") -> AccountScope {
        AccountScope(apiOrigin: origin, principalID: principal)
    }

    @Test func defaultsAreNativeContractAndDoNotPersistAutoplay() throws {
        let value = LocalPreferences()
        #expect(value.theme == .system && value.chatFont == .manrope)
        #expect(value.selectedModelID == "auto:quick" && value.webSearchPreferred)
        #expect(value.voice == .casualFemale && value.playbackSpeed == .normal)
        #expect(MaplePlaybackSpeed.allCases.map(\.rawValue) == [1, 1.25, 1.5, 2])
        #expect(
            Set(MapleVoice.allCases.map(\.rawValue))
                == Set([
                    "neutral_female", "neutral_male", "casual_female", "casual_male", "cheerful_female", "ar_male",
                    "de_female", "de_male", "es_female", "es_male", "fr_female", "fr_male", "hi_female", "hi_male",
                    "it_female", "it_male", "nl_female", "nl_male", "pt_female", "pt_male",
                ]))
        let object = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(value)) as? [String: Any])
        #expect(
            Set(object.keys)
                == Set(["theme", "chatFont", "selectedModelID", "webSearchPreferred", "voice", "playbackSpeed"]))
    }

    @Test func originAndPrincipalAreIsolatedAndLogoutRetainsChoicesAcrossRestart() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let store = PreferencesStore(suiteName: suite), owner = scope()
        let choice = LocalPreferences(
            theme: .dark, chatFont: .serif, selectedModelID: "auto:powerful", webSearchPreferred: false,
            voice: .frenchMale, playbackSpeed: .fast)
        #expect(try await store.activate(owner) == LocalPreferences())
        try await store.save(choice, scope: owner, revision: 1)
        try await store.retire(owner)
        let otherOrigin = scope(origin: "https://other.invalid")
        #expect(try await store.activate(otherOrigin) == LocalPreferences())
        try await store.retire(otherOrigin)
        let otherPrincipal = scope(principal: "different")
        #expect(try await store.activate(otherPrincipal) == LocalPreferences())
        try await store.retire(otherPrincipal)
        let restarted = PreferencesStore(suiteName: suite)
        #expect(try await restarted.activate(scope()) == choice)
        let key = try PreferencesStore.key(owner)
        #expect(!key.contains(owner.principalID) && !key.contains(owner.apiOrigin))
        let bytes = try #require(UserDefaults(suiteName: suite)?.data(forKey: key))
        #expect(bytes.count < 4_096)
        #expect(!String(decoding: bytes, as: UTF8.self).contains(owner.apiOrigin))
    }

    @Test func lateWritesAndRetireCannotChangeReplacementGeneration() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let store = PreferencesStore(suiteName: suite), old = scope(), new = scope()
        _ = try await store.activate(old)
        try await store.retire(old)
        _ = try await store.activate(new)
        let choice = LocalPreferences(theme: .light)
        try await store.save(choice, scope: new, revision: 1)
        await #expect(throws: PreferencesIssue.sessionChanged) {
            try await store.save(.init(theme: .dark), scope: old, revision: 9)
        }
        await #expect(throws: PreferencesIssue.sessionChanged) { try await store.activate(old) }
        await #expect(throws: PreferencesIssue.sessionChanged) { try await store.retire(old, removePersisted: true) }
        try await store.retire(new)
        await #expect(throws: PreferencesIssue.sessionChanged) { try await store.retire(old, removePersisted: true) }
        #expect(try await store.activate(scope()) == choice)
    }

    @Test func staleRevisionCannotUndoLatestChoiceAndDeletionRemovesOnlyOwner() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let store = PreferencesStore(suiteName: suite), owner = scope(), other = scope(principal: "other")
        _ = try await store.activate(other)
        try await store.save(.init(theme: .light), scope: other, revision: 1)
        try await store.retire(other)
        _ = try await store.activate(owner)
        try await store.save(.init(theme: .dark), scope: owner, revision: 3)
        try await store.save(.init(theme: .light), scope: owner, revision: 2)
        #expect(try await store.activate(owner).theme == .dark)
        try await store.retire(owner)
        try await store.retire(owner, removePersisted: true)
        #expect(try await store.activate(scope()) == LocalPreferences())
        let restarted = PreferencesStore(suiteName: suite)
        #expect(try await restarted.activate(scope(principal: "other")).theme == .light)
    }

    @Test func corruptAndFutureRecordsFailWithoutBeingOverwritten() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let defaults = try #require(UserDefaults(suiteName: suite)), owner = scope()
        let key = try PreferencesStore.key(owner), store = PreferencesStore(suiteName: suite)
        for data in [Data("invalid".utf8), Data(repeating: 0, count: 4_097)] {
            defaults.set(data, forKey: key)
            await #expect(throws: PreferencesIssue.invalidRecord) { try await store.activate(owner) }
            #expect(defaults.data(forKey: key) == data)
        }
        var record = try #require(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(LocalPreferences())) as? [String: Any])
        let future = try JSONSerialization.data(withJSONObject: ["version": 2, "preferences": record])
        defaults.set(future, forKey: key)
        await #expect(throws: PreferencesIssue.unsupportedVersion) { try await store.activate(owner) }
        #expect(defaults.data(forKey: key) == future)
        record["voice"] = "not-a-provider-voice"
        defaults.set(try JSONSerialization.data(withJSONObject: ["version": 1, "preferences": record]), forKey: key)
        await #expect(throws: PreferencesIssue.invalidRecord) { try await store.activate(owner) }
    }

    @Test func confirmedOldAccountDeletionDoesNotDisturbAnotherSignedInAccount() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let store = PreferencesStore(suiteName: suite), old = scope(), current = scope(principal: "other")
        _ = try await store.activate(old)
        try await store.save(.init(theme: .dark), scope: old, revision: 1)
        try await store.retire(old)
        _ = try await store.activate(current)
        try await store.save(.init(theme: .light), scope: current, revision: 1)
        try await store.retire(old, removePersisted: true)
        #expect(try await store.activate(current).theme == .light)
        try await store.save(.init(theme: .light, voice: .germanMale), scope: current, revision: 2)
        let restarted = PreferencesStore(suiteName: suite)
        #expect(try await restarted.activate(scope()) == LocalPreferences())
    }

    @Test func modelIdentifierIsBoundedAndNotArbitraryText() {
        #expect(LocalPreferences.isValidModelID("vendor/model-1.2:auto"))
        #expect(LocalPreferences.isValidModelID(String(repeating: "a", count: 256)))
        for bad in ["", "model name", "\nmodel", String(repeating: "a", count: 257), "https://user@host?token=secret"] {
            #expect(!LocalPreferences.isValidModelID(bad))
        }
    }
}

@MainActor @Suite struct PreferencesControllerTests {
    @Test func rapidChoicesAndRetirementDrainTheLatestWrite() async throws {
        let suite = "preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let scope = AccountScope(apiOrigin: "https://preferences.invalid", principalID: "fixture")
        let store = PreferencesStore(suiteName: suite), controller = PreferencesController(scope: scope, store: store)
        #expect(!controller.setTheme(.dark))
        await controller.load()
        #expect(controller.setTheme(.dark))
        #expect(controller.setPlaybackSpeed(.double))
        #expect(controller.setVoice(.italianFemale))
        #expect(controller.setWebSearchPreferred(false))
        try await controller.retire()
        #expect(controller.state == .retired && !controller.setTheme(.light))
        let replacement = AccountScope(apiOrigin: scope.apiOrigin, principalID: scope.principalID)
        let retained = try await store.activate(replacement)
        #expect(retained.theme == .dark && retained.playbackSpeed == .double)
        #expect(retained.voice == .italianFemale && !retained.webSearchPreferred)
    }

    @Test func lateLoadCannotPublishAfterRetirement() async throws {
        let store = ControlledPreferencesStore(gateLoad: true)
        let controller = PreferencesController(scope: accountScope(), store: store)
        var publications = 0
        controller.onChange = { _ in publications += 1 }
        let loading = Task { await controller.load() }
        await store.waitForLoad()
        try await controller.retire()
        await store.finishLoad(.init(theme: .dark))
        await loading.value
        #expect(controller.state == .retired && controller.value == LocalPreferences())
        #expect(publications == 0)
    }

    @Test func saveFailureRetainsChoiceAndRetryDoesNotLeakError() async throws {
        let store = ControlledPreferencesStore(failSave: true)
        let controller = PreferencesController(scope: accountScope(), store: store)
        await controller.load()
        controller.setVoice(.hindiMale)
        await controller.flush()
        #expect(controller.saveIssue == .unavailable && controller.value.voice == .hindiMale)
        await store.allowSave()
        controller.retrySave()
        await controller.flush()
        #expect(controller.saveIssue == nil)
        #expect(await store.saved?.voice == .hindiMale)
        #expect(!controller.setSelectedModelID("not a model"))
    }

    @Test func loadFailureCanRetryWithoutSavingDefaultsOverUnknownRecord() async {
        let store = ControlledPreferencesStore(failLoad: true)
        let controller = PreferencesController(scope: accountScope(), store: store)
        await controller.load()
        #expect(controller.state == .failed(.unsupportedVersion))
        #expect(!controller.setWebSearchPreferred(false))
        #expect(await store.saved == nil)
        await store.allowLoad()
        await controller.load()
        #expect(controller.state == .ready)
    }
}

private actor ControlledPreferencesStore: PreferencesStoring {
    struct Failure: Error {}
    var saved: LocalPreferences?
    private var failSave: Bool
    private var failLoad: Bool
    private let gateLoad: Bool
    private var continuation: CheckedContinuation<LocalPreferences, Never>?
    private var started = false
    init(gateLoad: Bool = false, failSave: Bool = false, failLoad: Bool = false) {
        self.gateLoad = gateLoad; self.failSave = failSave; self.failLoad = failLoad
    }
    func activate(_ scope: AccountScope) async throws -> LocalPreferences {
        started = true
        if failLoad { throw PreferencesIssue.unsupportedVersion }
        if gateLoad { return await withCheckedContinuation { continuation = $0 } }
        return .init()
    }
    func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) throws {
        if failSave { throw Failure() }
        saved = value
    }
    func retire(_ scope: AccountScope, removePersisted: Bool) {}
    func waitForLoad() async { while !started { await Task.yield() } }
    func finishLoad(_ value: LocalPreferences) { continuation?.resume(returning: value); continuation = nil }
    func allowSave() { failSave = false }
    func allowLoad() { failLoad = false }
}
