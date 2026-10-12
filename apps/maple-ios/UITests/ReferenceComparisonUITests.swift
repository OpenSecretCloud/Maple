import Foundation
import UIKit
import XCTest

/// Captures ordinary local UI for side-by-side reference review. This has no
/// product fixture launch flag and never seeds or fabricates a conversation.
/// Screenshot data must be synthetic content belonging to the dedicated account.
@MainActor final class ReferenceComparisonUITests: XCTestCase {
    private struct CaptureInput: Decodable {
        let account: String
        var orientation: String
        var appearance: String
        let deviceClass: String
        let conversationID: String?
        let imageConversationID: String?
        let projectID: String?
        let groups: [CaptureGroup]?
    }
    private struct CaptureGroup: Decodable {
        let group: String
        let orientation: String
        let appearance: String
        let budgetSeconds: Int
    }
    private struct Credentials: Decodable { let email: String; let password: String }
    private var app: XCUIApplication!
    private var input: CaptureInput!
    private var activeGroup: CaptureGroup?
    private var batchAuthenticated = false
    private var verifiedCell: String?
    private var observedAppearance: String?
    private var observedWebPreference: String?
    private var accountVerified = false
    private var menuSelectionsVerified = false
    private var sidebarSelectionWitness: (conversationID: String, messageID: String)?
    private var composer: XCUIElement { app.textViews["chat.composer"] }

    private func readPrivate<T: Decodable>(_ name: String) throws -> T {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let path = support.appendingPathComponent(name)
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular,
            let size = attributes[.size] as? NSNumber, size.intValue <= 8_192
        else { throw ReferenceCaptureIssue.invalidInput }
        let value = try JSONDecoder().decode(T.self, from: Data(contentsOf: path))
        try FileManager.default.removeItem(at: path)
        return value
    }

    private func loadInput() throws {
        if input != nil { return }
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_REFERENCE_UI"] == "1")
        continueAfterFailure = false
        input = try readPrivate("MapleReferenceCapture.json")
        guard ["free", "pro"].contains(input.account), ["portrait", "landscape"].contains(input.orientation),
            ["light", "dark"].contains(input.appearance), ["phone", "tablet"].contains(input.deviceClass)
        else { throw ReferenceCaptureIssue.invalidInput }
    }

