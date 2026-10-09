import XCTest

/// Shipping native account/model/settings views over synthetic server responses.
/// Real authenticated billing/catalog requests have a separate local API gate.
@MainActor final class EntitlementUITests: XCTestCase {
    private var app: XCUIApplication!
    private var referenceScenario = "unknown"
    private func referenceScreenshot(_ surface: String) {
        let capture = XCTAttachment(screenshot: app.screenshot())
        capture.name = "Reference — injected entitlement — " + referenceScenario + " — " + surface
        capture.lifetime = .keepAlways; add(capture)
    }
    private var editor: XCUIElement { app.textViews["entitlement.composer"] }
    private func launch(guest: Bool = false) {
        continueAfterFailure = false
        referenceScenario = guest ? "guest-unknown" : "unknown"
        app = XCUIApplication(); app.launchArguments = ["--maple-entitlement-fixture"]
        if guest { app.launchArguments.append("--maple-entitlement-guest") }
        app.launch()
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        waitLabel("entitlement.billing", "unknown")
    }
    private func waitLabel(_ id: String, _ value: String) {
        let match = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", value), object: app.staticTexts[id])
        XCTAssertEqual(XCTWaiter.wait(for: [match], timeout: 10), .completed)
    }
    private func tap(_ id: String) {
        let button = app.buttons[id]
        XCTAssertTrue(button.waitForExistence(timeout: 5))
        if !button.isHittable { app.swipeUp() }
        button.tap()
    }
    private func scenario(_ id: String, billing: String) {
        if !app.buttons["entitlement.scenarios"].isHittable { app.swipeDown() }
        tap("entitlement.scenarios"); tap("entitlement.scenario." + id)
        waitLabel("entitlement.billing", billing)
        referenceScenario = id
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }
    private func notice(_ feature: String, containing text: String) {
        tap("entitlement.media." + feature)
        let message = app.staticTexts["media.access.message"]
        XCTAssertTrue(message.waitForExistence(timeout: 5))
        XCTAssertTrue(message.label.contains(text))
        XCTAssertFalse(app.buttons["Upgrade"].exists)
        XCTAssertFalse(app.buttons["Buy"].exists)
        referenceScreenshot(feature + " access explanation")
        tap("media.access.close")
    }

    func testUnknownLoadingFailureRemainBlockedAndPreserveDraft() {
        launch()
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        notice("images", containing: "unavailable")
        scenario("loading", billing: "loading")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        XCTAssertFalse(app.staticTexts["Checking account access…"].exists)
        notice("dictation", containing: "still loading")
        scenario("failed", billing: "failed")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        XCTAssertTrue(app.staticTexts["Account access is unavailable. Your draft is preserved."].exists)
        tap("entitlement.settings")
        XCTAssertTrue(
            app.staticTexts["Your plan could not be loaded. Try refreshing account status."].waitForExistence(
                timeout: 5))
        XCTAssertFalse(app.staticTexts["Free"].exists)
        referenceScreenshot("failed read-only plan")
        tap("entitlement.settings.close")
        scenario("pro", billing: "paid")
        XCTAssertTrue(app.buttons["entitlement.send"].isEnabled)
    }

    func testFreeAndPaidModelMediaGatesStayReadOnly() {
        launch(); scenario("free", billing: "Free")
        XCTAssertTrue(app.buttons["entitlement.send"].isEnabled)
        tap("entitlement.models")
        XCTAssertTrue(app.buttons["settings.model.auto:quick"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["settings.model.auto:quick"].isEnabled)
        XCTAssertFalse(app.buttons["settings.model.auto:powerful"].isEnabled)
        XCTAssertFalse(app.buttons["settings.model.synthetic-disabled"].exists)
        referenceScreenshot("Free model picker")
        tap("entitlement.models.close")
        notice("images", containing: "paid plan")
        notice("dictation", containing: "paid plan")
        notice("speech", containing: "paid plan")
        notice("webSearch", containing: "paid plan")
        waitLabel("entitlement.web-preference", "Web preference on")
        waitLabel("entitlement.web-effective", "Effective web off")
        scenario("pro", billing: "paid")
        for feature in ["images", "dictation", "speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        waitLabel("entitlement.web-effective", "Effective web on")
        tap("entitlement.models"); tap("settings.model.synthetic-text")
        waitLabel("entitlement.selected-model", "synthetic-text")
        XCTAssertTrue(app.buttons["entitlement.send"].isEnabled)
        notice("images", containing: "supports images")
        notice("webSearch", containing: "supports web search")
        XCTAssertEqual(app.buttons["entitlement.media.dictation"].value as? String, "available")
        waitLabel("entitlement.web-preference", "Web preference on")
        waitLabel("entitlement.web-effective", "Effective web off")
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }

    func testGuestSeatUsageAndUnknownPlanShowDistinctAuthorityReasons() {
        launch(guest: true); scenario("free", billing: "Free")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        XCTAssertTrue(
            app.staticTexts["This guest account can’t start a chat. Sign in with an eligible account."].exists)
        referenceScreenshot("existing guest chat restriction")
        scenario("pro", billing: "paid")
        XCTAssertTrue(app.buttons["entitlement.send"].isEnabled)
        scenario("seat", billing: "paid")
        let seat = app.staticTexts["Your team’s seat allocation needs attention."]
        XCTAssertTrue(seat.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        notice("images", containing: "seat allocation")
        scenario("usage", billing: "paid")
        XCTAssertTrue(app.staticTexts["You’ve reached your current chat allowance."].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        notice("dictation", containing: "usage limit")
        scenario("unknownPlan", billing: "unknownPlan")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        notice("webSearch", containing: "plan details are unavailable")
        tap("entitlement.settings")
        XCTAssertTrue(
            app.staticTexts["Plan details are unavailable. Paid features remain unavailable until your plan is known."]
                .waitForExistence(timeout: 5))
        for label in ["Upgrade", "Manage subscription", "Restore purchases", "Developer API", "Manage team"] {
            XCTAssertFalse(app.buttons[label].exists)
        }
        referenceScreenshot("unknown read-only plan")
        tap("entitlement.settings.close")
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }
    func testShippingRetryFetchesRecoveredServiceAndPreservesUnsentDraft() {
        launch(); scenario("failed", billing: "failed")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        XCTAssertTrue(app.buttons["Retry"].exists)
        referenceScreenshot("account error with shipping Retry control and preserved draft")
        tap("entitlement.prepare-retry")
        waitLabel("entitlement.retry-service", "service ready; account unchanged")
        waitLabel("entitlement.billing", "failed")
        XCTAssertFalse(app.buttons["entitlement.send"].isEnabled)
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
        tap("Retry")
        waitLabel("entitlement.billing", "paid")
        let enabled = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "enabled == true"), object: app.buttons["entitlement.send"])
        XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 10), .completed)
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
        XCTAssertFalse(app.buttons["Retry"].exists)
        XCTAssertFalse(app.buttons["Upgrade"].exists)
    }

}
