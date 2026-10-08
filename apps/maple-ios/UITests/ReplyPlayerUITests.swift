import XCTest

/// Uses the real native player and protected temporary files with synthetic audio.
/// This proves simulator behavior; physical routes and lock playback are separate.
@MainActor final class ReplyPlayerUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launch(reset: Bool) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-player-fixture"]
        if reset { app.launchArguments.append("--maple-player-fixture-reset") }
        app.launch()
        label("player.fixture.ready", equals: "ready")
        label("player.fixture.issue", equals: "none")
    }
    private func label(_ id: String, equals value: String, timeout: TimeInterval = 15) {
        let element = app.staticTexts[id]
        XCTAssertTrue(element.waitForExistence(timeout: timeout))
        let match = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", value), object: element)
        XCTAssertEqual(XCTWaiter.wait(for: [match], timeout: timeout), .completed)
    }
    private func tap(_ id: String) {
        let button = app.buttons[id].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        for _ in 0..<8 where !button.isHittable {
            let scroll = app.scrollViews.firstMatch
            if button.frame.midY < app.frame.midY { scroll.swipeDown() } else { scroll.swipeUp() }
        }
        XCTAssertTrue(button.isHittable); button.tap()
    }
    private func autoplay(_ value: String) {
        let control = app.switches["audio.autoplay"]
        XCTAssertTrue(control.waitForExistence(timeout: 10))
        if control.value as? String != value { control.tap() }
        XCTAssertEqual(control.value as? String, value)
    }

    func testSavedSpeedAppliesAfterChatChangeAndToNewEligibleAutoplay() {
        launch(reset: true); tap("player.fixture.play")
        label("player.fixture.rate", equals: "1.0")
        tap("audio.play-pause"); tap("audio.speed"); tap("1.5×")
        tap("player.fixture.checkpoint"); label("player.fixture.saved", equals: "saved")
        tap("audio.close"); label("player.fixture.bytes", equals: "0")
        app.terminate(); launch(reset: false)
        label("player.fixture.saved-speed", equals: "1.5")
        XCTAssertFalse(app.buttons["audio.close"].exists)
        tap("player.fixture.change-chat")
        label("player.fixture.visible-chat", equals: "synthetic-other-chat")
        tap("player.fixture.play"); label("player.fixture.rate", equals: "1.5")
        XCTAssertEqual(app.switches["audio.autoplay"].value as? String, "0")
        autoplay("1")
        for _ in 0..<3 { tap("audio.forward") }
        label("player.fixture.ended", equals: "ended")
        tap("player.fixture.complete")
        label("player.fixture.current", equals: "automatic-reply-1")
        label("player.fixture.rate", equals: "1.5")
        label("player.fixture.default-rate", equals: "1.5")
        label("player.fixture.metadata-policy", equals: "Maple-only")
        label("player.fixture.saved-speed", equals: "1.5")
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Native eligible autoplay uses saved speed after chat change and cold restart"
        attachment.lifetime = .keepAlways; add(attachment)
        tap("audio.close"); label("player.fixture.bytes", equals: "0")
        label("player.fixture.metadata-policy", equals: "absent"); app.terminate()
    }

    func testNativeTransportMetadataAndColdLaunchPreferenceBoundaries() {
        launch(reset: true)
        XCTAssertFalse(app.buttons["audio.close"].exists)
        tap("player.fixture.play")
        label("player.fixture.metadata-policy", equals: "Maple-only")
        label("player.fixture.rate", equals: "1.0")
        XCTAssertEqual(app.buttons["audio.play-pause"].label, "Pause reply audio")
        tap("audio.play-pause")
        label("player.fixture.rate", equals: "0.0")
        XCTAssertEqual(app.buttons["audio.play-pause"].label, "Play reply audio")
        // Paused, clamped ten-second seeks produce stable metadata observations.
        tap("audio.back"); label("player.fixture.elapsed", equals: "0")
        tap("audio.forward"); label("player.fixture.elapsed", equals: "10")
        tap("audio.back"); label("player.fixture.elapsed", equals: "0")
        tap("audio.speed"); tap("1.5×")
        label("player.fixture.default-rate", equals: "1.5")
        label("player.fixture.saved-speed", equals: "1.5")
        autoplay("1")
        tap("player.fixture.checkpoint"); label("player.fixture.saved", equals: "saved")
        tap("audio.play-pause"); label("player.fixture.rate", equals: "1.5")
        label("player.fixture.metadata-policy", equals: "Maple-only")
        app.terminate()

        launch(reset: false)
        label("player.fixture.saved-speed", equals: "1.5")
        label("player.fixture.bytes", equals: "0")
        label("player.fixture.metadata-policy", equals: "absent")
        XCTAssertFalse(app.buttons["audio.close"].exists)
        tap("player.fixture.play")
        label("player.fixture.rate", equals: "1.5")
        let automatic = app.switches["audio.autoplay"]
        XCTAssertTrue(automatic.waitForExistence(timeout: 10))
        XCTAssertEqual(automatic.value as? String, "0", "A prior listening session must not restore autoplay")
        tap("audio.close")
        label("player.fixture.metadata-policy", equals: "absent")
        label("player.fixture.bytes", equals: "0")
        XCTAssertFalse(app.buttons["audio.close"].exists)
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Native audio closed with saved speed and no metadata or temporary files"
        attachment.lifetime = .keepAlways; add(attachment)
        app.terminate()
    }
}