    private func start(signedIn: Bool) throws {
        try loadInput()
        if let activeGroup {
            input.orientation = activeGroup.orientation; input.appearance = activeGroup.appearance
        }
        // Authentication is setup for signed-in reference surfaces. Keep that
        // form in portrait, then restore the admitted capture cell before Settings.
        let launchOrientation = signedIn && !batchAuthenticated ? "portrait" : input.orientation
        XCUIDevice.shared.orientation = launchOrientation == "portrait" ? .portrait : .landscapeLeft
        app = XCUIApplication()
        app.launchArguments = ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launch()
        let available = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.textFields["auth.identifier"].exists || self.app.buttons["chat.new"].exists
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [available], timeout: 45), .completed)
        if batchAuthenticated {
            // Never silently reauthenticate, cross accounts, or reuse an anonymous
            // launch after an earlier group retired the admitted account.
            XCTAssertTrue(signedIn); XCTAssertTrue(app.buttons["chat.new"].exists)
            XCTAssertFalse(app.textFields["auth.identifier"].exists)
            try alignCellIfNeeded()
        } else {
            if app.buttons["chat.new"].exists {
                try openSettings(); tap("settings.sign-out")
            }
            XCTAssertTrue(app.textFields["auth.identifier"].waitForExistence(timeout: 30))
            if signedIn {
                let credentials: Credentials = try readPrivate("MapleUITestCredentials.json")
                defer { UIPasteboard.general.items = [] }
                try pasteCredential(credentials.email, field: app.textFields["auth.identifier"])
                try pasteCredential(credentials.password, field: app.secureTextFields["auth.password"])
                UIPasteboard.general.items = []
                tap("auth.submit", in: app.scrollViews.containing(.button, identifier: "auth.submit").firstMatch)
                XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 45))
                XCTAssertFalse(app.staticTexts["auth.error"].exists)
                XCUIDevice.shared.orientation = input.orientation == "portrait" ? .portrait : .landscapeLeft
                try alignPreferences(expectedEmail: credentials.email)
                batchAuthenticated = activeGroup != nil
            }
        }
        assertFullWindow()
    }

    private func assertFullWindow() {
        let short: CGFloat = input.deviceClass == "tablet" ? 834 : 402
        let long: CGFloat = input.deviceClass == "tablet" ? 1_210 : 874
        let expected =
            input.orientation == "portrait"
            ? CGSize(width: short, height: long) : CGSize(width: long, height: short)
        let oriented = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                abs(self.app.frame.width - expected.width) <= 1
                    && abs(self.app.frame.height - expected.height) <= 1
                    && abs(self.app.frame.minX) <= 1 && abs(self.app.frame.minY) <= 1
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [oriented], timeout: 15), .completed)
    }

    /// A completed group receipt records reached UI steps only. The private
    /// runner accepts no group as passed unless the complete XCTest also passes.
    private func groupReceipt(_ phase: String, index: Int, elapsed: TimeInterval? = nil) throws {
        let group = try XCTUnwrap(activeGroup)
        var record: [String: Any] = [
            "kind": "reference-group", "phase": phase, "index": index,
            "key": [input.account, input.deviceClass, group.orientation, group.appearance, group.group].joined(
                separator: "."),
            "scope": "Reached steps; acceptance requires authoritative XCTest success",
        ]
        if let elapsed { record["elapsed_seconds"] = elapsed }
        let data = try JSONSerialization.data(withJSONObject: record, options: .sortedKeys)
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "reference-group.\(index).\(phase)"
        attachment.lifetime = .keepAlways; add(attachment)
    }

    func testCaptureReferenceBatch() throws {
        try loadInput()
        let groups = try XCTUnwrap(input.groups)
        guard !groups.isEmpty, groups.count <= 16 else { throw ReferenceCaptureIssue.invalidInput }
        var keys = Set<String>()
        for group in groups {
            guard ["portrait", "landscape"].contains(group.orientation), ["light", "dark"].contains(group.appearance),
                (90...300).contains(group.budgetSeconds), group.group != "authentication",
                keys.insert([group.orientation, group.appearance, group.group].joined(separator: ".")).inserted
            else { throw ReferenceCaptureIssue.invalidInput }
        }
        for (index, group) in groups.enumerated() {
            activeGroup = group
            try groupReceipt("started", index: index)
            let began = Date()
            switch group.group {
            case "new-chat": try testCaptureNewChatAndKeyboard()
            case "corpus": try testCaptureSharedRichConversation()
            case "reasoning": try testCaptureSharedReasoningDisclosure()
            case "edge-navigation": try testLeadingEdgeMenuAndClosePreserveDraftAndTranscriptPosition()
            case "images": try testCaptureSharedImageConversation()
            case "library": try testCaptureLibraryAndHistory()
            case "project": try testCaptureSharedProjectAndInstructions()
            case "access": try testCaptureReadOnlyModelsAndFeatureAccess()
            case "settings": try testCaptureReadOnlyAccountAndPreferences()
            case "security": try testCaptureSecurityAndDeletionWarnings()
            case "player": try testCaptureManualReplyPlayer()
            default: throw ReferenceCaptureIssue.invalidInput
            }
            // Initial real sign-in and each new cell's Settings visit have
            // separate finite allowances; blocking XCTest itself has a batch cap.
            let previous = index > 0 ? groups[index - 1] : nil
            let changedCell = previous?.orientation != group.orientation || previous?.appearance != group.appearance
            let allowance = group.budgetSeconds + (index == 0 ? 420 : (changedCell ? 180 : 0))
            let elapsed = Date().timeIntervalSince(began)
            XCTAssertLessThanOrEqual(elapsed, Double(allowance))
            try groupReceipt("completed", index: index, elapsed: elapsed)
        }
        // The phone Pro schedule ends in portrait/light. Its real account stays
        // signed in for the already-planned ordinary-root cases that follow.
    }

    private func pasteCredential(_ value: String, field: XCUIElement) throws {
        guard field.waitForExistence(timeout: 15) else { throw ReferenceCaptureIssue.missingControl }
        let identifier = field.identifier
        UIPasteboard.general.setItems(
            [["public.utf8-plain-text": value]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(30)])
        captureCredentialPasteState("published", value: value, field: field, identifier: identifier)
        field.tap()
        field.press(forDuration: 1.2)
        let menu = app.menuItems["Paste"].firstMatch, button = app.buttons["Paste"].firstMatch
        if menu.waitForExistence(timeout: 3) {
            captureCredentialPasteState(
                "before-paste", value: value, field: field, identifier: identifier, paste: menu)
            menu.tap()
            captureCredentialPasteState(
                "after-paste", value: value, field: field, identifier: identifier, paste: menu)
        } else if button.waitForExistence(timeout: 3) {
            captureCredentialPasteState(
                "before-paste", value: value, field: field, identifier: identifier, paste: button)
            button.tap()
            captureCredentialPasteState(
                "after-paste", value: value, field: field, identifier: identifier, paste: button)
        } else if identifier == "auth.identifier" || identifier == "auth.password" {
            // Only use the public paste command when neither edit-menu action
            // is available. Never redeliver after a failed commitment assertion.
            captureCredentialPasteState("before-paste", value: value, field: field, identifier: identifier)
            field.typeKey("v", modifierFlags: .command)
            captureCredentialPasteState("after-paste", value: value, field: field, identifier: identifier)
        } else {
            throw ReferenceCaptureIssue.missingControl
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
    private func captureCredentialPasteState(
        _ phase: String, value: String, field: XCUIElement, identifier: String, paste: XCUIElement? = nil
    ) {
        guard identifier == "auth.identifier" || identifier == "auth.password" else { return }
        let pasteboard = UIPasteboard.general
        var record: [String: Any] = [
            "phase": phase,
            "clipboard": [
                "hasStrings": pasteboard.hasStrings,
                "items": pasteboard.numberOfItems,
                "changeCount": pasteboard.changeCount,
            ],
        ]
        let fieldExists = field.exists
        let fieldValue = fieldExists ? field.value : nil
        record["field"] = [
            "identifier": identifier,
            "exists": fieldExists,
            "hittable": fieldExists && field.isHittable,
            "valueIsString": fieldValue is String,
            "fixtureMatches": fieldValue as? String == value,
        ]
        if let paste {
            let pasteExists = paste.exists
            record["paste"] = [
                "exists": pasteExists,
                "hittable": pasteExists && paste.isHittable,
                "enabled": pasteExists && paste.isEnabled,
            ]
        }
        guard let data = try? JSONSerialization.data(withJSONObject: record, options: .sortedKeys) else { return }
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "credential-paste.\(identifier).\(phase)"
        attachment.lifetime = .keepAlways; add(attachment)
    }
    private func pasteSynthetic(_ value: String) throws {
        try replaceSyntheticText(composer, with: "", in: app)
        defer { UIPasteboard.general.items = [] }
        try pasteCredential(value, field: composer)
        let matches = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                Array((self.composer.value as? String ?? "").utf8) == Array(value.utf8)
            }, object: composer)
        XCTAssertEqual(XCTWaiter.wait(for: [matches], timeout: 10), .completed)
    }
    private func visibleRegion(of element: XCUIElement) -> CGRect {
        guard element.exists else { return .null }
        return element.frame.intersection(app.frame)
    }
    private func hasVisibleContent(_ collection: XCUIElement, prefix: String? = nil) -> Bool {
        let region = visibleRegion(of: collection)
        guard region.width >= 100, region.height >= 100 else { return false }
        let descendants = collection.descendants(matching: .any)
        let candidates =
            prefix.map {
                descendants.matching(NSPredicate(format: "identifier BEGINSWITH %@", $0))
            } ?? descendants
        return candidates.allElementsBoundByIndex.contains {
            $0.exists && $0.isHittable && !$0.frame.intersection(region).isEmpty
        }
    }
    private func visibleCollection() -> XCUIElement? {
        app.collectionViews.allElementsBoundByIndex.first { hasVisibleContent($0) }
    }
    private func visibleLibrary() -> XCUIElement? {
        let sidebar = app.collectionViews["library.sidebar-list"]
        return hasVisibleContent(sidebar, prefix: "library.") ? sidebar : nil
    }
    private func keyboardIsVisible() -> Bool {
        let buttons = app.keyboards.buttons.allElementsBoundByIndex
        guard buttons.count <= 160 else {
            XCTFail("Keyboard control inventory exceeds the bounded visibility check")
            return true
        }
        let visibleKey = buttons.contains {
            $0.exists && $0.isHittable && !$0.frame.intersection(app.frame).isEmpty
        }
        let hide = app.buttons["Hide keyboard"].firstMatch
        return visibleKey || (hide.exists && hide.isHittable)
    }
    private func composerKeyboardGeometry() -> (clear: Bool, frames: [CGRect], scalars: [String: Double]) {
        let bounds = app.frame, editor = composer, send = app.buttons["chat.send"].firstMatch
        let buttons = app.keyboards.buttons.allElementsBoundByIndex
        if buttons.count > 160 {
            XCTFail("Keyboard control inventory exceeds the bounded visibility check")
        }
        let keyboardVisible =
            buttons.count <= 160
            && buttons.contains {
                $0.exists && $0.isHittable && !$0.frame.intersection(bounds).isEmpty
            }
        let keyboardCount = app.keyboards.count, keyboardElement = app.keyboards.firstMatch
        let keyboard =
            keyboardVisible && keyboardCount == 1 && keyboardElement.exists
            ? keyboardElement.frame.intersection(bounds) : .null
        let editorExists = editor.exists, sendExists = send.exists
        let editorHittable = editorExists && editor.isHittable, sendHittable = sendExists && send.isHittable
        let input = editorExists ? editor.frame : .null, action = sendExists ? send.frame : .null
        let frames = [bounds, input, action, keyboard]
        let finite = frames.allSatisfy {
            [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                && $0.width > 0 && $0.height > 0
        }
        let targetsHittable = editorHittable && sendHittable
        let minimumSize = input.width >= 44 && input.height >= 44 && action.width >= 44 && action.height >= 44
        let contained = finite && bounds.contains(input) && bounds.contains(action)
        let aboveKeyboard = finite && input.maxY <= keyboard.minY && action.maxY <= keyboard.minY
        let clear =
            keyboardVisible && keyboardCount == 1 && finite
            && targetsHittable && minimumSize && contained && aboveKeyboard
        var scalars: [String: Double] = [
            "keyboardVisible": keyboardVisible ? 1 : 0, "keyboardButtonCount": Double(buttons.count),
            "keyboardContainerCount": Double(keyboardCount), "finiteFrames": finite ? 1 : 0,
            "composerExists": editorExists ? 1 : 0, "composerHittable": editorHittable ? 1 : 0,
            "sendExists": sendExists ? 1 : 0, "sendHittable": sendHittable ? 1 : 0,
            "minimum44Targets": minimumSize ? 1 : 0, "containedInWindow": contained ? 1 : 0,
            "aboveKeyboard": aboveKeyboard ? 1 : 0, "clear": clear ? 1 : 0,
            "composerClearance": Double(keyboard.minY - input.maxY),
            "sendClearance": Double(keyboard.minY - action.maxY),
        ]
        for (name, frame) in zip(["window", "composer", "send", "keyboard"], frames) {
            scalars[name + "X"] = Double(frame.minX); scalars[name + "Y"] = Double(frame.minY)
            scalars[name + "Width"] = Double(frame.width); scalars[name + "Height"] = Double(frame.height)
        }
        return (clear, frames, scalars.mapValues { $0.isFinite ? $0 : -Double.greatestFiniteMagnitude })
    }
    private func sameKeyboardGeometry(_ previous: [CGRect], _ current: [CGRect]) -> Bool {
        guard previous.count == current.count else { return false }
        return zip(previous, current).allSatisfy { before, after in
            [
                before.minX - after.minX, before.minY - after.minY, before.width - after.width,
                before.height - after.height,
            ]
            .allSatisfy { abs($0) <= 0.5 }
        }
    }
    private func dismissTabletKeyboard() {
        guard input.deviceClass == "tablet", keyboardIsVisible() else { return }
        // This public system control was observed on the owned iPad. Its keyboard
        // AX container can remain after dismissal, so require absent visible keys.
        let hide = app.buttons["Hide keyboard"].firstMatch
        XCTAssertTrue(hide.waitForExistence(timeout: 5)); XCTAssertTrue(hide.isHittable)
        hide.tap()
        let hidden = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in !self.keyboardIsVisible() }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 10), .completed)
    }
    private func scroll(
        _ directionDown: Bool, in explicit: XCUIElement? = nil, toward target: XCUIElement? = nil
    ) {
        // Keep the same list owner throughout a reveal. A visible iPad sidebar
        // can report isHittable=false on its container after the first swipe.
        // Never redirect a Library/Form gesture into the adjacent transcript.
        let container = explicit ?? visibleCollection()
        guard let container, hasVisibleContent(container) else {
            XCTFail("The owning list has no visible interactive content")
            return
        }
        var region = visibleRegion(of: container).insetBy(dx: 16, dy: 24)
        if keyboardIsVisible() {
            for keyboard in app.keyboards.allElementsBoundByIndex where keyboard.exists {
                let overlap = keyboard.frame.intersection(region)
                if !overlap.isEmpty { region.size.height = max(0, overlap.minY - region.minY - 16) }
            }
        }
        guard region.width >= 44, region.height >= 100 else {
            XCTFail("The owning list has no unobstructed scroll region")
            return
        }
        // Reveal takes shorter steps and brings a known clipped target toward
        // the visible region's center. Direct capture scrolling keeps its span.
        let inset: CGFloat = target == nil ? 0.15 : 0.325
        var dragDown = directionDown
        if let target, target.exists {
            let frame = target.frame
            if !frame.isEmpty && !frame.isNull && !frame.isInfinite && frame.midY.isFinite {
                dragDown = frame.midY < region.midY
            }
        }
        let top = CGPoint(x: region.midX, y: region.minY + region.height * inset)
        let bottom = CGPoint(x: region.midX, y: region.maxY - region.height * inset)
        let origin = app.coordinate(withNormalizedOffset: .zero)
        func coordinate(_ point: CGPoint) -> XCUICoordinate {
            origin.withOffset(CGVector(dx: point.x - app.frame.minX, dy: point.y - app.frame.minY))
        }
        coordinate(dragDown ? top : bottom).press(
            forDuration: 0.05, thenDragTo: coordinate(dragDown ? bottom : top))
    }
    private func reveal(
        _ element: XCUIElement, down: Bool = false, attempts: Int = 8, in explicit: XCUIElement? = nil
    ) {
        var owner = explicit
        for _ in 0..<attempts {
            if element.exists && element.isHittable { break }
            if owner == nil { owner = visibleCollection() }
            guard let owner else { XCTFail("No owning list is available"); return }
            scroll(down, in: owner, toward: element)
        }
        XCTAssertTrue(element.exists); XCTAssertTrue(element.isHittable)
    }
    private func tap(
        _ id: String, down: Bool = false, attempts: Int = 8, in owner: XCUIElement? = nil
    ) {
        let button = id == "chat.menu" ? app.mapleMenuButton : app.buttons[id].firstMatch
        reveal(button, down: down, attempts: attempts, in: owner)
        XCTAssertTrue(button.isEnabled); button.tap()
    }
    private func tapSelectionMenuCommand(_ label: String) throws {
        guard label == "Select chats" || label == "Done selecting" else {
            throw ReferenceCaptureIssue.missingControl
        }
        // A menu command belongs to the popover, not the scrollable Library.
        // Resolve both public menu roles without dragging the background list.
        let commands = app.descendants(matching: .any).matching(
            NSPredicate(
                format: "label == %@ AND (elementType == %d OR elementType == %d)", label,
                XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.menuItem.rawValue))
        func visibleCommand() -> XCUIElement? {
            let candidates = commands.allElementsBoundByIndex
            guard candidates.count <= 8 else { return nil }
            let visible = candidates.filter {
                guard $0.exists, $0.isEnabled, $0.isHittable else { return false }
                let frame = $0.frame
                return [frame.minX, frame.minY, frame.width, frame.height].allSatisfy { $0.isFinite }
                    && frame.width > 0 && frame.height > 0 && app.frame.contains(frame)
            }
            return visible.count == 1 ? visible[0] : nil
        }
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in visibleCommand() != nil }, object: app)
        let result = XCTWaiter.wait(for: [ready], timeout: 5)
        let command = visibleCommand()
        let record = XCTAttachment(
            string:
                "ready=\(result == .completed); candidates=\(min(commands.count, 9)); role=\(command?.elementType.rawValue ?? 0)"
        )
        record.name = "Library selection menu command"; record.lifetime = .keepAlways; add(record)
        XCTAssertEqual(result, .completed); XCTAssertLessThanOrEqual(commands.count, 8)
        guard result == .completed, let command else { throw ReferenceCaptureIssue.missingControl }
        command.tap()
    }
    @discardableResult private func openLibrary() throws -> XCUIElement {
        dismissTabletKeyboard()
        if let library = visibleLibrary() { return library }
        tap("chat.menu")
        let visible = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in self.visibleLibrary() != nil }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [visible], timeout: 20), .completed)
        return try XCTUnwrap(visibleLibrary())
    }
    private func openSettings() throws {
        _ = try openLibrary()
        // Settings is an explicit action in the principal Maple menu.
        tap("library.actions", attempts: 24, in: app)
        tap("library.settings", attempts: 24, in: app)
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 20))
    }
    private func returnFromModelPicker() {
        XCTAssertTrue(app.collectionViews["settings.models"].exists)
        let back = app.navigationBars.buttons.matching(
            NSPredicate(format: "label == %@ OR label == %@", "Settings", "Back")
        ).firstMatch
        XCTAssertTrue(back.exists); XCTAssertTrue(back.isHittable); back.tap()
    }
    private func closeSettings() {
        if app.buttons["library.settings-close"].exists {
            tap("library.settings-close")
        } else {
            let back = app.navigationBars["Settings"].buttons.firstMatch
            XCTAssertTrue(back.isHittable); back.tap()
        }
        if app.buttons["library.close"].exists { tap("library.close") }
    }
    private var cellKey: String { "\(input.orientation).\(input.appearance)" }

    private func verifyPlan() {
        reveal(app.staticTexts["Current plan"], down: true)
        XCTAssertTrue(app.staticTexts[input.account == "pro" ? "Pro" : "Free"].exists)
    }

    private func selectAndVerifyAppearance() {
        tap("settings.theme"); tap(input.appearance == "light" ? "Light" : "Dark")
        let expected = input.appearance == "light" ? "Light" : "Dark"
        let control = app.buttons["settings.theme"]
        let selected = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                control.value as? String == expected || control.label == "Appearance, " + expected
            }, object: control)
        XCTAssertEqual(XCTWaiter.wait(for: [selected], timeout: 10), .completed)
        observedAppearance = input.appearance
    }

    private func verifyWebPreference() {
        let web = app.switches["settings.web-search"]
        reveal(web)
        if web.isEnabled && web.value as? String == "1" { turnWebPreferenceOff(web) }
        let value = web.value as? String
        if input.account == "pro" {
            XCTAssertTrue(web.isEnabled); XCTAssertEqual(value, "0")
        } else {
            // A confirmed Free account cannot change this disabled preference.
            // Record its real saved state and authority without fabricating off.
            XCTAssertFalse(web.isEnabled); XCTAssertTrue(value == "0" || value == "1")
        }
        observedWebPreference = value
    }

    private func turnWebPreferenceOff(_ web: XCUIElement) {
        guard let tap = tapWebPreference(web) else { return }
        let off = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in web.exists && web.isEnabled && web.value as? String == "0" },
            object: web)
        let result = XCTWaiter.wait(for: [off], timeout: 5)
        recordWebTarget(
            "after", web: web, window: app.windows.firstMatch.frame, point: tap.point, strategy: tap.strategy)
        XCTAssertEqual(result, .completed)
    }

    @discardableResult
    private func tapWebPreference(_ web: XCUIElement) -> (point: CGPoint, strategy: String)? {
        XCTAssertTrue(web.isEnabled); XCTAssertTrue(web.isHittable)
        let window = app.windows.firstMatch.frame
        let row = web.frame
        guard row.width >= 160, row.height >= 28, row.height <= 120,
            [row.minX, row.minY, row.width, row.height].allSatisfy({ $0.isFinite })
        else { XCTFail("The Web search control has unexpected geometry"); return nil }
        let nativeSwitches = web.switches.allElementsBoundByIndex.filter {
            $0.exists && $0.isHittable && $0.frame.width >= 20 && $0.frame.width <= 100
                && $0.frame.height >= 20 && $0.frame.height <= 100 && row.contains($0.frame)
        }
        guard nativeSwitches.count <= 1 else {
            XCTFail("The Web search control has ambiguous native switches"); return nil
        }
        let target: XCUICoordinate
        let point: CGPoint
        let strategy: String
        if let native = nativeSwitches.first {
            let frame = native.frame
            point = CGPoint(x: frame.midX, y: frame.midY)
            target = native.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            strategy = "unique-native-switch"
        } else {
            // This reference suite launches English/LTR. On the observed wide
            // Settings and picker rows, XCTest's default center tap misses the trailing
            // switch. Derive one control-region point from its fresh frame.
            point = CGPoint(x: row.maxX - 48, y: row.midY)
            target = web.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5))
                .withOffset(CGVector(dx: -48, dy: 0))
            strategy = "observed-trailing-region"
        }
        guard window.contains(point), row.contains(point) else {
            XCTFail("The Web search target is outside its current window or row"); return nil
        }
        recordWebTarget("before", web: web, window: window, point: point, strategy: strategy)
        target.tap()
        // Exactly one tap; no second-point fallback, setting injection, or retry.
        return (point, strategy)
    }

    private func recordWebTarget(
        _ phase: String, web: XCUIElement, window: CGRect, point: CGPoint, strategy: String
    ) {
        let record = XCTAttachment(
            string: "strategy=\(strategy); window=\(window); row=\(web.frame); "
                + "point=(\(point.x),\(point.y)); enabled=\(web.isEnabled); value=\(web.value as? String ?? "nil")")
        record.name = "reference.web-preference." + phase
        record.lifetime = .keepAlways; add(record)
    }

    private func alignPreferences(expectedEmail: String) throws {
        try openSettings()
        // Compare in memory without putting the expected email in an XCUI query,
        // assertion message, receipt or command argument.
        let sameAccount = app.staticTexts.allElementsBoundByIndex.contains { $0.label == expectedEmail }
        XCTAssertTrue(sameAccount, "The visible account must match the admitted dedicated fixture")
        accountVerified = sameAccount
        verifyPlan()
        selectAndVerifyAppearance()
        tap("settings.font"); tap("Manrope")
        tap("Default model"); tap("settings.model.auto:quick")
        returnFromModelPicker()
        verifyWebPreference()
        try alignPlaybackSpeed()
        verifiedCell = cellKey
        closeSettings()
    }

    private func alignPlaybackSpeed() throws {
        let forms = app.collectionViews.matching(identifier: "settings.screen")
        XCTAssertEqual(forms.count, 1)
        let form = forms.firstMatch
        guard forms.count == 1, form.exists else { throw ReferenceCaptureIssue.missingControl }
        let controls = form.buttons.matching(identifier: "settings.speed")
        let speed = controls.firstMatch
        func rowIsClear() -> Bool {
            let window = app.windows.firstMatch, navigation = app.navigationBars["Settings"].firstMatch
            guard controls.count == 1, form.exists, window.exists, navigation.exists,
                speed.exists, speed.isEnabled, speed.isHittable
            else { return false }
            let frames = [app.frame, window.frame, form.frame, navigation.frame, speed.frame]
            guard
                frames.allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                })
            else { return false }
            let region = frames[0].intersection(frames[1]).intersection(frames[2])
            return !region.isNull && !region.isEmpty && region.contains(frames[4])
                && frames[4].minY >= frames[3].maxY + 8
                && frames[4].maxY <= region.maxY - 8
        }
        // Retain the original eight-step row reveal; a hittable clipped row
        // alone does not prove that a tap can open its native picker.
        for _ in 0..<8 {
            if rowIsClear() { break }
            scroll(false, in: form, toward: speed)
        }
        // SwiftUI may create this offscreen row only after its Form is scrolled.
        XCTAssertEqual(controls.count, 1)
        let clear = rowIsClear()
        XCTAssertTrue(clear, "Playback speed must clear the navigation and window boundaries")
        guard clear else { throw ReferenceCaptureIssue.missingControl }
        func selected() -> Bool {
            speed.exists && (speed.value as? String == "1×" || speed.label == "Playback speed, 1×")
        }
        if !selected() {
            speed.tap()
            // This option belongs to the picker opened by the exact owned row;
            // never scroll the background Form to reveal a popup command.
            let options = app.descendants(matching: .any).matching(
                NSPredicate(
                    format: "label == %@ AND (elementType == %d OR elementType == %d)", "1×",
                    XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.menuItem.rawValue))
            func visibleOption() -> XCUIElement? {
                let candidates = options.allElementsBoundByIndex
                guard candidates.count <= 8 else { return nil }
                let visible = candidates.filter {
                    guard $0.exists, $0.isEnabled, $0.isHittable else { return false }
                    let frame = $0.frame
                    return [frame.minX, frame.minY, frame.width, frame.height].allSatisfy { $0.isFinite }
                        && frame.width > 0 && frame.height > 0 && app.frame.contains(frame)
                }
                return visible.count == 1 ? visible[0] : nil
            }
            let ready = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in visibleOption() != nil }, object: app)
            let result = XCTWaiter.wait(for: [ready], timeout: 5)
            XCTAssertEqual(result, .completed)
            guard result == .completed, let option = visibleOption() else {
                throw ReferenceCaptureIssue.missingControl
            }
            option.tap()
        }
        // An already-selected row supplies its readback without a new wait.
        if rowIsClear() && selected() { return }
        let readback = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in rowIsClear() && selected() }, object: speed)
        let result = XCTWaiter.wait(for: [readback], timeout: 10)
        XCTAssertEqual(result, .completed)
        guard result == .completed, selected() else { throw ReferenceCaptureIssue.missingControl }
    }

    private func alignCellIfNeeded() throws {
        guard verifiedCell != cellKey else { return }
        try openSettings(); verifyPlan(); selectAndVerifyAppearance(); verifyWebPreference()
        verifiedCell = cellKey; closeSettings()
    }

    private func capture(_ surface: String) {
        XCTAssertFalse(app.staticTexts["Configuration unavailable"].exists)
        assertFullWindow()
        let prefix = "native.\(input.account).\(input.deviceClass).\(input.orientation).\(input.appearance).\(surface)"
        let keyboardVisible = keyboardIsVisible()
        if surface == "new-chat-keyboard-shown" { XCTAssertTrue(keyboardVisible) }
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        screenshot.name = prefix; screenshot.lifetime = .keepAlways; add(screenshot)
        let metadata: [String: Any] = [
            "surface": surface, "account_alias": input.account as String,
            "device_class": input.deviceClass as String, "orientation": input.orientation as String,
            "appearance": input.appearance as String, "width_points": app.frame.width,
            "height_points": app.frame.height, "keyboard_visible": keyboardVisible,
            "account_privately_verified": accountVerified,
            "observed_appearance_setting": observedAppearance ?? "signed-out-system-appearance",
            "appearance_evidence": "Native Settings picker value; rendered screenshot requires visual review",
            "observed_web_preference": observedWebPreference ?? "not-applicable",
            "web_authority": !accountVerified
                ? "not-applicable" : (input.account == "free" ? "disabled-confirmed-free" : "eligible-pro-aligned-off"),
        ]
        if let data = try? JSONSerialization.data(withJSONObject: metadata, options: .sortedKeys),
            let text = String(data: data, encoding: .utf8)
        {
            let record = XCTAttachment(string: text)
            record.name = prefix + ".metadata"; record.lifetime = .keepAlways; add(record)
        }
    }

    private func openMenuChats() throws -> XCUIElement {
        let sidebar = try openLibrary()
        XCTAssertTrue(app.textFields["library.search"].waitForExistence(timeout: 20))
        XCTAssertTrue(app.buttons["library.actions"].isHittable)
        XCTAssertFalse(app.buttons["library.history"].exists)
        XCTAssertFalse(app.navigationBars["All chats"].exists)
        return sidebar
    }

    private func assertMenuReturned() {
        XCTAssertTrue(composer.waitForExistence(timeout: 20))
        XCTAssertFalse(app.collectionViews["library.history-list"].exists)
        XCTAssertFalse(app.navigationBars["All chats"].exists)
        XCTAssertFalse(app.buttons["library.close"].exists)
        if input.deviceClass == "phone" {
            let closed = XCTNSPredicateExpectation(
                predicate: NSPredicate(format: "exists == false"),
                object: app.collectionViews["library.sidebar-list"])
            XCTAssertEqual(XCTWaiter.wait(for: [closed], timeout: 10), .completed)
        } else {
            XCTAssertNotNil(visibleLibrary(), "Regular iPad retains the root sidebar after selection")
        }
    }

    private func verifyMenuSelections(_ id: String) throws {
        guard input.deviceClass == "tablet", !menuSelectionsVerified else { return }
        // Once per batch, select the exact shared chat through the existing
        // Projects/Pinned/Recent menu, then verify the distinct New Chat draft.
        // Neither path sends or modifies the retained New Chat draft.
        tap("chat.new")
        XCTAssertTrue(composer.waitForExistence(timeout: 20))
        let messages = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message."))
        let emptyNewChat = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: messages.firstMatch)
        XCTAssertEqual(XCTWaiter.wait(for: [emptyNewChat], timeout: 15), .completed)
        let retainedDraft = try XCTUnwrap(composer.value as? String)
        _ = try openMenuChats()
        let sidebarRow = try XCTUnwrap(
            MenuChatUI.find(
                id, in: app, projectIDs: input.projectID.map { [$0] }, pageLimit: 16))
        sidebarRow.tap(); assertMenuReturned()
        XCTAssertTrue(messages.firstMatch.waitForExistence(timeout: 30))
        let sidebarMessageID = messages.firstMatch.identifier

        let second = try openMenuChats()
        let newChat = second.buttons["library.new-chat"]
        reveal(newChat, down: true, attempts: 16, in: second)
        newChat.tap(); assertMenuReturned()
        XCTAssertFalse(messages.firstMatch.exists)
        XCTAssertTrue(composer.value as? String == retainedDraft, "New Chat draft must survive menu chat selection")
        menuSelectionsVerified = true
        sidebarSelectionWitness = (id, sidebarMessageID)
        let receipt = XCTAttachment(
            string: "sidebar-chat-has-assistant=true; sidebar-new-chat-empty=true; retained-draft=true")
        receipt.name = "reference.menu-chat-and-new-chat-selection"; receipt.lifetime = .keepAlways; add(receipt)
    }

    private func openConversation(_ id: String?) throws {
        guard let id, !id.isEmpty else {
            throw XCTSkip("No explicit shared synthetic conversation ID supplied for this surface")
        }
        try verifyMenuSelections(id)
        _ = try openMenuChats()
        guard
            let row = try MenuChatUI.find(
                id, in: app, projectIDs: input.projectID.map { [$0] }, pageLimit: 16)
        else {
            XCTFail("The explicit shared conversation was not found; refusing to capture another row")
            throw ReferenceCaptureIssue.missingControl
        }
        XCTAssertEqual(row.identifier, "library.chat." + id)
        row.tap()
        assertMenuReturned()
        let messages = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message."))
        XCTAssertTrue(messages.firstMatch.waitForExistence(timeout: 30))
        if let witness = sidebarSelectionWitness, witness.conversationID == id {
            XCTAssertTrue(messages.matching(identifier: witness.messageID).firstMatch.waitForExistence(timeout: 30))
        }
        XCTAssertFalse(app.staticTexts["chat.error"].exists)
    }

    func testCaptureAuthenticationSurfaces() throws {
        try start(signedIn: false)
        capture("entry-email")
        tap("Use an existing guest account")
        let guestReady = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.textFields["auth.identifier"].exists
                    && self.app.textFields["auth.identifier"].placeholderValue == "Guest account ID"
                    && self.app.buttons["Sign in with email"].exists
                    && !self.app.buttons["auth.forgot"].exists
                    && !self.app.buttons["auth.apple"].exists
                    && !self.app.buttons["auth.google"].exists
                    && !self.app.buttons["auth.github"].exists
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [guestReady], timeout: 10), .completed)
        XCTAssertEqual(app.textFields["auth.identifier"].placeholderValue, "Guest account ID")
        capture("existing-guest-sign-in")
        tap("Sign in with email")
        let emailReady = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.textFields["auth.identifier"].exists
                    && self.app.textFields["auth.identifier"].placeholderValue == "Email"
                    && self.app.buttons["auth.forgot"].exists && self.app.buttons["auth.forgot"].isEnabled
                    && self.app.buttons["Create an account"].exists
                    && self.app.buttons["Create an account"].isEnabled
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [emailReady], timeout: 10), .completed)
        tap("Create an account")
        XCTAssertTrue(app.textFields["Name (optional)"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.textFields["Name (optional)"].exists)
        capture("email-signup")
        tap("Already have an account? Sign in")
        let returnedEmailReady = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.textFields["auth.identifier"].exists
                    && self.app.textFields["auth.identifier"].placeholderValue == "Email"
                    && self.app.buttons["auth.forgot"].exists && self.app.buttons["auth.forgot"].isEnabled
                    && self.app.buttons["Create an account"].exists
                    && self.app.buttons["Create an account"].isEnabled
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [returnedEmailReady], timeout: 10), .completed)
        tap("auth.forgot")
        XCTAssertTrue(app.textFields["auth.reset-email"].waitForExistence(timeout: 15))
        capture("password-reset-request")
        // No signup, reset request, provider request or deletion is submitted.
        app.terminate()
    }

    func testCaptureNewChatAndKeyboard() throws {
        try start(signedIn: true); tap("chat.new")
        dismissTabletKeyboard()
        func hiddenWelcomeFrames() -> [CGRect]? {
            let window = app.windows.firstMatch, transcript = app.scrollViews["chat.transcript"]
            let navigation = app.mapleChatNavigationBar
            guard window.exists, transcript.exists, navigation.exists, composer.exists,
                composer.isHittable, !keyboardIsVisible()
            else { return nil }
            let bounds = window.frame, content = transcript.frame, bar = navigation.frame, editor = composer.frame
            let frames = [bounds, content, bar, editor]
            guard
                frames.allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                }), bounds.contains(editor)
            else { return nil }
            let region = content.intersection(bounds)
            let candidates = transcript.descendants(matching: .any).matching(
                NSPredicate(
                    format: "label CONTAINS %@ AND label CONTAINS %@", "Research anything...",
                    "Encrypted and private at every step")
            ).allElementsBoundByIndex
            guard candidates.count <= 8 else {
                XCTFail("New Chat welcome inventory exceeds its bounded check"); return nil
            }
            let visible = candidates.filter {
                $0.exists && $0.isHittable && !$0.frame.intersection(region).isEmpty
            }
            guard visible.count == 1 else { return nil }
            let welcome = visible[0].frame
            guard [welcome.minX, welcome.minY, welcome.width, welcome.height].allSatisfy({ $0.isFinite }),
                welcome.width > 0, welcome.height > 0, region.contains(welcome),
                welcome.minY >= bar.maxY, welcome.maxY <= editor.minY
            else { return nil }
            return [bounds, content, bar, welcome, editor]
        }
        var hiddenFrames: [CGRect]?, hiddenSince: TimeInterval?, hiddenObservations = 0
        let hidden = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard let frames = hiddenWelcomeFrames() else {
                    hiddenFrames = nil; hiddenSince = nil; hiddenObservations = 0
                    return false
                }
                let now = ProcessInfo.processInfo.systemUptime
                if let hiddenFrames, self.sameKeyboardGeometry(hiddenFrames, frames) {
                    hiddenObservations += 1
                } else {
                    hiddenFrames = frames; hiddenSince = now; hiddenObservations = 1
                }
                return hiddenObservations >= 2 && now - (hiddenSince ?? now) >= 0.25
            }, object: app)
        let hiddenResult = XCTWaiter.wait(for: [hidden], timeout: 15)
        let finalHiddenFrames = hiddenWelcomeFrames()
        let finalHiddenStable =
            finalHiddenFrames.map { frames in
                hiddenFrames.map { sameKeyboardGeometry($0, frames) } ?? false
            } ?? false
        var hiddenGeometry: [String: Double] = [
            "waitCompleted": hiddenResult == .completed ? 1 : 0,
            "welcomeReady": finalHiddenFrames != nil ? 1 : 0,
            "stableObservations": Double(hiddenObservations), "finalGeometryStable": finalHiddenStable ? 1 : 0,
        ]
        for (name, frame) in zip(
            ["window", "transcript", "navigation", "welcome", "composer"], finalHiddenFrames ?? [])
        {
            hiddenGeometry[name + "X"] = Double(frame.minX); hiddenGeometry[name + "Y"] = Double(frame.minY)
            hiddenGeometry[name + "Width"] = Double(frame.width); hiddenGeometry[name + "Height"] = Double(frame.height)
        }
        let hiddenData = try JSONSerialization.data(withJSONObject: hiddenGeometry, options: .sortedKeys)
        let hiddenRecord = XCTAttachment(string: String(decoding: hiddenData, as: UTF8.self))
        hiddenRecord.name = "New Chat hidden welcome geometry"; hiddenRecord.lifetime = .keepAlways; add(hiddenRecord)
        XCTAssertEqual(hiddenResult, .completed)
        XCTAssertTrue(finalHiddenFrames != nil, "The full welcome must fit between navigation and composer")
        XCTAssertTrue(finalHiddenStable, "Hidden welcome geometry must remain settled before capture")
        guard hiddenResult == .completed, finalHiddenFrames != nil, finalHiddenStable else {
            throw ReferenceCaptureIssue.missingControl
        }
        XCTAssertFalse(keyboardIsVisible())
        capture("new-chat-keyboard-hidden")
        try pasteSynthetic("Maple comparison — مرحبا 日本語 🌱")
        var stableFrames: [CGRect]?, stableSince: TimeInterval?, stableObservations = 0
        let shown = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let geometry = self.composerKeyboardGeometry()
                guard geometry.clear else {
                    stableFrames = nil; stableSince = nil; stableObservations = 0
                    return false
                }
                let now = ProcessInfo.processInfo.systemUptime
                if let stableFrames, self.sameKeyboardGeometry(stableFrames, geometry.frames) {
                    stableObservations += 1
                } else {
                    stableFrames = geometry.frames; stableSince = now; stableObservations = 1
                }
                return stableObservations >= 2 && now - (stableSince ?? now) >= 0.25
            }, object: app)
        let shownResult = XCTWaiter.wait(for: [shown], timeout: 10)
        let finalGeometry = composerKeyboardGeometry()
        let finalStable = stableFrames.map { sameKeyboardGeometry($0, finalGeometry.frames) } ?? false
        var scalars = finalGeometry.scalars
        scalars["waitCompleted"] = shownResult == .completed ? 1 : 0
        scalars["stableObservations"] = Double(stableObservations)
        scalars["finalGeometryStable"] = finalStable ? 1 : 0
        let data = try JSONSerialization.data(withJSONObject: scalars, options: .sortedKeys)
        let geometryRecord = XCTAttachment(string: String(decoding: data, as: UTF8.self))
        geometryRecord.name = "New Chat keyboard clearance geometry"
        geometryRecord.lifetime = .keepAlways; add(geometryRecord)
        XCTAssertEqual(shownResult, .completed)
        XCTAssertTrue(finalGeometry.clear, "Composer and Send must fit above the visible keyboard")
        XCTAssertTrue(finalStable, "Keyboard and input geometry must remain settled before capture")
        guard shownResult == .completed, finalGeometry.clear, finalStable else {
            throw ReferenceCaptureIssue.missingControl
        }
        capture("new-chat-keyboard-shown")
        app.terminate()
    }

    private func swipeTranscript(
        _ transcript: XCUIElement, down: Bool, velocity: XCUIGestureVelocity, span: CGFloat = 0.5
    ) throws {
        let window = app.windows.firstMatch
        let navigation = app.mapleChatNavigationBar
        let editor = composer
        guard transcript.exists, window.exists, navigation.exists, editor.exists else {
            XCTFail("Transcript scrolling requires its window, navigation and composer")
            throw ReferenceCaptureIssue.missingControl
        }
        let frame = transcript.frame, bounds = window.frame, bar = navigation.frame, input = editor.frame
        guard
            [frame, bounds, bar, input].allSatisfy({
                [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                    && $0.width > 0 && $0.height > 0
            })
        else {
            XCTFail("Transcript scrolling requires finite visible geometry")
            throw ReferenceCaptureIssue.missingControl
        }
        let region = frame.intersection(bounds)
        let top = max(region.minY, bar.maxY), bottom = min(region.maxY, input.minY)
        guard !region.isNull, !region.isEmpty, region.width >= 44, bottom - top >= 44,
            span.isFinite, span > 0, span <= 0.5
        else {
            XCTFail("Transcript scrolling has no unobstructed region between navigation and composer")
            throw ReferenceCaptureIssue.missingControl
        }
        // Stay inside the measured band, away from either excluded boundary.
        // Coordinates remain relative to this transcript, never a fallback app gesture.
        let inset = (bottom - top) * (1 - span) / 2
        let upper = CGPoint(x: region.midX, y: top + inset)
        let lower = CGPoint(x: region.midX, y: bottom - inset)
        let origin = transcript.coordinate(withNormalizedOffset: .zero)
        func coordinate(_ point: CGPoint) -> XCUICoordinate {
            origin.withOffset(CGVector(dx: point.x - frame.minX, dy: point.y - frame.minY))
        }
        coordinate(down ? upper : lower).press(
            forDuration: 0.05, thenDragTo: coordinate(down ? lower : upper),
            withVelocity: velocity, thenHoldForDuration: 0)
    }

    func testCaptureSharedRichConversation() throws {
        try start(signedIn: true); try openConversation(input.conversationID)
        let transcript = app.scrollViews["chat.transcript"]
        for _ in 0..<10 { try swipeTranscript(transcript, down: true, velocity: .fast) }
        capture("rich-conversation-top")
        for page in 1...3 {
            try swipeTranscript(transcript, down: false, velocity: .slow); capture("rich-conversation-page-\(page)")
        }
        for _ in 0..<10 { try swipeTranscript(transcript, down: false, velocity: .fast) }
        capture("rich-conversation-bottom")
        app.terminate()
    }

    // Supplemental pixels only: existing rich workflow acceptance stays unchanged.
    func testCapturePhoneLandscapeRichMiddleSupplement() throws {
        try loadInput()
        guard input.account == "pro", input.deviceClass == "phone",
            input.orientation == "landscape", input.groups == nil
        else { throw ReferenceCaptureIssue.invalidInput }
        try start(signedIn: true)
        let clock = ContinuousClock(), began = clock.now
        func checkWorkBudget() throws {
            guard began.duration(to: clock.now) < .seconds(180) else {
                XCTFail("Landscape rich supplement exceeded its capture-work budget")
                throw ReferenceCaptureIssue.missingControl
            }
        }
        try openConversation(input.conversationID)
        XCTAssertFalse(keyboardIsVisible())
        let transcript = app.scrollViews["chat.transcript"]
        let assistant = transcript.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
        )
        .containing(.button, identifier: "Copy code").firstMatch
        let code = assistant.buttons["Copy code"].firstMatch
        func clearCodeHeader() -> Bool {
            let window = app.windows.firstMatch
            let navigation = app.mapleChatNavigationBar
            guard transcript.exists, window.exists, navigation.exists, composer.exists,
                code.exists, code.isHittable
            else { return false }
            let frames = [transcript.frame, window.frame, navigation.frame, composer.frame, code.frame]
            guard
                frames.allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                })
            else { return false }
            let region = frames[0].intersection(frames[1])
            return !region.isNull && !region.isEmpty && region.contains(frames[4])
                && frames[4].minY >= frames[2].maxY + 8
                && frames[4].maxY <= frames[3].minY - 8
        }
        for _ in 0..<16 {
            if clearCodeHeader() { break }
            try checkWorkBudget()
            let navigation = app.mapleChatNavigationBar
            let towardEarlier =
                code.exists && code.frame.midY.isFinite
                && code.frame.midY < navigation.frame.maxY + 8
            try swipeTranscript(transcript, down: towardEarlier, velocity: .slow)
        }
        try checkWorkBudget()
        let headerIsClear = clearCodeHeader()
        XCTAssertTrue(headerIsClear, "Assistant code header must clear navigation and composer")
        guard headerIsClear else { throw ReferenceCaptureIssue.missingControl }
        capture("rich-supplement-code-anchor")
        // The table precedes code in this already-qualified synthetic corpus.
        // Overlapping real frames cover earlier rows and the response below code;
        // names deliberately make no automatic table/math/Web visibility claim.
        for page in 1...2 {
            try checkWorkBudget()
            try swipeTranscript(transcript, down: true, velocity: .slow)
            capture("rich-supplement-earlier-\(page)")
        }
        for page in 1...13 {
            try checkWorkBudget()
            try swipeTranscript(transcript, down: false, velocity: .slow)
            capture("rich-supplement-later-\(page)")
        }
        try checkWorkBudget()
        app.terminate()
    }

    func testCaptureSharedReasoningDisclosure() throws {
        try start(signedIn: true); try openConversation(input.conversationID)
        let transcript = app.scrollViews["chat.transcript"]
        for _ in 0..<10 { transcript.swipeDown(velocity: .fast) }
        let thinking = app.buttons["Thinking"].firstMatch
        for _ in 0..<16 where !thinking.isHittable { transcript.swipeUp(velocity: .slow) }
        XCTAssertTrue(thinking.exists); XCTAssertTrue(thinking.isHittable)
        capture("reasoning-collapsed")
        thinking.tap(); capture("reasoning-expanded")
        // The paired screenshots establish expansion and content layout; merely
        // tapping this real disclosure is not an automatic visual-parity pass.
        app.terminate()
    }

    func testLeadingEdgeMenuAndClosePreserveDraftAndTranscriptPosition() throws {
        try start(signedIn: true)
        try XCTSkipUnless(input.deviceClass == "phone", "Leading-edge menu applies to compact navigation")
        try openConversation(input.conversationID)
        try replaceSyntheticText(composer, with: "Unsent edge-navigation comparison draft", in: app)
        // Dismiss through Menu before measuring the transcript anchor.
        let keyboard = app.keyboards.firstMatch
        XCTAssertTrue(keyboard.exists)
        let keys = keyboard.keys.allElementsBoundByIndex + keyboard.buttons.allElementsBoundByIndex
        XCTAssertLessThanOrEqual(keys.count, 160)
        XCTAssertTrue(keys.contains { $0.exists && $0.isHittable && $0.frame.intersects(app.frame) })
        tap("chat.menu")
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        try assertPhysicalLeftDrawer()
        let hidden = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in !self.app.keyboards.firstMatch.exists }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 10), .completed)
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        tap("library.close")
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, "Unsent edge-navigation comparison draft")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        let transcript = app.scrollViews["chat.transcript"]
        // Retain an actual visible message container rather than infer position
        // from a scroll-view offset that XCTest does not expose.
        let messages = transcript.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
        ).allElementsBoundByIndex
        let visible = messages.first {
            $0.frame.intersects(transcript.frame) && $0.frame.width > 40 && $0.frame.height > 20
        }
        let anchor = try XCTUnwrap(visible)
        let identifier = anchor.identifier, originalY = anchor.frame.minY
        let originalHeight = anchor.frame.height
        let start = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: app.frame.minX + 4, dy: transcript.frame.midY))
        let end = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: app.frame.minX + app.frame.width * 0.65, dy: transcript.frame.midY))
        start.press(forDuration: 0.1, thenDragTo: end, withVelocity: .fast, thenHoldForDuration: 0)
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        try assertPhysicalLeftDrawer()
        capture("leading-edge-menu")
        // The full-screen menu lives behind this retained chat. Its X returns
        // to the same runtime, draft and visible transcript position.
        tap("library.close")
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, "Unsent edge-navigation comparison draft")
        let afterEdgeClose = transcript.descendants(matching: .any).matching(identifier: identifier).firstMatch
        XCTAssertTrue(afterEdgeClose.exists)
        XCTAssertEqual(afterEdgeClose.frame.minY, originalY, accuracy: 3)
        XCTAssertEqual(afterEdgeClose.frame.height, originalHeight, accuracy: 3)
        // A slow short pull held before release has no flick momentum.
        // Native navigation must settle back without opening the menu.
        let originalX = afterEdgeClose.frame.minX
        let cancelledStart = app.coordinate(withNormalizedOffset: .zero).withOffset(
            CGVector(dx: app.frame.minX + 4, dy: transcript.frame.midY))
        let cancelledEnd = cancelledStart.withOffset(CGVector(dx: 44, dy: 0))
        cancelledStart.press(
            forDuration: 0.1, thenDragTo: cancelledEnd,
            withVelocity: .slow, thenHoldForDuration: 0.3)
        let afterCancelledPreview = transcript.descendants(matching: .any).matching(identifier: identifier).firstMatch
        let settled = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard !self.app.buttons["library.close"].exists,
                    !self.app.descendants(matching: .any).matching(identifier: "library.drawer").firstMatch.exists,
                    self.composer.exists, self.composer.isHittable,
                    self.composer.value as? String == "Unsent edge-navigation comparison draft",
                    afterCancelledPreview.exists
                else { return false }
                let frame = afterCancelledPreview.frame
                return abs(frame.minX - originalX) <= 3 && abs(frame.minY - originalY) <= 3
                    && abs(frame.height - originalHeight) <= 3
            }, object: app)
        let settledResult = XCTWaiter.wait(for: [settled], timeout: 10)
        let cancelledFrame = afterCancelledPreview.exists ? afterCancelledPreview.frame : .zero
        let cancellationGeometry: [String: Double] = [
            "waitCompleted": settledResult == .completed ? 1 : 0,
            "originalAnchorX": Double(originalX), "originalAnchorY": Double(originalY),
            "originalAnchorHeight": Double(originalHeight),
            "returnedAnchorX": Double(cancelledFrame.minX), "returnedAnchorY": Double(cancelledFrame.minY),
            "returnedAnchorHeight": Double(cancelledFrame.height),
            "closeExists": app.buttons["library.close"].exists ? 1 : 0,
            "drawerExists": app.descendants(matching: .any).matching(identifier: "library.drawer").firstMatch.exists
                ? 1 : 0,
            "composerExists": composer.exists ? 1 : 0, "composerHittable": composer.isHittable ? 1 : 0,
        ].mapValues { $0.isFinite ? $0 : -Double.greatestFiniteMagnitude }
        let cancellationData = try JSONSerialization.data(withJSONObject: cancellationGeometry, options: .sortedKeys)
        let cancellationRecord = XCTAttachment(string: String(decoding: cancellationData, as: UTF8.self))
        cancellationRecord.name = "Physical-left cancelled preview geometry"
        cancellationRecord.lifetime = .keepAlways; add(cancellationRecord)
        // Compare query identity and bounded geometry without recording message content.
        func elementGeometry(_ element: XCUIElement) -> [String: Any] {
            let exists = element.exists
            let frame = exists ? element.frame : .zero
            let finite = [frame.minX, frame.minY, frame.width, frame.height].allSatisfy { $0.isFinite }
            let canQueryHittability = exists && finite && frame.width > 0 && frame.height > 0
            return [
                "exists": exists, "type": element.elementType.rawValue,
                "x": frame.minX.isFinite ? Double(frame.minX) : -Double.greatestFiniteMagnitude,
                "y": frame.minY.isFinite ? Double(frame.minY) : -Double.greatestFiniteMagnitude,
                "width": frame.width.isFinite ? Double(frame.width) : -Double.greatestFiniteMagnitude,
                "height": frame.height.isFinite ? Double(frame.height) : -Double.greatestFiniteMagnitude,
                "hittabilityQueried": canQueryHittability,
                "hittable": canQueryHittability ? element.isHittable : false,
            ]
        }
        func candidateGeometry(_ query: XCUIElementQuery) -> [String: Any] {
            let count = query.count
            let leafTypes = [
                XCUIElement.ElementType.staticText.rawValue, XCUIElement.ElementType.textView.rawValue,
                XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.image.rawValue,
                XCUIElement.ElementType.link.rawValue,
            ]
            let candidates: [[String: Any]] = (0..<min(count, 8)).map { index in
                let candidate = query.element(boundBy: index)
                var record = elementGeometry(candidate)
                record["index"] = index
                if index < 2 {
                    let leaves = candidate.descendants(matching: .any).matching(
                        NSPredicate(format: "elementType IN %@", leafTypes))
                    let leafCount = leaves.count
                    record["leafCount"] = leafCount
                    record["leaves"] = (0..<min(leafCount, 8)).map { leafIndex in
                        var leaf = elementGeometry(leaves.element(boundBy: leafIndex))
                        leaf["index"] = leafIndex
                        return leaf
                    }
                }
                return record
            }
            return ["count": count, "candidates": candidates]
        }
        let identityGeometry: [String: Any] = [
            "heldOriginalAnchor": elementGeometry(anchor),
            "heldAfterEdgeClose": elementGeometry(afterEdgeClose),
            "freshAfterCancelledPreview": elementGeometry(afterCancelledPreview),
            "appMatches": candidateGeometry(app.descendants(matching: .any).matching(identifier: identifier)),
            "transcriptMatches": candidateGeometry(
                transcript.descendants(matching: .any).matching(identifier: identifier)),
        ]
        let identityData = try JSONSerialization.data(withJSONObject: identityGeometry, options: .sortedKeys)
        let identityRecord = XCTAttachment(string: String(decoding: identityData, as: UTF8.self))
        identityRecord.name = "Physical-left cancelled preview query identity geometry"
        identityRecord.lifetime = .keepAlways; add(identityRecord)
        XCTAssertEqual(settledResult, .completed)
        XCTAssertFalse(app.buttons["library.close"].exists)
        XCTAssertFalse(app.descendants(matching: .any).matching(identifier: "library.drawer").firstMatch.exists)
        XCTAssertTrue(composer.exists); XCTAssertTrue(composer.isHittable)
        XCTAssertEqual(composer.value as? String, "Unsent edge-navigation comparison draft")
        XCTAssertTrue(afterCancelledPreview.exists)
        XCTAssertEqual(afterCancelledPreview.frame.minX, originalX, accuracy: 3)
        XCTAssertEqual(afterCancelledPreview.frame.minY, originalY, accuracy: 3)
        XCTAssertEqual(afterCancelledPreview.frame.height, originalHeight, accuracy: 3)
        capture("leading-edge-cancel-retained-position")
        tap("chat.menu")
        XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
        try assertPhysicalLeftDrawer()
        tap("library.close")
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        XCTAssertEqual(composer.value as? String, "Unsent edge-navigation comparison draft")
        let returned = transcript.descendants(matching: .any).matching(identifier: identifier).firstMatch
        XCTAssertTrue(returned.exists)
        XCTAssertEqual(returned.frame.minY, originalY, accuracy: 3)
        XCTAssertEqual(returned.frame.height, originalHeight, accuracy: 3)
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        capture("leading-edge-return-retained-position")
        app.terminate()
    }

    private func assertPhysicalLeftDrawer() throws {
        let candidates = app.descendants(matching: .any).matching(identifier: "library.drawer")
        let drawer = candidates.firstMatch
        let window = app.windows.firstMatch
        let positioned = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard drawer.exists, window.exists else { return false }
                let frame = drawer.frame, bounds = window.frame
                guard
                    [
                        frame.minX, frame.minY, frame.width, frame.height,
                        bounds.minX, bounds.minY, bounds.width, bounds.height,
                    ].allSatisfy({ $0.isFinite }),
                    frame.width > 200, frame.height > 0, bounds.width > 0, bounds.height > 0
                else { return false }
                return abs(frame.minX - bounds.minX) <= 1 && abs(frame.maxX - bounds.maxX) <= 1
                    && abs(frame.minY - bounds.minY) <= 1 && abs(frame.maxY - bounds.maxY) <= 1
                    && abs(frame.width - bounds.width) <= 1 && abs(frame.height - bounds.height) <= 1
            }, object: drawer)
        let result = XCTWaiter.wait(for: [positioned], timeout: 10)
        let count = candidates.count
        let typed = app.otherElements.matching(identifier: "library.drawer")
        var geometry: [String: Double] = [
            "waitCompleted": result == .completed ? 1 : 0,
            "candidateCount": Double(min(count, 9)), "candidateLimitExceeded": count > 8 ? 1 : 0,
            "typedOtherCount": Double(min(typed.count, 9)),
        ]
        for (name, element) in [
            ("typedOther", typed.firstMatch), ("drawer", drawer), ("window", window),
            ("scrim", app.buttons["library.scrim"]),
            ("alertDrawer", app.alerts["library.drawer"]), ("sheetDrawer", app.sheets["library.drawer"]),
            ("sidebar", app.collectionViews["library.sidebar-list"]),
            ("close", app.buttons["library.close"]),
            ("searchField", app.textFields["library.search"]),
            ("menuActions", app.buttons["library.actions"]),
        ] {
            let exists = element.exists
            let frame = exists ? element.frame : .zero
            geometry[name + "Exists"] = exists ? 1 : 0
            geometry[name + "Hittable"] = exists && element.isHittable ? 1 : 0
            geometry[name + "Type"] = exists ? Double(element.elementType.rawValue) : 0
            geometry[name + "X"] = Double(frame.minX)
            geometry[name + "Y"] = Double(frame.minY)
            geometry[name + "Width"] = Double(frame.width)
            geometry[name + "Height"] = Double(frame.height)
        }
        geometry = geometry.mapValues { $0.isFinite ? $0 : -Double.greatestFiniteMagnitude }
        let data = try JSONSerialization.data(withJSONObject: geometry, options: .sortedKeys)
        let record = XCTAttachment(string: String(decoding: data, as: UTF8.self))
        record.name = "Physical-left drawer geometry"; record.lifetime = .keepAlways; add(record)
        XCTAssertLessThanOrEqual(count, 8)
        XCTAssertEqual(count, 1, "The full-screen menu must have one accessibility owner")
        XCTAssertEqual(typed.count, 1)
        XCTAssertEqual(drawer.elementType, .other, "The menu must remain scene navigation")
        XCTAssertEqual(result, .completed)
        XCTAssertFalse(app.alerts["library.drawer"].exists)
        XCTAssertFalse(app.sheets["library.drawer"].exists)
        XCTAssertFalse(app.buttons["library.scrim"].exists, "The menu fills the screen without an exposed chat strip")
        XCTAssertFalse(composer.exists && composer.isHittable, "The offscreen chat must not accept input")
        let close = app.buttons["library.close"]
        XCTAssertTrue(close.exists); XCTAssertTrue(close.isHittable)
        XCTAssertGreaterThanOrEqual(close.frame.width, 43.5)
        XCTAssertGreaterThanOrEqual(close.frame.height, 43.5)
        XCTAssertTrue(window.frame.insetBy(dx: -0.5, dy: -0.5).contains(close.frame))
    }

    func testCaptureSharedImageConversation() throws {
        try start(signedIn: true); try openConversation(input.imageConversationID)
        let image = app.buttons["View message image"].firstMatch
        let transcript = app.scrollViews["chat.transcript"]
        let window = app.windows.firstMatch
        let navigation = app.mapleChatNavigationBar
        func imageIsVisible() -> Bool {
            guard image.exists, image.isEnabled, image.isHittable,
                transcript.exists, window.exists, navigation.exists, composer.exists
            else { return false }
            let frames = [image.frame, transcript.frame, window.frame, navigation.frame, composer.frame]
            guard
                frames.allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                })
            else { return false }
            let region = transcript.frame.intersection(window.frame)
            let top = max(region.minY, navigation.frame.maxY + 12)
            let bottom = min(region.maxY, composer.frame.minY - 24)
            let target = CGRect(x: image.frame.midX - 22, y: image.frame.midY - 22, width: 44, height: 44)
            return !region.isNull && !region.isEmpty && image.frame.contains(target)
                && region.contains(target) && target.minY >= top && target.maxY <= bottom
        }
        // A retained thumbnail can report hittable even while under the composer.
        // Seek earlier content only while the thumbnail is absent or above it.
        for _ in 0..<12 where !imageIsVisible() {
            if image.exists && image.frame.midY >= composer.frame.minY - 24 { break }
            try swipeTranscript(transcript, down: true, velocity: .slow)
        }
        // The shared image can belong to a later turn. If the earlier-content
        // search reaches the top, seek later content within the same transcript.
        for _ in 0..<12 where !imageIsVisible() {
            try swipeTranscript(transcript, down: false, velocity: .slow)
        }
        XCTAssertTrue(image.waitForExistence(timeout: 15)); XCTAssertTrue(image.isHittable)
        let visible = imageIsVisible()
        let record = XCTAttachment(
            string:
                "visible=\(visible); image=\(image.frame); window=\(window.frame); navigation=\(navigation.frame); composer=\(composer.frame)"
        )
        record.name = "Image preview tap clearance"; record.lifetime = .keepAlways; add(record)
        XCTAssertTrue(visible, "The image tap target must clear navigation and composer")
        guard visible else { throw ReferenceCaptureIssue.missingControl }
        capture("image-message")
        guard imageIsVisible() else { throw ReferenceCaptureIssue.missingControl }
        image.tap(); XCTAssertTrue(app.buttons["images.close"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.images["Attached image"].waitForExistence(timeout: 20))
        XCTAssertFalse(app.staticTexts["Image unavailable"].exists)
        capture("image-fullscreen")
        tap("images.close"); app.terminate()
    }

    func testCaptureLibraryAndHistory() throws {
        try start(signedIn: true); let library = try openLibrary()
        capture("library")
        XCTAssertTrue(app.textFields["library.search"].exists)
        XCTAssertTrue(app.buttons["library.actions"].isHittable)
        XCTAssertFalse(app.buttons["library.history"].exists)
        XCTAssertFalse(app.navigationBars["All chats"].exists)
        XCTAssertFalse(app.navigationBars["Maple"].exists, "The logo remains without a separate Maple title")
        let expectsEmptyLibrary = input.account == "free" && input.conversationID == nil && input.projectID == nil
        if input.deviceClass == "phone" {
            if let projectID = input.projectID {
                try MenuChatUI.expand(projectID, in: app, pageLimit: 24)
            }
            let window = app.windows.firstMatch
            let row = library.buttons.matching(
                NSPredicate(format: "identifier BEGINSWITH %@", "library.chat.")
            ).firstMatch
            let contentWitness = expectsEmptyLibrary ? library.buttons["library.new-chat"] : row
            if !expectsEmptyLibrary {
                // SwiftUI List materializes offscreen rows after scrolling.
                reveal(row, attempts: 24, in: library)
                XCTAssertTrue(row.waitForExistence(timeout: 20))
            }
            let expanded = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in
                    guard library.exists, window.exists, contentWitness.exists, contentWitness.isHittable else {
                        return false
                    }
                    if expectsEmptyLibrary {
                        guard library.staticTexts["Pinned"].exists, library.staticTexts["Recent"].exists,
                            library.buttons["library.new-project"].exists, !row.exists,
                            !self.app.buttons["library.clear-search"].exists,
                            !self.app.buttons["library.pinned.load-more"].exists,
                            !self.app.buttons["library.recent.load-more"].exists,
                            !self.app.staticTexts["library.error"].exists,
                            !library.activityIndicators.allElementsBoundByIndex.contains(where: { $0.exists })
                        else { return false }
                    }
                    let frame = library.frame, bounds = window.frame
                    return frame.width > 0 && frame.height > 0 && contentWitness.frame.intersects(frame)
                        && abs(frame.width - bounds.width) <= 1
                        && abs(frame.minX - bounds.minX) <= 1 && abs(frame.maxX - bounds.maxX) <= 1
                }, object: library)
            let result = XCTWaiter.wait(for: [expanded], timeout: 10)
            var geometry: [String: Double] = ["waitCompleted": result == .completed ? 1 : 0]
            for (name, element) in [
                ("menuList", library), ("drawer", app.otherElements["library.drawer"]),
                ("window", window), ("scrim", app.buttons["library.scrim"]),
            ] {
                let exists = element.exists
                let frame = exists ? element.frame : .zero
                geometry[name + "Exists"] = exists ? 1 : 0
                geometry[name + "Hittable"] = exists && element.isHittable ? 1 : 0
                geometry[name + "X"] = Double(frame.minX)
                geometry[name + "Y"] = Double(frame.minY)
                geometry[name + "Width"] = Double(frame.width)
                geometry[name + "Height"] = Double(frame.height)
            }
            geometry = geometry.mapValues { $0.isFinite ? $0 : -Double.greatestFiniteMagnitude }
            let data = try JSONSerialization.data(withJSONObject: geometry, options: .sortedKeys)
            let record = XCTAttachment(string: String(decoding: data, as: UTF8.self))
            record.name = "Menu chat-list geometry"; record.lifetime = .keepAlways; add(record)
            XCTAssertEqual(result, .completed)
            XCTAssertFalse(app.buttons["library.scrim"].exists)
        }
        capture("menu-chat-list")
        tap("library.actions"); try tapSelectionMenuCommand("Select chats")
        let move = app.buttons["Move selected chats"].firstMatch
        let delete = app.buttons["Delete selected chats"].firstMatch
        XCTAssertTrue(move.exists); XCTAssertFalse(move.isEnabled)
        XCTAssertTrue(delete.exists); XCTAssertFalse(delete.isEnabled)
        capture("menu-selection")
        tap("library.actions"); try tapSelectionMenuCommand("Done selecting")
        XCTAssertFalse(move.exists); XCTAssertFalse(delete.exists)
        app.terminate()
    }

    func testCaptureSharedProjectAndInstructions() throws {
        try start(signedIn: true)
        guard let id = input.projectID else { throw XCTSkip("No explicit shared synthetic project ID supplied") }
        let library = try openLibrary()
        let project = app.buttons.matching(identifier: "library.project." + id).firstMatch
        reveal(project, down: true, attempts: 24, in: library)
        project.press(forDuration: 1); tap("library.project.manage." + id)
        capture("project")
        tap("Project instructions")
        XCTAssertTrue(app.textViews["library.instructions-text"].waitForExistence(timeout: 20))
        capture("project-instructions")
        tap("library.instructions-cancel"); app.terminate()
    }

    func testCaptureReadOnlyModelsAndFeatureAccess() throws {
        try start(signedIn: true); tap("chat.new")
        try replaceSyntheticText(composer, with: "Unsent comparison draft", in: app)
        composer.tap()
        tap("chat.model")
        XCTAssertTrue(app.buttons["settings.model.auto:quick"].waitForExistence(timeout: 20))
        let web = app.switches["model.web-search"]
        XCTAssertTrue(web.waitForExistence(timeout: 20))
        XCTAssertEqual(web.value as? String, observedWebPreference)
        XCTAssertEqual(web.isEnabled, input.account == "pro")
        XCTAssertFalse(app.buttons["chat.web-search"].exists)
        capture("model-selection")
        if input.account == "free" {
            XCTAssertEqual(app.buttons["model.web-search-unavailable"].value as? String, "Unavailable")
            tap("model.web-search-unavailable")
            XCTAssertTrue(app.staticTexts["media.access.message"].waitForExistence(timeout: 15))
            capture("web-access"); tap("media.access.close")
            XCTAssertTrue(web.exists); XCTAssertFalse(web.isEnabled)
            XCTAssertEqual(web.value as? String, observedWebPreference)
        } else {
            let original = try XCTUnwrap(observedWebPreference)
            XCTAssertTrue(original == "0" || original == "1")
            let changed = original == "1" ? "0" : "1"
            tapWebPreference(web)
            let flipped = XCTNSPredicateExpectation(
                predicate: NSPredicate(format: "value == %@", changed), object: web)
            XCTAssertEqual(XCTWaiter.wait(for: [flipped], timeout: 10), .completed)
            tap("Done")
            XCTAssertEqual(composer.value as? String, "Unsent comparison draft")
            composer.tap()
            tap("chat.model")
            let reopened = app.switches["model.web-search"]
            XCTAssertTrue(reopened.waitForExistence(timeout: 20)); XCTAssertTrue(reopened.isEnabled)
            XCTAssertEqual(reopened.value as? String, changed, "The picker must read back the saved preference")
            tapWebPreference(reopened)
            let restored = XCTNSPredicateExpectation(
                predicate: NSPredicate(format: "value == %@", original), object: reopened)
            XCTAssertEqual(XCTWaiter.wait(for: [restored], timeout: 10), .completed)
        }
        tap("Done")
        XCTAssertEqual(composer.value as? String, "Unsent comparison draft")
        if input.account == "free" {
            for (control, surface) in [("dictation.start", "dictation-access")] {
                XCTAssertEqual(app.buttons[control].value as? String, "Unavailable")
                tap(control)
                XCTAssertTrue(app.staticTexts["media.access.message"].waitForExistence(timeout: 15))
                capture(surface); tap("media.access.close")
                XCTAssertEqual(composer.value as? String, "Unsent comparison draft")
            }
            XCTAssertEqual(app.buttons["chat.attach"].value as? String, "Unavailable")
            tap("chat.attach"); tap("Photo library")
            XCTAssertTrue(app.staticTexts["media.access.message"].waitForExistence(timeout: 15))
            capture("image-access"); tap("media.access.close")
            XCTAssertEqual(composer.value as? String, "Unsent comparison draft")
        }
        app.terminate()
    }

    func testCaptureReadOnlyAccountAndPreferences() throws {
        try start(signedIn: true); try openSettings()
        let forms = app.collectionViews.matching(identifier: "settings.screen")
        XCTAssertEqual(forms.count, 1)
        let form = forms.firstMatch
        XCTAssertTrue(form.exists)
        guard forms.count == 1, form.exists else { throw ReferenceCaptureIssue.missingControl }
        func tapSettingsRow(_ id: String, down: Bool = false) {
            let button = form.buttons[id].firstMatch
            reveal(button, down: down, in: form)
            XCTAssertTrue(button.isEnabled); button.tap()
        }
        for _ in 0..<5 { scroll(true, in: form) }
        capture("settings-account-plan")
        let voice = form.buttons["settings.voice"].firstMatch
        let speed = form.buttons["settings.speed"].firstMatch
        let notes = form.staticTexts.matching(
            NSPredicate(
                format: "label == %@",
                "Text size follows your system settings. Autoplay is off each time Maple launches."))
        let note = notes.firstMatch
        func preferencesAreVisible() -> Bool {
            let window = app.windows.firstMatch, navigation = app.navigationBars["Settings"].firstMatch
            guard forms.count == 1, form.exists, window.exists, navigation.exists,
                voice.exists, voice.isHittable, speed.exists, speed.isHittable,
                notes.count == 1, note.exists
            else { return false }
            let bounds = app.frame, windowFrame = window.frame, formFrame = form.frame
            let bar = navigation.frame, voiceFrame = voice.frame, speedFrame = speed.frame, noteFrame = note.frame
            guard
                [bounds, windowFrame, formFrame, bar, voiceFrame, speedFrame, noteFrame].allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                })
            else { return false }
            let region = formFrame.intersection(bounds).intersection(windowFrame)
            let top = max(region.minY, bar.maxY)
            guard !region.isNull, !region.isInfinite, region.width > 0, region.maxY > top else { return false }
            let visible = CGRect(x: region.minX, y: top, width: region.width, height: region.maxY - top)
            return [voiceFrame, speedFrame, noteFrame].allSatisfy { visible.contains($0) }
        }
        // Keep the original reveal allowance, with the named Form as its owner.
        // A hittable Voice row alone cannot qualify the lower Preferences content.
        for _ in 0..<8 {
            if preferencesAreVisible() { break }
            scroll(false, in: form, toward: note)
        }
        XCTAssertTrue(voice.exists); XCTAssertTrue(voice.isHittable)
        let preferencesVisible = preferencesAreVisible()
        XCTAssertTrue(preferencesVisible, "Voice, playback speed and their explanatory note must be fully visible")
        guard preferencesVisible else { throw ReferenceCaptureIssue.missingControl }
        capture("settings-preferences")
        tapSettingsRow("settings.theme", down: true); capture("settings-appearance-options")
        tap(input.appearance == "light" ? "Light" : "Dark")
        tapSettingsRow("Default model"); capture("settings-models")
        returnFromModelPicker()
        tapSettingsRow("Default instructions")
        XCTAssertTrue(app.textViews["library.instructions-text"].waitForExistence(timeout: 20))
        capture("account-instructions"); tap("library.instructions-cancel")
        reveal(form.buttons["Contact support"].firstMatch, in: form); capture("settings-about")
        app.terminate()
    }

    func testCaptureSecurityAndDeletionWarnings() throws {
        try start(signedIn: true); try openSettings()
        tap("Change password")
        XCTAssertTrue(app.secureTextFields["auth.current-password"].waitForExistence(timeout: 15))
        capture("change-password"); tap("Done")
        tap("Reset password")
        XCTAssertTrue(app.textFields["auth.reset-email"].waitForExistence(timeout: 15))
        capture("signed-in-password-reset"); tap("Done")
        tap("settings.delete-history")
        XCTAssertTrue(app.buttons["Delete all chats"].waitForExistence(timeout: 10))
        capture("delete-history-warning")
        if app.buttons["Cancel"].exists {
            tap("Cancel")
        } else {
            // Confirmation popovers can omit Cancel. Use the leading app
            // margin, and never place a dismissal tap on the destructive action.
            let window = app.windows.firstMatch, warning = app.buttons["Delete all chats"].firstMatch
            guard window.exists, warning.exists else { throw ReferenceCaptureIssue.missingControl }
            let bounds = window.frame, appBounds = app.frame, action = warning.frame
            let point = CGPoint(x: appBounds.minX + appBounds.width * 0.02, y: appBounds.minY + appBounds.height * 0.5)
            guard
                [bounds, appBounds, action].allSatisfy({
                    [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                        && $0.width > 0 && $0.height > 0
                }), point.x.isFinite, point.y.isFinite, bounds.contains(point), !action.contains(point)
            else {
                XCTFail("Deletion warning dismissal requires a safe point outside the destructive action")
                throw ReferenceCaptureIssue.missingControl
            }
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.02, dy: 0.5)).tap()
        }
        let dismissed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in !self.app.buttons["Delete all chats"].exists }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 10), .completed)
        tap("settings.delete-account")
        XCTAssertTrue(app.textFields["auth.delete-confirmation"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["auth.request-delete"].isEnabled)
        capture("delete-account-warning"); tap("Done")
        // No deletion phrase, reset confirmation, or destructive action is submitted.
        app.terminate()
    }

    func testCaptureManualReplyPlayer() throws {
        try start(signedIn: true)
        try XCTSkipUnless(input.account == "pro", "Read aloud requires the dedicated eligible Pro fixture")
        try openConversation(input.conversationID)
        let speaker = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "message.speak.")).firstMatch
        let transcript = app.scrollViews["chat.transcript"]
        for _ in 0..<12 {
            let top = app.navigationBars.firstMatch.frame.maxY + 12, bottom = composer.frame.minY - 24
            if speaker.isHittable && speaker.frame.minY >= top && speaker.frame.maxY <= bottom { break }
            if speaker.frame.maxY > bottom {
                transcript.swipeUp(velocity: .slow)
            } else {
                transcript.swipeDown(velocity: .slow)
            }
        }
        XCTAssertGreaterThanOrEqual(speaker.frame.minY, app.navigationBars.firstMatch.frame.maxY + 12)
        XCTAssertLessThanOrEqual(speaker.frame.maxY, composer.frame.minY - 24)
        XCTAssertTrue(speaker.isHittable); XCTAssertTrue(speaker.isEnabled); speaker.tap()
        XCTAssertTrue(app.buttons["audio.close"].waitForExistence(timeout: 15))
        capture("player-preparing-or-ready")
        let playable = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "enabled == true"), object: app.buttons["audio.back"])
        XCTAssertEqual(XCTWaiter.wait(for: [playable], timeout: 90), .completed)
        if app.buttons["audio.play-pause"].label == "Pause reply audio" { tap("audio.play-pause") }
        capture("player-paused")
        try openLibrary(); capture("player-in-library")
        tap("audio.close"); app.terminate()
    }
}

private enum ReferenceCaptureIssue: Error { case invalidInput, missingControl }
