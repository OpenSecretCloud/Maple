import XCTest

@MainActor final class ChatWorkflowUITests: XCTestCase {
    private var app: XCUIApplication!
    private var composer: XCUIElement { app.textViews["chat.fixture.composer"] }
    private func launch(
        reset: Bool = true, empty: Bool = false, images: Bool = false, dictation: Bool = false, context: Bool = false,
        lifecycle: Bool = false
    ) {
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-chat-workflow-fixture"]
        if reset { app.launchArguments.append("--maple-chat-workflow-reset") }
        if empty { app.launchArguments.append("--maple-chat-workflow-empty") }
        if images { app.launchArguments.append("--maple-chat-workflow-images") }
        if dictation { app.launchArguments.append("--maple-chat-workflow-dictation") }
        if context { app.launchArguments.append("--maple-chat-workflow-context") }
        if lifecycle { app.launchArguments.append("--maple-chat-workflow-lifecycle") }
        app.launch(); XCTAssertTrue(composer.waitForExistence(timeout: 20))
    }
    private func tap(_ identifier: String) {
        let button = app.buttons[identifier].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        for _ in 0..<6 where !button.isHittable {
            if button.frame.midY < app.frame.midY { app.swipeDown() } else { app.swipeUp() }
        }
        XCTAssertTrue(button.isHittable); button.tap()
    }
    private func waitLabel(_ identifier: String, _ value: String) {
        let field = app.staticTexts[identifier]
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        let change = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", value), object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [change], timeout: 15), .completed)
    }
    private func replaceComposer(_ text: String) throws {
        try replaceSyntheticText(composer, with: text, in: app)
    }
    private func checkpoint() { tap("chat.fixture.checkpoint"); waitLabel("chat.fixture.saved", "saved") }

    func testProtectedQueueEditAndHeldComposerSurviveRealProcessRestart() throws {
        launch(); tap("queue.open"); tap("Edit")
        XCTAssertEqual(composer.value as? String, "Queued A")
        try replaceComposer("Edited A awaiting review")
        waitLabel("chat.fixture.held", "Held draft B"); checkpoint()
        app.terminate(); launch(reset: false)
        XCTAssertEqual(composer.value as? String, "Edited A awaiting review")
        waitLabel("chat.fixture.held", "Held draft B")
        waitLabel("chat.fixture.queued", "Queued A")
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.posts", "POSTs 0")
        tap("queue.save")
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.queued", "Edited A awaiting review"); checkpoint()
        app.terminate(); launch(reset: false)
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.queued", "Edited A awaiting review")
        waitLabel("chat.fixture.posts", "POSTs 0")
        tap("queue.open"); tap("Edit"); try replaceComposer("Discard this edit")
        tap("chat.fixture.cancel-edit")
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.queued", "Edited A awaiting review")
    }

    func testInterruptedCanonicalRunRestoresAndReconcilesWithoutAnotherPost() throws {
        launch(empty: true); tap("chat.send")
        waitLabel("chat.fixture.posts", "POSTs 1"); waitLabel("chat.fixture.durability", "durable")
        let canonicalID = app.staticTexts["chat.fixture.message-id"].label
        XCTAssertNotNil(UUID(uuidString: canonicalID)); XCTAssertEqual(canonicalID, canonicalID.lowercased())
        tap("chat.fixture.interrupt"); waitLabel("chat.fixture.phase", "recovering")
        try replaceComposer("New draft while result is uncertain")
        checkpoint(); app.terminate(); launch(reset: false, empty: true)
        waitLabel("chat.fixture.message-id", canonicalID); waitLabel("chat.fixture.posts", "POSTs 0")
        XCTAssertEqual(composer.value as? String, "New draft while result is uncertain")
        tap("chat.recover")
        waitLabel("chat.fixture.issue", "uncertainResponse")
        waitLabel("chat.fixture.last-lookup", canonicalID); waitLabel("chat.fixture.posts", "POSTs 0")
        tap("chat.fixture.allow-recovery"); tap("chat.recover")
        waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.fixture.lookups", "Exact lookups 2"); waitLabel("chat.fixture.posts", "POSTs 0")
        waitLabel("chat.fixture.message.recovered-assistant", "Recovered without replay")
        XCTAssertEqual(composer.value as? String, "New draft while result is uncertain")
    }

