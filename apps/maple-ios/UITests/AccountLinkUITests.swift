import XCTest

/// Normal local account/root, actual onOpenURL delivery, shipping parser/forms.
/// The finite DEBUG bridge supplies only synthetic routes for unsigned loopback
/// builds. It does not request reset mail, reset a password or install credentials.
@MainActor final class AccountLinkUITests: XCTestCase {
    private var app: XCUIApplication!
    private var composer: XCUIElement { app.textViews["chat.composer"] }

    private func launch() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-account-link-fixture"]; app.launch()
        XCTAssertTrue(composer.waitForExistence(timeout: 30))
    }
    private func draft(_ value: String) throws {
        try replaceSyntheticText(composer, with: value, in: app)
    }
    private func deliver(_ route: String, coldLaunch: Bool = false) throws {
        let url = try XCTUnwrap(URL(string: "cloud.opensecret.maple.native.dev://ui-test-account-link/" + route))
        // Use system URL dispatch for warm delivery; application.open is
        // launch-oriented and is reserved for the intentional cold-launch case.
        if coldLaunch { app.open(url) } else { XCUIDevice.shared.system.open(url) }
    }
    private func delivered(_ count: Int) {
        let marker = app.staticTexts["account-link.delivered"]
        XCTAssertTrue(marker.waitForExistence(timeout: 20))
        let expectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Delivered links \(count)"), object: marker)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 15), .completed)
    }
    private func tap(_ identifier: String) {
        let button = app.buttons[identifier].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        for _ in 0..<5 where !button.isHittable { app.swipeUp() }
        XCTAssertTrue(button.isHittable); button.tap()
    }

    func testUnownedAndMalformedNativeCallbacksCannotReplaceSessionOrDraft() throws {
        try launch(); try draft("Root callback ownership stays local")
        let values = [
            "cloud.opensecret.maple.native.dev://auth?handoff_grant=e30.e30.AA",
            "cloud.opensecret.maple.native.dev://auth?handoff_grant=e30.e30.AA&handoff_grant=e30.e30.AQ",
            "cloud.opensecret.maple.native.dev://wrong-owner?handoff_grant=e30.e30.AA",
        ]
        for (index, value) in values.enumerated() {
            XCUIDevice.shared.system.open(try XCTUnwrap(URL(string: value))); delivered(index + 1)
            XCTAssertTrue(composer.waitForExistence(timeout: 15))
            XCTAssertEqual(composer.value as? String, "Root callback ownership stays local")
            XCTAssertFalse(app.buttons["auth.confirm-account"].exists)
            XCTAssertFalse(app.textFields["auth.reset-email"].exists)
        }
    }

    func testColdResetLinkSurvivesRestoreWithoutAdoptingExternalSecretOrEmail() throws {
        try launch(); try draft("Draft survives cold account link")
        // Normal background handling durably saves the draft before termination.
        XCUIDevice.shared.press(.home); app.terminate()
        try deliver("reset", coldLaunch: true)
        let email = app.textFields["auth.reset-email"]
        XCTAssertTrue(email.waitForExistence(timeout: 30))
        XCTAssertFalse(email.isEnabled)
        XCTAssertNotEqual(email.value as? String, "synthetic-other@example.test")
        XCTAssertTrue(
            app.staticTexts[
                "Start the reset in this app to use your code. A code alone cannot restore the private confirmation secret."
            ].exists)
        XCTAssertFalse(app.buttons["auth.confirm-reset"].exists)
        tap("auth.request-reset")
        XCTAssertTrue(app.staticTexts["Resetting your password deletes private data"].waitForExistence(timeout: 10))
        let warnings = app.popovers.containing(.sheet, identifier: "Resetting your password deletes private data")
        XCTAssertEqual(warnings.count, 1)
        let warning = warnings.element
        XCTAssertTrue(warning.buttons["Continue with reset"].exists)
        let dismissRegions = app.otherElements.matching(
            NSPredicate(format: "identifier == %@ AND label == %@", "PopoverDismissRegion", "dismiss popup"))
        XCTAssertEqual(dismissRegions.count, 1)
        let dismissRegion = dismissRegions.element
        XCTAssertTrue(dismissRegion.exists)
        XCTAssertTrue(dismissRegion.isEnabled)
        XCTAssertTrue(dismissRegion.isHittable)
        dismissRegion.tap()
        let warningDismissed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: warning)
        XCTAssertEqual(XCTWaiter.wait(for: [warningDismissed], timeout: 5), .completed)
        tap("Done")
        XCTAssertTrue(composer.waitForExistence(timeout: 20))
        XCTAssertEqual(composer.value as? String, "Draft survives cold account link")
        delivered(1)
    }

    func testVerificationLinkDismissesMenuBeforePresentingOwnedLocalForm() throws {
        try launch(); try draft("Draft survives verification route")
        tap("chat.menu")
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        try deliver("verify")
        let code = app.textFields["auth.verify-code"]
        XCTAssertTrue(code.waitForExistence(timeout: 30))
        XCTAssertEqual(code.value as? String, "00000000-0000-4000-8000-000000000042")
        XCTAssertFalse(app.buttons["library.close"].exists)
        // A nonexistent synthetic verification code exercises the real local SDK
        // error path without changing the account or sending verification mail.
        XCTAssertTrue(app.staticTexts["auth.ceremony-error"].waitForExistence(timeout: 30))
        tap("Done")
        XCTAssertTrue(composer.waitForExistence(timeout: 20))
        XCTAssertEqual(composer.value as? String, "Draft survives verification route")
        delivered(1)
    }

    func testForeignOriginAndDuplicateResetCodeCannotOpenAnAccountFlow() throws {
        try launch(); try draft("Strict root route ownership")
        for (index, route) in ["foreign-reset", "duplicate-reset"].enumerated() {
            try deliver(route); delivered(index + 1)
            XCTAssertTrue(composer.waitForExistence(timeout: 15))
            XCTAssertEqual(composer.value as? String, "Strict root route ownership")
            XCTAssertFalse(app.textFields["auth.reset-email"].exists)
            XCTAssertFalse(app.textFields["auth.verify-code"].exists)
        }
    }
}
