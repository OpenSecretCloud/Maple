import XCTest

@MainActor
final class RendererUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launchFixture() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture"]
        app.launch()
        XCTAssertTrue(
            app.textFields["renderer.paste"].waitForExistence(timeout: 15)
                || app.textViews["renderer.paste"].waitForExistence(timeout: 5))
    }

    func testHostedMessageEquationEnrichmentRetainsLifecycleOwnership() {
        runHostedMessageEquationLifecycle(attachedWindow: false)
    }

    func testHostedMessageEquationEnrichmentWithOwnedWindowAttachment() {
        runHostedMessageEquationLifecycle(attachedWindow: true)
    }

    func testColdProductionMessageViewPublishesSixEquations() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-renderer-fixture", "--maple-renderer-lifecycle", "--maple-renderer-cold-equations",
        ]
        app.launch()
        let state = app.staticTexts["renderer.cold-state"]
        XCTAssertTrue(state.waitForExistence(timeout: 15))
        let complete = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Published equations: 6; ink: 6"), object: state)
        let result = XCTWaiter.wait(for: [complete], timeout: 30)
        let diagnostic = XCTAttachment(string: state.label)
        diagnostic.name = "Cold production MessageView equation publication"
        diagnostic.lifetime = .keepAlways; add(diagnostic)
        let pixels = XCTAttachment(screenshot: app.screenshot())
        pixels.name = "Cold production MessageView six equation pixels"
        pixels.lifetime = .keepAlways; add(pixels)
        XCTAssertEqual(result, .completed)
    }

    private func runHostedMessageEquationLifecycle(attachedWindow: Bool) {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-renderer-lifecycle"]
        if attachedWindow { app.launchArguments.append("--maple-renderer-owned-window") }
        // Both legacy selectors now exercise the same production-owned host.
        var attachment = [
            "windowHostRequested": 1, "windowHostFound": 1, "windowHostAttached": 1, "windowHostDetached": 1,
        ]
        attachment["navigationDeadlineMilliseconds"] = 12_000
        app.launch()
        let state = app.staticTexts["renderer.lifecycle-state"]
        XCTAssertTrue(state.waitForExistence(timeout: 15))
        waitForMessageLifecycle(
            [
                "shown": 1, "prepared": 1, "currentSource": 1, "currentGeneration": 1,
                "appeared": 1, "disappeared": 0, "sources": 2, "published": 0,
                "waiting": 1, "entered": 1, "cancelled": 0, "started": 0, "failed": 0,
            ], phase: "prepared before equation release")
        // Copy code is created only by the actual prepared MessageView branch.
        XCTAssertTrue(app.buttons["Copy code"].exists)
        XCTAssertTrue(app.buttons["Copy code"].isHittable)
        app.buttons["renderer.lifecycle-release"].tap()
        waitForMessageLifecycle(
            [
                "published": 2, "waiting": 0, "released": 1, "cancelled": 0,
                "started": 1, "completed": 1, "attachments": 2, "ink": 2, "failed": 0,
                "appeared": 1, "disappeared": 0, "currentSource": 1, "currentGeneration": 1,
            ].merging(attachment) { _, new in new }, phase: "actual equation attachments published", timeout: 30)

        app.buttons["renderer.lifecycle-revise"].tap()
        waitForMessageLifecycle(
            [
                "published": 0, "waiting": 1, "entered": 2, "started": 1, "currentSource": 1,
            ], phase: "pending old generation")
        app.buttons["renderer.lifecycle-revoke"].tap()
        waitForMessageLifecycle(
            [
                "published": 0, "waiting": 1, "entered": 3, "cancelled": 1, "started": 1,
                "currentSource": 1, "currentGeneration": 1, "appeared": 1, "disappeared": 0,
            ], phase: "generation revoked pending equation work")
        app.buttons["renderer.lifecycle-release"].tap()
        waitForMessageLifecycle(
            [
                "published": 2, "waiting": 0, "released": 2, "cancelled": 1,
                "started": 2, "completed": 2, "attachments": 2, "ink": 2, "failed": 0,
                "currentSource": 1, "currentGeneration": 1, "appeared": 1, "disappeared": 0,
            ].merging(attachment) { _, new in new }, phase: "replacement generation published", timeout: 30)

        app.buttons["renderer.lifecycle-revise"].tap()
        waitForMessageLifecycle(
            [
                "published": 0, "waiting": 1, "entered": 4, "started": 2, "currentSource": 1,
            ], phase: "pending before actual removal")
        app.buttons["renderer.lifecycle-remove"].tap()
        waitForMessageLifecycle(
            [
                "shown": 0, "prepared": 0, "published": 0, "waiting": 0, "cancelled": 2,
                "started": 2, "completed": 2, "appeared": 1, "disappeared": 1, "failed": 0,
            ], phase: "actual disappearance cancelled pending work")
    }

    private func waitForMessageLifecycle(_ expected: [String: Int], phase: String, timeout: TimeInterval = 10) {
        let state = app.staticTexts["renderer.lifecycle-state"]
        let ready = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard let data = state.label.data(using: .utf8),
                    let values = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                else { return false }
                return expected.allSatisfy { (values[$0.key] as? NSNumber)?.intValue == $0.value }
            }, object: state)
        let result = XCTWaiter.wait(for: [ready], timeout: timeout)
        let diagnostic = XCTAttachment(string: state.label)
        diagnostic.name = "Synthetic MessageView lifecycle: " + phase
        diagnostic.lifetime = .keepAlways
        add(diagnostic)
        let pixels = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        pixels.name = "Hosted MessageView: " + phase
        pixels.lifetime = .keepAlways
        add(pixels)
        XCTAssertEqual(result, .completed, phase)
    }

    func testOfflineEquationPixelsAndContainment() {
        launchFixture()
        app.buttons["renderer.equation-checks"].tap()
        let complete = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Equations 6/6; network blocked"),
            object: app.staticTexts["renderer.equation-status"])
        XCTAssertEqual(XCTWaiter.wait(for: [complete], timeout: 35), .completed)
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Offline equations and positive-control containment canary"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testRendererFixtureDisplaysNativeTextCodeAndTable() {
        launchFixture()
        let paragraph = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Start alpha")).firstMatch
        XCTAssertTrue(paragraph.waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["Copy code"].waitForExistence(timeout: 10))
        app.buttons["Copy code"].tap()
        XCTAssertTrue(app.buttons["Copied"].waitForExistence(timeout: 1))
        XCTAssertTrue(app.buttons["Copy code"].waitForExistence(timeout: 4))
        pasteSelection()
        let expected = "let synthetic = \"copy code\" // preserved\nprint(synthetic)"
        let copied = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value == %@", expected), object: pasteField)
        XCTAssertEqual(XCTWaiter.wait(for: [copied], timeout: 5), .completed)
        let actual = pasteField.value as? String ?? ""
        XCTAssertTrue(actual.contains("let synthetic"))
        XCTAssertEqual(Array(actual.utf8), Array(expected.utf8))
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Native renderer synthetic corpus"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testNativeSelectionSurvivesAppend() {
        launchFixture()
        XCTAssertTrue(app.buttons["Copy code"].waitForExistence(timeout: 15))
        app.buttons["renderer.append"].tap()
        let paragraph = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Start alpha")).firstMatch
        XCTAssertTrue(paragraph.waitForExistence(timeout: 15))
        paragraph.coordinate(withNormalizedOffset: CGVector(dx: 0.12, dy: 0.25)).press(forDuration: 1.2)
        let copy = app.menuItems["Copy"].firstMatch
        let copyButton = app.buttons["Copy"].firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 5) || copyButton.waitForExistence(timeout: 3))
        XCTAssertEqual(app.staticTexts["renderer.revision"].label, "Revision 0")
        // Wait on an observable streamed append, leaving native selection active.
        let revised = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Revision 1"),
            object: app.staticTexts["renderer.revision"])
        XCTAssertEqual(XCTWaiter.wait(for: [revised], timeout: 15), .completed)
        let preserved = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", "Selection preserved"),
            object: app.staticTexts["renderer.selection-status"])
        XCTAssertEqual(XCTWaiter.wait(for: [preserved], timeout: 5), .completed)
        if copy.exists {
            copy.tap()
        } else if copyButton.exists {
            copyButton.tap()
        } else {
            paragraph.typeKey("c", modifierFlags: .command)
        }
        pasteSelection()
        XCTAssertFalse((pasteField.value as? String ?? "").isEmpty)
    }

    func testNativePartialSelectionControl() {
        launchFixture()
        XCTAssertTrue(app.buttons["Copy code"].waitForExistence(timeout: 15))
        let paragraph = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Start alpha")).firstMatch
        paragraph.coordinate(withNormalizedOffset: CGVector(dx: 0.12, dy: 0.25)).press(forDuration: 1.2)
        let copy = app.menuItems["Copy"].firstMatch
        let copyButton = app.buttons["Copy"].firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 5) || copyButton.waitForExistence(timeout: 3))
        if copy.exists { copy.tap() } else { copyButton.tap() }
        pasteSelection()
        XCTAssertFalse((pasteField.value as? String ?? "").isEmpty)
    }

    func testEquationSourcesReachNativeAccessibility() {
        launchFixture()
        XCTAssertTrue(app.buttons["Copy code"].waitForExistence(timeout: 15))
        // Poll a content-free native probe, allowing bounded equation enrichment.
        for _ in 0..<5 {
            app.buttons["renderer.ax-check"].tap()
            if app.staticTexts["renderer.ax-status"].label == "Equation AX present" { return }
        }
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Native equation accessibility diagnostic counts"
        attachment.lifetime = .keepAlways
        add(attachment)
        XCTFail(
            "Required equation sources did not reach native accessibility custom content; "
                + app.staticTexts["renderer.ax-counts"].label)
    }

    func testWideTableScrollPreservesVerticalReading() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-renderer-layout"]
        app.launch()
        let first = app.staticTexts["table first marker"]
        XCTAssertTrue(first.waitForExistence(timeout: 15))
        let initial = first.frame.minX
        let tableY = first.frame.midY / app.frame.height
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: tableY)).press(
            forDuration: 0.1,
            thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: tableY)))
        XCTAssertLessThan(first.frame.minX, initial - 20)
        let after = app.staticTexts["After wide table reading anchor."]
        for _ in 0..<5 where !after.isHittable { app.swipeUp() }
        XCTAssertTrue(after.isHittable)
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Wide table horizontal and vertical native gestures"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }

    func testLargeBoldDarkRTLRendererRemainsReadable() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-renderer-fixture", "--maple-renderer-layout", "--maple-renderer-large",
            "--maple-renderer-dark", "--maple-renderer-rtl",
        ]
        app.launch()
        let first = app.staticTexts["table first marker"]
        XCTAssertTrue(first.waitForExistence(timeout: 15))
        // This short three-word cell must not become a single-character vertical
        // column when Dynamic Type increases the font and cell padding.
        XCTAssertLessThan(first.frame.height, first.frame.width)
        let header = app.staticTexts["Primary choice"]
        XCTAssertTrue(header.exists)
        XCTAssertLessThan(header.frame.height, header.frame.width * 0.75)
        let top = XCTAttachment(screenshot: app.screenshot())
        top.name = "Large Bold Text dark RTL table"
        top.lifetime = .keepAlways
        add(top)
        let end = app.staticTexts["Final reading anchor after code and equation."]
        for _ in 0..<12 where !end.isHittable { app.swipeUp() }
        XCTAssertTrue(end.isHittable)
        let bottom = XCTAttachment(screenshot: app.screenshot())
        bottom.name = "Large Bold Text dark RTL code and math"
        bottom.lifetime = .keepAlways
        add(bottom)
    }

    func testLargeRTLCodeHeaderAndHorizontalReach() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-renderer-fixture", "--maple-renderer-layout", "--maple-renderer-large",
            "--maple-renderer-dark", "--maple-renderer-rtl",
        ]
        app.launch()
        let copy = app.buttons["Copy code"]
        XCTAssertTrue(copy.waitForExistence(timeout: 15))
        for _ in 0..<12 where !copy.isHittable { app.swipeUp() }
        XCTAssertTrue(copy.isHittable)
        let code = app.staticTexts.containing(
            NSPredicate(format: "label CONTAINS %@", "unbroken_long_identifier_0123456789")
        ).firstMatch
        let scroll = app.scrollViews["message.code.scroll"]
        XCTAssertTrue(code.exists && scroll.exists)
        XCTAssertGreaterThanOrEqual(copy.frame.height, 44)
        XCTAssertGreaterThanOrEqual(copy.frame.width, 44)
        XCTAssertLessThanOrEqual(copy.frame.maxY + 4, code.frame.minY)
        XCTAssertGreaterThan(code.frame.width, scroll.frame.width + 20)
        let initialX = code.frame.minX
        let headerY = copy.frame.minY
        let surrounding = [
            "After wide table reading anchor.",
            "Final reading anchor after code and equation.",
        ].map { fragment in
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", fragment)).firstMatch
        }
        for anchor in surrounding { XCTAssertTrue(anchor.exists && anchor.isHittable) }
        let surroundingFrames = surrounding.map(\.frame)
        func verifySurrounding(_ phase: String) {
            // Observe the actual surrounding native geometry after the horizontal
            // gesture. This both synchronizes against a concrete UI invariant and
            // distinguishes displaced body content from a screenshot-only anomaly.
            let settled = XCTNSPredicateExpectation(
                predicate: NSPredicate { _, _ in
                    zip(surrounding, surroundingFrames).allSatisfy { anchor, original in
                        guard anchor.exists && anchor.isHittable else { return false }
                        let current = anchor.frame
                        return abs(current.minX - original.minX) < 1
                            && abs(current.minY - original.minY) < 1
                            && abs(current.width - original.width) < 1
                            && abs(current.height - original.height) < 1
                    }
                }, object: app)
            let result = XCTWaiter.wait(for: [settled], timeout: 5)
            let geometry = zip(surrounding, surroundingFrames).enumerated().map { index, pair in
                "anchor\(index) before=\(pair.1) after=\(pair.0.frame) hittable=\(pair.0.isHittable)"
            }.joined(separator: "\n")
            let diagnostic = XCTAttachment(
                string: "\(phase); app=\(app.frame); code=\(code.frame); scroll=\(scroll.frame)\n" + geometry)
            diagnostic.name = "Synthetic surrounding renderer geometry \(phase)"
            diagnostic.lifetime = .keepAlways; add(diagnostic)
            let synchronized = XCTAttachment(screenshot: app.screenshot())
            synchronized.name = "Surrounding prose after observed geometry \(phase)"
            synchronized.lifetime = .keepAlways; add(synchronized)
            XCTAssertEqual(result, .completed, "Horizontal code scrolling must retain the surrounding body geometry")
        }
        // The RTL viewport starts at the trailing edge. Reach both physical
        // ends with native gestures; code bytes and bidi policy stay unchanged.
        for _ in 0..<12 where code.frame.minX < scroll.frame.minX - 2 {
            scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.5)).press(
                forDuration: 0.05,
                thenDragTo: scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5)))
        }
        XCTAssertGreaterThan(code.frame.minX, initialX + 20)
        XCTAssertGreaterThanOrEqual(code.frame.minX, scroll.frame.minX - 2)
        XCTAssertEqual(copy.frame.minY, headerY, accuracy: 1)
        let leading = XCTAttachment(screenshot: app.screenshot())
        leading.name = "Large RTL code leading edge and measured header"
        leading.lifetime = .keepAlways; add(leading)
        verifySurrounding("leading")
        for _ in 0..<12 where code.frame.maxX > scroll.frame.maxX + 2 {
            scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5)).press(
                forDuration: 0.05,
                thenDragTo: scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.5)))
        }
        XCTAssertLessThanOrEqual(code.frame.maxX, scroll.frame.maxX + 2)
        XCTAssertEqual(copy.frame.minY, headerY, accuracy: 1)
        let trailing = XCTAttachment(screenshot: app.screenshot())
        trailing.name = "Large RTL code trailing edge and measured header"
        trailing.lifetime = .keepAlways; add(trailing)
        verifySurrounding("trailing")
    }

    func testMixedMarkdownPartialCopyPreservesLineBreaks() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-renderer-corpus"]
        app.launch()
        XCTAssertTrue(app.buttons["renderer.copy-lines"].waitForExistence(timeout: 15))
        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Soft first"))
                .firstMatch.waitForExistence(timeout: 10))
        // The initial plain Text fallback also contains these words. The actual
        // prepared code control proves StructuredText owns this Copy operation.
        XCTAssertTrue(app.buttons["Copy code"].firstMatch.waitForExistence(timeout: 10))
        waitForNativeCopySpan("renderer.copy-lines")
        app.buttons["renderer.copy-lines"].tap()
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "Synthetic GFM soft and hard line-break copy probe"
        shot.lifetime = .keepAlways
        add(shot)
        let diagnostic = XCTAttachment(
            string: app.staticTexts["renderer.line-diagnostics"].label + "\n"
                + app.staticTexts["renderer.copy-scalars"].label)
        diagnostic.name = "Synthetic mixed-line selected and copied native scalar diagnostics"
        diagnostic.lifetime = .keepAlways; add(diagnostic)
        XCTAssertEqual(app.staticTexts["renderer.line-status"].label, "Soft newline 1; hard newline 1")
        XCTAssertEqual(
            app.staticTexts["renderer.representation-status"].label, "Canonical LF and raw UTF8 preserved")
    }

    func testFullMarkdownCorpusRemainsVisibleThroughLowerBlocks() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-renderer-corpus"]
        app.launch()
        XCTAssertTrue(app.buttons["renderer.copy-lines"].waitForExistence(timeout: 15))
        let scroll = app.scrollViews["renderer.corpus-scroll"]
        XCTAssertTrue(scroll.exists)

        func reveal(_ fragment: String) -> XCUIElement {
            let text = app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", fragment)).firstMatch
            func visible() -> Bool {
                guard text.exists else { return false }
                let overlap = text.frame.intersection(scroll.frame)
                return !overlap.isNull && overlap.height >= min(30, text.frame.height) && overlap.width > 0
            }
            for _ in 0..<20 where !visible() {
                // Native selection overlays can own the hit point of readable
                // Text. Inspect actual visible bounds and use small directed drags.
                let downward = text.exists && text.frame.midY < scroll.frame.midY
                let startY = downward ? 0.35 : 0.65
                let endY = downward ? 0.65 : 0.35
                scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: startY)).press(
                    forDuration: 0.05,
                    thenDragTo: scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: endY)))
            }
            XCTAssertTrue(text.exists, "Missing synthetic corpus text: \(fragment)")
            XCTAssertTrue(visible(), "Unreachable synthetic corpus text: \(fragment)")
            return text
        }
        func record(_ name: String) {
            let shot = XCTAttachment(screenshot: app.screenshot())
            shot.name = name; shot.lifetime = .keepAlways; add(shot)
        }

        _ = reveal("Soft first")
        record("Full corpus top: separate soft and hard lines, heading hierarchy and emphasis")
        _ = reveal("Completed task marker")
        record("Full corpus lists: nested bullets, static task checkboxes and ordered numbering")
        _ = reveal("Indented ordinary prose")
        XCTAssertEqual(
            app.buttons.matching(identifier: "Copy code").count, 1,
            "Only the explicit Swift fence is code; indented ordinary prose must remain prose")
        record("Full corpus middle: quotation and normalized indented prose")
        _ = reveal("reference link")
        _ = reveal("Preserved remote image alt")
        record("Full corpus links, untrusted HTML and remote-image fallback")
        _ = reveal("print(synthetic)")
        XCTAssertTrue(app.buttons["Copy code"].exists)
        record("Full corpus fenced source retains code formatting and copy control")
        _ = reveal("Incomplete original source")
        record("Full corpus lower: multilingual text, matrix, cases and original malformed math")
        XCTAssertTrue(
            app.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "unsupportedcommand")).firstMatch.exists
        )
    }

    func testMultilingualNativePartialCopyPreservesOriginalBytes() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-renderer-fixture", "--maple-renderer-corpus"]
        app.launch()
        XCTAssertTrue(app.buttons["renderer.copy-multilingual"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["Copy code"].firstMatch.waitForExistence(timeout: 10))
        waitForNativeCopySpan("renderer.copy-multilingual")
        app.buttons["renderer.copy-multilingual"].tap()
        let diagnostic = XCTAttachment(
            string: app.staticTexts["renderer.line-diagnostics"].label
                + "\n" + app.staticTexts["renderer.copy-scalars"].label)
        diagnostic.name = "Synthetic multilingual native selected Copy bytes"
        diagnostic.lifetime = .keepAlways; add(diagnostic)
        XCTAssertEqual(app.staticTexts["renderer.multilingual-status"].label, "Multilingual bytes preserved")
        XCTAssertEqual(app.staticTexts["renderer.representation-status"].label, "Raw UTF8 preserved")
        // The status above compares the actual selected native substring with this
        // synthetic corpus value byte-for-byte before leaving the selection fixture.
        let expected = "العربية עברית 中文 👩🏽‍💻 e\u{301}."
        app.terminate()
        launchFixture()
        // Reuse the existing native editor and Paste menu, without rewriting the
        // clipboard, assigning editor text, or applying a test-only expiration.
        pasteSelection()
        let pasted = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "value == %@", expected), object: pasteField)
        XCTAssertEqual(XCTWaiter.wait(for: [pasted], timeout: 5), .completed)
        XCTAssertEqual(Array((pasteField.value as? String ?? "").utf8), Array(expected.utf8))
    }

    private func waitForNativeCopySpan(_ buttonID: String) {
        let button = app.buttons[buttonID]
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: button)
        let result = XCTWaiter.wait(for: [ready], timeout: 20)
        let diagnostic = XCTAttachment(string: app.staticTexts["renderer.copy-readiness"].label)
        diagnostic.name = "Synthetic native span readiness before first Copy"
        diagnostic.lifetime = .keepAlways; add(diagnostic)
        XCTAssertEqual(result, .completed, "Copy requires the actual matching native input span")
    }

    private var pasteField: XCUIElement {
        // SwiftUI's vertical TextField can change native accessibility element
        // type after pasting multiple lines. Keep the locator independent of type.
        app.descendants(matching: .any).matching(identifier: "renderer.paste").firstMatch
    }

    private func pasteSelection() {
        pasteField.tap()
        let tip = app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Speed up your typing"))
            .firstMatch
        if tip.waitForExistence(timeout: 2), app.buttons["Continue"].exists { app.buttons["Continue"].tap() }
        pasteField.press(forDuration: 1.0)
        let paste = app.menuItems["Paste"].firstMatch
        let pasteButton = app.buttons["Paste"].firstMatch
        XCTAssertTrue(paste.waitForExistence(timeout: 3) || pasteButton.waitForExistence(timeout: 3))
        if paste.exists { paste.tap() } else { pasteButton.tap() }
    }
}

