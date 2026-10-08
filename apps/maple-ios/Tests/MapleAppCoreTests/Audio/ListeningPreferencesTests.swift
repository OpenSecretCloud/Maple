import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ListeningPreferencesTests {
    @Test func recreatedPreferencesAndListeningSessionKeepSpeedButNeverAutoplay() async throws {
        let suite = "listening-preferences-tests.\(UUID().uuidString)"
        defer { UserDefaults.standard.removePersistentDomain(forName: suite) }
        let oldScope = accountScope()
        let originalPreferences = PreferencesController(scope: oldScope, store: PreferencesStore(suiteName: suite))
        await originalPreferences.load()
        let original = ReplyListeningSession(
            scope: oldScope, preparer: ListeningPreparer(), player: ListeningPlayer())
        original.onSpeedChanged = { [weak originalPreferences] speed in originalPreferences?.setPlaybackSpeed(speed) }
        original.setAutoplay(true, chatID: "chat")
        original.setSpeed(.fast)
        await originalPreferences.flush()
        #expect(originalPreferences.saveIssue == nil && original.autoplayEnabled(chatID: "chat"))
        try await original.invalidate(); try await originalPreferences.retire()

        let newScope = AccountScope(apiOrigin: oldScope.apiOrigin, principalID: oldScope.principalID)
        let loaded = PreferencesController(scope: newScope, store: PreferencesStore(suiteName: suite))
        await loaded.load()
        #expect(loaded.state == .ready && loaded.value.playbackSpeed == .fast)
        let preparer = ListeningPreparer(), player = ListeningPlayer()
        let fresh = ReplyListeningSession(
            scope: newScope, preparer: preparer, player: player,
            voice: loaded.value.voice, speed: loaded.value.playbackSpeed)
        fresh.setVisibility(chatID: "chat", foreground: true)
        #expect(!fresh.isPresented && !fresh.autoplayEnabled(chatID: "chat"))
        fresh.completed(speechReply("ignored"), wasVisible: true, wasForeground: true)
        #expect(await preparer.requests.isEmpty)
        fresh.playReply(speechReply("manual")); await fresh.waitForPreparation()
        #expect(player.speed == .fast && player.loads == ["manual"])
        fresh.close()
        fresh.setAutoplay(true, chatID: "chat")
        fresh.completed(speechReply("automatic"), wasVisible: true, wasForeground: true)
        await fresh.waitForPreparation()
        #expect(player.speed == .fast && player.loads == ["manual", "automatic"])
        #expect(fresh.phase == .playing)
        try await fresh.invalidate(); try await loaded.retire()
    }
}
