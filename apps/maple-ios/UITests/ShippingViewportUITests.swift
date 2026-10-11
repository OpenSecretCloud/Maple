import XCTest

/// Synthetic history and inference inputs; the view and all runtime/layout paths
/// are the production ChatDetail. This does not claim a local backend round trip.
@MainActor
final class ShippingViewportUITests: XCTestCase {
    @MainActor func testLaunchCentersBrandAndHandsLockToNativeToolbar() {
        let app = XCUIApplication()
        app.launchArguments = ["--maple-launch-transition-fixture"]
        app.launch()
        let logo = app.images["startup.logo"], lock = app.images["startup.lock"]
        XCTAssertTrue(logo.waitForExistence(timeout: 15))
        XCTAssertTrue(lock.exists)
        let window = app.windows.firstMatch.frame
        XCTAssertEqual(logo.frame.midX, window.midX, accuracy: 2)
        XCTAssertEqual(logo.frame.midY, window.midY, accuracy: 2)
        XCTAssertEqual(lock.frame.midX, logo.frame.midX, accuracy: 2)
        XCTAssertGreaterThan(lock.frame.midY, logo.frame.maxY)
        app.buttons["startup.fixture.continue"].tap()
        XCTAssertTrue(app.staticTexts["startup.fixture.complete"].waitForExistence(timeout: 5))
        let toolbarLock = app.images["chat.privacy"]
        XCTAssertTrue(toolbarLock.exists)
        XCTAssertLessThan(toolbarLock.frame.midY, window.minY + window.height * 0.2)
        XCTAssertFalse(logo.exists)
    }
    private var app: XCUIApplication!