@MainActor
final class ComposerKeyboardUITests: XCTestCase {
    private var app: XCUIApplication!
    private var field: XCUIElement {
        let view = app.textViews["composer.probe"]
        return view.exists ? view : app.textFields["composer.probe"]
    }

    private func launchFixture() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-composer-fixture"]
        app.launch()
        XCTAssertTrue(app.staticTexts["composer.sends"].waitForExistence(timeout: 15))
        dismissKeyboardTip()
    }

    func testHardwareReturnSendsAndShiftReturnInsertsNewline() {
        launchFixture()
        field.tap()
        field.typeText("synthetic")
        field.typeKey(.return, modifierFlags: .shift)
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
        field.typeText("second line")
        field.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 1")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
    }

    private func launchNativeControlFixture() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-composer-fixture", "--maple-native-keyboard-control"]
        app.launch()
        let control = app.textViews["composer.native-control"]
        XCTAssertTrue(control.waitForExistence(timeout: 15))
        control.tap()
        dismissKeyboardTip()
    }

    /// The plain control must establish real modifier delivery and release before
    /// the separately selected owned-composer case is admitted on this artifact.
    func testHeldShiftNativeReturnDeliveryAndRelease() throws {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.typeText("abc")
        XCTAssertEqual(control.value as? String, "abc")
        XCUIElement.perform(withKeyModifiers: .shift) {
            control.typeKey(.return, modifierFlags: .shift)
        }
        recordNativeState(control)
        XCTAssertEqual(try nativeReturnMasks(), [131_072])
        XCTAssertEqual(control.value as? String, "abc\n")
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")

        // This key is outside the held block: its native mask proves release.
        control.typeKey(.return, modifierFlags: [])
        recordNativeState(control)
        XCTAssertEqual(try nativeReturnMasks(), [131_072, 0])
        XCTAssertEqual(control.value as? String, "abc\n\n")
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 2")
    }

    private func nativeReturnMasks() throws -> [Int] {
        let diagnostics = app.staticTexts["composer.native-diagnostics"].label
        let fields = diagnostics.components(separatedBy: "; ")
        XCTAssertTrue(fields.contains("scopeDropped 0"))
        XCTAssertTrue(fields.contains("lifetimeDropped 0"))
        let marker = "lifetime[ordinal:kind:hid:mask:phase:ended:cancelled:matched] "
        let start = try XCTUnwrap(diagnostics.range(of: marker))
        let events = diagnostics[start.upperBound...].split(separator: ";")
        XCTAssertFalse(events.isEmpty)
        XCTAssertLessThanOrEqual(events.count, 64)
        var masks: [Int] = []
        for (index, event) in events.enumerated() {
            let parts = event.split(separator: ":", omittingEmptySubsequences: false)
            let eightParts = try XCTUnwrap(parts.count == 8 ? parts : nil)
            let values = try eightParts.map { try XCTUnwrap(Int($0)) }
            XCTAssertEqual(values[0], index + 1)
            if values[1] == 1 {
                XCTAssertEqual(values[2], 40)
                XCTAssertEqual(values[7], 1)
                masks.append(values[3])
            }
        }
        return masks
    }

    func testNativeUnmodifiedReturnKeyInjectionControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.typeText("native")
        control.typeKey(.return, modifierFlags: [])
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
    }

    /// A distinct documented physical key diagnoses delivery; it does not
    /// substitute for the separate Return/ShiftReturn acceptance cases.
    func testNativeEnterKeyInjectionControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.typeText("native")
        control.typeKey("x", modifierFlags: [])
        XCTAssertEqual((control.value as? String)?.lowercased(), "nativex")
        control.typeKey(.enter, modifierFlags: [])
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
    }

    func testNativeLiteralUnmodifiedReturnKeyInjectionControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.typeText("native")
        control.typeKey("\r", modifierFlags: [])
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
    }

    func testNativeHardwareKeyInjectionControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.tap()
        control.typeText("native")
        control.typeKey("x", modifierFlags: [])
        XCTAssertEqual((control.value as? String)?.lowercased(), "nativex")
        control.typeKey(.return, modifierFlags: .shift)
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
        control.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 2")
    }

    func testNativeLiteralReturnKeyInjectionControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.tap()
        control.typeText("native")
        control.typeKey("x", modifierFlags: [])
        XCTAssertEqual((control.value as? String)?.lowercased(), "nativex")
        control.typeKey("\r", modifierFlags: .shift)
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
        control.typeKey("\r", modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 2")
    }

    private func recordNativeState(_ control: XCUIElement) {
        let value = control.value as? String ?? ""
        let scalars = value.unicodeScalars.map { String($0.value, radix: 16) }.joined(separator: ",")
        let diagnostic = XCTAttachment(
            string: "AX scalars " + scalars + "\n"
                + app.staticTexts["composer.native-diagnostics"].label)
        diagnostic.name = "Synthetic native key delivery and text state"
        diagnostic.lifetime = .keepAlways; add(diagnostic)
    }

    func testNativeSoftwareReturnControl() {
        launchNativeControlFixture()
        let control = app.textViews["composer.native-control"]
        control.typeText("native")
        let key = app.keyboards.buttons.matching(NSPredicate(format: "label ==[c] %@", "return")).firstMatch
        XCTAssertTrue(key.waitForExistence(timeout: 5))
        key.tap()
        recordNativeState(control)
        XCTAssertEqual(app.staticTexts["composer.native-newlines"].label, "Native newlines 1")
    }

    func testSwiftUIHardwareKeyInjectionControl() {
        launchFixture()
        field.tap()
        field.typeText("synthetic")
        field.typeKey("x", modifierFlags: [])
        XCTAssertEqual((field.value as? String)?.lowercased(), "syntheticx")
    }

    func testPlainHardwareReturnSends() {
        launchFixture()
        field.tap()
        field.typeText("synthetic")
        field.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 1")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
    }

    func testSoftwareReturnInsertsNewline() {
        launchFixture()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        field.typeText("synthetic")
        let lower = app.keyboards.buttons["return"]
        let upper = app.keyboards.buttons["Return"]
        XCTAssertTrue(lower.exists || upper.exists)
        if lower.exists { lower.tap() } else { upper.tap() }
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
    }

    func testHardwareReturnDoesNotSendMarkedComposition() {
        launchFixture()
        field.tap()
        field.typeText("prefix ")
        app.buttons["composer.mark"].tap()
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 1")
        field.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        app.buttons["composer.read-mark"].tap()
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 0")
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "SwiftUI controlled marked-text keyboard probe"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func dismissKeyboardTip() {
        let tip = app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Speed up your typing"))
            .firstMatch
        if tip.waitForExistence(timeout: 2), app.buttons["Continue"].exists { app.buttons["Continue"].tap() }
    }
}

