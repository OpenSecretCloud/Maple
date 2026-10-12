import XCTest

/// Real local signed-in root with deterministic microphone/transcription/path
/// inputs. Account, navigation, draft persistence, composer and ownership are
/// the same implementation used by the ordinary local build.
@MainActor final class RootLifecycleUITests: XCTestCase {
    private var app: XCUIApplication!
    private var liveWebSearchOriginal: String?

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

    private func tapLiveControl(_ identifier: String) {
        let button = identifier == "chat.menu" ? app.mapleMenuButton : app.buttons[identifier].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        XCTAssertTrue(button.isEnabled && button.isHittable, "Unavailable control: " + identifier)
        button.tap()
    }

    private func waitForLiveUI(_ condition: @escaping @MainActor () -> Bool, timeout: TimeInterval = 15) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: timeout), .completed)
    }

    private func restoreLiveWebSearchIfNeeded() {
        guard let original = liveWebSearchOriginal else { return }
        app.terminate(); app.launchArguments = []; app.launch()
        guard app.buttons["chat.model"].waitForExistence(timeout: 30) else {
            XCTFail("The local root was unavailable while restoring its preference"); return
        }
        tapLiveControl("chat.model")
        let toggle = app.switches["model.web-search"]
        guard toggle.waitForExistence(timeout: 15), toggle.isEnabled else {
            XCTFail("The local web-search preference could not be restored"); return
        }
        if toggle.value as? String != original { tapNativeSwitch(toggle, in: app) }
        waitForLiveUI { toggle.value as? String == original }
        liveWebSearchOriginal = nil
    }

    /// Uses the ordinary signed-in compact root, retained UIKit navigation and
    /// real local services. It creates no chat/project and never sends a turn.
    func testRootChatOptionsProjectNavigationAndMenuSearchHitTargets() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = []; app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        tapLiveControl("chat.new")
        XCTAssertEqual(app.buttons["chat.model"].label, "Chat options")
        tapLiveControl("chat.model")
        let project = app.buttons["chat-options.project"]
        XCTAssertTrue(project.waitForExistence(timeout: 15) && project.isEnabled)
        tapLiveControl("chat-options.project")
        XCTAssertTrue(app.buttons["project-picker.project.none"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["project-picker.new-project"].isEnabled)
        let back = app.navigationBars.buttons.firstMatch
        XCTAssertTrue(back.waitForExistence(timeout: 10) && back.isHittable); back.tap()
        XCTAssertTrue(project.waitForExistence(timeout: 15) && project.isEnabled)

        let toggle = app.switches["model.web-search"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 15) && toggle.isEnabled)
        let original = try XCTUnwrap(toggle.value as? String)
        XCTAssertTrue(original == "0" || original == "1")
        liveWebSearchOriginal = original
        let restore: @MainActor @Sendable () -> Void = { [weak self] in self?.restoreLiveWebSearchIfNeeded() }
        addTeardownBlock { await restore() }
        tapNativeSwitch(toggle, in: app)
        waitForLiveUI { toggle.value as? String == (original == "1" ? "0" : "1") }
        tapNativeSwitch(toggle, in: app); waitForLiveUI { toggle.value as? String == original }
        liveWebSearchOriginal = nil
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)).tap()
        waitForLiveUI { !project.exists && self.app.buttons["chat.model"].isHittable }

        tapLiveControl("chat.menu")
        let search = app.textFields["library.search"]
        XCTAssertTrue(search.waitForExistence(timeout: 15) && search.isHittable)
        // Enter near the field edge, then clear near the button edge rather
        // than only testing the centered glyph's small visible footprint.
        search.coordinate(withNormalizedOffset: CGVector(dx: 0.04, dy: 0.5)).tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        let query = "Native search no matching chat"
        search.typeText(query)
        waitForLiveUI { search.value as? String == query }
        let clear = app.buttons["library.clear-search"]
        XCTAssertTrue(clear.waitForExistence(timeout: 10))
        XCTAssertGreaterThanOrEqual(clear.frame.width, 44)
        XCTAssertGreaterThanOrEqual(clear.frame.height, 44)
        clear.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5)).tap()
        waitForLiveUI {
            let value = search.value as? String
            return value != nil && (value == "" || value == search.placeholderValue)
                && !clear.exists && self.app.buttons["library.close"].isHittable
        }
        XCTAssertFalse(app.buttons["chat.model"].isHittable, "Clearing search must not open a chat behind it")
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        XCTAssertLessThanOrEqual(search.frame.maxY, app.keyboards.firstMatch.frame.minY)

        tapLiveControl("library.close")
        waitForLiveUI { self.app.buttons["chat.model"].isHittable && !self.app.keyboards.firstMatch.exists }
        tapLiveControl("chat.menu")
        waitForLiveUI { search.isHittable && self.app.buttons["library.close"].isHittable }
        XCTAssertFalse(app.keyboards.firstMatch.exists, "A retained hidden menu must relinquish search focus")
        search.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        waitForLiveUI { search.frame.maxY <= self.app.keyboards.firstMatch.frame.minY }
        tapLiveControl("library.close")
        waitForLiveUI { self.app.buttons["chat.model"].isHittable && !self.app.keyboards.firstMatch.exists }
        XCTAssertFalse(app.buttons["chat.stop"].exists, "Options and searching must never dispatch a turn")
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Actual local root after options and search navigation"
        screenshot.lifetime = .keepAlways; add(screenshot)
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
        app.mapleMenuButton.tap(); app.buttons["library.close"].tap()
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, "Explicit local offline draft stays unsent.")
    }

    func testRootMenuChatNavigationAndBackgroundStopCapture() throws {
        try launch(); try replaceDraft("Menu capture owner. ")
        startCapture(); app.mapleMenuButton.tap()
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        app.buttons["library.close"].tap(); waitForStops(1); completeTranscript()
        try replaceDraft("Background capture owner. ")
        startCapture(); XCUIDevice.shared.press(.home); app.activate()
        waitForStops(2); completeTranscript()
        let heldNewChatDraft = app.textViews["chat.composer"].value as? String
        app.mapleMenuButton.tap()
        let recent = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "library.chat.")).firstMatch
        XCTAssertTrue(recent.waitForExistence(timeout: 20))
        let savedChatID = recent.identifier
        recent.tap(); try replaceDraft("Navigation capture owner. ")
        startCapture(); app.buttons["chat.new"].tap()
        waitForStops(3)
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, heldNewChatDraft)
        app.buttons["lifecycle.transcript"].tap()
        XCTAssertEqual(app.textViews["chat.composer"].value as? String, heldNewChatDraft)
        app.mapleMenuButton.tap()
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