    private func launchFixture(windowProof: Bool = false, pickerProof: Bool = false, followScrollProof: Bool = false) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture"]
        if windowProof { app.launchArguments.append("--maple-shipping-window-proof") }
        if pickerProof { app.launchArguments.append("--maple-shipping-picker-proof") }
        if followScrollProof { app.launchArguments.append("--maple-shipping-follow-scroll-proof") }
        app.launch()
        waitSummary("Ready 1; older 0; messages 8")
        XCTAssertTrue(app.textViews["chat.composer"].exists)
        XCTAssertTrue(app.buttons["chat.model"].exists, "Chat options remain available in the compact composer")
        XCTAssertLessThanOrEqual(app.textViews["chat.composer"].frame.height, 60)
        XCTAssertTrue(app.buttons["message.copy.viewport-user-20"].exists)
        XCTAssertTrue(app.buttons["message.speak.viewport-assistant-20"].exists)
    }

    func testUserMessagesStayOnRightAndWebToolRowsRemainVisibleWithDetails() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-shipping-viewport-fixture", "--maple-shipping-transcript-rows-proof"]
        app.launch()
        defer { app.terminate() }
        waitSummary("Ready 1; older 0; messages 8")

        let bubble = app.otherElements["message.bubble.viewport-user-20"]
        let assistant = app.otherElements["assistant.message.viewport-assistant-20"]
        let transcript = app.scrollViews["chat.transcript"]
        XCTAssertTrue(bubble.waitForExistence(timeout: 10))
        XCTAssertTrue(assistant.exists)
        XCTAssertTrue(transcript.exists)
        // AX containers follow the rendered glyphs/actions, so an assistant's
        // right edge is not the available transcript edge. Measure the centered
        // column (850-point cap, 20-point inset) in the actual scroll viewport;
        // this also checks the same trailing alignment in wide iPad windows.
        let columnWidth = min(transcript.frame.width, 850)
        let contentLeading = transcript.frame.midX - columnWidth / 2 + 20
        let contentTrailing = transcript.frame.midX + columnWidth / 2 - 20
        XCTAssertEqual(assistant.frame.minX, contentLeading, accuracy: 2)
        XCTAssertGreaterThan(bubble.frame.minX, contentLeading + 24)
        XCTAssertEqual(bubble.frame.maxX, contentTrailing, accuracy: 2)
        XCTAssertTrue(app.buttons["message.copy.viewport-user-20"].exists)
        XCTAssertGreaterThan(
            app.buttons["message.copy.viewport-user-20"].frame.midX, bubble.frame.midX,
            "The user's copy action follows the trailing message bubble")

        let search = app.buttons["tool.activity.viewport-search-20"]
        let nativeSearch = app.buttons["tool.activity.viewport-native-search-20"]
        let openedPages = app.buttons["tool.activity.viewport-open-20"]
        for row in [search, nativeSearch, openedPages] {
            XCTAssertTrue(row.exists && row.isHittable, "Tools have visible interactive rows without ordinary text")
            XCTAssertGreaterThanOrEqual(row.frame.height, 44)
            XCTAssertEqual(row.value as? String, "Collapsed")
        }
        XCTAssertTrue(search.label.contains("Search results"))
        XCTAssertTrue(nativeSearch.label.contains("Search results"))
        XCTAssertTrue(openedPages.label.contains("Opened pages"))
        XCTAssertFalse(app.buttons["tool.activity.viewport-search-result-20"].exists)
        XCTAssertFalse(app.buttons["tool.activity.viewport-open-result-20"].exists)
        capture("Trailing user bubble and all retained web tool rows are visible")

        search.tap()
        let searchDetails = app.descendants(matching: .any)["tool.details.viewport-search-20"].firstMatch
        XCTAssertTrue(searchDetails.waitForExistence(timeout: 10))
        // Textual's paragraphs render as SwiftUI.Text (StaticText.label),
        // with a separate native selection overlay rather than a UITextView.
        let resultText = searchDetails.staticTexts.matching(
            NSPredicate(format: "label CONTAINS %@", "Synthetic searchable garden result")
        ).firstMatch
        XCTAssertTrue(resultText.waitForExistence(timeout: 10))
        XCTAssertTrue(resultText.isHittable)
        XCTAssertGreaterThan(resultText.frame.height, 0)
        XCTAssertFalse(
            searchDetails.staticTexts.matching(
                NSPredicate(format: "label CONTAINS %@", "Synthetic opened page contents")
            ).firstMatch.exists,
            "Search details contain only their matching tool result")
        XCTAssertEqual(search.value as? String, "Expanded")
        capture("Search opens the exact paired result details")
        search.tap()
        XCTAssertFalse(searchDetails.exists)

        nativeSearch.tap()
        XCTAssertTrue(app.staticTexts["No tool details available."].waitForExistence(timeout: 10))
        nativeSearch.tap()
        XCTAssertFalse(app.staticTexts["No tool details available."].exists)

        openedPages.tap()
        let pageDetails = app.descendants(matching: .any)["tool.details.viewport-open-20"].firstMatch
        XCTAssertTrue(pageDetails.waitForExistence(timeout: 10))
        let pageText = pageDetails.staticTexts.matching(
            NSPredicate(format: "label CONTAINS %@", "Synthetic opened page contents")
        ).firstMatch
        XCTAssertTrue(pageText.waitForExistence(timeout: 10))
        XCTAssertTrue(pageText.isHittable)
        XCTAssertGreaterThan(pageText.frame.height, 0)
        XCTAssertFalse(
            pageDetails.staticTexts.matching(
                NSPredicate(format: "label CONTAINS %@", "Synthetic searchable garden result")
            ).firstMatch.exists,
            "Opened pages contain only their matching tool result")
        capture("Opened pages keeps its exact paired result accessible")
        openedPages.tap()
        XCTAssertEqual(openedPages.value as? String, "Collapsed")
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
    }

    func testExistingChatComposerExpandsWithoutReplacingDraftAndPickerKeepsWidth() throws {
        launchFixture(pickerProof: true)
        defer { app.terminate() }
        let composer = app.textViews["chat.composer"]
        XCTAssertLessThanOrEqual(composer.frame.height, 60)
        XCTAssertTrue(app.buttons["chat.send"].exists)
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        capture("Existing chat starts with a compact composer")

        let draft = "Synthetic draft survives composer expansion and picker resizing"
        try replaceSyntheticText(composer, with: draft, in: app)
        let model = app.buttons["chat.model"]
        XCTAssertTrue(model.waitForExistence(timeout: 10))
        XCTAssertTrue(model.isHittable)
        XCTAssertEqual(composer.value as? String, draft)
        capture("Focused composer expands around the same native draft")
        model.tap()
        let picker = app.collectionViews["settings.models"]
        XCTAssertTrue(picker.waitForExistence(timeout: 10))
        XCTAssertTrue(app.switches["model.web-search"].exists)
        XCTAssertFalse(app.buttons["Done"].exists)
        XCTAssertFalse(app.navigationBars["Default model"].exists)
        let initialFrame = picker.frame
        XCTAssertGreaterThan(initialFrame.width, 250)
        XCTAssertGreaterThan(initialFrame.minY - app.windows.firstMatch.frame.minY, 44)
        XCTAssertTrue(app.buttons["chat-options.project"].exists && app.buttons["chat-options.project"].isHittable)
        for id in ["auto:quick", "auto:powerful"] {
            let option = app.buttons["settings.model.\(id)"]
            XCTAssertTrue(option.exists && option.isHittable, "The initial sheet exposes project and primary choices")
        }
        let lastAdvanced = app.buttons["settings.model.synthetic-advanced-4"]
        XCTAssertFalse(
            lastAdvanced.exists && lastAdvanced.isHittable, "Additional models remain below the initial view")
        capture("Partial Chat options sheet exposes Project, Web Search, Quick and Powerful")
        picker.swipeUp(velocity: .slow)
        let expanded = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                picker.exists && picker.frame.height > initialFrame.height + 60
                    && abs(picker.frame.width - initialFrame.width) <= 1
            }, object: picker)
        XCTAssertEqual(XCTWaiter.wait(for: [expanded], timeout: 10), .completed)
        XCTAssertEqual(picker.frame.width, initialFrame.width, accuracy: 1)
        capture("Model picker expands without changing its presentation width")
        // Dismiss through UIKit's own grabber. Actual model selection is tested
        // with a signed-in local account; this isolated environment has no auth.
        let grabber = picker.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
            .withOffset(CGVector(dx: 0, dy: 8))
        let away = picker.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.75))
        grabber.press(forDuration: 0.15, thenDragTo: away)
        waitForModelPickerDismissal(picker)
        XCTAssertTrue(composer.exists)
        XCTAssertEqual(composer.value as? String, draft)
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
    }

    func testModelPickerOutsideTapDismissesAndReopensWithoutChangingDraft() throws {
        launchFixture(pickerProof: true)
        defer { app.terminate() }
        let composer = app.textViews["chat.composer"]
        let draft = "Synthetic draft survives outside taps and reopening the model picker"
        try replaceSyntheticText(composer, with: draft, in: app)
        let model = app.buttons["chat.model"]
        for attempt in 1...2 {
            if !model.exists || !model.isHittable { composer.tap() }
            XCTAssertTrue(model.waitForExistence(timeout: 10))
            XCTAssertTrue(model.isHittable)
            model.tap()
            let picker = app.collectionViews["settings.models"]
            XCTAssertTrue(picker.waitForExistence(timeout: 10))
            let keyboardDismissed = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in !self.app.keyboards.firstMatch.exists }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [keyboardDismissed], timeout: 10), .completed)
            // A real control inside the sheet must still receive its tap.
            let webSearch = app.switches["model.web-search"]
            XCTAssertTrue(webSearch.exists && webSearch.isHittable)
            let previousValue = try XCTUnwrap(webSearch.value as? String)
            tapWebSearchSwitch(webSearch)
            let changed = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in webSearch.exists && webSearch.value as? String != previousValue },
                object: webSearch)
            XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 10), .completed)
            XCTAssertTrue(picker.exists)
            let window = app.windows.firstMatch.frame
            let outsideHeight = picker.frame.minY - window.minY
            XCTAssertGreaterThan(outsideHeight, 44)
            let outside = app.coordinate(withNormalizedOffset: .zero).withOffset(
                CGVector(
                    dx: window.midX - app.frame.minX,
                    dy: window.minY + outsideHeight / 2 - app.frame.minY))
            outside.tap()
            waitForModelPickerDismissal(picker)
            XCTAssertTrue(composer.exists)
            XCTAssertEqual(composer.value as? String, draft)
            capture("Outside tap dismisses model picker and preserves draft — attempt \(attempt)")
        }
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
    }

    func testExistingComposerTapAndEdgeSwipeRecoverAfterModelPickerDismissal() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-shipping-viewport-fixture", "--maple-shipping-pagination-menu-proof",
            "--maple-shipping-picker-proof",
        ]
        app.launch()
        defer { app.terminate() }
        waitSummary("Ready 1; older 0; messages 8")
        let composer = app.textViews["chat.composer"]
        let model = app.buttons["chat.model"]
        var draft = "Synthetic first character and draft survive picker and edge navigation"
        XCTAssertTrue(composer.exists)
        XCTAssertTrue(model.exists && model.isHittable)
        XCTAssertLessThanOrEqual(composer.frame.height, 60)
        try replaceSyntheticText(
            composer, with: draft, in: app, centeredNativeFocus: true,
            beforeTyping: { self.waitForExpandedSoftwareKeyboard(composer: composer, model: model) })

        for attempt in 1...2 {
            XCTAssertEqual(composer.value as? String, draft)
            model.tap()
            let picker = app.collectionViews["settings.models"]
            XCTAssertTrue(picker.waitForExistence(timeout: 10))
            let project = app.buttons["chat-options.project"]
            XCTAssertTrue(project.exists && project.isEnabled && project.isHittable)
            project.tap()
            let noProject = app.buttons["project-picker.project.none"]
            XCTAssertTrue(noProject.waitForExistence(timeout: 10))
            let back = app.navigationBars.buttons.firstMatch
            XCTAssertTrue(back.exists && back.isEnabled && back.isHittable)
            back.tap()
            XCTAssertTrue(project.waitForExistence(timeout: 10))
            project.tap()
            let selection = app.buttons["project-picker.project." + (attempt == 1 ? "fixture-project" : "none")]
            XCTAssertTrue(selection.waitForExistence(timeout: 10))
            XCTAssertTrue(selection.isEnabled && selection.isHittable); selection.tap()
            XCTAssertTrue(picker.waitForExistence(timeout: 10), "Project selection returns to the same options sheet")
            XCTAssertTrue(app.switches["model.web-search"].exists)
            let window = app.windows.firstMatch.frame
            let outsideHeight = picker.frame.minY - window.minY
            XCTAssertGreaterThan(outsideHeight, 44)
            app.coordinate(withNormalizedOffset: .zero).withOffset(
                CGVector(
                    dx: window.midX - app.frame.minX,
                    dy: window.minY + outsideHeight / 2 - app.frame.minY)
            ).tap()
            waitForModelPickerDismissal(picker)
            XCTAssertEqual(composer.value as? String, draft)

            let transcript = app.scrollViews["chat.transcript"]
            XCTAssertTrue(transcript.exists)
            let start = app.coordinate(withNormalizedOffset: .zero).withOffset(
                CGVector(dx: app.frame.minX + 4, dy: transcript.frame.midY))
            start.press(
                forDuration: 0.1,
                thenDragTo: start.withOffset(
                    CGVector(dx: app.frame.width * 0.65, dy: 0)))
            let close = app.buttons["shipping-pagination.close"]
            XCTAssertTrue(
                close.waitForExistence(timeout: 15),
                "A fresh physical-left contact must work after picker dismissal")
            XCTAssertTrue(app.otherElements["shipping-pagination.library"].exists)
            close.tap()
            XCTAssertTrue(composer.waitForExistence(timeout: 15))
            XCTAssertEqual(composer.value as? String, draft)
            let compact = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in
                    composer.exists && composer.isHittable && composer.frame.height <= 60
                        && !self.app.keyboards.firstMatch.exists
                }, object: composer)
            XCTAssertEqual(XCTWaiter.wait(for: [compact], timeout: 10), .completed)

            // One ordinary touch must restore the expanded editor and keyboard;
            // no second focus tap or simulated binding change is permitted.
            composer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
            waitForExpandedSoftwareKeyboard(composer: composer, model: model)
            let suffix = " \(attempt)"
            composer.typeText(suffix)
            draft += suffix
            XCTAssertEqual(composer.value as? String, draft)
            capture("Existing composer and edge navigation recover after picker — attempt \(attempt)")
        }
    }

    private func waitForExpandedSoftwareKeyboard(composer: XCUIElement, model: XCUIElement) {
        let expanded = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard composer.exists, composer.isHittable, model.exists, model.isHittable,
                    self.app.keyboards.firstMatch.exists
                else { return false }
                return composer.frame.maxY <= self.app.keyboards.firstMatch.frame.minY
            }, object: composer)
        let outcome = XCTWaiter.wait(for: [expanded], timeout: 10)
        if outcome != .completed {
            capture("Single-tap composer focus failure")
        }
        XCTAssertEqual(
            outcome, .completed,
            "A single native touch must expand the editor above the software keyboard; "
                + "composer=\(composer.exists), model=\(model.exists), "
                + "keyboard=\(app.keyboards.firstMatch.exists)")
    }

    private func waitForModelPickerDismissal(_ picker: XCUIElement) {
        let dismissed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: picker)
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed)
    }

    private func tapWebSearchSwitch(_ row: XCUIElement) {
        XCTAssertTrue(row.isEnabled)
        let frame = row.frame
        XCTAssertGreaterThanOrEqual(frame.width, 160)
        XCTAssertTrue((28...120).contains(frame.height))
        let nativeSwitches = row.switches.allElementsBoundByIndex.filter {
            $0.exists && $0.isHittable && (20...100).contains($0.frame.width)
                && (20...100).contains($0.frame.height) && frame.contains($0.frame)
        }
        XCTAssertLessThanOrEqual(nativeSwitches.count, 1)
        let target: XCUICoordinate
        let point: CGPoint
        if let native = nativeSwitches.first {
            point = CGPoint(x: native.frame.midX, y: native.frame.midY)
            target = native.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        } else {
            // The English/LTR row is a full-width accessibility Switch. Its
            // center misses the native trailing toggle, as in the reference suite.
            point = CGPoint(x: frame.maxX - 48, y: frame.midY)
            target = row.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5))
                .withOffset(CGVector(dx: -48, dy: 0))
        }
        XCTAssertTrue(frame.contains(point))
        XCTAssertTrue(app.windows.firstMatch.frame.contains(point))
        target.tap()
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

    func testCancelledLeadingEdgePreservesRetainedAssistantGeometryAndDraft() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-shipping-viewport-fixture", "--maple-shipping-pagination-menu-proof",
            "--maple-shipping-complex-markdown-proof",
        ]
        app.launch()
        defer { app.terminate() }
        waitSummary("Ready 1; older 0; messages 8")
        let preparedMessage = app.descendants(matching: .any).matching(
            identifier: "assistant.message.viewport-assistant-20"
        ).firstMatch
        // This no-formula fixture completes preparation synchronously with
        // publication. Its code button witnesses completed rich rendering
        // before the first occlusion, rather than just loaded message data.
        XCTAssertTrue(preparedMessage.buttons["Copy code"].firstMatch.waitForExistence(timeout: 15))
        let composer = app.textViews["chat.composer"]
        let draft = "Synthetic draft survives a cancelled native edge preview"
        try replaceSyntheticText(
            composer, with: draft, in: app, centeredNativeFocus: true,
            beforeTyping: {
                let keyboard = self.app.keyboards.firstMatch
                let model = self.app.buttons["chat.model"]
                var lastFrames: [CGRect]?
                var stableSince: TimeInterval?
                let ready = XCTNSPredicateExpectation(
                    predicate: NSPredicate { _, _ in
                        guard model.exists, keyboard.exists, composer.exists, composer.isHittable else {
                            lastFrames = nil; stableSince = nil
                            return false
                        }
                        let editorFrame = composer.frame, keyboardFrame = keyboard.frame
                        let frames = [editorFrame, keyboardFrame]
                        guard
                            frames.allSatisfy({
                                $0.minX.isFinite && $0.minY.isFinite && $0.width.isFinite && $0.height.isFinite
                                    && $0.width > 0 && $0.height > 0
                            }), editorFrame.maxY <= keyboardFrame.minY
                        else {
                            lastFrames = nil; stableSince = nil
                            return false
                        }
                        let now = ProcessInfo.processInfo.systemUptime
                        if frames != lastFrames {
                            lastFrames = frames; stableSince = now
                            return false
                        }
                        return stableSince.map { now - $0 >= 0.25 } ?? false
                    }, object: composer)
                guard XCTWaiter.wait(for: [ready], timeout: 10) == .completed else {
                    XCTFail("Expanded native editor and keyboard did not reach stable typing geometry")
                    throw NativeTextEntryIssue.unavailable
                }
            })
        app.buttons["shipping-pagination.open"].tap()
        let menu = app.otherElements["shipping-pagination.library"]
        let close = app.buttons["shipping-pagination.close"]
        XCTAssertTrue(close.waitForExistence(timeout: 15))
        let hiddenKeyboard = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in !self.app.keyboards.firstMatch.exists }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [hiddenKeyboard], timeout: 10), .completed)
        close.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, draft)
        let owners = try windowOwnership()
        let transcript = app.scrollViews["chat.transcript"]
        let leading = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: app.frame.minX + 4, dy: transcript.frame.midY))
        leading.press(
            forDuration: 0.1,
            thenDragTo: leading.withOffset(CGVector(dx: min(190, app.frame.width * 0.55), dy: 0)))
        XCTAssertTrue(close.waitForExistence(timeout: 15), "A real edge gesture must open the retained menu")
        XCTAssertTrue(menu.exists)
        close.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        let anchor = transcript.descendants(matching: .any).matching(identifier: preparedMessage.identifier).firstMatch
        XCTAssertTrue(anchor.exists)
        XCTAssertTrue(anchor.buttons["Copy code"].firstMatch.exists)
        XCTAssertTrue(anchor.frame.intersects(transcript.frame))
        let original = anchor.frame
        let start = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: app.frame.minX + 4, dy: transcript.frame.midY))
        start.press(forDuration: 0.1, thenDragTo: start.withOffset(CGVector(dx: 44, dy: 0)))
        let returnedAnchor = transcript.descendants(matching: .any).matching(identifier: anchor.identifier).firstMatch
        let settled = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard !menu.exists, !close.exists, composer.exists, composer.isHittable,
                    composer.value as? String == draft, returnedAnchor.exists
                else { return false }
                let returned = returnedAnchor.frame
                return abs(returned.minX - original.minX) <= 3 && abs(returned.minY - original.minY) <= 3
                    && abs(returned.height - original.height) <= 3
            }, object: app)
        let result = XCTWaiter.wait(for: [settled], timeout: 10)
        let geometry = XCTAttachment(
            string:
                "Original assistant frame: \(original); retained: \(anchor.exists ? anchor.frame : .zero); fresh match: \(returnedAnchor.exists ? returnedAnchor.frame : .zero); candidate count: \(transcript.descendants(matching: .any).matching(identifier: anchor.identifier).count)"
        )
        geometry.name = "Cancelled retained assistant geometry"; geometry.lifetime = .keepAlways; add(geometry)
        XCTAssertEqual(result, .completed)
        XCTAssertEqual(try windowOwnership(), owners)
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
        capture("Cancelled edge preserves the retained assistant, draft and native composer")
    }

    func testShippingStreamPreservesEarlierReadingAnchor() {
        launchFixture()
        app.buttons["shipping-viewport.start"].tap()
        waitSummary("running 1")
        let emit = app.buttons["shipping-viewport.emit"]
        let streamReady = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: emit)
        XCTAssertEqual(XCTWaiter.wait(for: [streamReady], timeout: 10), .completed)
        XCTAssertTrue(app.buttons["chat.stop"].exists && app.buttons["chat.stop"].isEnabled)
        XCTAssertFalse(app.buttons["chat.send"].exists, "Stop occupies the send button while a response is active")
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
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

    func testNewStreamingReplyItemsFollowTheBottomAndRespectEarlierReading() throws {
        launchFixture(followScrollProof: true)
        defer { app.terminate() }
        app.buttons["shipping-viewport.start"].tap()
        waitSummary("running 1")
        let emit = app.buttons["shipping-viewport.emit"]
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: emit)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed)
        emit.tap()
        waitSummary("chunks 40; running 1")
        let transcript = app.scrollViews["chat.transcript"]
        let tail = app.buttons["message.copy.viewport-live-assistant"]
        XCTAssertTrue(
            app.staticTexts["shipping-viewport.scroll-metrics"].exists,
            "This fixture must expose native geometry before testing scrolling")
        func distanceFromBottom() -> Double? {
            let element = app.staticTexts["shipping-viewport.scroll-metrics"]
            guard element.exists, let value = element.value as? String,
                let data = value.data(using: .utf8),
                let object = try? JSONSerialization.jsonObject(with: data) as? [String: Double]
            else { return nil }
            return object["distanceFromBottom"]
        }
        for _ in 0..<16 where !(distanceFromBottom().map { $0 < 30 } ?? false) {
            transcript.swipeUp(velocity: .slow)
        }
        let beforeAppend = XCTAttachment(
            string: app.staticTexts["shipping-viewport.scroll-metrics"].value as? String ?? "missing")
        beforeAppend.name = "Actual viewport metrics before append"; beforeAppend.lifetime = .keepAlways;
        add(beforeAppend)
        XCTAssertLessThan(
            try XCTUnwrap(distanceFromBottom()), 30, "The reader must actually be near the native viewport bottom")
        XCTAssertTrue(tail.exists && tail.isHittable && transcript.frame.contains(tail.frame))
        let append = app.buttons["shipping-viewport.append-item"]
        XCTAssertTrue(append.exists && append.isEnabled)
        append.tap()
        let appendedTail = app.buttons["message.copy.viewport-follow-item-0"]
        let followed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                appendedTail.exists && appendedTail.isHittable && transcript.frame.contains(appendedTail.frame)
                    && (distanceFromBottom().map { $0 < 30 } ?? false)
            }, object: appendedTail)
        XCTAssertEqual(XCTWaiter.wait(for: [followed], timeout: 10), .completed)
        capture("New streaming reply item follows the reader at the bottom")

        let anchor = readingAnchor(20)
        for _ in 0..<16 where !(anchor.exists && anchor.isHittable) { transcript.swipeDown(velocity: .slow) }
        waitForReadingAnchor(anchor, index: 20)
        let before = anchor.frame.midY
        XCTAssertGreaterThan(try XCTUnwrap(distanceFromBottom()), 100)
        append.tap()
        let count = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label CONTAINS %@", "messages 12"),
            object: app.staticTexts["shipping-viewport.summary"])
        XCTAssertEqual(XCTWaiter.wait(for: [count], timeout: 10), .completed)
        waitForReadingAnchor(anchor, index: 20)
        XCTAssertEqual(anchor.frame.midY, before, accuracy: 3)
        XCTAssertFalse(app.buttons["message.copy.viewport-follow-item-1"].isHittable)
        capture("Appended streaming reply leaves an earlier reading anchor unchanged")
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
        for control in [composer, app.buttons["chat.send"], app.buttons["queue.open"]] {
            XCTAssertTrue(control.exists && control.isHittable)
            let target = control.frame
            XCTAssertGreaterThanOrEqual(target.height, 44)
            XCTAssertGreaterThanOrEqual(target.width, 44)
            XCTAssertTrue(frame.insetBy(dx: -1, dy: -1).contains(target))
        }
        XCTAssertTrue(app.buttons["chat.model"].exists)
        XCTAssertLessThanOrEqual(
            composer.frame.height, 60, "The keyboard-hidden composer remains compact after resizing")
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