    func testForegroundReconcilesUnresolvedOwnerWithoutReplayOrQueueResume() throws {
        try automaticRecovery(foreground: true)
    }
    func testReopeningReconcilesUnresolvedOwnerWithoutReplayOrQueueResume() throws {
        try automaticRecovery(foreground: false)
    }
    private func automaticRecovery(foreground: Bool) throws {
        launch(empty: true, lifecycle: true); tap("chat.send")
        waitLabel("chat.fixture.posts", "POSTs 1")
        let canonical = app.staticTexts["chat.fixture.message-id"].label
        XCTAssertNotNil(UUID(uuidString: canonical))
        try replaceComposer("Paused queued A remains owned during recovery")
        tap("chat.send"); tap("queue.open"); tap("queue.pause"); tap("Done")
        try replaceComposer("Newer draft B must survive lifecycle recovery")
        tap("chat.fixture.interrupt"); waitLabel("chat.fixture.phase", "recovering")
        tap("chat.fixture.allow-recovery")
        if foreground {
            XCUIDevice.shared.press(.home); app.activate()
        } else {
            tap("chat.fixture.navigate"); tap("chat.fixture.navigate")
        }
        waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.fixture.lookups", "Exact lookups 1")
        waitLabel("chat.fixture.last-lookup", canonical)
        waitLabel("chat.fixture.posts", "POSTs 1")
        waitLabel("chat.fixture.queue-state", "paused")
        waitLabel("chat.fixture.queued", "Paused queued A remains owned during recovery")
        waitLabel("chat.fixture.message.recovered-assistant", "Recovered without replay")
        XCTAssertEqual(composer.value as? String, "Newer draft B must survive lifecycle recovery")
        XCTAssertFalse(app.buttons["chat.stop"].exists)
    }

    func testStopBeforeResponseIDCancelsTheLateCreatedResponse() throws {
        launch(empty: true); tap("chat.send"); waitLabel("chat.fixture.posts", "POSTs 1")
        try replaceComposer("Keep typing after Stop")
        tap("chat.stop"); waitLabel("chat.fixture.phase", "stopping")
        tap("chat.fixture.created")
        waitLabel("chat.fixture.cancels", "Cancels 1"); waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.fixture.posts", "POSTs 1")
        XCTAssertEqual(composer.value as? String, "Keep typing after Stop")
    }

    func testOfflineBlocksDispatchAndReconnectLeavesQueuePausedUntilExplicitResume() throws {
        launch(); tap("chat.fixture.connectivity")
        XCTAssertTrue(
            app.staticTexts["Offline — your draft stays here"].exists
                || app.otherElements["chat.fixture.offline"].exists)
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        try replaceComposer("Offline edits remain local")
        tap("queue.open"); XCTAssertFalse(app.buttons["queue.resume"].isEnabled); tap("Done")
        tap("chat.fixture.connectivity")
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.posts", "POSTs 0")
        XCTAssertEqual(composer.value as? String, "Offline edits remain local")
        tap("queue.open"); tap("queue.resume"); tap("Done")
        waitLabel("chat.fixture.posts", "POSTs 1"); waitLabel("chat.fixture.durability", "durable")
        XCTAssertEqual(composer.value as? String, "Offline edits remain local")
    }
    func testOfflineDuringActiveRunKeepsStopAndReconnectDoesNotResumeQueue() throws {
        launch(empty: true); tap("chat.send")
        waitLabel("chat.fixture.posts", "POSTs 1")
        tap("chat.fixture.created"); waitLabel("chat.fixture.phase", "streaming")
        try replaceComposer("Queued A survives an offline active run")
        tap("chat.send")
        waitLabel("chat.fixture.queued", "Queued A survives an offline active run")
        try replaceComposer("Held draft B remains unsent")
        tap("chat.fixture.connectivity")
        waitLabel("chat.fixture.queue-state", "paused")
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        XCTAssertTrue(app.buttons["chat.stop"].isEnabled)
        tap("chat.stop")
        waitLabel("chat.fixture.cancels", "Cancels 1")
        waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.fixture.posts", "POSTs 1")
        waitLabel("chat.fixture.queued", "Queued A survives an offline active run")
        XCTAssertEqual(composer.value as? String, "Held draft B remains unsent")
        tap("chat.fixture.connectivity")
        waitLabel("chat.fixture.queue-state", "paused")
        waitLabel("chat.fixture.posts", "POSTs 1")
        tap("chat.fixture.navigate"); tap("chat.fixture.navigate")
        waitLabel("chat.fixture.queue-state", "paused")
        waitLabel("chat.fixture.posts", "POSTs 1")
        XCTAssertEqual(composer.value as? String, "Held draft B remains unsent")
        checkpoint(); app.terminate(); launch(reset: false, empty: true)
        waitLabel("chat.fixture.queue-state", "paused")
        waitLabel("chat.fixture.posts", "POSTs 0")
        waitLabel("chat.fixture.queued", "Queued A survives an offline active run")
        XCTAssertEqual(composer.value as? String, "Held draft B remains unsent")
    }

