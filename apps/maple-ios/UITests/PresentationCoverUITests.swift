import XCTest

@MainActor final class PresentationCoverUITests: XCTestCase {
    func testImageCoverBlocksCompletionEligibilityUntilDismissal() {
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-presentation-cover-fixture"]
        app.launch()
        let summary = app.staticTexts["cover.summary"]
        XCTAssertTrue(summary.waitForExistence(timeout: 15))
        XCTAssertEqual(summary.label, "Leases 0; completions 0; covered 0; plays 0; haptics 0; pending 0; failed 0")
        let image = app.buttons["cover.open"]
        XCTAssertTrue(image.waitForExistence(timeout: 10))
        image.tap()
        let close = app.buttons["images.close"]
        XCTAssertTrue(close.waitForExistence(timeout: 10))
        let inside = app.staticTexts["cover.inside-summary"]
        XCTAssertTrue(inside.waitForExistence(timeout: 10))
        XCTAssertEqual(inside.label, "Leases 1; completions 0; covered 0; plays 0; haptics 0; pending 0; failed 0")
        // Trigger only after the actual viewer and its control are accessible.
        // A presenter-onDisappear lease release therefore cannot pass this test.
        app.buttons["cover.covered-completion"].tap()
        let covered = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label == %@", "Leases 1; completions 1; covered 1; plays 0; haptics 0; pending 0; failed 0"),
            object: inside)
        XCTAssertEqual(XCTWaiter.wait(for: [covered], timeout: 10), .completed)
        XCTAssertTrue(close.exists)
        close.tap()
        let blocked = "Leases 0; completions 1; covered 1; plays 0; haptics 0; pending 0; failed 0"
        let dismissed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", blocked), object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed)
        // Changing visibility must not replay a completion missed while covered.
        let catchup = expectation(description: "Observe no visibility catch-up")
        XCTAssertEqual(XCTWaiter.wait(for: [catchup], timeout: 1), .timedOut)
        XCTAssertEqual(summary.label, blocked)
        app.buttons["cover.visible-completion"].tap()
        let eligible = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label == %@", "Leases 0; completions 2; covered 1; plays 1; haptics 1; pending 0; failed 0"),
            object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [eligible], timeout: 10), .completed)
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Image cover suppresses completion and visible completion restores eligibility"
        attachment.lifetime = .keepAlways
        add(attachment)
        app.terminate()
    }

    func testImageCoverDismissalRestoresDirectNativeMenu() {
        continueAfterFailure = false
        let app = XCUIApplication()
        app.launchArguments = [
            "--maple-renderer-fixture", "--maple-presentation-cover-fixture",
            "--maple-presentation-cover-navigation-proof",
        ]
        app.launch()
        defer { app.terminate() }
        let summary = app.staticTexts["cover.summary"]
        XCTAssertTrue(summary.waitForExistence(timeout: 15))
        XCTAssertEqual(summary.label, "Leases 0; completions 0; covered 0; plays 0; haptics 0; pending 0; failed 0")
        let initialMenu = app.mapleMenuButton
        XCTAssertTrue(initialMenu.waitForExistence(timeout: 10))
        XCTAssertTrue(initialMenu.isHittable)

        app.buttons["cover.open"].tap()
        let closeImage = app.buttons["images.close"]
        XCTAssertTrue(closeImage.waitForExistence(timeout: 10))
        let inside = app.staticTexts["cover.inside-summary"]
        XCTAssertTrue(inside.waitForExistence(timeout: 10))
        XCTAssertEqual(inside.label, "Leases 1; completions 0; covered 0; plays 0; haptics 0; pending 0; failed 0")
        app.buttons["cover.covered-completion"].tap()
        let covered = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label == %@", "Leases 1; completions 1; covered 1; plays 0; haptics 0; pending 0; failed 0"),
            object: inside)
        XCTAssertEqual(XCTWaiter.wait(for: [covered], timeout: 10), .completed)
        closeImage.tap()
        let blocked = "Leases 0; completions 1; covered 1; plays 0; haptics 0; pending 0; failed 0"
        let dismissed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", blocked), object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed)

        // Tap the real native bar immediately after dismissal. No edge gesture,
        // response, remount or fixture control can repair it before this check.
        let menu = app.mapleMenuButton
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(menu.isHittable)
        menu.tap()
        let library = app.staticTexts["cover.library"]
        let closeMenu = app.buttons["cover.library-close"]
        XCTAssertTrue(library.waitForExistence(timeout: 10))
        let menuReady = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == true AND enabled == true AND hittable == true"),
            object: closeMenu)
        XCTAssertEqual(XCTWaiter.wait(for: [menuReady], timeout: 10), .completed)
        XCTAssertTrue(library.isHittable)
        closeMenu.tap()
        let returned = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: closeMenu)
        XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 10), .completed)
        XCTAssertTrue(summary.waitForExistence(timeout: 10))
        XCTAssertTrue(summary.isHittable)
        XCTAssertEqual(summary.label, blocked)

        let catchup = expectation(description: "Observe no completion catch-up after native navigation")
        XCTAssertEqual(XCTWaiter.wait(for: [catchup], timeout: 1), .timedOut)
        XCTAssertEqual(summary.label, blocked)
        app.buttons["cover.visible-completion"].tap()
        let eligible = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label == %@", "Leases 0; completions 2; covered 1; plays 1; haptics 1; pending 0; failed 0"),
            object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [eligible], timeout: 10), .completed)
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Image cover dismissal restores direct native Menu and Close"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
