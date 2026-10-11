import XCTest

@MainActor final class DictationUITests: XCTestCase {
    private var app: XCUIApplication!
    private var composer: XCUIElement { app.textViews["dictation.fixture.composer"] }
    private func launchFixture() {
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-dictation-fixture"]; app.launch()
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
    }
    private func tap(_ identifier: String) {
        let control = app.buttons[identifier].firstMatch
        XCTAssertTrue(control.waitForExistence(timeout: 10))
        for _ in 0..<6 where !control.isHittable {
            if control.frame.midY < app.frame.midY { app.swipeDown() } else { app.swipeUp() }
        }
        XCTAssertTrue(control.isHittable); control.tap()
    }
    private func waitForPhase(_ phase: String) {
        let changed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", phase), object: app.staticTexts["dictation.fixture.phase"])
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
    }
    private func startAndStop() {
        tap("dictation.start"); waitForPhase("recording")
        XCTAssertFalse(app.buttons["dictation.fixture.send"].isEnabled)
        tap("dictation.stop"); waitForPhase("transcribing")
    }

    func testInlineStopProgressAndInsertionNeverSendTheDraft() {
        launchFixture(); composer.tap()
        tap("dictation.fixture.cursor-end")
        let caret = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Caret 15"),
            object: app.staticTexts["dictation.fixture.caret"])
        XCTAssertEqual(XCTWaiter.wait(for: [caret], timeout: 10), .completed)
        tap("dictation.start"); waitForPhase("recording")
        composer.typeText(" typed")
        tap("dictation.stop"); waitForPhase("transcribing")
        tap("dictation.fixture.complete"); waitForPhase("idle")
        XCTAssertEqual(composer.value as? String, "Keep this draft typed spoken text")
        XCTAssertEqual(app.staticTexts["dictation.fixture.sends"].label, "Sends 0")
        XCTAssertTrue(app.buttons["dictation.fixture.send"].isEnabled)
    }

    func testFailedTranscriptionOffersRetryAndDiscardPreservesText() {
        launchFixture(); startAndStop()
        tap("dictation.fixture.fail"); waitForPhase("failed")
        XCTAssertTrue(app.buttons["dictation.retry"].exists)
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        tap("dictation.retry"); waitForPhase("transcribing")
        tap("dictation.discard"); waitForPhase("idle")
        tap("dictation.fixture.complete")
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertTrue(app.buttons["dictation.fixture.send"].isEnabled)
    }

    func testDeniedPermissionHasVisibleRecoveryWithoutChangingDraft() {
        launchFixture(); tap("dictation.fixture.deny")
        tap("dictation.start")
        XCTAssertTrue(
            app.otherElements["dictation.error"].waitForExistence(timeout: 10)
                || app.buttons["Open Settings"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Open Settings"].exists)
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertTrue(app.buttons["dictation.fixture.send"].isEnabled)
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Dictation permission recovery"; screenshot.lifetime = .keepAlways; add(screenshot)
    }

    func testMicrophoneRespondsAcrossItsFullIconTargetAndDeniedRecoveryStaysVisible() {
        launchFixture()
        let start = app.buttons["dictation.start"]
        XCTAssertTrue(start.waitForExistence(timeout: 10))
        for _ in 0..<6 where !start.isHittable { app.swipeUp() }
        XCTAssertTrue(start.isHittable)
        XCTAssertGreaterThanOrEqual(start.frame.width, 44)
        XCTAssertGreaterThanOrEqual(start.frame.height, 44)
        // These points lie outside the small mic glyph, inside its full button.
        start.coordinate(withNormalizedOffset: CGVector(dx: 0.08, dy: 0.5)).tap()
        waitForPhase("recording")
        let stop = app.buttons["dictation.stop"]
        XCTAssertTrue(stop.isHittable)
        stop.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
        waitForPhase("transcribing")
        tap("dictation.discard"); waitForPhase("idle")
        tap("dictation.fixture.deny")
        let deniedStart = app.buttons["dictation.start"]
        XCTAssertTrue(deniedStart.isHittable)
        deniedStart.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.08)).tap()
        XCTAssertTrue(app.buttons["Open Settings"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["Dismiss"].isHittable)
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertTrue(app.buttons["dictation.fixture.send"].isEnabled)
    }

    func testMarkedHeldDraftReceivesResultAfterQueueEditorSwitch() {
        launchFixture(); composer.tap()
        tap("dictation.start"); waitForPhase("recording")
        tap("dictation.fixture.mark")
        tap("dictation.fixture.edit"); waitForPhase("transcribing")
        XCTAssertEqual(composer.value as? String, "Queued A")
        tap("dictation.fixture.complete"); waitForPhase("idle")
        XCTAssertEqual(composer.value as? String, "Queued A")
        XCTAssertTrue(app.staticTexts["dictation.fixture.held"].label.contains("spoken text"))
        tap("dictation.fixture.cancel-edit")
        XCTAssertTrue((composer.value as? String)?.contains("spoken text") == true)
        XCTAssertEqual(app.staticTexts["dictation.fixture.sends"].label, "Sends 0")
    }
    func testCancellingPendingEditorARejectsLateResultAndKeepsHeldB() {
        retirePendingEditor(delete: false)
    }

    func testDeletingPendingEditorARejectsLateResultAndKeepsHeldB() {
        retirePendingEditor(delete: true)
    }

    func testCompletedEditorARequiresExplicitSaveWhileHeldBStillTranscribes() {
        beginTwoOwnerTranscriptions()
        tap("dictation.fixture.complete-latest")
        waitForRequests("Requests 2; returned 1")
        let inserted = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                (self.composer.value as? String)?.contains("late editor text") == true
                    && self.app.buttons["queue.save"].isEnabled
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [inserted], timeout: 15), .completed)
        let edited = composer.value as? String
        XCTAssertEqual(app.staticTexts["dictation.fixture.queued"].label, "Queued A")
        XCTAssertEqual(app.staticTexts["dictation.fixture.editor-state"].label, "editing")
        tap("queue.save")
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertEqual(app.staticTexts["dictation.fixture.queued"].label, edited)
        XCTAssertFalse(app.buttons["dictation.fixture.send"].isEnabled, "Held B still owns its pending transcript")
        tap("dictation.fixture.complete"); waitForPhase("idle")
        XCTAssertTrue((composer.value as? String)?.contains("spoken text") == true)
        XCTAssertFalse((composer.value as? String)?.contains("late editor text") == true)
        XCTAssertEqual(app.staticTexts["dictation.fixture.queued"].label, edited)
        XCTAssertEqual(app.staticTexts["dictation.fixture.sends"].label, "Sends 0")
    }

    private func beginTwoOwnerTranscriptions() {
        launchFixture()
        tap("dictation.start"); waitForPhase("recording")
        tap("dictation.fixture.edit"); waitForPhase("transcribing")
        waitForRequests("Requests 1; returned 0")
        XCTAssertEqual(composer.value as? String, "Queued A")
        XCTAssertTrue(app.buttons["queue.save"].isEnabled, "Pending held B must not block unrelated editor A")
        startAndStop(); waitForRequests("Requests 2; returned 0")
        XCTAssertFalse(app.buttons["queue.save"].isEnabled, "Pending A must block committing A")
    }

    private func retirePendingEditor(delete: Bool) {
        beginTwoOwnerTranscriptions()
        if delete {
            tap("dictation.fixture.queue")
            let remove = app.buttons["Delete"].firstMatch
            XCTAssertTrue(remove.waitForExistence(timeout: 10)); remove.tap(); tap("Done")
        } else {
            tap("dictation.fixture.cancel-edit")
        }
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertFalse(app.buttons["dictation.fixture.send"].isEnabled)
        tap("dictation.fixture.complete-latest")
        waitForRequests("Requests 2; returned 1")
        XCTAssertEqual(composer.value as? String, "Keep this draft")
        XCTAssertEqual(app.staticTexts["dictation.fixture.queued"].label, delete ? "none" : "Queued A")
        tap("dictation.fixture.complete"); waitForPhase("idle")
        XCTAssertTrue((composer.value as? String)?.contains("spoken text") == true)
        XCTAssertFalse((composer.value as? String)?.contains("late editor text") == true)
        XCTAssertEqual(app.staticTexts["dictation.fixture.queued"].label, delete ? "none" : "Queued A")
        XCTAssertEqual(app.staticTexts["dictation.fixture.sends"].label, "Sends 0")
        XCTAssertTrue(app.buttons["dictation.fixture.send"].isEnabled)
    }

    private func waitForRequests(_ expected: String) {
        let changed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", expected),
            object: app.staticTexts["dictation.fixture.requests"])
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
    }

}
