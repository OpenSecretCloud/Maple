import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct ListeningTransitionTests {
    @Test func speakerUsesSavedVoiceAndSpeedWithoutEnablingAutoplay() async {
        let (session, preparer, player) = listeningFixture()
        session.updatePreferences(voice: .frenchMale, speed: .fast)
        session.playReply(speechReply("one")); await session.waitForPreparation()
        #expect(session.phase == .playing && player.speed == .fast)
        #expect(!session.autoplayEnabled(chatID: "chat"))
        #expect(await preparer.requests.first?.voice == .frenchMale)
        var saved: MaplePlaybackSpeed?
        session.onSpeedChanged = { saved = $0 }
        session.setSpeed(.double)
        #expect(saved == .double && player.speed == .double)
        session.playReply(speechReply("two", chat: "other")); await session.waitForPreparation()
        #expect(player.loads == ["one", "two"] && player.speed == .double)
        #expect(await preparer.retired.count == 1)
    }

    @Test func offscreenCompletionAndReturningNeverStartCatchUpAudio() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.setVisibility(chatID: "other", foreground: true)
        session.completed(speechReply("missed"), wasVisible: false, wasForeground: true)
        session.setVisibility(chatID: "chat", foreground: true)
        #expect(session.pendingCount == 0 && !session.isPresented && player.playCount == 0)
        session.completed(speechReply("now"), wasVisible: true, wasForeground: true)
        await session.waitForPreparation()
        #expect(player.loads == ["now"] && player.playCount == 1)
        session.completed(speechReply("recovered"), wasVisible: true, wasForeground: true, recovered: true)
        session.completed(speechReply("stopped"), wasVisible: true, wasForeground: true, wasStopped: true)
        #expect(session.pendingCount == 0)
    }

    @Test func navigationKeepsCurrentPlayingButBlocksAutomaticAdvancement() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.completed(speechReply("two"), wasVisible: true, wasForeground: true)
        session.setVisibility(chatID: "other", foreground: false)
        #expect(player.isPlaying)
        player.finish(); #expect(session.phase == .ended && session.pendingCount == 1)
        session.setVisibility(chatID: "chat", foreground: true)
        #expect(player.playCount == 1 && session.phase == .ended)
        session.setVisibility(chatID: "other", foreground: false)
        #expect(session.play()); await session.waitForPreparation()
        #expect(player.loads == ["one", "two"] && player.isPlaying)
    }

    @Test func manualPauseAndRoutePauseRetainFIFOUntilExplicitPlay() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.pause()
        session.completed(speechReply("two"), wasVisible: true, wasForeground: true)
        session.completed(speechReply("three"), wasVisible: true, wasForeground: true)
        session.routeDisconnected(); session.routeReconnected()
        #expect(player.playCount == 1 && session.manualPaused && session.routePaused)
        #expect(session.pending.map(\.responseID) == ["two", "three"])
        #expect(session.play()); #expect(!session.manualPaused && !session.routePaused)
        player.finish(); await session.waitForPreparation()
        #expect(player.loads == ["one", "two"] && session.pending.map(\.responseID) == ["three"])
    }

    @Test func captureAndTranscriptionBlockPlayAndReviewRequiresExplicitResume() async {
        let (session, _, player) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.startDictation(); #expect(!session.play() && !player.isPlaying)
        session.transcriptionStarted(); #expect(!session.play())
        session.dictationFinishedOrCancelled()
        #expect(session.dictationHold == .review && player.playCount == 1)
        #expect(session.play()); #expect(player.playCount == 2 && session.dictationHold == .none)
    }

    @Test func manualPauseSurvivesDictationAndSendWithoutSkippingPendingReplies() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.pause(); session.startDictation(); session.dictationFinishedOrCancelled()
        session.completed(speechReply("two"), wasVisible: true, wasForeground: true)
        session.sentAfterDictation(chatID: "chat", turnID: "new-turn")
        session.completed(speechReply("new", turn: "new-turn"), wasVisible: true, wasForeground: true)
        #expect(session.manualPaused && player.playCount == 1 && session.current?.responseID == "one")
        #expect(session.pending.map(\.responseID) == ["two", "new"])
        #expect(session.play()); player.finish(); await session.waitForPreparation()
        #expect(session.current?.responseID == "two")
    }

    @Test func soleReviewHoldArmsOnlyTheExplicitlySentTurnForReplacement() async {
        let (session, preparer, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("old")); await session.waitForPreparation()
        session.startDictation(); session.dictationFinishedOrCancelled()
        session.sentAfterDictation(chatID: "chat", turnID: "armed")
        #expect(player.playCount == 1 && !player.isPlaying && session.current?.responseID == "old")
        session.completed(speechReply("unrelated", turn: "not-armed"), wasVisible: true, wasForeground: true)
        #expect(session.current?.responseID == "old" && player.playCount == 1)
        session.completed(speechReply("new", turn: "armed"), wasVisible: true, wasForeground: true)
        await session.waitForPreparation()
        #expect(session.current?.responseID == "new" && player.playCount == 2)
        #expect(session.pending.isEmpty)
        #expect(await preparer.retired.count == 1)
    }

    @Test func explicitResumeCancelsDictationReplacementArm() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("old")); await session.waitForPreparation()
        session.startDictation(); session.dictationFinishedOrCancelled()
        session.sentAfterDictation(chatID: "chat", turnID: "armed")
        #expect(session.play())
        session.completed(speechReply("new", turn: "armed"), wasVisible: true, wasForeground: true)
        #expect(session.current?.responseID == "old" && session.pending.map(\.responseID) == ["new"])
        #expect(player.playCount == 2)
    }

    @Test func closeDisablesAutoplayAndRejectsLatePreparationAndPlayerEvents() async throws {
        let (session, preparer, player) = listeningFixture(), gate = ChatTestGate()
        session.setAutoplay(true, chatID: "chat"); await preparer.hold(gate)
        session.playReply(speechReply("one"))
        await gate.waitForArrival(); session.close(); await gate.release()
        try await session.waitForCleanup()
        await preparer.waitForDiscard()
        #expect(!session.isPresented && !session.autoplayEnabled(chatID: "chat") && player.playCount == 0)
        #expect(await preparer.discarded.count == 1)
    }

    @Test func preparationCannotAutomaticallyStartAfterLeavingItsChat() async {
        let (session, preparer, player) = listeningFixture(), gate = ChatTestGate()
        session.setAutoplay(true, chatID: "chat"); await preparer.hold(gate)
        session.completed(speechReply("one"), wasVisible: true, wasForeground: true)
        await gate.waitForArrival(); session.setVisibility(chatID: "other", foreground: false)
        await gate.release(); await session.waitForPreparation()
        #expect(session.phase == .paused && player.playCount == 0)
        session.setVisibility(chatID: "chat", foreground: true)
        #expect(player.playCount == 0)
    }

    @Test func pendingCapacityIsExplicitAndSkipsClampWithoutResumingPause() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation(); session.pause()
        for id in 0..<17 { session.completed(speechReply("pending-\(id)"), wasVisible: true, wasForeground: true) }
        #expect(session.pendingCount == 16 && session.issue == .capacity)
        session.skip(seconds: -10); #expect(session.position == 0)
        session.skip(seconds: 1_000); #expect(session.position == 30)
        #expect(session.phase == .ended && player.playCount == 1 && session.manualPaused)
    }

    @Test func retirementClearsAutoplayAndIgnoresOldEndedCallback() async throws {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        let old = try #require(player.currentID), callback = player.onEvent
        try await session.invalidate()
        callback?(.ended(old))
        session.completed(speechReply("late"), wasVisible: true, wasForeground: true)
        #expect(session.phase == .closed && !session.isPresented && session.autoplayChats.isEmpty)
        #expect(player.playCount == 1)
    }
    @Test func cleanupFailureStaysVisibleAndRetryClearsExactRetiredOwner() async throws {
        let (session, preparer, _) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        await preparer.failRetire(true)
        session.close()
        await #expect(throws: SpeechIssue.storage) { try await session.waitForCleanup() }
        #expect(session.cleanupRequired && !session.isPresented)
        await preparer.failRetire(false)
        try await session.retryCleanup()
        #expect(!session.cleanupRequired)
        #expect(await preparer.retired.count == 1)
    }
    @Test func systemResumeRequiresRealInterruptionPermissionAndNeverOverridesManualPause() async {
        let (session, _, player) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.interruptionBegan(); session.interruptionEnded(shouldResume: false)
        #expect(player.playCount == 1 && session.routePaused)
        #expect(session.play())
        session.interruptionBegan(); session.interruptionEnded(shouldResume: true)
        #expect(player.playCount == 3 && !session.routePaused)
        session.interruptionBegan(); session.pause(); session.interruptionEnded(shouldResume: true)
        #expect(player.playCount == 3 && session.manualPaused)
    }
    @Test func staleRemoteCommandCannotControlReplacementSession() async throws {
        let (session, _, player) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        let old = try #require(session.sessionID)
        session.playReply(speechReply("two")); await session.waitForPreparation()
        #expect(!session.remote(.pause, sessionID: old) && player.isPlaying)
        let current = try #require(session.sessionID)
        #expect(session.remote(.forwardTenSeconds, sessionID: current) && session.position == 10)
        #expect(session.remote(.pause, sessionID: current) && !player.isPlaying)
    }
    @Test func typedCompletionFromRetiredSamePrincipalGenerationIsIgnored() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        let old = AccountScope(apiOrigin: session.scope.apiOrigin, principalID: session.scope.principalID)
        let completion = ChatCompletion(
            scope: old, runID: UUID(), conversationID: "chat", messageID: UUID().uuidString,
            responseID: "late", assistantItems: [], recovered: false, wasStopped: false,
            wasVisible: true, wasForeground: true)
        session.completed(completion)
        #expect(!session.isPresented && player.playCount == 0)
    }
    @Test func admissionRefreshNeverInterruptsBufferedPlaybackOrDropsPendingFIFO() async {
        let preparer = ListeningPreparer(), player = ListeningPlayer()
        let admission = ListeningAdmission()
        let session = ReplyListeningSession(
            scope: accountScope(), preparer: preparer, player: player, admission: { admission.issue })
        session.setVisibility(chatID: "chat", foreground: true); session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.completed(speechReply("two"), wasVisible: true, wasForeground: true)
        admission.issue = .unavailable
        session.pause(); #expect(session.play() && player.isPlaying)
        player.finish()
        #expect(session.phase == .ended && session.pending.map(\.responseID) == ["two"])
        #expect(session.issue == .unavailable && !session.play())
        #expect(await preparer.requests.count == 1)
        admission.issue = nil
        #expect(session.play()); await session.waitForPreparation()
        #expect(session.current?.responseID == "two" && player.isPlaying)
    }
    @Test func backwardSeekAfterEndMakesCurrentAudioResumableWithoutNewSynthesis() async {
        let (session, preparer, player) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        player.finish(); #expect(session.phase == .ended)
        session.skip(seconds: -10)
        #expect(session.phase == .paused && session.position == 20)
        #expect(session.play() && player.isPlaying)
        #expect(await preparer.requests.count == 1)
    }
    @Test func seekToEndWhilePausedAllowsExplicitOffscreenPlayOfAlreadyPendingReply() async {
        let (session, _, player) = listeningFixture()
        session.setAutoplay(true, chatID: "chat")
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.completed(speechReply("two"), wasVisible: true, wasForeground: true)
        session.pause(); session.skip(seconds: 1_000)
        #expect(session.phase == .ended && session.manualPaused && !player.isPlaying)
        session.setVisibility(chatID: "other", foreground: false)
        #expect(session.play()); await session.waitForPreparation()
        #expect(session.current?.responseID == "two" && player.isPlaying)
    }
    @Test func transcriptionRetryReacquiresHoldAfterExplicitReviewResume() async {
        let (session, _, player) = listeningFixture()
        session.playReply(speechReply("one")); await session.waitForPreparation()
        session.startDictation(); session.dictationFinishedOrCancelled()
        #expect(session.play() && player.isPlaying)
        session.transcriptionStarted()
        #expect(session.dictationHold == .transcription && !player.isPlaying && !session.play())
        session.dictationFinishedOrCancelled()
        #expect(session.dictationHold == .review && player.playCount == 2)
    }
}