@MainActor
final class OwnedComposerUITests: XCTestCase {
    private var app: XCUIApplication!
    private var field: XCUIElement { app.textViews["composer.probe"] }

    private func launchFixture() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-composer-fixture", "--maple-owned-composer"]
        app.launch()
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.tap()
        let tip = app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Speed up your typing"))
            .firstMatch
        if tip.waitForExistence(timeout: 2), app.buttons["Continue"].exists { app.buttons["Continue"].tap() }
    }

    func testHardwareReturnAndShiftReturn() {
        launchFixture()
        field.typeText("synthetic")
        field.typeKey(.return, modifierFlags: .shift)
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
        field.typeText("second")
        field.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 1")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
    }

    /// Run only after the same artifact's plain-native held-modifier control passes.
    func testHeldShiftReturnProducesDistinctNewlines() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-composer-fixture", "--maple-owned-composer", "--maple-composer-order-events",
        ]
        app.launch()
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.tap()
        let tip = app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Speed up your typing"))
            .firstMatch
        if tip.waitForExistence(timeout: 2), app.buttons["Continue"].exists { app.buttons["Continue"].tap() }
        field.typeText("abc")
        XCTAssertEqual(field.value as? String, "abc")
        app.buttons["composer.order-record"].tap()
        XCTAssertEqual(app.staticTexts["composer.order-events"].label, "Recording synthetic input order")

        XCUIElement.perform(withKeyModifiers: .shift) {
            self.field.typeKey(.return, modifierFlags: .shift)
        }
        let firstText = field.value as? String
        let firstNewlines = app.staticTexts["composer.newlines"].label
        let firstSends = app.staticTexts["composer.sends"].label
        let firstEdits = app.staticTexts["composer.edit-summary"].label
        if firstText != "abc\n" || firstNewlines != "Newlines 1" || firstSends != "Sends 0"
            || !firstEdits.hasSuffix("invalid 0")
        {
            _ = readHeldModifierDiagnostics()
        }
        XCTAssertEqual(firstText, "abc\n")
        XCTAssertEqual(firstNewlines, "Newlines 1")
        XCTAssertEqual(firstSends, "Sends 0")
        XCTAssertTrue(firstEdits.hasSuffix("invalid 0"))

        // Two distinct key events in one held context must produce two more LFs.
        XCUIElement.perform(withKeyModifiers: .shift) {
            self.field.typeKey(.return, modifierFlags: .shift)
            self.field.typeKey(.return, modifierFlags: .shift)
        }
        let diagnostics = readHeldModifierDiagnostics()
        XCTAssertEqual(field.value as? String, "abc\n\n\n")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 3")
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertTrue(app.staticTexts["composer.edit-summary"].label.hasSuffix("invalid 0"))
        let lines = diagnostics.split(separator: "\n")
        let parts = try XCTUnwrap(lines.first).components(separatedBy: "; ")
        let header = try XCTUnwrap(parts.count == 3 ? parts : nil)
        XCTAssertEqual(header[0], "Composer input diagnostics v1")
        XCTAssertTrue(header[1].hasPrefix("recorded="))
        let count = try XCTUnwrap(Int(header[1].dropFirst("recorded=".count)))
        XCTAssertEqual(header[2], "dropped=0")
        let events = Array(lines.dropFirst())
        XCTAssertEqual(events.count, count)
        XCTAssertGreaterThan(count, 0)
        XCTAssertLessThanOrEqual(count, 256)
        for (index, event) in events.enumerated() {
            XCTAssertTrue(event.hasPrefix("\(index + 1): "))
        }
        let commands = events.filter { $0.contains(": command.entry;") }
        XCTAssertEqual(commands.count, 3)
        for command in commands {
            XCTAssertTrue(command.contains("; kind=newline; modifierMask=131072; ownerAccepted=true; eligible=true;"))
        }
        XCTAssertEqual(events.filter { $0.contains(": command.exit;") }.count, 3)
        XCTAssertFalse(events.contains { $0.contains("; kind=submit;") || $0.contains(": submit.entry;") })
        XCTAssertFalse(events.contains { $0.contains("; overflow=true;") })
    }

    private func readHeldModifierDiagnostics() -> String {
        app.buttons["composer.order-read"].tap()
        let value = app.staticTexts["composer.order-events"].label
        let diagnostic = XCTAttachment(string: value)
        diagnostic.name = "Synthetic held Shift command receipt"
        diagnostic.lifetime = .keepAlways
        add(diagnostic)
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        screenshot.name = "Synthetic held Shift newline state"
        screenshot.lifetime = .keepAlways
        add(screenshot)
        return value
    }

    func testLiteralHardwareReturnAndShiftReturn() {
        launchFixture()
        field.typeText("synthetic")
        field.typeKey("\r", modifierFlags: .shift)
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
        field.typeText("second")
        field.typeKey("\r", modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 1")
        XCTAssertFalse(app.keyboards.firstMatch.exists)
    }

    func testSoftwareReturnRemainsNewline() {
        launchFixture()
        field.typeText("synthetic")
        let key =
            app.keyboards.buttons["return"].exists ? app.keyboards.buttons["return"] : app.keyboards.buttons["Return"]
        XCTAssertTrue(key.exists)
        key.tap()
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
    }

    func testActualTapRestoresSoftwareKeyboardAfterOwnedCommand() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = [
            "--maple-composer-fixture", "--maple-owned-composer", "--maple-composer-presentation-check",
        ]
        app.launch()
        XCTAssertTrue(field.waitForExistence(timeout: 15))
        field.tap()
        let tip = app.staticTexts.containing(NSPredicate(format: "label CONTAINS[c] %@", "Speed up your typing"))
            .firstMatch
        if tip.waitForExistence(timeout: 2), app.buttons["Continue"].exists { app.buttons["Continue"].tap() }
        let softwareReturn = app.keyboards.buttons.matching(
            NSPredicate(format: "label ==[c] %@ OR identifier ==[c] %@", "return", "return")
        ).firstMatch
        XCTAssertTrue(softwareReturn.waitForExistence(timeout: 10))
        field.typeText("abc")
        XCTAssertEqual(field.value as? String, "abc")
        let setup = app.buttons["composer.presentation-send"]
        XCTAssertTrue(setup.isHittable)
        setup.tap()
        XCTAssertEqual(app.staticTexts["composer.presentation-status"].label, "Owned command dispatched")
        let sent = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in self.app.staticTexts["composer.sends"].label == "Sends 1" }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [sent], timeout: 10), .completed)
        let hidden = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !softwareReturn.exists }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 10), .completed)
        let before = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        before.name = "Owned command dismissed software keys"; before.lifetime = .keepAlways; add(before)

        // This is an actual native tap, not a direct restoration-method call.
        field.tap()
        XCTAssertTrue(softwareReturn.waitForExistence(timeout: 10))
        XCTAssertTrue(softwareReturn.isHittable)
        field.typeText("next")
        XCTAssertEqual(field.value as? String, "next")
        softwareReturn.tap()
        XCTAssertEqual(field.value as? String, "next\n")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 1")
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 1")
        XCTAssertTrue(app.staticTexts["composer.edit-summary"].label.hasSuffix("invalid 0"))
        let after = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        after.name = "Native tap restored software Return newline"; after.lifetime = .keepAlways; add(after)
    }

    func testRepeatedTextExactDeletionAndCompositionEvents() {
        launchFixture()
        app.buttons["composer.repeated"].tap()
        XCTAssertEqual(field.value as? String, "aaaa")
        app.buttons["composer.delete-first"].tap()
        XCTAssertEqual(field.value as? String, "aaa")
        XCTAssertEqual(app.staticTexts["composer.edit-summary"].label, "Edit 0:1:0; invalid 0")
        app.buttons["composer.mark"].tap()
        XCTAssertEqual(app.staticTexts["composer.native-marked"].label, "Native marked 1")
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 1")
        field.typeKey(.return, modifierFlags: [])
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
        app.buttons["composer.unmark"].tap()
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 0")
        XCTAssertTrue(app.staticTexts["composer.edit-summary"].label.hasSuffix("invalid 0"))
        app.buttons["composer.mark"].tap()
        XCTAssertEqual(app.staticTexts["composer.native-marked"].label, "Native marked 1")
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 1")
        app.buttons["composer.clear"].tap()
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 0")
        XCTAssertEqual(app.staticTexts["composer.newlines"].label, "Newlines 0")
        XCTAssertEqual(field.value as? String ?? "", "")
        app.buttons["composer.mark"].tap()
        XCTAssertEqual(app.staticTexts["composer.native-marked"].label, "Native marked 1")
        XCTAssertEqual(app.staticTexts["composer.marked"].label, "Marked 1")
        app.buttons["composer.switch-owner"].tap()
        app.buttons["composer.read-previous-owner"].tap()
        XCTAssertEqual(app.staticTexts["composer.previous-owner"].label, "Previous owner committed")
    }

    func testReentrantOwnerChangeCannotRedirectStateOrSubmit() {
        launchFixture()
        app.buttons["composer.reentrant-check"].tap()
        XCTAssertEqual(app.staticTexts["composer.reentrant-status"].label, "Owner callbacks fenced")
        XCTAssertEqual(app.staticTexts["composer.sends"].label, "Sends 0")
    }

    func testNativeComposerMinimumTapSurface() {
        launchFixture()
        XCTAssertTrue(field.isHittable)
        XCTAssertGreaterThanOrEqual(field.frame.height, 44)
        XCTAssertGreaterThanOrEqual(field.frame.width, 44)
        let otherInput = app.textViews["composer.native-control"]
        XCTAssertTrue(otherInput.exists && otherInput.isHittable)
        otherInput.tap()
        let otherValue = otherInput.value as? String ?? ""
        field.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.9)).tap()
        // Target the application, not the field: the coordinate tap must acquire
        // input itself rather than typeText implicitly refocusing that field.
        app.typeText("x")
        XCTAssertEqual((field.value as? String ?? "").lowercased(), "x")
        XCTAssertEqual(otherInput.value as? String ?? "", otherValue)
        XCTAssertGreaterThanOrEqual(field.frame.height, 44)
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Native composer owns its minimum input target"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }

    func testProposedEditSurvivesPreMutationSelection() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-composer-fixture", "--maple-composer-order-check"]
        app.launch()
        let check = app.buttons["composer.order-check"]
        XCTAssertTrue(check.waitForExistence(timeout: 15))
        XCTAssertTrue(check.isHittable)
        check.tap()
        let status = app.staticTexts["composer.order-status"]
        let complete = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label CONTAINS %@", "nativeImagePasteRoutesProvidersAndPreservesTextFallback:"),
            object: status)
        XCTAssertEqual(XCTWaiter.wait(for: [complete], timeout: 15), .completed)
        let diagnostic = XCTAttachment(string: status.label)
        diagnostic.name = "Controlled production Coordinator callback order"
        diagnostic.lifetime = .keepAlways
        add(diagnostic)
        let expectedCases = [
            "proposedEditSurvivesPreMutationSelectionAndPublishesExactlyOnce",
            "sameTextReplacementWaitsForCompletionThenCarriesExactIntent",
            "rejectedProposalDoesNotCreateAnEditOrPoisonTheNextKnownEdit",
            "unmatchedActualMutationStillFailsClosed",
            "mutationWithoutAProposalStillFailsClosed",
            "oldOwnerFlushRetainsExactEditAndRoutesItOnlyToOldOwner",
            "overflowingActualInputStillFailsClosed",
            "ownedInputCompletionPublishesAfterNestedCallbacks",
            "authoritativeSameOwnerUpdateRetiresPendingProposal",
            "resignationIncludesPriorInputBeforeSubmit",
            "resignationOrdersPriorInputBeforeShiftNewline",
            "hardwareReturnAndKeypadCommandsUseSameGuardedPath",
            "ordinaryTextInputPreservesNewlinesWithoutSubmitting",
            "markdownNewlineRetainsExactDictationAnchorAndComposition",
            "markedInputDoesNotExposeOrExecuteReturnCommands",
            "retiredOwnerCannotExecuteCachedReturnCommand",
            "reentrantHardwareCommandCannotSubmitTwice",
            "distinctShiftActionsEachInsertOneNewline",
            "refusedResignationDoesNotSendOrClear",
            "compositionDuringResignationCancelsWithoutCommit",
            "revokedOwnerWithoutInstallationCannotSend",
            "successorIsInstalledBeforeHardwareFocusResumes",
            "replacementDuringResignationCancelsSend",
            "nestedFocusLossDuringResignationCancelsSend",
            "pasteDuringResignationCancelsSend",
            "publicationRevocationCancelsHardwareSend",
            "postReturnTextStaysWithAuthoritativeSuccessor",
            "sameOwnerSubmitClearsBeforePendingNativeInsertion",
            "rotatedSubmitRoutesPendingInputOnlyToSuccessor",
            "rejectedAndReservedSubmitRetainCurrentDraft",
            "reservedOwnerAppliesLaterAuthoritativeSuccessor",
            "submitRefreshCannotReplaceReentrantOwner",
            "submitRefreshRejectsNilAndDifferentAccount",
            "postSubmitPreservesExactInputDuringNativeAssignment",
            "postSubmitPreservesExactInputDuringFocusChange",
            "postSubmitReplacementRevokesUnpublishedNativeInput",
            "postSubmitUnknownNativeMutationKeepsMarkerFailure",
            "postSubmitNestedRenderWithoutInputStillUpdates",
            "postSubmitStaleOwnerRenderCannotReclaimSuccessor",
            "postSubmitEqualBaselineRuleEndsAtOuterUnwind",
            "ordinarySameOwnerInstallPreservesMarkedComposition",
            "hardwareSendDismissalPreservesSuccessorKeyboardInput",
            "keyboardPresentationPreservesSendDisposition",
            "explicitEditingRestoresSystemKeyboardWithExactInput",
            "ordinaryOwnerAndTeardownResetKeyboardPresentation",
            "keyboardPresentationReentryCannotReclaimReplacement",
            "keyboardRestorationPreservesMarkedAndRevokedOwners",
            "compactInteractionReclaimsRetainedKeyboardFocus",
            "compactInteractionSurvivesSameOwnerRender",
            "compactInteractionStartsUnfocusedEditorWithExactInput",
            "compactInteractionCannotReclaimRevokedPresentation",
            "lateNativeFocusCallbacksCannotReopenOrDismissEditor",
            "unrequestedNativeFocusCannotReopenDismissedComposer",
            "nativeTextInsetsKeepCaretAndPlaceholderAligned",
            "nativeImagePasteRoutesProvidersAndPreservesTextFallback",
        ]
        XCTAssertEqual(status.label, expectedCases.map { "\($0): passed" }.joined(separator: "\n"))
    }

    func testBatchedEditsPreserveTheUnchangedMiddleMarker() {
        launchFixture()
        app.buttons["composer.repeated"].tap()
        XCTAssertEqual(field.value as? String, "aaaa")
        app.buttons["composer.batch"].tap()
        XCTAssertEqual(field.value as? String, "baac")
        XCTAssertEqual(app.staticTexts["composer.transactions"].label, "0:1:1|3:1:1")
        XCTAssertEqual(app.staticTexts["composer.marker"].label, "Marker 2")
        XCTAssertTrue(app.staticTexts["composer.edit-summary"].label.hasSuffix("invalid 0"))
    }
}
