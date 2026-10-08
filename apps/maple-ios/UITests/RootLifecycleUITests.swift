import XCTest

/// Real local signed-in root with deterministic microphone/transcription/path
/// inputs. Account, navigation, draft persistence, composer and ownership are
/// the same implementation used by the ordinary local build.
@MainActor final class RootLifecycleUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launch() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-local-lifecycle-fixture"]; app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        XCTAssertTrue(app.buttons["lifecycle.connectivity"].waitForExistence(timeout: 10))
        app.buttons["chat.new"].tap()
    }
    private func replaceDraft(_ text: String) throws {
        try replaceSyntheticText(app.textViews["chat.composer"], with: text, in: app)
    }
    private func waitForStops(_ count: Int) {
        let match = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.staticTexts["lifecycle.capture-counts"].label.contains("stops \(count),")
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [match], timeout: 15), .completed)
        XCTAssertFalse(app.buttons["dictation.stop"].exists)
    }
    private func startCapture() {
        let start = app.buttons["dictation.start"]
        XCTAssertTrue(start.waitForExistence(timeout: 10)); XCTAssertTrue(start.isEnabled); start.tap()
        XCTAssertTrue(app.buttons["dictation.stop"].waitForExistence(timeout: 10))
    }
    private func completeTranscript() {
        app.buttons["lifecycle.transcript"].tap()
        let inserted = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                (self.app.textViews["chat.composer"].value as? String)?.contains("synthetic dictated text") == true
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [inserted], timeout: 15), .completed)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
    }

    func testRootOfflineAndReconnectPreserveUnsentDraftWithoutDispatch() throws {
        try launch(); try replaceDraft("Explicit local offline draft stays unsent.")
        app.buttons["lifecycle.connectivity"].tap()
        XCTAssertTrue(app.staticTexts["Offline — your draft stays here"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, "Explicit local offline draft stays unsent.")
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        app.buttons["lifecycle.connectivity"].tap()
        let online = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                !self.app.staticTexts["Offline — your draft stays here"].exists
                    && self.app.buttons["chat.send"].isEnabled
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [online], timeout: 15), .completed)
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, "Explicit local offline draft stays unsent.")
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        app.buttons["chat.menu"].tap(); app.buttons["library.close"].tap()
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, "Explicit local offline draft stays unsent.")
    }

    func testRootMenuChatNavigationAndBackgroundStopCapture() throws {
        try launch(); try replaceDraft("Menu capture owner. ")
        startCapture(); app.buttons["chat.menu"].tap()
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        app.buttons["library.close"].tap(); waitForStops(1); completeTranscript()
        try replaceDraft("Background capture owner. ")
        startCapture(); XCUIDevice.shared.press(.home); app.activate()
        waitForStops(2); completeTranscript()
        let heldNewChatDraft = app.textViews["chat.composer"].value as? String
        app.buttons["chat.menu"].tap()
        let recent = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "library.chat.")).firstMatch
        XCTAssertTrue(recent.waitForExistence(timeout: 20))
        let savedChatID = recent.identifier
        recent.tap(); try replaceDraft("Navigation capture owner. ")
        startCapture(); app.buttons["chat.new"].tap()
        waitForStops(3)
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, heldNewChatDraft)
        app.buttons["lifecycle.transcript"].tap()
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, heldNewChatDraft)
        app.buttons["chat.menu"].tap()
        XCTAssertTrue(app.buttons[savedChatID].waitForExistence(timeout: 20)); app.buttons[savedChatID].tap()
        let originalOwner = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                (self.app.textViews["chat.composer"].value as? String)?.contains("synthetic dictated text") == true
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [originalOwner], timeout: 15), .completed)
        XCTAssertFalse(app.buttons["chat.stop"].exists, "Finishing a transcript must never send its draft")
        app.terminate(); app.launch()
        XCTAssertTrue(app.textViews["chat.composer"].waitForExistence(timeout: 30))
        XCTAssertFalse(app.buttons["dictation.stop"].exists)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
        XCTAssertEqual(app.staticTexts["lifecycle.capture-counts"].label, "Starts 0, stops 0, cancelled 0")
    }
}
