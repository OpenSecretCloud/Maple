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

    private func waitSendEnabled(_ expected: Bool) {
        let changed = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "enabled == %@", NSNumber(value: expected)),
            object: app.buttons["entitlement.send"])
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 10), .completed)
    }

    private func tapPickerChoice(_ label: String) {
        let choices = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "label == %@ AND (elementType == %d OR elementType == %d)", label,
                XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.menuItem.rawValue))
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                choices.allElementsBoundByIndex.filter { $0.isEnabled && $0.isHittable }.count == 1
            }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 5), .completed)
        let visible = choices.allElementsBoundByIndex.filter { $0.isEnabled && $0.isHittable }
        XCTAssertEqual(visible.count, 1)
        visible.first?.tap()
    }

    private func tapSettingsRow(_ id: String) {
        let forms = app.collectionViews.matching(identifier: "settings.screen")
        let form = forms.firstMatch
        XCTAssertTrue(form.waitForExistence(timeout: 5))
        XCTAssertEqual(forms.count, 1)
        let controls = form.buttons.matching(identifier: id)
        let button = controls.firstMatch
        func rowIsClear() -> Bool {
            let window = app.windows.firstMatch, navigation = app.navigationBars["Settings"].firstMatch
            guard controls.count == 1, button.exists, button.isEnabled, button.isHittable,
                window.exists, navigation.exists
            else { return false }
            let region = window.frame.intersection(form.frame)
            return !region.isNull && !region.isEmpty && region.contains(button.frame)
                && button.frame.minY >= navigation.frame.maxY + 8
                && button.frame.maxY <= region.maxY - 8
        }
        // A Settings List may not create a row until it scrolls into view.
        // Reveal the exact owned control before requiring existence or tapping.
        for _ in 0..<8 {
            if rowIsClear() { break }
            form.swipeUp()
        }
        XCTAssertEqual(controls.count, 1)
        let clear = rowIsClear()
        XCTAssertTrue(clear, "The Settings picker must clear the navigation and window boundaries")
        guard clear else { return }
        button.tap()
    }

    func testBillingLoadingAndFailurePreserveAvailableChatAndDraft() {
        launch()
        XCTAssertTrue(app.buttons["entitlement.send"].isEnabled)
        notice("images", containing: "unavailable")
        for feature in ["speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        waitLabel("entitlement.web-effective", "Effective web on")
        scenario("loading", billing: "loading")
        waitSendEnabled(true)
        notice("dictation", containing: "still loading")
        for feature in ["speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        scenario("failed", billing: "failed")
        waitSendEnabled(true)
        for feature in ["speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        XCTAssertFalse(app.staticTexts["Account access is unavailable. Your draft is preserved."].exists)
        tap("entitlement.settings")
        XCTAssertTrue(
            app.staticTexts["Your plan could not be loaded. Try refreshing account status."].waitForExistence(
                timeout: 5))
        XCTAssertFalse(app.staticTexts["Free Plan"].exists)
        referenceScreenshot("failed read-only plan with chat still available")
        tap("entitlement.settings.close")
        scenario("pro", billing: "paid")
        waitSendEnabled(true)
    }

    func testFreeLockedModelsExplainAccessAndPaidMediaIgnoreModelCapabilities() {
        launch(); scenario("free", billing: "Free")
        waitSendEnabled(true)
        tap("entitlement.models")
        let quick = app.buttons["settings.model.auto:quick"]
        let powerful = app.buttons["settings.model.auto:powerful"]
        XCTAssertTrue(quick.waitForExistence(timeout: 5))
        XCTAssertTrue(quick.isEnabled)
        XCTAssertTrue(powerful.isEnabled)
        XCTAssertTrue(powerful.label.contains("Reasoning"))
        XCTAssertTrue(powerful.label.contains("New"))
        XCTAssertFalse(powerful.label.contains("Pro"))
        XCTAssertFalse(app.buttons["settings.model.synthetic-disabled"].exists)
        powerful.tap()
        let message = app.staticTexts["media.access.message"]
        XCTAssertTrue(message.waitForExistence(timeout: 5))
        XCTAssertEqual(message.label, "Get access to our most advanced AI models for superior performance")
        XCTAssertTrue(app.navigationBars["Access Power target"].exists)
        XCTAssertTrue(app.staticTexts["All models run in secure, encrypted environments"].exists)
        XCTAssertFalse(app.buttons["Upgrade"].exists)
        XCTAssertFalse(app.buttons["Buy"].exists)
        referenceScreenshot("locked model read-only explanation with badges")
        tap("media.access.close")
        tap("entitlement.models.close")
        waitLabel("entitlement.selected-model", "auto:quick")
        notice("images", containing: "paid plan")
        notice("dictation", containing: "paid plan")
        notice("speech", containing: "paid plan")
        XCTAssertEqual(app.buttons["entitlement.media.webSearch"].value as? String, "available")
        notice("webSearch", containing: "available for your account")
        waitLabel("entitlement.web-preference", "Web preference on")
        waitLabel("entitlement.web-effective", "Effective web on")
        scenario("pro", billing: "paid")
        for feature in ["images", "dictation", "speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        waitLabel("entitlement.web-effective", "Effective web on")
        tap("entitlement.models"); tap("settings.model.synthetic-text")
        waitLabel("entitlement.selected-model", "synthetic-text")
        waitSendEnabled(true)
        for feature in ["images", "dictation", "speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        waitLabel("entitlement.web-preference", "Web preference on")
        waitLabel("entitlement.web-effective", "Effective web on")
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }

    func testOnlyKnownFreeGuestsAreBlockedAndUnknownPlansKeepImagesAndDictationUnavailable() {
        launch(guest: true); scenario("free", billing: "Free")
        waitSendEnabled(false)
        XCTAssertTrue(
            app.staticTexts["This guest account can’t start a chat. Sign in with an eligible account."].exists)
        referenceScreenshot("existing guest chat restriction")
        scenario("pro", billing: "paid")
        waitSendEnabled(true)
        scenario("seat", billing: "paid")
        waitSendEnabled(true)
        XCTAssertEqual(app.buttons["entitlement.media.images"].value as? String, "available")
        scenario("usage", billing: "paid")
        waitSendEnabled(true)
        XCTAssertEqual(app.buttons["entitlement.media.dictation"].value as? String, "available")
        scenario("unknownPlan", billing: "unknownPlan")
        waitSendEnabled(true)
        for feature in ["speech", "webSearch"] {
            XCTAssertEqual(app.buttons["entitlement.media." + feature].value as? String, "available")
        }
        waitLabel("entitlement.web-effective", "Effective web on")
        notice("images", containing: "plan details are unavailable")
        notice("dictation", containing: "plan details are unavailable")
        tap("entitlement.settings")
        XCTAssertFalse(
            app.staticTexts["Plan details are unavailable. Paid features remain unavailable until your plan is known."]
                .exists)
        XCTAssertEqual(app.staticTexts["settings.plan-name"].label, "Unrecognized fixture plan Plan")
        for label in ["Upgrade", "Manage subscription", "Restore purchases", "Developer API", "Manage team"] {
            XCTAssertFalse(app.buttons[label].exists)
        }
        referenceScreenshot("unknown read-only plan")
        tap("entitlement.settings.close")
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }

    func testSettingsRefreshRecoversBillingWithoutBlockingAvailableChat() {
        launch(); scenario("failed", billing: "failed")
        waitSendEnabled(true)
        tap("entitlement.prepare-retry")
        waitLabel("entitlement.retry-service", "service ready; account unchanged")
        waitLabel("entitlement.billing", "failed")
        tap("entitlement.settings")
        tap("Refresh account status")
        XCTAssertTrue(app.staticTexts["settings.plan-name"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.staticTexts["settings.plan-name"].label, "Pro Annual Plan")
        tap("entitlement.settings.close")
        waitLabel("entitlement.billing", "paid")
        waitSendEnabled(true)
        XCTAssertEqual(editor.value as? String, "Keep this unsent draft")
    }

    func testPlanDatesAndPreferenceDescriptionsMatchResearch() {
        launch(); scenario("pro", billing: "paid")
        tap("entitlement.settings")
        XCTAssertEqual(app.staticTexts["settings.plan-name"].label, "Pro Annual Plan")
        XCTAssertTrue(app.staticTexts["Renews on"].exists)
        let fontDescription = app.staticTexts["settings.font-description"]
        if !fontDescription.isHittable { app.swipeUp() }
        XCTAssertEqual(fontDescription.label, "Maple's default, with a compact modern feel.")
        tapSettingsRow("settings.voice")
        XCTAssertTrue(
            app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Default voices")).firstMatch
                .exists)
        XCTAssertTrue(
            app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Reference accents")).firstMatch
                .exists)
        tapPickerChoice("Casual — Female")
        tapSettingsRow("settings.speed")
        tapPickerChoice("1.2×")
        tap("entitlement.settings.close")
        for id in ["pass", "zaprite"] {
            scenario(id, billing: "paid")
            tap("entitlement.settings")
            XCTAssertEqual(app.staticTexts["settings.plan-name"].label, "Pro Annual Plan")
            XCTAssertTrue(app.staticTexts["Expires on"].exists)
            XCTAssertFalse(app.staticTexts["Renews on"].exists)
            referenceScreenshot(id + " expiry label")
            tap("entitlement.settings.close")
        }
    }

    func testThinkingOptionsFollowTheModelAndPreserveTheSavedChoice() {
        launch(); scenario("free", billing: "Free")
        tap("entitlement.models")
        let thinking = app.buttons["model.thinking-level"]
        XCTAssertTrue(thinking.waitForExistence(timeout: 5))
        XCTAssertEqual(thinking.value as? String, "Default")
        thinking.tap()
        referenceScreenshot("thinking native menu default subtitle")
        let defaultChoice = app.buttons["model.thinking-level.default"]
        let offChoice = app.buttons["model.thinking-level.none"]
        XCTAssertTrue(defaultChoice.waitForExistence(timeout: 5))
        XCTAssertEqual(defaultChoice.label, "Default")
        XCTAssertTrue(defaultChoice.isHittable && offChoice.isHittable)
        // iOS renders the subtitle but exposes only the action title to XCTest.
        // Keep its exact copy in the screenshot and require its native second
        // line's layout; the missing-subtitle baseline has equal-height rows.
        XCTAssertGreaterThan(defaultChoice.frame.height, offChoice.frame.height)
        for label in ["Off", "Low", "High"] {
            XCTAssertTrue(
                app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", label)).firstMatch
                    .exists)
        }
        tapPickerChoice("High")
        XCTAssertEqual(thinking.value as? String, "High")
        tap("settings.model.synthetic-low")
        waitLabel("entitlement.selected-model", "synthetic-low")
        tap("entitlement.models")
        XCTAssertEqual(thinking.value as? String, "Default")
        thinking.tap()
        for label in ["Off", "High"] {
            XCTAssertFalse(
                app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", label)).firstMatch.exists)
        }
        tapPickerChoice("Low")
        XCTAssertEqual(thinking.value as? String, "Low")
        tap("settings.model.auto:quick")
        waitLabel("entitlement.selected-model", "auto:quick")
        tap("entitlement.models")
        XCTAssertEqual(thinking.value as? String, "Low")
        thinking.tap(); tapPickerChoice("High")
        tap("settings.model.synthetic-text")
        waitLabel("entitlement.selected-model", "synthetic-text")
        tap("entitlement.models")
        XCTAssertFalse(thinking.exists)
        tap("settings.model.auto:quick")
        waitLabel("entitlement.selected-model", "auto:quick")
        tap("entitlement.models")
        XCTAssertEqual(thinking.value as? String, "High")
        referenceScreenshot("thinking selection retained across non-reasoning model")
        tap("entitlement.models.close")
    }
}
