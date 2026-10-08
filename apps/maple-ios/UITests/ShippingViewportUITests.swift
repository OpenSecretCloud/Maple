import XCTest

/// Synthetic history and inference inputs; the view and all runtime/layout paths
/// are the production ChatDetail. This does not claim a local backend round trip.
@MainActor
final class ShippingViewportUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launchFixture(windowProof: Bool = false) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture"]
        if windowProof { app.launchArguments.append("--maple-shipping-window-proof") }
        app.launch()
        waitSummary("Ready 1; older 0; messages 8")
        XCTAssertTrue(app.textViews["chat.composer"].exists)
        XCTAssertTrue(app.buttons["chat.model"].exists)
        XCTAssertTrue(app.buttons["message.copy.viewport-user-20"].exists)
        XCTAssertTrue(app.buttons["message.speak.viewport-assistant-20"].exists)
    }

    func testShippingComposerWaitsForLoadedDraftAndRetainsNativeEdits() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture", "--maple-shipping-draft-load-proof"]
        app.launch()
        let state = app.staticTexts["shipping-draft.state"]
        XCTAssertTrue(state.waitForExistence(timeout: 15))
        let held = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "Load held"), object: state)
        XCTAssertEqual(XCTWaiter.wait(for: [held], timeout: 15), .completed)
        let loading = app.descendants(matching: .any)["chat.loading"]
        XCTAssertTrue(loading.exists)
        XCTAssertFalse(app.textViews["chat.composer"].exists)
        XCTAssertFalse(app.keyboards.firstMatch.exists)

        let release = app.buttons["shipping-draft.release"]
        XCTAssertTrue(release.exists && release.isEnabled && release.isHittable)
        release.tap()
        waitSummary("Ready 1; older 0; messages 8")
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertFalse(loading.exists)
        XCTAssertEqual(composer.value as? String, "Stored synthetic draft arrives before typing")
        let model = app.staticTexts["shipping-draft.model"]
        XCTAssertEqual(model.label, "Stored synthetic draft arrives before typing")
        let edited = "Synthetic native edit remains in the retained draft"
        try replaceSyntheticText(composer, with: edited, in: app)
        let published = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", edited), object: model)
        XCTAssertEqual(XCTWaiter.wait(for: [published], timeout: 15), .completed)
        XCTAssertEqual(composer.value as? String, edited)

        let navigate = app.buttons["shipping-draft.navigate"]
        XCTAssertTrue(navigate.exists && navigate.isEnabled && navigate.isHittable)
        navigate.tap()
        XCTAssertTrue(app.staticTexts["shipping-draft.away"].waitForExistence(timeout: 15))
        XCTAssertFalse(composer.exists)
        XCTAssertEqual(model.label, edited)
        XCTAssertTrue(navigate.exists && navigate.isEnabled && navigate.isHittable)
        navigate.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, edited)
        XCTAssertEqual(model.label, edited)
        waitSummary("running 0")
        capture("Shipping composer waits for its stored draft and retains native edits")
        app.terminate()
    }

    func testCompactCachedDraftProjectionRetainsNativeAccessibilityAndOwners() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture", "--maple-shipping-compact-projection-proof"]
        app.launch()
        waitSummary("Ready 1; older 0; messages 8")
        let composer = app.textViews["chat.composer"]
        let diagnostic = app.staticTexts["shipping-projection.ownership"]
        XCTAssertTrue(diagnostic.waitForExistence(timeout: 15))
        func ownership() throws -> [String: String] {
            let raw = try XCTUnwrap(diagnostic.value as? String)
            return try JSONDecoder().decode([String: String].self, from: Data(raw.utf8))
        }
        func waitForProjection(_ selected: String, nativeText: String) {
            let ready = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in
                    guard let owner = try? ownership(), owner["selected"] == selected else { return false }
                    return composer.exists && composer.isHittable && composer.value as? String == nativeText
                }, object: composer)
            XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed)
        }
        waitForProjection("A", nativeText: "")
        let edited = "Cached A native draft stays private and unchanged"
        try replaceSyntheticText(composer, with: edited, in: app)
        let published = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in (try? ownership()["aText"]) == edited }, object: diagnostic)
        XCTAssertEqual(XCTWaiter.wait(for: [published], timeout: 10), .completed)
        let before = try ownership()
        for key in ["aInstance", "bInstance", "aStorageOwner", "bStorageOwner", "aDraftOwner", "bDraftOwner"] {
            XCTAssertNotNil(UUID(uuidString: try XCTUnwrap(before[key])))
        }
        XCTAssertNotEqual(before["aInstance"], before["bInstance"])
        XCTAssertNotEqual(before["aStorageOwner"], before["bStorageOwner"])
        XCTAssertNotEqual(before["aDraftOwner"], before["bDraftOwner"])
        XCTAssertEqual(before["aLoading"], "false"); XCTAssertEqual(before["bLoading"], "false")
        XCTAssertEqual(before["running"], "false")

        let selectB = app.buttons["shipping-projection.select-b"]
        XCTAssertTrue(selectB.exists && selectB.isEnabled && selectB.isHittable)
        selectB.tap()
        waitForProjection("B", nativeText: "Cached B draft stays separate")
        let onB = try ownership()
        XCTAssertEqual(onB["aText"], edited)
        for key in ["aInstance", "bInstance", "aStorageOwner", "bStorageOwner", "aDraftOwner", "bDraftOwner"] {
            XCTAssertEqual(onB[key], before[key])
        }
        let selectA = app.buttons["shipping-projection.select-a"]
        XCTAssertTrue(selectA.exists && selectA.isEnabled && selectA.isHittable)
        selectA.tap()
        // Returning to cached A performs no model edit or load. Require the
        // production hosting hierarchy to expose the same complete native value.
        waitForProjection("A", nativeText: edited)
        let returned = try ownership()
        XCTAssertEqual(returned, before)
        XCTAssertTrue(composer.exists && composer.isHittable)
        XCTAssertEqual(composer.value as? String, edited)
        capture("Production compact host returns to the same cached native draft and owners")
        app.terminate()
    }

    func testShippingPaginationPreservesReadingAnchor() {
        launchFixture()
        let anchor = readingAnchor(20)
        waitForReadingAnchor(anchor, index: 20)
        // Invoke the real in-transcript pagination action, then hold its service
        // result while recording the exact visible text position.
        let load = app.buttons["Load earlier messages"]
        XCTAssertTrue(load.isHittable)
        load.tap()
        waitSummary("older 1; messages 8")
        waitForReadingAnchor(anchor, index: 20)
        let before = anchor.frame.midY
        app.buttons["shipping-viewport.release-older"].tap()
        waitSummary("older 0; messages 28")
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertEqual(anchor.frame.midY, before, accuracy: 3)
        XCTAssertFalse(load.exists)
        XCTAssertTrue(app.textViews["chat.composer"].isHittable)
        capture("Shipping history prepend retains reading anchor and composer inset")
    }

    func testShippingPaginationCompletesBehindRetainedCompactMenu() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture", "--maple-shipping-pagination-menu-proof"]
        app.launch()
        defer { app.terminate() }
        waitSummary("Ready 1; older 0; messages 8")
        let composer = app.textViews["chat.composer"]
        let anchor = readingAnchor(20)
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertTrue(composer.exists && composer.isHittable)
        let owners = try windowOwnership()
        for key in ["instance", "storageOwner", "draftOwner"] {
            XCTAssertNotNil(UUID(uuidString: try XCTUnwrap(owners[key])))
        }
        let load = app.buttons["Load earlier messages"]
        XCTAssertTrue(load.exists && load.isEnabled && load.isHittable)
        load.tap()
        waitSummary("older 1; messages 8")
        waitForReadingAnchor(anchor, index: 20)
        let before = anchor.frame.midY
        capture("Retained Chat anchor before pagination completes behind Menu")

        let open = app.buttons["shipping-pagination.open"]
        XCTAssertTrue(open.exists && open.isEnabled && open.isHittable)
        open.tap()
        let menu = app.otherElements["shipping-pagination.library"]
        let close = app.buttons["shipping-pagination.close"]
        let hidden = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                menu.exists && close.exists && close.isHittable && !composer.exists && !anchor.exists
            }, object: menu)
        XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 15), .completed)
        let viewport = app.otherElements["shipping-pagination.viewport"]
        XCTAssertTrue(viewport.exists && viewport.frame.width.isFinite && viewport.frame.width > 0)
        XCTAssertEqual(viewport.frame.width, app.windows.firstMatch.frame.width, accuracy: 1)
        XCTAssertEqual(try windowOwnership(), owners)
        let release = app.buttons["shipping-pagination.release-older"]
        XCTAssertTrue(release.exists && release.isEnabled && release.isHittable)
        release.tap()
        waitSummary("older 0; messages 28")
        XCTAssertFalse(composer.exists)
        XCTAssertFalse(anchor.exists)
        XCTAssertTrue(menu.exists && close.isHittable)
        XCTAssertEqual(try windowOwnership(), owners)
        capture("Pagination completes while the same Chat stays behind full-screen Menu")

        close.tap()
        let returned = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                composer.exists && composer.isHittable && !menu.exists
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 15), .completed)
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertEqual(anchor.frame.midY, before, accuracy: 3)
        XCTAssertEqual(try windowOwnership(), owners)
        XCTAssertFalse(load.exists)
        XCTAssertTrue(composer.isHittable)
        capture("Full-screen Menu returns the same Chat owner and pagination anchor")
    }

    func testShippingStreamPreservesEarlierReadingAnchor() {
        launchFixture()
        app.buttons["shipping-viewport.start"].tap()
        waitSummary("running 1")
        let emit = app.buttons["shipping-viewport.emit"]
        let streamReady = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: emit)
        XCTAssertEqual(XCTWaiter.wait(for: [streamReady], timeout: 10), .completed)
        XCTAssertTrue(app.otherElements["chat.progress"].exists || app.staticTexts["chat.progress"].exists)
        let anchor = readingAnchor(20)
        let transcript = app.scrollViews["chat.transcript"]
        XCTAssertTrue(transcript.exists)
        for _ in 0..<12 where !(anchor.exists && anchor.isHittable) { transcript.swipeDown(velocity: .slow) }
        waitForReadingAnchor(anchor, index: 20)
        let before = anchor.frame.midY
        emit.tap()
        waitSummary("chunks 40; running 1")
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertEqual(anchor.frame.midY, before, accuracy: 3)
        app.buttons["shipping-viewport.finish"].tap()
        waitSummary("chunks 40; running 0")
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertEqual(anchor.frame.midY, before, accuracy: 3)
        XCTAssertTrue(app.textViews["chat.composer"].isHittable)
        capture("Shipping stream and completion retain earlier reading anchor")
    }

    /// Uses the system handle observed in the full-screen iPad simulator,
    /// not a synthetic SwiftUI frame or a rotation. The dedicated iPad starts
    /// portrait at 834 x 1210 points; keep the window in that state for other tests.
    func testActualIPadWindowResizeRetainsShippingComposerAndPausedQueueOwners() throws {
        launchFixture(windowProof: true)
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        let original = window.frame
        try XCTSkipUnless(
            abs(original.width - 834) < 2 && abs(original.height - 1210) < 2,
            "Requires the observed 834 x 1210 portrait iPad window and its visible system resize handle")
        let composer = app.textViews["chat.composer"]
        let draft = "Window draft B edited before actual system resize"
        try replaceSyntheticText(composer, with: draft, in: app)
        if app.keyboards.firstMatch.exists {
            // The owned iPad exposes this public system keyboard button. A
            // transcript swipe did not dismiss it in the actual resize setup.
            let hideKeyboard = app.buttons["Hide keyboard"].firstMatch
            XCTAssertTrue(hideKeyboard.waitForExistence(timeout: 5))
            XCTAssertTrue(hideKeyboard.isHittable)
            hideKeyboard.tap()
            // On the actual iPad, XCTest retained a keyboard container after the
            // recorded frame showed its keys/accessory dismissed. Require that
            // no interactive keyboard button obstructs this window instead.
            let hidden = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in self.keyboardDoesNotObstruct(window: original) }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 10), .completed)
        }
        XCTAssertTrue(keyboardDoesNotObstruct(window: original))
        let originalOwner = try windowOwnership()
        XCTAssertEqual(originalOwner["queueCount"], "1")
        XCTAssertEqual(originalOwner["queuePaused"], "true")
        XCTAssertEqual(originalOwner["queuedText"], "Window queued A remains paused")
        XCTAssertEqual(originalOwner["running"], "false")
        for key in ["instance", "storageOwner", "draftOwner", "queueOwner", "queueDraftOwner", "queueMessageID"] {
            XCTAssertNotNil(UUID(uuidString: try XCTUnwrap(originalOwner[key])))
        }
        try assertWindowControls(composer: composer, in: original)
        captureWindow("iPad full-screen before system resize", frame: original)

        // Always attempt to restore this exact window if a later assertion fails.
        // Coordinates are relative to the current observed window, never another app.
        var restored = false
        defer {
            if !restored && window.exists {
                dragResizeHandle(from: window.frame, to: CGPoint(x: original.maxX - 10, y: original.maxY - 15))
                _ = waitForWindow(window, matching: original, timeout: 15)
            }
        }
        dragResizeHandle(
            from: original,
            to: CGPoint(x: original.minX + original.width * 0.72, y: original.minY + original.height * 0.76))
        let changed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let frame = window.frame
                return frame.width < original.width - 80 && frame.height < original.height - 80
            }, object: window)
        XCTAssertEqual(
            XCTWaiter.wait(for: [changed], timeout: 15), .completed,
            "Only an observed OS window-frame reduction establishes a real resize")
        let compact = window.frame
        XCTAssertGreaterThan(compact.width, 300)
        XCTAssertGreaterThan(compact.height, 500)
        XCTAssertEqual(try windowOwnership(), originalOwner)
        XCTAssertEqual(composer.value as? String, draft)
        try assertWindowControls(composer: composer, in: compact)
        captureWindow("iPad actual smaller window retains shipping controls and owners", frame: compact)

        // The shipping QueueReview must still show the retained paused entry.
        app.buttons["queue.open"].tap()
        XCTAssertTrue(app.buttons["queue.resume"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["Window queued A remains paused"].exists)
        XCTAssertFalse(app.buttons["queue.pause"].exists)
        app.buttons["Done"].tap()
        XCTAssertEqual(composer.value as? String, draft)
        XCTAssertEqual(try windowOwnership(), originalOwner)

        dragResizeHandle(from: window.frame, to: CGPoint(x: original.maxX - 10, y: original.maxY - 15))
        XCTAssertTrue(waitForWindow(window, matching: original, timeout: 15), "Restore the original iPad window")
        restored = true
        XCTAssertEqual(try windowOwnership(), originalOwner)
        XCTAssertEqual(composer.value as? String, draft)
        try assertWindowControls(composer: composer, in: window.frame)
        captureWindow("iPad restored full-screen retains the same draft and queue", frame: window.frame)
    }

    private func keyboardDoesNotObstruct(window: CGRect) -> Bool {
        let buttons = app.keyboards.buttons.allElementsBoundByIndex
        guard buttons.count <= 160 else { return false }
        let visibleKey = buttons.contains { button in
            button.exists && button.isHittable && !button.frame.intersection(window).isEmpty
        }
        let returnKey = app.keyboards.buttons.matching(NSPredicate(format: "label ==[c] %@", "return")).firstMatch
        let hideKeyboard = app.buttons["Hide keyboard"].firstMatch
        return !visibleKey && !(returnKey.exists && returnKey.isHittable)
            && !(hideKeyboard.exists && hideKeyboard.isHittable)
    }

    private func windowOwnership() throws -> [String: String] {
        let diagnostic = app.staticTexts["shipping-window.ownership"]
        XCTAssertTrue(diagnostic.waitForExistence(timeout: 10))
        let raw = try XCTUnwrap(diagnostic.value as? String)
        return try JSONDecoder().decode([String: String].self, from: Data(raw.utf8))
    }

    private func assertWindowControls(composer: XCUIElement, in frame: CGRect) throws {
        for control in [composer, app.buttons["chat.model"], app.buttons["chat.send"], app.buttons["queue.open"]] {
            XCTAssertTrue(control.exists && control.isHittable)
            let target = control.frame
            XCTAssertGreaterThanOrEqual(target.height, 44)
            XCTAssertGreaterThanOrEqual(target.width, 44)
            XCTAssertTrue(frame.insetBy(dx: -1, dy: -1).contains(target))
        }
        XCTAssertFalse(app.buttons["chat.stop"].exists)
    }

    private func dragResizeHandle(from frame: CGRect, to point: CGPoint) {
        let origin = app.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(
            CGVector(dx: frame.maxX - app.frame.minX - 10, dy: frame.maxY - app.frame.minY - 15))
        let end = origin.withOffset(CGVector(dx: point.x - app.frame.minX, dy: point.y - app.frame.minY))
        start.press(forDuration: 0.2, thenDragTo: end)
    }

    private func waitForWindow(_ window: XCUIElement, matching expected: CGRect, timeout: TimeInterval) -> Bool {
        let predicate = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let actual = window.frame
                return abs(actual.minX - expected.minX) < 2 && abs(actual.minY - expected.minY) < 2
                    && abs(actual.width - expected.width) < 2 && abs(actual.height - expected.height) < 2
            }, object: window)
        return XCTWaiter.wait(for: [predicate], timeout: timeout) == .completed
    }

    private func captureWindow(_ name: String, frame: CGRect) {
        // A resized iPad window can have a nonzero screen origin. Capture the
        // system surface directly so XCTest does not apply an incorrect app crop.
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        screenshot.name = name; screenshot.lifetime = .keepAlways; add(screenshot)
        let metadata = XCTAttachment(
            string:
                "Window frame points: x=\(frame.minX), y=\(frame.minY), width=\(frame.width), height=\(frame.height)"
        )
        metadata.name = name + " — measured frame"; metadata.lifetime = .keepAlways; add(metadata)
    }

    private func readingAnchor(_ index: Int) -> XCUIElement {
        // Prepared text is selected through its native UITextInput overlay. The
        // rendered StaticText beneath it can have a valid frame but no hit point.
        app.otherElements["message.viewport-user-\(index)"]
            .textViews.matching(NSPredicate(format: "value == %@", "Viewport reading anchor \(index)")).firstMatch
    }

    private func waitForReadingAnchor(_ anchor: XCUIElement, index: Int) {
        let expected = "Viewport reading anchor \(index)"
        let owner = app.otherElements["message.viewport-user-\(index)"]
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard owner.exists, anchor.exists, anchor.value as? String == expected else { return false }
                let frame = anchor.frame
                return frame.minX.isFinite && frame.minY.isFinite && frame.width.isFinite && frame.height.isFinite
                    && frame.width > 0 && frame.height > 0 && !frame.intersection(self.app.frame).isEmpty
                    && anchor.isHittable
            }, object: anchor)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 15), .completed)
        XCTAssertEqual(anchor.value as? String, expected)
    }
    private func waitSummary(_ fragment: String) {
        let summary = app.staticTexts["shipping-viewport.summary"]
        XCTAssertTrue(summary.waitForExistence(timeout: 15))
        let expected = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label CONTAINS %@ AND NOT label CONTAINS %@", fragment, "failed 1"),
            object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [expected], timeout: 15), .completed)
    }
    private func capture(_ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
    }
}