    func testRestoredQueueEditDeletePreservesHeldDraftAndImagesAcrossNavigation() throws {
        launch(images: true)
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 1")
        tap("queue.open"); tap("Edit")
        waitLabel("chat.fixture.image-counts", "Composer images 1; held images 2; queued images 1")
        try replaceComposer("Edited A with retained image")
        checkpoint(); app.terminate(); launch(reset: false, images: true)
        XCTAssertEqual(composer.value as? String, "Edited A with retained image")
        waitLabel("chat.fixture.image-counts", "Composer images 1; held images 2; queued images 1")
        tap("chat.fixture.navigate")
        XCTAssertTrue(app.staticTexts["chat.fixture.away"].waitForExistence(timeout: 5))
        tap("chat.fixture.navigate")
        XCTAssertEqual(composer.value as? String, "Edited A with retained image")
        tap("queue.open"); tap("Delete"); tap("Done")
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 0")
        waitLabel("chat.fixture.posts", "POSTs 0")
        checkpoint(); app.terminate(); launch(reset: false, images: true)
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 0")
        waitLabel("chat.fixture.posts", "POSTs 0")
    }

    func testActiveReplyCompletionCannotConsumeTheQueueItemBeingEdited() throws {
        launch(empty: true); tap("chat.send"); waitLabel("chat.fixture.posts", "POSTs 1")
        tap("chat.fixture.created")
        try replaceComposer("Queued A during active reply"); tap("chat.send")
        try replaceComposer("Held B while A is edited")
        tap("queue.open"); tap("queue.pause"); tap("Edit")
        try replaceComposer("Edited A during completion")
        tap("chat.fixture.complete"); waitLabel("chat.fixture.phase", "idle")
        XCTAssertEqual(composer.value as? String, "Edited A during completion")
        waitLabel("chat.fixture.held", "Held B while A is edited")
        waitLabel("chat.fixture.queued", "Queued A during active reply")
        waitLabel("chat.fixture.posts", "POSTs 1")
        tap("chat.fixture.cancel-edit")
        XCTAssertEqual(composer.value as? String, "Held B while A is edited")
        waitLabel("chat.fixture.queued", "Queued A during active reply")
        waitLabel("chat.fixture.posts", "POSTs 1")
    }

    func testRestoredQueueEditSavePreservesOriginalImageOwnersAndRestartPause() throws {
        try finishRestoredImageEdit(save: true)
    }

    func testRestoredQueueEditCancelPreservesOriginalImageOwnersAndRestartPause() throws {
        try finishRestoredImageEdit(save: false)
    }

    private func finishRestoredImageEdit(save: Bool) throws {
        launch(images: true)
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 1")
        let heldImages = app.staticTexts["chat.fixture.composer-image-ids"].label
        let originalImages = app.staticTexts["chat.fixture.queued-image-ids"].label
        XCTAssertNotEqual(heldImages, "none"); XCTAssertNotEqual(originalImages, "none")
        XCTAssertNotEqual(heldImages, originalImages)
        tap("queue.open"); tap("Edit")
        try replaceComposer("Unfinished A keeps its image")
        waitLabel("chat.fixture.composer-image-ids", originalImages)
        waitLabel("chat.fixture.held-image-ids", heldImages)
        waitLabel("chat.fixture.queued", "Queued A")
        checkpoint(); app.terminate(); launch(reset: false, images: true)
        XCTAssertEqual(composer.value as? String, "Unfinished A keeps its image")
        waitLabel("chat.fixture.composer-image-ids", originalImages)
        waitLabel("chat.fixture.held-image-ids", heldImages)
        waitLabel("chat.fixture.queued-image-ids", originalImages)
        tap("chat.fixture.navigate"); tap("chat.fixture.navigate")
        XCTAssertEqual(composer.value as? String, "Unfinished A keeps its image")
        tap("queue.open")
        XCTAssertFalse(app.buttons["queue.resume"].isEnabled, "Resume cannot bypass a restored unfinished edit")
        tap("Done")
        tap(save ? "queue.save" : "chat.fixture.cancel-edit")
        let restoredComposer = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value == %@", "Held draft B"), object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [restoredComposer], timeout: 15), .completed)
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.composer-image-ids", heldImages)
        waitLabel("chat.fixture.held-image-ids", "none")
        waitLabel("chat.fixture.queued-image-ids", originalImages)
        let expectedQueued = save ? "Unfinished A keeps its image" : "Queued A"
        waitLabel("chat.fixture.queued", expectedQueued)
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.posts", "POSTs 0")
        checkpoint(); app.terminate(); launch(reset: false, images: true)
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.composer-image-ids", heldImages)
        waitLabel("chat.fixture.queued-image-ids", originalImages)
        waitLabel("chat.fixture.queued", expectedQueued)
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.posts", "POSTs 0")
        tap("queue.open"); XCTAssertTrue(app.buttons["queue.resume"].isEnabled); tap("Done")
    }

