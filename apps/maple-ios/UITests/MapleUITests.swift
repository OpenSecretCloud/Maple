import XCTest
import UIKit

final class MapleUITests: XCTestCase {
    /// A host-side fixture runner may place this file in the test runner's own
    /// private container. Credentials never enter app launch arguments, source,
    /// assertions or XCTest's typeText activity log. The real form is exercised.
    @MainActor func testLocalSignIn() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        struct Fixture: Decodable { let email: String; let password: String }
        let root = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true)
        let file = root.appendingPathComponent("MapleUITestCredentials.json")
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard let size = attributes[.size] as? NSNumber, size.intValue < 4096 else {
            XCTFail("Invalid local fixture size"); return
        }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: file))
        try FileManager.default.removeItem(at: file)
        defer { UIPasteboard.general.items = [] }
        let app = XCUIApplication()
        app.launch()
        if app.buttons["chat.menu"].waitForExistence(timeout: 3) {
            let settings = try openLocalSettings(in: app)
            let signOut = app.buttons["settings.sign-out"]
            try revealLocalControl(signOut, owner: settings, in: app)
            signOut.tap()
        }
        let identifier = app.textFields["auth.identifier"]
        XCTAssertTrue(identifier.waitForExistence(timeout: 30))
        try paste(fixture.email, into: identifier, in: app)
        try paste(fixture.password, into: app.secureTextFields["auth.password"], in: app)
        UIPasteboard.general.items = []
        app.buttons["auth.submit"].tap()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 45))
        XCTAssertFalse(app.staticTexts["auth.error"].exists)
    }

    /// Consumes the same private one-shot guest receipt as the browser proof.
    /// No new guest is created by the application; no inference is requested.
    @MainActor func testLocalExistingGuestSignInUsesSharedGuestReceipt() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        struct Fixture: Decodable { let id: String; let password: String }
        let root = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let file = root.appendingPathComponent("MapleUITestGuestCredentials.json")
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard let size = attributes[.size] as? NSNumber, size.intValue > 0, size.intValue < 4096 else {
            XCTFail("Invalid local guest fixture size"); return
        }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: file))
        try FileManager.default.removeItem(at: file)
        guard UUID(uuidString: fixture.id) != nil, !fixture.password.isEmpty else {
            XCTFail("Invalid local guest fixture shape"); return
        }
        defer { UIPasteboard.general.items = [] }
        let app = XCUIApplication(); app.launch()
        if app.buttons["chat.menu"].waitForExistence(timeout: 3) {
            let settings = try openLocalSettings(in: app)
            let signOut = app.buttons["settings.sign-out"]
            try revealLocalControl(signOut, owner: settings, in: app)
            signOut.tap()
        }
        let guestMode = app.buttons["Use an existing guest account"]
        XCTAssertTrue(guestMode.waitForExistence(timeout: 30))
        if !guestMode.isHittable { app.swipeUp() }
        XCTAssertTrue(guestMode.isHittable); guestMode.tap()
        let identifier = app.textFields["auth.identifier"]
        XCTAssertTrue(identifier.waitForExistence(timeout: 10))
        if !identifier.isHittable { app.swipeDown() }
        try paste(fixture.id, into: identifier, in: app)
        try paste(fixture.password, into: app.secureTextFields["auth.password"], in: app)
        UIPasteboard.general.items = []
        app.buttons["auth.submit"].tap()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 45))
        XCTAssertFalse(app.staticTexts["auth.error"].exists)
        let settings = try openLocalSettings(in: app)
        let guest = app.staticTexts["Guest account"]
        try revealLocalControl(guest, owner: settings, in: app)
        let capture = XCTAttachment(screenshot: app.screenshot())
        capture.name = "Reference — real local existing guest — ordinary native account settings"
        capture.lifetime = .keepAlways; add(capture)
        let signOut = app.buttons["settings.sign-out"]
        try revealLocalControl(signOut, owner: settings, in: app)
        signOut.tap()
        XCTAssertTrue(app.textFields["auth.identifier"].waitForExistence(timeout: 20))
    }

    @MainActor private func openLocalSettings(in app: XCUIApplication) throws -> XCUIElement {
        app.buttons["chat.menu"].tap()
        func library() -> XCUIElement? {
            app.collectionViews.allElementsBoundByIndex.first { owner in
                let region = owner.frame.intersection(app.frame)
                guard region.width >= 100, region.height >= 100 else { return false }
                return owner.descendants(matching: .any).matching(
                    NSPredicate(format: "identifier BEGINSWITH %@", "library.")
                ).allElementsBoundByIndex.prefix(512).contains {
                    $0.isHittable && !$0.frame.intersection(region).isEmpty
                }
            }
        }
        let visible = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in library() != nil }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [visible], timeout: 20), .completed)
        _ = try XCTUnwrap(library())
        let settings = app.buttons["library.settings"]
        XCTAssertTrue(settings.waitForExistence(timeout: 20)); XCTAssertTrue(settings.isHittable)
        XCTAssertTrue(settings.isEnabled); settings.tap()
        let form = app.collectionViews["settings.screen"]
        XCTAssertTrue(form.waitForExistence(timeout: 15))
        return form
    }

    @MainActor private func revealLocalControl(
        _ element: XCUIElement, owner: XCUIElement, in app: XCUIApplication, attempts: Int = 8
    ) throws {
        for _ in 0..<attempts {
            if element.exists && element.isHittable { return }
            let region = owner.frame.intersection(app.frame).insetBy(dx: 16, dy: 24)
            guard owner.exists, region.width >= 44, region.height >= 100 else {
                XCTFail("Local proof owning list is unavailable"); throw NativeTextEntryIssue.unavailable
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
        XCTAssertTrue(element.exists && element.isHittable)
        guard element.exists && element.isHittable else { throw NativeTextEntryIssue.unavailable }
    }

    @MainActor private func paste(_ value: String, into field: XCUIElement, in app: XCUIApplication) throws {
        UIPasteboard.general.setItems(
            [["public.utf8-plain-text": value]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(30)])
        field.tap()
        field.press(forDuration: 1.2)
        let paste = app.menuItems["Paste"]
        if paste.waitForExistence(timeout: 3) {
            paste.tap()
        } else {
            let button = app.buttons["Paste"]
            XCTAssertTrue(button.waitForExistence(timeout: 3))
            button.tap()
        }
    }

    @MainActor func testConfiguredAppOpens() throws {
        let app = XCUIApplication()
        app.launchArguments = ["--maple-startup-diagnostic"]
        app.launch()
        let login = app.textFields["auth.identifier"]
        let chat = app.buttons["chat.new"]
        let available = NSPredicate { _, _ in login.exists || chat.exists }
        expectation(for: available, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        XCTAssertFalse(app.staticTexts["Configuration unavailable"].exists)
    }

    /// Opt in only after signing the dedicated local fixture into this exact
    /// simulator. No credentials are passed through XCTest or its activity log.
    @MainActor func testLocalChatSendAndRetainedDraft() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        try replaceSyntheticText(composer, with: "Reply with exactly: Maple local smoke passed.", in: app)
        app.buttons["chat.send"].tap()
        let sent = NSPredicate { _, _ in
            app.descendants(matching: .any).matching(
                NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
            ).count > 0
                && !app.buttons["chat.stop"].exists
        }
        expectation(for: sent, evaluatedWith: app)
        waitForExpectations(timeout: 90)
        XCTAssertFalse(app.staticTexts["chat.error"].exists)
        composer.tap()
        composer.typeText("A retained local draft")
        app.buttons["chat.menu"].tap()
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 10))
        app.buttons["library.close"].tap()
        XCTAssertEqual(composer.value as? String, "A retained local draft")
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Local chat and retained draft"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }

    @MainActor func testLocalReplyAudioSurvivesNavigationAndCloses() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        try replaceSyntheticText(
            composer,
            with:
                "Reply with exactly: Maple can read this synthetic local test reply aloud. The player keeps its current reply when the menu opens and closes.",
            in: app)
        app.buttons["chat.send"].tap()
        let speaker = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "message.speak.")).firstMatch
        let completed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in speaker.exists && speaker.isEnabled && !app.buttons["chat.stop"].exists },
            object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [completed], timeout: 90), .completed)
        try revealReplyAction(speaker, above: composer, in: app)
        speaker.tap()
        XCTAssertTrue(app.buttons["audio.close"].waitForExistence(timeout: 15))
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in app.buttons["audio.back"].isEnabled }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 60), .completed)
        let transport = app.buttons["audio.play-pause"]
        if transport.label == "Pause reply audio" { transport.tap() }
        app.buttons["audio.back"].tap()
        app.buttons["audio.speed"].tap()
        let speed = app.buttons["1.5×"]
        XCTAssertTrue(speed.waitForExistence(timeout: 5)); speed.tap()
        app.buttons["chat.menu"].tap()
        XCTAssertTrue(app.buttons["audio.close"].waitForExistence(timeout: 10))
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Local reply player in menu"; screenshot.lifetime = .keepAlways; add(screenshot)
        app.buttons["library.close"].tap()
        XCTAssertTrue(app.buttons["audio.close"].waitForExistence(timeout: 10))
        app.buttons["audio.close"].tap()
        XCTAssertFalse(app.buttons["audio.close"].waitForExistence(timeout: 2))
    }

    @MainActor private func revealReplyAction(
        _ action: XCUIElement, above composer: XCUIElement, in app: XCUIApplication
    ) throws {
        for _ in 0..<8 {
            let top = app.navigationBars.firstMatch.frame.maxY + 12
            let bottom = composer.frame.minY - 24
            guard bottom - top >= 90 else { throw NativeTextEntryIssue.unavailable }
            if action.isHittable, action.frame.minY >= top, action.frame.maxY <= bottom { return }
            let origin = app.coordinate(withNormalizedOffset: .zero)
            let x = app.frame.midX
            origin.withOffset(CGVector(dx: x, dy: bottom - 12)).press(
                forDuration: 0.05,
                thenDragTo: origin.withOffset(CGVector(dx: x, dy: top + 12)))
        }
        XCTFail("Reply action did not become visible above the composer")
        throw NativeTextEntryIssue.unavailable
    }

    @MainActor func testLocalExplicitImagePastePreviewAndRemoval() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        composer.tap(); composer.typeText(" Image fixture draft.")
        let draft = composer.value as? String
        let image = UIGraphicsImageRenderer(size: CGSize(width: 800, height: 600)).image { context in
            UIColor.white.setFill(); context.fill(CGRect(x: 0, y: 0, width: 800, height: 600))
            ("Maple synthetic image\nReadable screenshot text 12345" as NSString).draw(
                in: CGRect(x: 40, y: 100, width: 720, height: 400),
                withAttributes: [.font: UIFont.systemFont(ofSize: 36), .foregroundColor: UIColor.black])
        }
        let data = try XCTUnwrap(image.pngData())
        UIPasteboard.general.setItems(
            [["public.png": data]],
            options: [
                .localOnly: true, .expirationDate: Date().addingTimeInterval(120),
            ])
        defer { UIPasteboard.general.items = [] }
        app.buttons["chat.attach"].tap()
        app.buttons["Paste image"].tap()
        XCTAssertTrue(app.buttons["images.paste"].waitForExistence(timeout: 10))
        app.buttons["images.paste"].tap()
        let attached = app.buttons["View attached image"].firstMatch
        XCTAssertTrue(attached.waitForExistence(timeout: 20))
        XCTAssertEqual(composer.value as? String, draft)
        attached.tap()
        XCTAssertTrue(app.buttons["images.close"].waitForExistence(timeout: 10))
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Synthetic pasted image preview"; screenshot.lifetime = .keepAlways; add(screenshot)
        app.pinch(withScale: 2, velocity: 1)
        app.buttons["images.close"].tap()
        let remove = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "images.remove.")).firstMatch
        XCTAssertTrue(remove.waitForExistence(timeout: 5)); remove.tap()
        XCTAssertFalse(attached.waitForExistence(timeout: 2))
        XCTAssertEqual(composer.value as? String, draft)
    }
    @MainActor func testLocalNavigationSettingsAndRetainedDraft() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        composer.tap(); composer.typeText(" Navigation retains this draft.")
        let draft = composer.value as? String
        let settings = try openLocalSettings(in: app)
        let instructions = app.buttons["Default instructions"]
        try revealLocalControl(instructions, owner: settings, in: app)
        instructions.tap()
        let editor = app.textViews["library.instructions-text"]
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        editor.tap(); editor.typeText(" Unsaved local navigation fixture.")
        app.buttons["library.instructions-cancel"].tap()
        XCTAssertTrue(app.buttons["Discard changes"].waitForExistence(timeout: 5))
        app.buttons["Discard changes"].tap()
        XCTAssertFalse(editor.waitForExistence(timeout: 1))
        let screen = XCTAttachment(screenshot: app.screenshot())
        screen.name = "Native local settings"; screen.lifetime = .keepAlways; add(screen)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 10))
        app.buttons["library.close"].tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 5))
        XCTAssertEqual(composer.value as? String, draft)
    }

    @MainActor func testLocalColdLaunchAndForegroundPreserveDistinctDrafts() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        let conversationID = try coldLaunchConversationID()
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        try replaceSyntheticDraft(composer, with: "Saved New Chat draft for cold launch.")
        try openColdLaunchConversation(conversationID, in: app)
        try replaceSyntheticDraft(composer, with: "Existing chat survives foreground.")
        // Dismiss the software keyboard and keep a visible message as a geometry
        // witness across a process-alive background/foreground transition.
        app.scrollViews["chat.transcript"].swipeDown()
        let message = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
        ).firstMatch
        XCTAssertTrue(message.waitForExistence(timeout: 15))
        let visible = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                message.frame.width > 0 && message.frame.height > 0
                    && !message.frame.intersection(app.frame).isEmpty
            }, object: message)
        XCTAssertEqual(XCTWaiter.wait(for: [visible], timeout: 15), .completed)
        // Rich replies may finish laying out bundled equations after becoming visible.
        // Measure a stable native frame before attributing movement to the lifecycle transition.
        var lastFrame = message.frame
        var stableSince = Date()
        let settled = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let frame = message.frame
                guard frame.width > 0, frame.height > 0, !frame.intersection(app.frame).isEmpty else {
                    lastFrame = frame
                    stableSince = Date()
                    return false
                }
                let delta = max(
                    abs(frame.minX - lastFrame.minX), abs(frame.minY - lastFrame.minY),
                    abs(frame.width - lastFrame.width), abs(frame.height - lastFrame.height))
                if delta > 0.5 {
                    lastFrame = frame
                    stableSince = Date()
                }
                return Date().timeIntervalSince(stableSince) >= 2
            }, object: message)
        XCTAssertEqual(XCTWaiter.wait(for: [settled], timeout: 15), .completed)
        let before = message.frame
        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, "Existing chat survives foreground.")
        XCTAssertEqual(message.frame.minY, before.minY, accuracy: 3)
        XCTAssertEqual(message.frame.height, before.height, accuracy: 3)
        app.buttons["chat.menu"].tap()
        app.buttons["library.close"].tap()
        XCTAssertEqual(composer.value as? String, "Existing chat survives foreground.")
        app.terminate(); app.launch()
        let newChatRestored = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                composer.exists && composer.value as? String == "Saved New Chat draft for cold launch."
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [newChatRestored], timeout: 30), .completed)
        XCTAssertEqual(composer.value as? String, "Saved New Chat draft for cold launch.")
        try openColdLaunchConversation(conversationID, in: app)
        let existingChatRestored = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                composer.exists && composer.value as? String == "Existing chat survives foreground."
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [existingChatRestored], timeout: 10), .completed)
        XCTAssertEqual(composer.value as? String, "Existing chat survives foreground.")
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Reopened existing chat retains its distinct draft"
        screenshot.lifetime = .keepAlways; add(screenshot)
    }

    @MainActor private func coldLaunchConversationID() throws -> String {
        struct Fixture: Decodable { let conversationID: String }
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
        let file = support.appendingPathComponent("MapleUITestReferenceChat.json")
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular,
            let size = attributes[.size] as? NSNumber, size.intValue > 0, size.intValue <= 1024
        else { throw NativeTextEntryIssue.unavailable }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: file))
        try FileManager.default.removeItem(at: file)
        guard UUID(uuidString: fixture.conversationID) != nil else { throw NativeTextEntryIssue.unavailable }
        return fixture.conversationID
    }

    @MainActor private func openColdLaunchConversation(_ id: String, in app: XCUIApplication) throws {
        let menu = app.buttons["chat.menu"]
        XCTAssertTrue(menu.waitForExistence(timeout: 10)); XCTAssertTrue(menu.isHittable); menu.tap()
        guard let row = try MenuChatUI.find(id, in: app, pageLimit: 24) else {
            XCTFail("The exact shared rich conversation must be selected; never substitute the latest row")
            throw NativeTextEntryIssue.unavailable
        }
        XCTAssertEqual(row.identifier, "library.chat." + id)
        XCTAssertTrue(row.isHittable); row.tap()
        XCTAssertTrue(app.textViews["chat.composer"].waitForExistence(timeout: 20))
        let message = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
        ).firstMatch
        XCTAssertTrue(message.waitForExistence(timeout: 30))
        XCTAssertFalse(app.staticTexts["chat.error"].exists)
    }

    @MainActor private func replaceSyntheticDraft(_ composer: XCUIElement, with text: String) throws {
        try replaceSyntheticText(composer, with: text, in: XCUIApplication())
    }

    @MainActor func testLocalSignOutClearsTextImagesAndSavedDraft() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        struct Fixture: Decodable { let email: String; let password: String }
        let root = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let file = root.appendingPathComponent("MapleUITestCredentials.json")
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        guard let size = attributes[.size] as? NSNumber, size.intValue < 4096 else {
            XCTFail("Invalid local fixture size"); return
        }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: file))
        try FileManager.default.removeItem(at: file)
        defer { UIPasteboard.general.items = [] }
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        try replaceSyntheticDraft(composer, with: "Private synthetic draft cleared by sign out.")
        let image = UIGraphicsImageRenderer(size: CGSize(width: 200, height: 100)).image { context in
            UIColor.systemBlue.setFill(); context.fill(CGRect(x: 0, y: 0, width: 200, height: 100))
        }
        UIPasteboard.general.setItems(
            [["public.png": try XCTUnwrap(image.pngData())]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
        app.buttons["chat.attach"].tap(); app.buttons["Paste image"].tap()
        XCTAssertTrue(app.buttons["images.paste"].waitForExistence(timeout: 10))
        app.buttons["images.paste"].tap()
        XCTAssertTrue(app.buttons["View attached image"].firstMatch.waitForExistence(timeout: 20))
        UIPasteboard.general.items = []
        let settings = try openLocalSettings(in: app)
        let signOut = app.buttons["settings.sign-out"]
        try revealLocalControl(signOut, owner: settings, in: app)
        signOut.tap()
        let identifier = app.textFields["auth.identifier"]
        XCTAssertTrue(identifier.waitForExistence(timeout: 30))
        XCTAssertFalse(app.alerts.firstMatch.exists, "Ordinary draft sign out must not ask for confirmation")
        try paste(fixture.email, into: identifier, in: app)
        try paste(fixture.password, into: app.secureTextFields["auth.password"], in: app)
        UIPasteboard.general.items = []
        app.buttons["auth.submit"].tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 45))
        XCTAssertEqual(composer.value as? String, "")
        XCTAssertFalse(app.buttons["View attached image"].firstMatch.exists)
        XCTAssertFalse(app.buttons["queue.open"].exists)
        app.terminate(); app.launch()
        XCTAssertTrue(composer.waitForExistence(timeout: 30))
        XCTAssertEqual(composer.value as? String, "")
        XCTAssertFalse(app.buttons["View attached image"].firstMatch.exists)
    }

    @MainActor func testLocalImagePickerCancellationAndUnavailableCameraPreserveDraft() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        try replaceSyntheticDraft(composer, with: "Picker cancellation preserves this draft.")
        let count = app.buttons.matching(identifier: "View attached image").count
        for source in ["Photo library", "Files"] {
            app.buttons["chat.attach"].tap(); app.buttons[source].tap()
            let presentedHierarchy = XCTAttachment(string: app.debugDescription)
            presentedHierarchy.name = "Synthetic image picker after \(source) tap hierarchy"
            presentedHierarchy.lifetime = .keepAlways; add(presentedHierarchy)
            let presentedScreen = XCTAttachment(screenshot: app.screenshot())
            presentedScreen.name = "Synthetic image picker after \(source) tap"
            presentedScreen.lifetime = .keepAlways; add(presentedScreen)
            let cancel = app.buttons["Cancel"].firstMatch
            let cancelAppeared = cancel.waitForExistence(timeout: 15)
            if !cancelAppeared {
                let missingCancelHierarchy = XCTAttachment(string: app.debugDescription)
                missingCancelHierarchy.name = "Synthetic image picker \(source) missing Cancel hierarchy"
                missingCancelHierarchy.lifetime = .keepAlways; add(missingCancelHierarchy)
            }
            XCTAssertTrue(cancelAppeared); cancel.tap()
            XCTAssertTrue(composer.waitForExistence(timeout: 10))
            XCTAssertEqual(composer.value as? String, "Picker cancellation preserves this draft.")
            XCTAssertEqual(app.buttons.matching(identifier: "View attached image").count, count)
            XCTAssertFalse(app.staticTexts["images.error"].exists)
        }
        app.buttons["chat.attach"].tap(); app.buttons["Take photo"].tap()
        XCTAssertTrue(app.staticTexts["images.error"].waitForExistence(timeout: 10))
        XCTAssertEqual(composer.value as? String, "Picker cancellation preserves this draft.")
        XCTAssertEqual(app.buttons.matching(identifier: "View attached image").count, count)
    }

    @MainActor func testLocalImageTenAttachmentLimitAndAtomicElevenRejection() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        try replaceSyntheticDraft(composer, with: "Atomic image limit draft.")
        removeSyntheticAttachments(in: app)
        let image = UIGraphicsImageRenderer(size: CGSize(width: 160, height: 100)).image { context in
            UIColor.white.setFill(); context.fill(CGRect(x: 0, y: 0, width: 160, height: 100))
            ("SYNTHETIC" as NSString).draw(
                at: CGPoint(x: 10, y: 30),
                withAttributes: [.font: UIFont.systemFont(ofSize: 20), .foregroundColor: UIColor.black])
        }
        let data = try XCTUnwrap(image.pngData())
        defer { UIPasteboard.general.items = [] }
        pasteSyntheticImages(Array(repeating: data, count: 11), in: app)
        XCTAssertTrue(app.staticTexts["images.error"].waitForExistence(timeout: 20))
        XCTAssertEqual(app.buttons.matching(identifier: "View attached image").count, 0)
        XCTAssertEqual(composer.value as? String, "Atomic image limit draft.")
        pasteSyntheticImages(Array(repeating: data, count: 10), in: app)
        let imported = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                app.buttons.matching(identifier: "View attached image").count == 10
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [imported], timeout: 30), .completed)
        app.buttons["chat.attach"].tap(); app.buttons["Paste image"].tap()
        XCTAssertTrue(app.staticTexts["images.error"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["images.paste"].exists)
        XCTAssertEqual(app.buttons.matching(identifier: "View attached image").count, 10)
        XCTAssertEqual(composer.value as? String, "Atomic image limit draft.")
        removeSyntheticAttachments(in: app)
    }

    @MainActor func testLocalDenseScreenshotPastePreviewPreservesSource() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let root = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
        let source = root.appendingPathComponent("MapleUITestDenseScreenshot.png")
        let data = try Data(contentsOf: source)
        XCTAssertLessThanOrEqual(data.count, 20 * 1024 * 1024)
        let sourceImage = try XCTUnwrap(UIImage(data: data)?.cgImage)
        XCTAssertEqual(sourceImage.width, 5120); XCTAssertEqual(sourceImage.height, 3200)
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        app.buttons["chat.new"].tap()
        let composer = app.textViews["chat.composer"]
        try replaceSyntheticDraft(composer, with: "Dense screenshot fixture draft.")
        removeSyntheticAttachments(in: app)
        defer { UIPasteboard.general.items = [] }
        pasteSyntheticImages([data], in: app)
        let image = app.buttons["View attached image"].firstMatch
        XCTAssertTrue(image.waitForExistence(timeout: 30)); image.tap()
        XCTAssertTrue(app.buttons["images.close"].waitForExistence(timeout: 10))
        let before = XCTAttachment(screenshot: app.screenshot())
        before.name = "Downscaled dense screenshot fitted preview"; before.lifetime = .keepAlways; add(before)
        app.pinch(withScale: 5, velocity: 1)
        let zoomed = XCTAttachment(screenshot: app.screenshot())
        zoomed.name = "Downscaled dense screenshot zoomed for legibility"; zoomed.lifetime = .keepAlways; add(zoomed)
        app.buttons["images.close"].tap()
        XCTAssertEqual(composer.value as? String, "Dense screenshot fixture draft.")
        XCTAssertEqual(try Data(contentsOf: source), data, "Import and preview must preserve the source file")
        removeSyntheticAttachments(in: app)
    }

    @MainActor private func pasteSyntheticImages(_ images: [Data], in app: XCUIApplication) {
        UIPasteboard.general.setItems(
            images.map { ["public.png": $0] },
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
        app.buttons["chat.attach"].tap(); app.buttons["Paste image"].tap()
        XCTAssertTrue(app.buttons["images.paste"].waitForExistence(timeout: 10))
        app.buttons["images.paste"].tap()
    }

    @MainActor private func removeSyntheticAttachments(in app: XCUIApplication) {
        for _ in 0..<10 {
            let remove = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "images.remove."))
                .firstMatch
            if !remove.exists { break }
            if !remove.isHittable { app.swipeDown() }
            remove.tap()
        }
        XCTAssertEqual(app.buttons.matching(identifier: "View attached image").count, 0)
    }

}
