import XCTest

@MainActor final class BrowserUITests: XCTestCase {
    private func fixture() throws -> XCUIApplication {
        let address = try XCTUnwrap(ProcessInfo.processInfo.environment["MAPLE_LINK_FIXTURE_URL"])
        let app = XCUIApplication()
        app.launchArguments = ["--maple-browser-fixture", "--maple-browser-url", address]
        app.launch(); continueAfterFailure = false
        XCTAssertTrue(app.textViews["browser.fixture.draft"].waitForExistence(timeout: 15))
        return app
    }
    private func tapPreparedLocalReference(in app: XCUIApplication) {
        // Textual's native selection overlay also handles link taps. The
        // underlying SwiftUI Link can exist without its own hittable point.
        let owners = app.otherElements.matching(NSPredicate(format: "label == %@", "Message"))
        let owner = owners.firstMatch
        let links = owner.links.matching(identifier: "Local reference")
        let fields = owner.textViews.matching(NSPredicate(format: "value == %@", "Local reference"))
        let field = fields.firstMatch
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard owners.count == 1, links.count == 1, fields.count == 1,
                    field.exists, field.isHittable
                else { return false }
                let nativeFrame = field.frame, linkFrame = links.firstMatch.frame
                guard
                    [
                        nativeFrame.minX, nativeFrame.minY, nativeFrame.width, nativeFrame.height,
                        linkFrame.minX, linkFrame.minY, linkFrame.width, linkFrame.height,
                    ].allSatisfy({ $0.isFinite }),
                    nativeFrame.width > 0, nativeFrame.height > 0, linkFrame.width > 0, linkFrame.height > 0,
                    app.frame.contains(nativeFrame)
                else { return false }
                return abs(nativeFrame.minX - linkFrame.minX) <= 1
                    && abs(nativeFrame.minY - linkFrame.minY) <= 1
                    && abs(nativeFrame.width - linkFrame.width) <= 1
                    && abs(nativeFrame.height - linkFrame.height) <= 1
            }, object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed)
        XCTAssertEqual(owners.count, 1); XCTAssertEqual(links.count, 1); XCTAssertEqual(fields.count, 1)
        XCTAssertEqual(field.value as? String, "Local reference")
        XCTAssertTrue(field.isHittable)
        field.tap()
    }
    func testDeclineAndRejectUnsafeSchemePreserveDraft() throws {
        let app = try fixture()
        let link = app.links["Local reference"]
        XCTAssertTrue(link.waitForExistence(timeout: 10)); tapPreparedLocalReference(in: app)
        XCTAssertTrue(app.buttons["link.confirm"].waitForExistence(timeout: 5))
        app.alerts.buttons["Cancel"].tap()
        XCTAssertEqual(app.textViews["browser.fixture.draft"].value as? String, "Keep this browser draft")
        app.buttons["browser.fixture.unsupported"].tap()
        XCTAssertTrue(app.alerts.firstMatch.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["link.confirm"].exists)
        app.alerts.buttons["Cancel"].tap()
        XCTAssertEqual(app.staticTexts["browser.fixture.visibility"].label, "Chat visible")
    }
    func testBrowserUsesCurrentPageForExternalEscapeAndReturnsToDraft() throws {
        let app = try fixture()
        let link = app.links["Local reference"]
        XCTAssertTrue(link.waitForExistence(timeout: 10)); tapPreparedLocalReference(in: app)
        let alerts = app.alerts.matching(NSPredicate(format: "label == %@", "Open external link?"))
        let alert = alerts.element(boundBy: 0)
        XCTAssertTrue(alert.waitForExistence(timeout: 5)); XCTAssertEqual(alerts.count, 1)
        let confirmations = alert.buttons.matching(
            NSPredicate(format: "identifier == %@ AND label == %@", "link.confirm", "Open link")
        )
        .allElementsBoundByIndex.filter { $0.descendants(matching: .button).count == 0 }
        XCTAssertEqual(confirmations.count, 1)
        let confirmation = try XCTUnwrap(confirmations.first)
        XCTAssertTrue(confirmation.exists); XCTAssertTrue(confirmation.isEnabled);
        XCTAssertTrue(confirmation.isHittable)
        confirmation.tap()
        XCTAssertTrue(app.buttons["browser.close"].waitForExistence(timeout: 10))
        let next = app.webViews.links["Second local page"]
        XCTAssertTrue(next.waitForExistence(timeout: 15)); next.tap()
        XCTAssertTrue(app.webViews.staticTexts["Second local page reached"].waitForExistence(timeout: 15))
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Local external browser second page"; screenshot.lifetime = .keepAlways; add(screenshot)
        app.buttons["browser.external"].tap()
        let safari = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
        XCTAssertTrue(safari.wait(for: .runningForeground, timeout: 15))
        XCTAssertTrue(safari.webViews.staticTexts["Second local page reached"].waitForExistence(timeout: 15))
        app.activate()
        XCTAssertTrue(app.buttons["browser.close"].waitForExistence(timeout: 10)); app.buttons["browser.close"].tap()
        XCTAssertTrue(app.textViews["browser.fixture.draft"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.textViews["browser.fixture.draft"].value as? String, "Keep this browser draft")
        XCTAssertEqual(app.staticTexts["browser.fixture.visibility"].label, "Chat visible")
    }
}
