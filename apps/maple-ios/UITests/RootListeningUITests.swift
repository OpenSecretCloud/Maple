import Foundation
import XCTest
import UIKit

/// Normal local AppEnvironment, SDK, durable runtime, speech service and native
/// player. Only microphone/transcription input and the timing of one next SDK
/// Responses request are controlled. No credentials enter XCTest activity logs.
@MainActor final class RootListeningUITests: XCTestCase {
    private var app: XCUIApplication!
    private var composer: XCUIElement { app.textViews["chat.composer"] }
    private var diagnostics: XCUIElement { app.staticTexts["listening.diagnostics"] }

    private func launch() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-local-listening-fixture"]; app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        XCTAssertTrue(diagnostics.waitForExistence(timeout: 20))
        tap("chat.new")
        _ = try waitState { $0.string("phase") == "closed" && $0.int("completions") == 0 }
    }

    private func state() throws -> [String: Any] {
        guard let text = diagnostics.value as? String, let data = text.data(using: .utf8), data.count <= 8_192,
            let value = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { throw ListeningProofIssue.diagnosticsUnavailable }
        return value
    }
    @discardableResult private func waitState(
        timeout: TimeInterval = 90, _ predicate: @escaping ([String: Any]) -> Bool
    ) throws -> [String: Any] {
        let condition = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard let value = try? self.state() else { return false }
                return predicate(value)
            }, object: diagnostics)
        guard XCTWaiter.wait(for: [condition], timeout: timeout) == .completed else {
            let lastState: String
            if let value = try? state(),
                let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
                data.count <= 8_192, let text = String(data: data, encoding: .utf8)
            {
                lastState = text
            } else {
                lastState = "Local listening diagnostics unavailable at timeout"
            }
            let record = XCTAttachment(string: lastState)
            record.name = "Local listening last state at transition timeout"
            record.lifetime = .keepAlways; add(record)
            XCTFail("Local listening state did not reach the required transition")
            throw ListeningProofIssue.transitionTimedOut
        }
        return try state()
    }
    private func tap(_ id: String) {
        let button = id == "chat.menu" ? app.mapleMenuButton : app.buttons[id].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15)); XCTAssertTrue(button.isEnabled)
        XCTAssertTrue(button.isHittable); button.tap()
    }
    private func selectNormalPlaybackSpeed() {
        let choices = app.buttons.matching(
            NSPredicate(format: "label == %@ AND identifier != %@", "1×", "audio.speed"))
        func availableChoices() -> [XCUIElement] {
            choices.allElementsBoundByIndex.filter { $0.exists && $0.isEnabled && $0.isHittable }
        }
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in availableChoices().count == 1 }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 15), .completed)
        let availableCount = availableChoices().count
        XCTAssertEqual(availableCount, 1, "Playback speed must have one available exact 1× choice")
        guard availableCount == 1 else { return }
        let choice = choices.firstMatch
        XCTAssertTrue(choice.exists, "The exact 1× playback speed choice must exist")
        XCTAssertTrue(choice.isEnabled, "The exact 1× playback speed choice must be enabled")
        XCTAssertTrue(choice.isHittable, "The exact 1× playback speed choice must be hittable")
        choices.firstMatch.tap()
    }
    private func replace(_ text: String) throws { try replaceSyntheticText(composer, with: text, in: app) }
    private func autoplay() {
        let control = app.switches["audio.autoplay"]
        XCTAssertTrue(control.waitForExistence(timeout: 15)); XCTAssertTrue(control.isHittable)
        if control.value as? String != "1" { control.tap() }
        XCTAssertEqual(control.value as? String, "1")
    }
    private func seedReply() throws -> String {
        try replace(
            "Reply with exactly this synthetic test paragraph: Maple keeps one reply ready while a new draft is composed. The native player can pause, resume, and keep pending replies in order. These sentences contain no personal information. Every test action is explicit, and closing the player clears the temporary reading session. A separate composer holds the next message while the original audio remains available. Opening the menu changes the visible screen without transferring ownership of a reply. Returning to the conversation keeps the same reading position and does not start a missed answer."
        )
        tap("chat.send")
        let completed = try waitState { $0.int("completions") == 1 }
        XCTAssertTrue(completed.bool("completionVisible")); XCTAssertTrue(completed.bool("completionForeground"))
        XCTAssertFalse(app.staticTexts["chat.error"].exists)
        let responseID = completed.string("completionID")
        XCTAssertNotEqual(responseID, "none")
        let record = XCTAttachment(string: completed.string("completionChat"))
        record.name = "Exact synthetic local listening chat ID for cleanup"; record.lifetime = .keepAlways; add(record)
        let speaker = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "message.speak.")).firstMatch
        XCTAssertTrue(speaker.waitForExistence(timeout: 10)); XCTAssertTrue(speaker.isEnabled)
        try reveal(speaker)
        speaker.tap()
        _ = try waitState { $0.string("current") == responseID && $0.string("phase") == "playing" }
        // Configuration must not consume the real reply before the case receives a playing seed.
        tap("audio.play-pause")
        _ = try waitState(timeout: 10) {
            $0.string("current") == responseID && $0.bool("manual") && $0.string("phase") == "paused"
        }
        tap("audio.speed"); selectNormalPlaybackSpeed()
        _ = try waitState(timeout: 10) { $0.number("speed") == 1 }
        autoplay()
        tap("audio.play-pause")
        _ = try waitState(timeout: 10) {
            $0.string("current") == responseID && !$0.bool("manual") && $0.string("phase") == "playing"
        }
        return responseID
    }
    private func reveal(_ action: XCUIElement) throws {
        for _ in 0..<10 {
            let top = max(app.navigationBars.firstMatch.frame.maxY, diagnostics.frame.maxY) + 12
            let bottom = composer.frame.minY - 24
            guard bottom - top >= 80 else { throw ListeningProofIssue.insufficientViewport }
            if action.isHittable, action.frame.minY >= top, action.frame.maxY <= bottom { return }
            let origin = app.coordinate(withNormalizedOffset: .zero), x = app.frame.midX
            origin.withOffset(CGVector(dx: x, dy: bottom - 12)).press(
                forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: x, dy: top + 12)))
        }
        XCTFail("Speaker did not become visible above the real composer")
        throw ListeningProofIssue.insufficientViewport
    }
    private func dictate() throws {
        try replace("Reply in one short sentence about this phrase: ")
        tap("dictation.start")
        _ = try waitState(timeout: 15) { $0.string("hold") == "capture" }
        if app.buttons["audio.play-pause"].exists { XCTAssertFalse(app.buttons["audio.play-pause"].isEnabled) }
        tap("dictation.stop")
        _ = try waitState(timeout: 15) { $0.string("hold") == "transcription" }
        if app.buttons["audio.play-pause"].exists { XCTAssertFalse(app.buttons["audio.play-pause"].isEnabled) }
        tap("lifecycle.transcript")
        _ = try waitState(timeout: 15) { $0.string("hold") == "review" }
        let inserted = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.composer.value as? String
                    == "Reply in one short sentence about this phrase: synthetic dictated text"
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [inserted], timeout: 15), .completed)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
    }
    private func submitHeld(expectedAcceptedCount: Int) throws -> [String: Any] {
        tap("listening.hold-request")
        _ = try waitState(timeout: 10) { $0.bool("armed") }
        tap("chat.send")
        let held = try waitState(timeout: 30) {
            $0.bool("waiting") && $0.int("acceptedCount") == expectedAcceptedCount
        }
        XCTAssertNotNil(UUID(uuidString: held.string("acceptedID")))
        return held
    }
    private func closePlayer() throws {
        tap("audio.close")
        _ = try waitState(timeout: 15) {
            $0.string("phase") == "closed" && $0.string("current") == "none"
                && $0.strings("pending").isEmpty && !$0.bool("autoplay")
        }
        XCTAssertFalse(app.buttons["audio.close"].exists)
    }
    private func snapshot(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }

    private struct ReloginCredentials: Decodable { let email: String; let password: String }
    private func credentials() throws -> ReloginCredentials {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let path = support.appendingPathComponent("MapleUITestCredentials.json")
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular,
            let size = attributes[.size] as? NSNumber, size.intValue <= 4_096
        else { throw ListeningProofIssue.diagnosticsUnavailable }
        let value = try JSONDecoder().decode(ReloginCredentials.self, from: Data(contentsOf: path))
        try FileManager.default.removeItem(at: path)
        return value
    }
    private func pasteCredential(_ value: String, into field: XCUIElement) throws {
        guard field.waitForExistence(timeout: 15) else { throw ListeningProofIssue.diagnosticsUnavailable }
        UIPasteboard.general.setItems(
            [["public.utf8-plain-text": value]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(30)])
        field.tap(); field.press(forDuration: 1.2)
        let menu = app.menuItems["Paste"].firstMatch, button = app.buttons["Paste"].firstMatch
        if menu.waitForExistence(timeout: 3) {
            menu.tap()
        } else if button.waitForExistence(timeout: 3) {
            button.tap()
        } else {
            throw ListeningProofIssue.diagnosticsUnavailable
        }
        // Keep the private clipboard item until this real form reports committed input.
        // The predicate exposes no credential value in its assertion or diagnostics.
        guard field.identifier == "auth.identifier" || field.identifier == "auth.password" else { return }
        let committed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                if field.identifier == "auth.identifier" {
                    return field.value as? String == value
                }
                let submit = self.app.buttons["auth.submit"].firstMatch
                return submit.exists && submit.isEnabled
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [committed], timeout: 10), .completed)
    }
    private func listButton(_ id: String, maximumSwipes: Int = 16) throws {
        let button = id == "chat.menu" ? app.mapleMenuButton : app.buttons[id].firstMatch
        for _ in 0..<maximumSwipes where !button.isHittable {
            guard let list = app.collectionViews.allElementsBoundByIndex.first(where: { $0.isHittable }) else {
                throw ListeningProofIssue.insufficientViewport
            }
            list.swipeUp()
        }
        XCTAssertTrue(button.exists); XCTAssertTrue(button.isHittable); XCTAssertTrue(button.isEnabled); button.tap()
    }
    private func signOutFromSettings() throws {
        let owner = app.descendants(matching: .any).matching(identifier: "settings.screen").firstMatch
        func visibleRegion() -> CGRect {
            guard owner.exists else { return .null }
            return owner.frame.intersection(app.frame).insetBy(dx: 16, dy: 24)
        }
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let region = visibleRegion()
                guard !region.isNull, !region.isInfinite, region.width >= 100, region.height >= 100 else {
                    return false
                }
                return owner.descendants(matching: .any).allElementsBoundByIndex.prefix(512).contains {
                    $0.exists && $0.isHittable && !$0.frame.intersection(region).isEmpty
                }
            }, object: owner)
        guard XCTWaiter.wait(for: [ready], timeout: 15) == .completed else {
            XCTFail("Settings did not expose its visible form")
            throw ListeningProofIssue.insufficientViewport
        }
        let button = owner.buttons["settings.sign-out"].firstMatch
        for _ in 0..<16 {
            if button.exists && button.isHittable { break }
            let region = visibleRegion()
            guard !region.isNull, !region.isInfinite, region.width >= 44, region.height >= 100 else {
                throw ListeningProofIssue.insufficientViewport
            }
            let origin = app.coordinate(withNormalizedOffset: .zero)
            origin.withOffset(
                CGVector(dx: region.midX - app.frame.minX, dy: region.maxY - region.height * 0.15 - app.frame.minY)
            )
            .press(
                forDuration: 0.05,
                thenDragTo: origin.withOffset(
                    CGVector(dx: region.midX - app.frame.minX, dy: region.minY + region.height * 0.15 - app.frame.minY))
            )
        }
        XCTAssertTrue(button.exists); XCTAssertTrue(button.isEnabled); XCTAssertTrue(button.isHittable)
        button.tap()
    }
    private func openExactChat(_ id: String) throws {
        tap("chat.menu")
        guard let button = try MenuChatUI.find(id, in: app, projectIDs: [], pageLimit: 20) else {
            XCTFail("The exact unassigned queue conversation is absent from the menu")
            throw ListeningProofIssue.insufficientViewport
        }
        XCTAssertEqual(button.identifier, "library.chat." + id)
        XCTAssertTrue(button.exists); XCTAssertTrue(button.isHittable); XCTAssertTrue(button.isEnabled); button.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 20))
    }

    func testOrdinarySignOutClearsRealQueueAndRejectsLateTranscriptAfterSameAccountRelogin() throws {
        try XCTSkipUnless(UIDevice.current.userInterfaceIdiom == .phone, "This scenario uses compact menu navigation")
        let credentials = try credentials()
        defer { UIPasteboard.general.items = [] }
        try launch()
        try replace("Synthetic request held before its actual SDK dispatch")
        let held = try submitHeld(expectedAcceptedCount: 1)
        let oldChat = held.string("assignedChat")
        XCTAssertNotEqual(oldChat, "none")
        try replace("Actual queued A must be cleared on sign out")
        tap("chat.send"); tap("queue.open")
        XCTAssertTrue(app.staticTexts["Actual queued A must be cleared on sign out"].waitForExistence(timeout: 10))
        if app.buttons["queue.pause"].exists { tap("queue.pause") }
        snapshot("Reference — actual local root queue review — request held before dispatch")
        tap("Done")
        try replace("Pending transcript belongs to the old account draft. ")
        tap("dictation.start"); tap("dictation.stop")
        _ = try waitState { $0.string("hold") == "transcription" }
        for _ in 0..<5 {
            tap("listening.inspect-transcripts")
            if try state().int("pendingTranscripts") == 1 { break }
        }
        _ = try waitState(timeout: 10) { $0.int("pendingTranscripts") == 1 }
        let record = XCTAttachment(string: oldChat)
        record.name = "Exact synthetic queue cleanup conversation ID"; record.lifetime = .keepAlways; add(record)
        tap("chat.menu")
        tap("library.actions")
        let settings = app.buttons["library.settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20)); XCTAssertTrue(settings.isHittable)
        XCTAssertTrue(settings.isEnabled); settings.tap(); try signOutFromSettings()
        XCTAssertTrue(app.textFields["auth.identifier"].waitForExistence(timeout: 30))
        XCTAssertFalse(app.alerts.firstMatch.exists, "Ordinary sign out must not ask to retain queued work")
        try pasteCredential(credentials.email, into: app.textFields["auth.identifier"])
        try pasteCredential(credentials.password, into: app.secureTextFields["auth.password"])
        UIPasteboard.general.items = []
        tap("auth.submit")
        XCTAssertTrue(composer.waitForExistence(timeout: 45))
        _ = try waitState { $0.int("acceptedCount") == 0 && $0.int("completions") == 0 && !$0.bool("waiting") }
        XCTAssertEqual(composer.value as? String, "")
        XCTAssertFalse(app.buttons["queue.open"].exists)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
        try replace("Replacement account draft stays private and unchanged")
        tap("lifecycle.transcript")
        _ = try waitState(timeout: 15) { $0.int("finishedTranscripts") == 1 }
        XCTAssertEqual(composer.value as? String, "Replacement account draft stays private and unchanged")
        try openExactChat(oldChat)
        XCTAssertEqual(composer.value as? String, "")
        XCTAssertFalse(app.buttons["queue.open"].exists)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        XCTAssertEqual(try state().int("acceptedCount"), 0)
        XCTAssertEqual(try state().int("completions"), 0)
        tap("chat.new")
        // Selection starts in a Task; wait for the retained draft to reach the
        // native editor before reading the same full-value invariant.
        let restoredDraft = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.composer.exists
                    && self.composer.value as? String == "Replacement account draft stays private and unchanged"
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [restoredDraft], timeout: 10), .completed)
        XCTAssertEqual(composer.value as? String, "Replacement account draft stays private and unchanged")
        app.terminate(); app.launch()
        XCTAssertTrue(composer.waitForExistence(timeout: 30))
        try openExactChat(oldChat)
        XCTAssertEqual(composer.value as? String, "")
        XCTAssertFalse(app.buttons["queue.open"].exists)
        XCTAssertFalse(app.buttons["dictation.discard"].exists)
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        snapshot("Ordinary root sign out removes real queued work and rejects the old transcript")
        app.terminate()
    }

    func testManualPauseSurvivesDictationSendAndPreservesPendingFIFO() throws {
        try launch()
        let original = try seedReply()
        tap("audio.play-pause")
        _ = try waitState(timeout: 10) { $0.bool("manual") && $0.string("phase") == "paused" }
        try dictate()
        let accepted = try submitHeld(expectedAcceptedCount: 2)
        XCTAssertEqual(accepted.string("acceptedCurrent"), original)
        XCTAssertEqual(accepted.string("acceptedPhase"), "paused")
        XCTAssertEqual(accepted.string("acceptedHold"), "none")
        XCTAssertEqual(accepted.string("acceptedArm"), accepted.string("acceptedID"))
        XCTAssertTrue(accepted.bool("manual"))
        tap("listening.release-request")
        let second = try waitState { $0.int("completions") == 2 }
        let firstPending = second.string("completionID"), firstTurn = second.string("completionMessage")
        XCTAssertEqual(firstTurn, accepted.string("acceptedID"))
        XCTAssertEqual(second.strings("pending"), [firstPending])
        XCTAssertEqual(second.strings("pendingTurns"), [firstTurn])
        XCTAssertEqual(second.string("current"), original); XCTAssertEqual(second.string("phase"), "paused")
        try replace("Reply with exactly: A second pending synthetic reply.")
        tap("chat.send")
        let third = try waitState { $0.int("completions") == 3 }
        let secondPending = third.string("completionID")
        XCTAssertEqual(third.strings("pending"), [firstPending, secondPending])
        XCTAssertEqual(third.string("current"), original); XCTAssertTrue(third.bool("manual"))

        tap("chat.menu"); XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        tap("library.close")
        let returned = try state()
        XCTAssertEqual(returned.string("phase"), "paused")
        XCTAssertEqual(returned.strings("pending"), [firstPending, secondPending])
        tap("chat.menu"); XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        // Explicit Play can resume the current reply while its chat is hidden.
        tap("audio.play-pause")
        _ = try waitState(timeout: 15) { $0.string("phase") == "playing" && !$0.bool("manual") }
        let duration = try state().number("duration")
        XCTAssertGreaterThan(duration, 0); XCTAssertLessThanOrEqual(duration, 120)
        for _ in 0..<Int(ceil(duration / 10)) { tap("audio.forward") }
        _ = try waitState(timeout: 15) { $0.string("phase") == "ended" }
        XCTAssertEqual(try state().string("current"), original)
        tap("audio.play-pause")
        let advanced = try waitState { $0.string("current") == firstPending && $0.string("phase") != "preparing" }
        XCTAssertNotEqual(advanced.string("phase"), "failed")
        XCTAssertEqual(advanced.string("currentTurn"), firstTurn)
        XCTAssertEqual(advanced.strings("pending"), [secondPending])
        snapshot("Normal root preserves paused FIFO and explicit offscreen resume")
        try closePlayer(); app.terminate()
    }

    func testOnlyTheAcceptedDictatedTurnReplacesTheSoleReviewHold() throws {
        try launch()
        let original = try seedReply()
        try dictate()
        let accepted = try submitHeld(expectedAcceptedCount: 2)
        XCTAssertFalse(accepted.bool("manual"))
        XCTAssertEqual(accepted.string("acceptedCurrent"), original)
        XCTAssertEqual(accepted.string("acceptedPhase"), "paused")
        XCTAssertEqual(accepted.string("acceptedHold"), "none")
        XCTAssertEqual(accepted.string("acceptedArm"), accepted.string("acceptedID"))
        // The gate is before the actual SDK request: Send alone resumed nothing.
        XCTAssertEqual(accepted.int("completions"), 1)
        tap("listening.release-request")
        let completed = try waitState { $0.int("completions") == 2 }
        XCTAssertEqual(completed.string("completionMessage"), accepted.string("acceptedID"))
        XCTAssertEqual(completed.string("current"), completed.string("completionID"))
        XCTAssertEqual(completed.string("currentTurn"), accepted.string("acceptedID"))
        XCTAssertTrue(completed.strings("pending").isEmpty)
        _ = try waitState { $0.string("phase") == "playing" }
        snapshot("Normal root replaces only the durably accepted dictated turn")
        try closePlayer(); app.terminate()
    }

    func testExplicitOffscreenPlayAndReturnNeverCollectAnOffscreenCompletion() throws {
        try launch()
        let original = try seedReply()
        try dictate()
        let accepted = try submitHeld(expectedAcceptedCount: 2)
        XCTAssertEqual(accepted.string("acceptedCurrent"), original)
        tap("chat.menu"); XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        tap("audio.play-pause")
        _ = try waitState(timeout: 15) { $0.string("phase") == "playing" && $0.string("hold") == "none" }
        tap("listening.release-request")
        let hidden = try waitState { $0.int("completions") == 2 }
        XCTAssertFalse(hidden.bool("completionVisible")); XCTAssertTrue(hidden.bool("completionForeground"))
        XCTAssertEqual(hidden.string("completionMessage"), accepted.string("acceptedID"))
        XCTAssertEqual(hidden.string("current"), original); XCTAssertTrue(hidden.strings("pending").isEmpty)
        tap("library.close")
        let returned = try state()
        XCTAssertEqual(returned.string("current"), original); XCTAssertTrue(returned.strings("pending").isEmpty)
        XCTAssertEqual(returned.int("completions"), 2)
        snapshot("Normal root does not collect or start offscreen replies on return")
        try closePlayer(); app.terminate()
    }

    func testNewChatAssignmentKeepsTheAcceptedDictationMessageID() throws {
        try launch(); try dictate()
        let accepted = try submitHeld(expectedAcceptedCount: 1)
        XCTAssertEqual(accepted.string("acceptedChat"), "none")
        XCTAssertEqual(accepted.string("acceptedArm"), "none")
        XCTAssertEqual(accepted.string("acceptedHold"), "review")
        XCTAssertNotEqual(accepted.string("assignedChat"), "none")
        XCTAssertEqual(accepted.string("assignedArm"), accepted.string("acceptedID"))
        XCTAssertEqual(accepted.string("hold"), "none")
        XCTAssertEqual(accepted.int("completions"), 0)
        tap("listening.release-request")
        let completed = try waitState { $0.int("completions") == 1 }
        XCTAssertEqual(completed.string("completionMessage"), accepted.string("acceptedID"))
        XCTAssertEqual(completed.string("completionChat"), accepted.string("assignedChat"))
        XCTAssertEqual(completed.string("phase"), "closed")
        XCTAssertEqual(completed.string("current"), "none")
        XCTAssertTrue(completed.strings("pending").isEmpty); XCTAssertFalse(completed.bool("autoplay"))
        let record = XCTAttachment(string: completed.string("completionChat"))
        record.name = "Exact synthetic New Chat listening ID for cleanup"; record.lifetime = .keepAlways; add(record)
        snapshot("Normal root preserves canonical dictated turn across New Chat assignment")
        app.terminate()
    }
}

private enum ListeningProofIssue: Error { case diagnosticsUnavailable, transitionTimedOut, insufficientViewport }
private extension Dictionary where Key == String, Value == Any {
    func string(_ key: String) -> String { self[key] as? String ?? "unavailable" }
    func strings(_ key: String) -> [String] { self[key] as? [String] ?? [] }
    func bool(_ key: String) -> Bool { self[key] as? Bool ?? false }
    func int(_ key: String) -> Int { self[key] as? Int ?? -1 }
    func number(_ key: String) -> Double { self[key] as? Double ?? -1 }
}
