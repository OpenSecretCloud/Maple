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
}