    func testPendingDictationDoesNotRestartButUnfinishedImageEditAndHeldDraftDo() throws {
        launch(images: true, dictation: true)
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 1")
        let heldImages = app.staticTexts["chat.fixture.composer-image-ids"].label
        let originalImages = app.staticTexts["chat.fixture.queued-image-ids"].label
        tap("queue.open"); tap("Edit"); try replaceComposer("Unfinished editor before capture")
        tap("dictation.start"); waitLabel("chat.fixture.dictation-phase", "recording")
        XCTAssertFalse(app.buttons["queue.save"].isEnabled)
        tap("dictation.stop"); waitLabel("chat.fixture.dictation-phase", "transcribing")
        checkpoint(); app.terminate(); launch(reset: false, images: true, dictation: true)
        XCTAssertEqual(composer.value as? String, "Unfinished editor before capture")
        waitLabel("chat.fixture.dictation-phase", "idle")
        XCTAssertFalse(app.buttons["dictation.stop"].exists)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
        XCTAssertTrue(app.buttons["queue.save"].isEnabled)
        waitLabel("chat.fixture.held", "Held draft B")
        waitLabel("chat.fixture.queued", "Queued A")
        waitLabel("chat.fixture.composer-image-ids", originalImages)
        waitLabel("chat.fixture.held-image-ids", heldImages)
        waitLabel("chat.fixture.queued-image-ids", originalImages)
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.posts", "POSTs 0")
        tap("chat.fixture.cancel-edit")
        XCTAssertEqual(composer.value as? String, "Held draft B")
        waitLabel("chat.fixture.composer-image-ids", heldImages)
        waitLabel("chat.fixture.queued", "Queued A")
        waitLabel("chat.fixture.posts", "POSTs 0")
    }

    func testContextLimitUsesShippingRecoveryControlsWithoutOverwritingNewDraftOrReplaying() throws {
        launch(empty: true, context: true)
        try replaceComposer("Rejected original message")
        tap("chat.fixture.hold-context"); tap("chat.send")
        waitLabel("chat.fixture.posts", "POSTs 1"); waitLabel("chat.fixture.context-gate", "held")
        try replaceComposer("New composer while rejection is pending")
        tap("chat.fixture.reject-context"); waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.error", "Your message exceeds the context limit for this model.")
        XCTAssertEqual(composer.value as? String, "New composer while rejection is pending")
        waitLabel("chat.fixture.unsent", "Rejected original message")
        tap("Restore unsent message")
        XCTAssertEqual(composer.value as? String, "Rejected original message")
        waitLabel("chat.fixture.unsent", "New composer while rejection is pending")
        waitLabel("chat.fixture.posts", "POSTs 1")
        checkpoint(); app.terminate(); launch(reset: false, empty: true, context: true)
        XCTAssertEqual(composer.value as? String, "Rejected original message")
        waitLabel("chat.fixture.unsent", "New composer while rejection is pending")
        waitLabel("chat.fixture.posts", "POSTs 0")
        tap("Restore unsent message")
        XCTAssertEqual(composer.value as? String, "New composer while rejection is pending")
        waitLabel("chat.fixture.unsent", "Rejected original message")
        waitLabel("chat.fixture.posts", "POSTs 0")
    }

    func testContextRejectedQueueHeadReturnsPausedWithItsImageAndHeldComposer() throws {
        launch(images: true, context: true)
        waitLabel("chat.fixture.image-counts", "Composer images 2; held images 0; queued images 1")
        let heldImages = app.staticTexts["chat.fixture.composer-image-ids"].label
        let queuedImages = app.staticTexts["chat.fixture.queued-image-ids"].label
        tap("chat.fixture.hold-context"); tap("queue.open"); tap("queue.resume"); tap("Done")
        waitLabel("chat.fixture.posts", "POSTs 1"); waitLabel("chat.fixture.context-gate", "held")
        try replaceComposer("Held composer survives queued rejection")
        tap("chat.fixture.reject-context"); waitLabel("chat.fixture.phase", "idle")
        waitLabel("chat.error", "Your message exceeds the context limit for this model.")
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.queued", "Queued A")
        waitLabel("chat.fixture.composer-image-ids", heldImages)
        waitLabel("chat.fixture.queued-image-ids", queuedImages)
        XCTAssertEqual(composer.value as? String, "Held composer survives queued rejection")
        waitLabel("chat.fixture.posts", "POSTs 1")
        checkpoint(); app.terminate(); launch(reset: false, images: true, context: true)
        XCTAssertEqual(composer.value as? String, "Held composer survives queued rejection")
        waitLabel("chat.fixture.queue-state", "paused"); waitLabel("chat.fixture.queued", "Queued A")
        waitLabel("chat.fixture.composer-image-ids", heldImages)
        waitLabel("chat.fixture.queued-image-ids", queuedImages)
        waitLabel("chat.fixture.posts", "POSTs 0")
    }

}
