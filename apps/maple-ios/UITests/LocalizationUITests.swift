import UIKit
import XCTest

/// Run against the dedicated signed-in local fixture on iPhone and a regular-width
/// iPad window. No chat is sent or server record is created. The text is synthetic.
@MainActor final class LocalizationUITests: XCTestCase {
    private enum Mode { case expanded, rightToLeft }
    private let sample = "العربية שלום — 123\n日本語 中文 한글\n👩🏽‍💻 👨‍👩‍👧‍👦 🇨🇦 e\u{301} A\u{30A}\nlogical end 456"

    /// A separate read-only system diagnostic, not a localization/reference pass.
    /// Uses one native touch to reveal Control Center; it never toggles a control.
    @MainActor func testReadOnlyIPadControlCenterRotationState() throws {
        continueAfterFailure = false
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        try XCTSkipUnless(UIDevice.current.userInterfaceIdiom == .pad)
        let app = XCUIApplication()
        app.activate()
        let restore: @MainActor @Sendable () throws -> Void = { [weak self] in
            XCUIDevice.shared.press(.home)
            app.activate()
            XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
            try self?.captureRotationObservation(app, name: "Maple after system observation")
        }
        addTeardownBlock { try await restore() }
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 10))
        try captureRotationObservation(app, name: "Maple before system observation")
        XCUIDevice.shared.press(.home)
        let system = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        XCTAssertTrue(system.wait(for: .runningForeground, timeout: 10))
        let frame = system.frame
        try captureRotationObservation(system, name: "Home before native edge gesture")
        XCTAssertEqual(frame.minX, 0, accuracy: 2)
        XCTAssertEqual(frame.minY, 0, accuracy: 2)
        XCTAssertEqual(frame.width, 834, accuracy: 2)
        XCTAssertEqual(frame.height, 1210, accuracy: 2)
        // Coordinates belong to the observed full-screen system owner, not an
        // inset application window. XCUICoordinate.press synthesizes touch.
        let origin = system.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: frame.width - 12, dy: 2))
        let end = origin.withOffset(CGVector(dx: frame.width - 12, dy: frame.height * 0.35))
        start.press(forDuration: 0.05, thenDragTo: end)
        let locks = system.buttons.matching(
            NSPredicate(
                format: "label CONTAINS[c] %@ AND (label CONTAINS[c] %@ OR label CONTAINS[c] %@)",
                "lock", "rotation", "orientation"))
        _ = locks.firstMatch.waitForExistence(timeout: 10)
        let candidates = locks.allElementsBoundByIndex
        // Never dump the system hierarchy, labels, media titles or arbitrary
        // values. Selection alone is not a documented rotation-lock accessor.
        let controls: [[String: Any]] = candidates.prefix(4).map { control in
            let rawValue = (control.value as? String)?.lowercased() ?? ""
            let value =
                ["0", "1", "on", "off", "locked", "unlocked"].contains(rawValue)
                ? rawValue : "unavailable-or-unrecognized"
            return [
                "frame": [control.frame.minX, control.frame.minY, control.frame.width, control.frame.height],
                "value": value, "selected": control.isSelected, "hittable": control.isHittable,
            ]
        }
        try captureRotationObservation(
            system, name: "Control Center rotation-lock observation",
            extra: [
                "matching_control_count": candidates.count, "controls": controls,
                "setting_mutated": false, "interpretation": "Observed AX only; unknown does not mean unlocked",
            ])
        XCTAssertEqual(candidates.count, 1, "Require one actual rotation-lock control; never infer missing means OFF")
        XCTAssertTrue(candidates.first?.isHittable == true)
    }

    private func captureRotationObservation(
        _ owner: XCUIApplication, name: String, extra: [String: Any] = [:]
    ) throws {
        let screenshot = XCUIScreen.main.screenshot()
        let image = screenshot.image
        var record = extra
        record["physical_orientation"] = XCUIDevice.shared.orientation.rawValue
        record["application_frame"] = [owner.frame.minX, owner.frame.minY, owner.frame.width, owner.frame.height]
        record["window_frames"] = owner.windows.allElementsBoundByIndex.prefix(4).map {
            [$0.frame.minX, $0.frame.minY, $0.frame.width, $0.frame.height]
        }
        record["screen_pixels"] = [image.cgImage?.width ?? 0, image.cgImage?.height ?? 0]
        let metadata = XCTAttachment(
            data: try JSONSerialization.data(withJSONObject: record, options: .sortedKeys),
            uniformTypeIdentifier: "public.json")
        metadata.name = name + " geometry"; metadata.lifetime = .keepAlways; add(metadata)
        let capture = XCTAttachment(screenshot: screenshot)
        capture.name = name; capture.lifetime = .keepAlways; add(capture)
    }

    @MainActor func testDoubleLengthLargeTextPreservesShippingNavigationAndLogicalDraft() throws {
        try run(.expanded)
    }

    @MainActor func testRightToLeftLargeTextMirrorsShippingControlsAndPreservesLogicalDraft() throws {
        try run(.rightToLeft)
    }

    /// An additional focused iPad proof, separate from the two full localization
    /// walkthroughs. It restores the observed OS window and never sends a chat.
    @MainActor func testActualIPadWindowResizeKeepsLargeRTLLibraryNavigationReachable() throws {
        continueAfterFailure = false
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        try XCTSkipUnless(UIDevice.current.userInterfaceIdiom == .pad)
        let app = XCUIApplication()
        app.launchArguments = arguments(mode: .rightToLeft) + ["--maple-library-local-cleanup"]
        app.launch()
        try waitForRoot(app)
        try assertObservedTraits(app, rtl: true)
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10))
        let original = window.frame
        try XCTSkipUnless(
            abs(original.minX) < 2 && abs(original.minY) < 2
                && abs(original.width - 834) < 2 && abs(original.height - 1210) < 2,
            "Requires the observed full portrait iPad window and its visible system resize handle")
        let restore: @MainActor @Sendable () -> Void = { [weak self] in
            guard let self else { return }
            guard window.exists else {
                XCTFail("The owned window must remain available for restoration")
                return
            }
            if self.libraryWindowFrame(window.frame, matches: original) { return }
            self.captureLibraryWindow(app, name: "Window before teardown restoration", frame: window.frame)
            self.resizeLibraryWindow(
                in: app, from: window.frame,
                to: CGPoint(x: original.maxX - 10, y: original.maxY - 15))
            XCTAssertTrue(self.waitForLibraryWindow(window, matching: original), "Restore the original iPad window")
            self.captureLibraryWindow(app, name: "Window after teardown restoration", frame: window.frame)
        }
        // XCTest executes registered teardown after a fail-fast assertion. Swift
        // defer alone did not run when XCTest ended the previous failing method.
        addTeardownBlock { await restore() }
        let fingerprint = try accountFingerprint(in: app)
        let composer = app.textViews["chat.composer"]
        let originalDraft = try XCTUnwrap(composer.value as? String)
        try openLibrary(app)
        try assertLibrarySearchAndMenu(in: app, window: original)
        captureLibraryWindow(app, name: "RTL large-text Library before system resize", frame: original)

        // These are the insets of the observed public bottom-right handle. Use
        // this window's fresh screen frame for each gesture, including restoration.
        resizeLibraryWindow(
            in: app, from: window.frame,
            to: CGPoint(x: original.minX + original.width * 0.72, y: original.minY + original.height * 0.76))
        let smaller = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let current = window.frame
                return current.width < original.width - 80 && current.height < original.height - 80
            }, object: window)
        guard XCTWaiter.wait(for: [smaller], timeout: 15) == .completed else {
            XCTFail("Only an observed OS window-frame reduction establishes the resize")
            throw NativeTextEntryIssue.unavailable
        }
        let compact = window.frame
        XCTAssertGreaterThan(compact.width, 300)
        XCTAssertGreaterThan(compact.height, 500)
        try openLibrary(app)
        try assertLibrarySearchAndMenu(in: app, window: compact)
        try assertPhysicalLeftDrawer(in: app)
        let close = app.buttons["library.close"]
        XCTAssertTrue(close.waitForExistence(timeout: 10), "The compact menu must provide its app-owned Close")
        try assertTarget(close, in: app)
        XCTAssertTrue(compact.insetBy(dx: -0.5, dy: -0.5).contains(close.frame))
        captureLibraryWindow(app, name: "RTL large-text Library in actual smaller window", frame: compact)
        try closeLibrary(app)
        XCTAssertTrue(composer.value as? String == originalDraft, "Navigation must retain the same draft")
        XCTAssertTrue(try accountFingerprint(in: app) == fingerprint, "Resize must retain the same account")

        resizeLibraryWindow(
            in: app, from: window.frame,
            to: CGPoint(x: original.maxX - 10, y: original.maxY - 15))
        guard waitForLibraryWindow(window, matching: original) else {
            XCTFail("Restore the original iPad window")
            throw NativeTextEntryIssue.unavailable
        }
        try assertObservedTraits(app, rtl: true)
        try openLibrary(app)
        try assertLibrarySearchAndMenu(in: app, window: window.frame)
        captureLibraryWindow(app, name: "RTL large-text Library after window restoration", frame: window.frame)
        app.buttons["library.actions"].tap()
        try assertNativeMenuCommand(app.buttons["library.settings"], in: app)
        app.buttons["library.settings"].tap()
        _ = try settingsCollection(app)
        let settingsClose = app.buttons["library.settings-close"]
        XCTAssertTrue(settingsClose.waitForExistence(timeout: 10), "Regular-width Settings must have app-owned Close")
        try assertTarget(settingsClose, in: app)
        try closeSettingsAndLibrary(app)
        XCTAssertTrue(composer.value as? String == originalDraft, "The same draft must survive restored navigation")
        XCTAssertTrue(try accountFingerprint(in: app) == fingerprint, "Navigation must retain the same account")
    }

    @MainActor private func accountFingerprint(in app: XCUIApplication) throws -> String {
        // Existing DEBUG/local diagnostic exposes only a digest. Compare it in
        // memory; never attach or print an account identifier or draft contents.
        let marker = app.staticTexts["library.cleanup-account"]
        XCTAssertTrue(marker.waitForExistence(timeout: 10))
        let value = marker.label
        guard value.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil else {
            throw NativeTextEntryIssue.unavailable
        }
        return value
    }

    @MainActor private func assertLibrarySearchAndMenu(
        in app: XCUIApplication, window: CGRect
    ) throws {
        let search = app.textFields["library.search"], actions = app.buttons["library.actions"]
        // These persistent controls must be reachable without scrolling loaded chats.
        for control in [search, actions] {
            try assertTarget(control, in: app)
            XCTAssertTrue(window.insetBy(dx: -0.5, dy: -0.5).contains(control.frame))
        }
        XCTAssertTrue(search.frame.intersection(actions.frame).isEmpty, "Search and Maple menu must not overlap")
        XCTAssertGreaterThanOrEqual(search.frame.minY, actions.frame.maxY, "Search floats below the Maple menu")
        XCTAssertLessThanOrEqual(search.frame.height + actions.frame.height, min(240, window.height * 0.3) + 1)
        let list = try XCTUnwrap(visibleCollection(in: app, prefix: "library."))
        let scrolling = visibleRegion(list, in: app)
        let contentTop = max(scrolling.minY, actions.frame.maxY)
        let contentBottom = min(scrolling.maxY, search.frame.minY)
        XCTAssertGreaterThanOrEqual(
            contentBottom - contentTop, 100, "Floating search and Maple menu must leave usable scrolling content")
        XCTAssertFalse(app.buttons["library.history"].exists)
    }

    @MainActor private func resizeLibraryWindow(in app: XCUIApplication, from frame: CGRect, to point: CGPoint) {
        let window = app.windows.firstMatch
        let origin = window.coordinate(withNormalizedOffset: .zero)
        let windowOrigin = window.frame.origin
        let start = origin.withOffset(
            CGVector(dx: frame.maxX - windowOrigin.x - 10, dy: frame.maxY - windowOrigin.y - 15))
        let end = origin.withOffset(CGVector(dx: point.x - windowOrigin.x, dy: point.y - windowOrigin.y))
        start.press(forDuration: 0.2, thenDragTo: end)
    }

    @MainActor private func waitForLibraryWindow(_ window: XCUIElement, matching expected: CGRect) -> Bool {
        let matching = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.libraryWindowFrame(window.frame, matches: expected)
            }, object: window)
        return XCTWaiter.wait(for: [matching], timeout: 15) == .completed
    }

    private func libraryWindowFrame(_ frame: CGRect, matches expected: CGRect) -> Bool {
        abs(frame.minX - expected.minX) < 2 && abs(frame.minY - expected.minY) < 2
            && abs(frame.width - expected.width) < 2 && abs(frame.height - expected.height) < 2
    }

    // The application snapshot uses a local (0, 0) origin for a resized iPad,
    // while window and descendant frames are screen-relative. The app has one
    // window; use that actual window consistently for containment and coordinates.
    private func windowFrame(in app: XCUIApplication) -> CGRect { app.windows.firstMatch.frame }

    private func recordTargetGeometry(_ element: XCUIElement, in app: XCUIApplication) {
        let geometry = XCTAttachment(
            string:
                "Application: \(app.frame); window: \(windowFrame(in: app)); target: \(element.frame)")
        // The identifier is app-owned; attach no label, value, account or draft.
        geometry.name = "Target geometry — " + element.identifier
        geometry.lifetime = .keepAlways; add(geometry)
    }

    @MainActor private func captureLibraryWindow(_ app: XCUIApplication, name: String, frame: CGRect) {
        let image = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        image.name = name; image.lifetime = .keepAlways; add(image)
        let geometry = XCTAttachment(
            string:
                "Window frame points: x=\(frame.minX), y=\(frame.minY), width=\(frame.width), height=\(frame.height)")
        geometry.name = name + " — measured frame"; geometry.lifetime = .keepAlways; add(geometry)
    }

    @MainActor private func run(_ mode: Mode) throws {
        continueAfterFailure = false
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        let app = XCUIApplication()
        defer { UIPasteboard.general.items = [] }
        app.launchArguments = arguments()
        app.launch()
        try waitForRoot(app)
        let ltrMenu = app.buttons["chat.menu"].frame
        let ltrNew = app.buttons["chat.new"].frame
        XCTAssertLessThan(ltrMenu.midX, ltrNew.midX, "Baseline toolbar must actually be left-to-right")
        let baselineLibrary = try openLibrary(app)
        let libraryNew = app.buttons["library.new-chat"]
        try reveal(libraryNew, in: app, owner: baselineLibrary, down: true, attempts: 24)
        XCTAssertTrue(libraryNew.waitForExistence(timeout: 10))
        let baseNewLabel = libraryNew.label
        try assertTarget(app.buttons["library.actions"], in: app)
        app.buttons["library.actions"].tap()
        try assertNativeMenuCommand(app.buttons["library.settings"], in: app)
        let baseSettingsLabel = app.buttons["library.settings"].label
        app.buttons["library.settings"].tap()
        _ = try settingsCollection(app)
        app.terminate()

        app.launchArguments = arguments(mode: mode)
        app.launch()
        try waitForRoot(app)
        try assertObservedTraits(app, rtl: mode == .rightToLeft)
        let menu = app.buttons["chat.menu"], newChat = app.buttons["chat.new"]
        try assertTarget(menu, in: app)
        try assertTarget(newChat, in: app)
        if mode == .rightToLeft {
            XCTAssertGreaterThan(menu.frame.midX, newChat.frame.midX, "RTL must mirror actual toolbar geometry")
        } else {
            XCTAssertLessThan(menu.frame.midX, newChat.frame.midX)
        }
        newChat.tap()
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        // Native paste preserves scalar order, combining marks and multiline text;
        // synthetic input never crosses a send or backend mutation boundary.
        try pasteSynthetic(sample, into: composer, in: app)
        try assertLogicalSelectionCopy(sample, from: composer, in: app)
        try assertComposerViewport(composer, in: app)
        capture(app, name: "Localization multiline CJK RTL emoji fallback")
        XCTAssertEqual(composer.value as? String, sample)
        try assertTarget(app.buttons["chat.attach"], in: app)
        try assertTarget(app.buttons["chat.model"], in: app)
        try assertTarget(app.buttons["chat.send"], in: app)
        XCTAssertFalse(app.buttons["chat.stop"].exists)
        // Navigation commits the protected draft before the later process restart.
        var activeLibrary = try openLibrary(app)
        let compactMenu = app.buttons["library.close"].exists
        if compactMenu {
            try assertPhysicalLeftDrawer(in: app)
            try closeLibrary(app)
            // Placement and opening remain physical-left even while all content
            // and native toolbar controls follow the user's RTL direction.
            let window = app.windows.firstMatch, frame = windowFrame(in: app)
            let origin = window.coordinate(withNormalizedOffset: .zero)
            let start = origin.withOffset(CGVector(dx: 4, dy: frame.height * 0.45))
            let end = origin.withOffset(CGVector(dx: min(190, frame.width * 0.55), dy: frame.height * 0.45))
            start.press(forDuration: 0.1, thenDragTo: end)
            XCTAssertTrue(app.buttons["library.close"].waitForExistence(timeout: 15))
            try assertPhysicalLeftDrawer(in: app)
            activeLibrary = try XCTUnwrap(visibleCollection(in: app, prefix: "library."))
        }
        // Bottom search and the top Maple menu remain reachable at the largest
        // text size in both directions without scrolling chats.
        try assertLibrarySearchAndMenu(in: app, window: windowFrame(in: app))
        try reveal(libraryNew, in: app, owner: activeLibrary, down: true, attempts: 24)
        XCTAssertTrue(libraryNew.waitForExistence(timeout: 10))
        if mode == .expanded { assertDoubled(libraryNew.label, baseline: baseNewLabel) }
        try assertTarget(libraryNew, in: app)
        capture(app, name: "Localization expanded or RTL shipping Library")
        app.buttons["library.actions"].tap()
        let settings = app.buttons["library.settings"]
        try assertNativeMenuCommand(settings, in: app)
        if mode == .expanded { assertDoubled(settings.label, baseline: baseSettingsLabel) }
        try assertNativeMenuCommand(settings, in: app)
        settings.tap()
        let settingsOwner = try settingsCollection(app)
        if compactMenu {
            let drawer = app.otherElements["library.drawer"], bounds = windowFrame(in: app)
            XCTAssertTrue(drawer.exists)
            recordTargetGeometry(drawer, in: app)
            XCTAssertEqual(app.descendants(matching: .any).matching(identifier: "library.drawer").count, 1)
            XCTAssertEqual(app.otherElements.matching(identifier: "library.drawer").count, 1)
            XCTAssertEqual(drawer.elementType, .other)
            XCTAssertEqual(drawer.frame.width, bounds.width, accuracy: 1)
            XCTAssertEqual(drawer.frame.height, bounds.height, accuracy: 1)
            XCTAssertEqual(drawer.frame.minX, bounds.minX, accuracy: 1)
            XCTAssertEqual(drawer.frame.maxX, bounds.maxX, accuracy: 1)
            XCTAssertEqual(drawer.frame.minY, bounds.minY, accuracy: 1)
            XCTAssertEqual(drawer.frame.maxY, bounds.maxY, accuracy: 1)
            XCTAssertFalse(app.alerts["library.drawer"].exists)
            XCTAssertFalse(app.sheets["library.drawer"].exists)
            XCTAssertFalse(app.buttons["library.scrim"].exists)
        }
        let theme = app.buttons["settings.theme"]
        try reveal(theme, in: app, owner: settingsOwner)
        try assertTarget(theme, in: app)
        let font = app.buttons["settings.font"]
        try reveal(font, in: app, owner: settingsOwner)
        try assertTarget(font, in: app)
        let signOut = app.buttons["settings.sign-out"]
        try reveal(signOut, in: app, owner: settingsOwner)
        try assertTarget(signOut, in: app)
        capture(app, name: "Localization shipping Settings large text")
        // Observe destructive/account actions without activating them.
        try closeSettingsAndLibrary(app)
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        XCTAssertEqual(composer.value as? String, sample)
        app.terminate(); app.launch()
        try waitForRoot(app)
        try assertObservedTraits(app, rtl: mode == .rightToLeft)
        XCTAssertEqual(composer.value as? String, sample, "Cold launch must retain exact logical Unicode draft")
        try assertLogicalSelectionCopy(sample, from: composer, in: app)
        try assertComposerViewport(composer, in: app)
        capture(app, name: "Localization cold restored multilingual draft")
        // Finish with an empty synthetic New Chat draft; no remote cleanup needed.
        try clearSynthetic(composer, in: app)
        try openLibrary(app)
        try closeLibrary(app)
    }

    private func arguments(mode: Mode? = nil) -> [String] {
        var result = [
            "--maple-localization-proof", "-AppleLanguages", "(en)", "-AppleLocale", "en_US",
            "-NSDoubleLocalizedStrings", mode == .expanded ? "YES" : "NO",
            "-AppleTextDirection", mode == .rightToLeft ? "YES" : "NO",
            "-NSForceRightToLeftWritingDirection", mode == .rightToLeft ? "YES" : "NO",
        ]
        if mode != nil {
            result += [
                "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL",
                "-UIAccessibilityBoldTextEnabled", "YES",
            ]
        }
        return result
    }

    @MainActor private func waitForRoot(_ app: XCUIApplication) throws {
        guard app.buttons["chat.new"].waitForExistence(timeout: 40),
            app.textViews["chat.composer"].waitForExistence(timeout: 15)
        else {
            XCTFail("Dedicated local signed-in application did not reach the shipping chat")
            throw NativeTextEntryIssue.unavailable
        }
    }

    @MainActor private func assertObservedTraits(_ app: XCUIApplication, rtl: Bool) throws {
        let element = app.staticTexts["localization.observed-traits"]
        XCTAssertTrue(element.waitForExistence(timeout: 10))
        let raw = try XCTUnwrap(element.value as? String)
        let traits = try JSONDecoder().decode([String: Bool].self, from: Data(raw.utf8))
        XCTAssertEqual(traits["swiftAccessibility"], true, "Actual SwiftUI Dynamic Type must be accessibility size")
        XCTAssertEqual(traits["uiAccessibility"], true, "Actual UIKit trait must be accessibility size")
        XCTAssertTrue(traits["systemBold"] == true || traits["uiBold"] == true, "Bold Text must actually be enabled")
        XCTAssertEqual(traits["swiftBold"], true, "Shipping SwiftUI must inherit bold legibility")
        XCTAssertEqual(traits["swiftRTL"], rtl)
        XCTAssertEqual(traits["uiRTL"], rtl)
    }

    @MainActor private func visibleRegion(_ collection: XCUIElement, in app: XCUIApplication) -> CGRect {
        guard collection.exists else { return .null }
        return collection.frame.intersection(windowFrame(in: app))
    }

    @MainActor private func hasVisibleContent(
        _ collection: XCUIElement, in app: XCUIApplication, prefix: String? = nil
    ) -> Bool {
        let region = visibleRegion(collection, in: app)
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

    @MainActor private func visibleCollection(in app: XCUIApplication, prefix: String) -> XCUIElement? {
        app.collectionViews.allElementsBoundByIndex.first {
            hasVisibleContent($0, in: app, prefix: prefix)
        }
    }

    @MainActor private func keyboardIsVisible(in app: XCUIApplication) -> Bool {
        let keys = app.keyboards.buttons.allElementsBoundByIndex
        guard keys.count <= 160 else { return true }
        let visibleKey = keys.contains {
            $0.exists && $0.isHittable && !$0.frame.intersection(windowFrame(in: app)).isEmpty
        }
        let hide = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "Hide keyboard"))
            .allElementsBoundByIndex.first {
                hasSystemTitle($0.label, title: "Hide keyboard") && $0.exists && $0.isHittable
            }
        return visibleKey || hide != nil
    }

    @MainActor private func dismissTabletKeyboard(in app: XCUIApplication) throws {
        guard UIDevice.current.userInterfaceIdiom == .pad, keyboardIsVisible(in: app) else { return }
        // Observed public iPad control. The keyboard AX container can survive
        // dismissal, so require no visible keys rather than container absence.
        let candidates = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "Hide keyboard"))
        _ = candidates.firstMatch.waitForExistence(timeout: 5)
        guard
            let hide = candidates.allElementsBoundByIndex.first(where: {
                hasSystemTitle($0.label, title: "Hide keyboard") && $0.isHittable
            })
        else { throw NativeTextEntryIssue.unavailable }
        hide.tap()
        let hidden = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in !self.keyboardIsVisible(in: app) }, object: app)
        guard XCTWaiter.wait(for: [hidden], timeout: 10) == .completed else {
            throw NativeTextEntryIssue.unavailable
        }
    }

    @MainActor @discardableResult private func openLibrary(_ app: XCUIApplication) throws -> XCUIElement {
        try dismissTabletKeyboard(in: app)
        if let owner = visibleCollection(in: app, prefix: "library.") { return owner }
        app.buttons["chat.menu"].tap()
        let visible = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in self.visibleCollection(in: app, prefix: "library.") != nil },
            object: app)
        guard XCTWaiter.wait(for: [visible], timeout: 15) == .completed,
            let owner = visibleCollection(in: app, prefix: "library.")
        else { throw NativeTextEntryIssue.unavailable }
        return owner
    }

    @MainActor private func assertPhysicalLeftDrawer(in app: XCUIApplication) throws {
        let candidates = app.descendants(matching: .any).matching(identifier: "library.drawer")
        let drawer = candidates.firstMatch, window = app.windows.firstMatch
        let positioned = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                guard drawer.exists, window.exists else { return false }
                let frame = drawer.frame, bounds = self.windowFrame(in: app)
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
        let count = candidates.count, typed = app.otherElements.matching(identifier: "library.drawer")
        var geometry: [String: Double] = [
            "waitCompleted": result == .completed ? 1 : 0,
            "candidateCount": Double(min(count, 9)), "candidateLimitExceeded": count > 8 ? 1 : 0,
            "typedOtherCount": Double(min(typed.count, 9)),
        ]
        for (name, element) in [
            ("typedOther", typed.firstMatch), ("drawer", drawer), ("window", window),
            ("scrim", app.buttons["library.scrim"]),
            ("alertDrawer", app.alerts["library.drawer"]), ("sheetDrawer", app.sheets["library.drawer"]),
            ("close", app.buttons["library.close"]),
        ] {
            let exists = element.exists, frame = exists ? element.frame : .zero
            geometry[name + "Exists"] = exists ? 1 : 0
            geometry[name + "Hittable"] = exists && element.isHittable ? 1 : 0
            geometry[name + "Type"] = exists ? Double(element.elementType.rawValue) : 0
            geometry[name + "X"] = Double(frame.minX); geometry[name + "Y"] = Double(frame.minY)
            geometry[name + "Width"] = Double(frame.width); geometry[name + "Height"] = Double(frame.height)
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
        let close = app.buttons["library.close"]
        try assertTarget(close, in: app)
        let composer = app.textViews["chat.composer"]
        XCTAssertFalse(composer.exists && composer.isHittable, "The offscreen chat must not accept input")
    }

    @MainActor private func settingsCollection(_ app: XCUIApplication) throws -> XCUIElement {
        // Shipping SettingsView names its Form. Use that owner even when only
        // the account section is visible at large text, never the covered sidebar.
        let owner = app.descendants(matching: .any).matching(identifier: "settings.screen").firstMatch
        let visible = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in self.hasVisibleContent(owner, in: app) }, object: app)
        guard XCTWaiter.wait(for: [visible], timeout: 15) == .completed else {
            throw NativeTextEntryIssue.unavailable
        }
        return owner
    }

    @MainActor private func closeLibrary(_ app: XCUIApplication) throws {
        let close = app.buttons["library.close"]
        if close.exists {
            try assertTarget(close, in: app)
            close.tap()
        } else {
            app.buttons["chat.menu"].tap()
        }
        XCTAssertTrue(app.textViews["chat.composer"].waitForExistence(timeout: 10))
    }

    @MainActor private func closeSettingsAndLibrary(_ app: XCUIApplication) throws {
        let close = app.buttons["library.settings-close"]
        if close.exists {
            try assertTarget(close, in: app)
            close.tap()
        } else {
            let back = app.navigationBars.buttons.element(boundBy: 0)
            try assertTarget(back, in: app)
            back.tap()
        }
        try closeLibrary(app)
    }

    @MainActor private func reveal(
        _ element: XCUIElement, in app: XCUIApplication, owner: XCUIElement,
        down: Bool = false, attempts: Int = 14
    ) throws {
        // Keep one actual list owner for the full reveal. A visible iPad sidebar
        // container may report isHittable=false; app-wide swipes hit the chat.
        for _ in 0..<attempts {
            if element.exists, element.isHittable,
                windowFrame(in: app).insetBy(dx: -0.5, dy: -0.5).contains(element.frame)
            {
                return
            }
            guard hasVisibleContent(owner, in: app) else { throw NativeTextEntryIssue.unavailable }
            var region = visibleRegion(owner, in: app).insetBy(dx: 16, dy: 24)
            if keyboardIsVisible(in: app) {
                for keyboard in app.keyboards.allElementsBoundByIndex where keyboard.exists {
                    let overlap = keyboard.frame.intersection(region)
                    if !overlap.isEmpty { region.size.height = max(0, overlap.minY - region.minY - 16) }
                }
            }
            guard region.width >= 44, region.height >= 100 else { throw NativeTextEntryIssue.unavailable }
            let top = CGPoint(x: region.midX, y: region.minY + region.height * 0.15)
            let bottom = CGPoint(x: region.midX, y: region.maxY - region.height * 0.15)
            let window = app.windows.firstMatch
            let origin = window.coordinate(withNormalizedOffset: .zero)
            let windowOrigin = window.frame.origin
            func coordinate(_ point: CGPoint) -> XCUICoordinate {
                origin.withOffset(CGVector(dx: point.x - windowOrigin.x, dy: point.y - windowOrigin.y))
            }
            coordinate(down ? top : bottom).press(forDuration: 0.05, thenDragTo: coordinate(down ? bottom : top))
        }
        // Record only finite rectangles; omitted nodes are absent or nonfinite.
        // Labels, identifiers, values and conversation contents are not attached.
        var rectangles: [String: [Double]] = [:]
        let window = app.windows.firstMatch
        for (name, frame) in [
            ("target", element.exists ? element.frame : CGRect.null),
            ("owner", owner.exists ? owner.frame : CGRect.null),
            ("window", window.exists ? window.frame : CGRect.null),
        ] {
            let values = [Double(frame.minX), Double(frame.minY), Double(frame.width), Double(frame.height)]
            if values.allSatisfy(\.isFinite) { rectangles[name] = values }
        }
        if let data = try? JSONSerialization.data(withJSONObject: rectangles, options: .sortedKeys) {
            let geometry = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
            geometry.name = "Localized control reveal geometry"
            geometry.lifetime = .keepAlways; add(geometry)
        }
        XCTFail("Shipping localized control could not be reached in its owning list")
        throw NativeTextEntryIssue.unavailable
    }

    @MainActor private func assertNativeMenuCommand(_ element: XCUIElement, in app: XCUIApplication) throws {
        // UIKit owns native popup row sizing. Keep app controls on assertTarget's
        // 44pt contract and require this exact command in the open Maple menu.
        guard element.waitForExistence(timeout: 10) else {
            XCTFail("The exact native Settings menu command must be available")
            throw NativeTextEntryIssue.unavailable
        }
        recordTargetGeometry(element, in: app)
        guard element.exists, element.identifier == "library.settings",
            app.buttons.matching(identifier: "library.settings").count == 1,
            element.isEnabled, element.isHittable
        else {
            XCTFail("The exact native Settings menu command must be available")
            throw NativeTextEntryIssue.unavailable
        }
        let frame = element.frame, window = windowFrame(in: app)
        let coordinates = [
            frame.minX, frame.minY, frame.width, frame.height,
            window.minX, window.minY, window.width, window.height,
        ]
        guard coordinates.allSatisfy(\.isFinite), frame.width > 0, frame.height > 0,
            window.width > 0, window.height > 0,
            window.insetBy(dx: -0.5, dy: -0.5).contains(frame)
        else {
            XCTFail("The native menu command must have finite visible bounds inside the window")
            throw NativeTextEntryIssue.unavailable
        }
        // These tests pin English; prefix matching also covers doubled labels.
        // Observe the other native commands without activating either action.
        for title in ["Select chats", "Delete all chats"] {
            let commands = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title))
            guard commands.count == 1, commands.firstMatch.isEnabled, commands.firstMatch.isHittable else {
                XCTFail("Require the currently presented Maple menu commands")
                throw NativeTextEntryIssue.unavailable
            }
        }
    }

    @MainActor private func assertTarget(_ element: XCUIElement, in app: XCUIApplication) throws {
        recordTargetGeometry(element, in: app)
        guard element.exists, element.isHittable else {
            XCTFail("Localized control must be visible and reachable")
            throw NativeTextEntryIssue.unavailable
        }
        XCTAssertGreaterThanOrEqual(element.frame.width, 43.5)
        XCTAssertGreaterThanOrEqual(element.frame.height, 43.5)
        XCTAssertTrue(
            windowFrame(in: app).insetBy(dx: -0.5, dy: -0.5).contains(element.frame),
            "Control must not overflow the window")
    }

    @MainActor private func assertComposerViewport(_ composer: XCUIElement, in app: XCUIApplication) throws {
        // Existing chats expose model/project controls only while the native
        // editor is focused. Enter that production state before measuring it.
        try assertTarget(composer, in: app)
        composer.tap()
        let window = windowFrame(in: app)
        let navigation = app.navigationBars.containing(.button, identifier: "chat.menu").firstMatch
        let keyboard = app.keyboards.firstMatch
        guard navigation.exists, keyboard.exists, app.keyboards.count == 1 else {
            XCTFail("Require the actual Chat navigation bar and one software keyboard")
            throw NativeTextEntryIssue.unavailable
        }
        let keys = keyboard.buttons.allElementsBoundByIndex
        guard keys.count <= 160 else {
            XCTFail("Keyboard control inventory exceeds the bounded visibility check")
            throw NativeTextEntryIssue.unavailable
        }
        let visibleKey = keys.first {
            $0.exists && $0.isHittable && !$0.frame.intersection(window).isEmpty
        }
        let bar = navigation.frame, keyboardFrame = keyboard.frame, editor = composer.frame
        let required = ["chat.attach", "chat.model", "chat.send"]
        let optional = ["chat.project", "dictation.start", "chat.stop"]
        let actions = (required + optional).map { ($0, app.buttons[$0].firstMatch) }
            .filter { required.contains($0.0) || $0.1.exists }
        let frames = actions.map { $0.1.frame }
        let allFrames = [window, bar, keyboardFrame, editor] + frames
        let finite = allFrames.allSatisfy {
            [$0.minX, $0.minY, $0.width, $0.height].allSatisfy { $0.isFinite }
                && $0.width > 0 && $0.height > 0
        }
        let region = CGRect(
            x: window.minX, y: bar.maxY, width: window.width,
            height: max(0, keyboardFrame.minY - bar.maxY))
        let reachable =
            composer.exists && composer.isHittable
            && actions.allSatisfy { $0.1.exists && $0.1.isHittable }
        let targets = [editor] + frames
        let sized = targets.allSatisfy { $0.width >= 43.5 && $0.height >= 43.5 }
        let contained =
            finite && region.height > 0 && window.contains(region)
            && targets.allSatisfy { region.contains($0) }
        let separated =
            frames.allSatisfy { $0.minY >= editor.maxY }
            && frames.indices.allSatisfy { index in
                frames.indices.filter { $0 > index }.allSatisfy {
                    let overlap = frames[index].intersection(frames[$0])
                    return overlap.isNull || overlap.width <= 0 || overlap.height <= 0
                }
            }
        let clear = visibleKey != nil && finite && reachable && sized && contained && separated
        // Public identifiers, booleans and rectangles only; never attach text,
        // selected ranges, field values or arbitrary keyboard labels.
        var geometry =
            "clear=\(clear); visibleKey=\(visibleKey != nil); finite=\(finite); "
            + "reachable=\(reachable); sized=\(sized); contained=\(contained); separated=\(separated)\n"
            + "window=\(window); navigation=\(bar); keyboard=\(keyboardFrame); editor=\(editor)"
        for (action, frame) in zip(actions, frames) { geometry += "\n\(action.0)=\(frame)" }
        let attachment = XCTAttachment(string: geometry)
        attachment.name = "Localization composer viewport geometry"; attachment.lifetime = .keepAlways
        add(attachment)
        XCTAssertTrue(clear, "The editor and every composer action must clear Chat navigation and the real keyboard")
        guard clear else { throw NativeTextEntryIssue.unavailable }
    }

    private func assertDoubled(_ label: String, baseline: String) {
        XCTAssertFalse(baseline.isEmpty)
        XCTAssertGreaterThanOrEqual(label.count, baseline.count * 2, "Rendered localized label must actually expand")
        XCTAssertGreaterThanOrEqual(label.components(separatedBy: baseline).count - 1, 2)
    }

    @MainActor private func clearSynthetic(_ field: XCUIElement, in app: XCUIApplication) throws {
        guard field.waitForExistence(timeout: 15), let previous = field.value as? String,
            previous.utf16.count <= 4_096
        else { throw NativeTextEntryIssue.unavailable }
        field.tap()
        if !previous.isEmpty {
            field.press(forDuration: 1.2)
            try editMenu("Select All", in: app).tap()
            field.typeText(XCUIKeyboardKey.delete.rawValue)
        }
        let empty = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in (field.value as? String) == "" }, object: field)
        guard XCTWaiter.wait(for: [empty], timeout: 5) == .completed else {
            XCTFail("Localized native selection did not clear the complete synthetic field")
            throw NativeTextEntryIssue.clearFailed
        }
    }

    @MainActor private func pasteSynthetic(_ text: String, into field: XCUIElement, in app: XCUIApplication) throws {
        try clearSynthetic(field, in: app)
        UIPasteboard.general.setItems(
            [["public.utf8-plain-text": text]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
        field.tap(); field.press(forDuration: 1.2)
        try editMenu("Paste", in: app).tap()
        let exact = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in (field.value as? String) == text }, object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [exact], timeout: 10), .completed)
        UIPasteboard.general.items = []
    }

    @MainActor private func assertLogicalSelectionCopy(_ text: String, from field: XCUIElement, in app: XCUIApplication)
        throws
    {
        field.tap(); field.press(forDuration: 1.2)
        try editMenu("Select All", in: app).tap()
        try editMenu("Copy", in: app).tap()
        // Verify the exported selection through the app's native Paste path.
        // Direct cross-process pasteboard reads can block the test runner.
        // Copy retains the native selection. Do not tap the field again: that
        // changes the selection and can dismiss the edit menu at large text.
        app.typeText(XCUIKeyboardKey.delete.rawValue)
        let empty = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in (field.value as? String) == "" }, object: field)
        guard XCTWaiter.wait(for: [empty], timeout: 5) == .completed else {
            XCTFail("Deleting the copied selection must empty the synthetic field before Paste")
            throw NativeTextEntryIssue.clearFailed
        }
        field.tap(); field.press(forDuration: 1.2)
        try editMenu("Paste", in: app).tap()
        let restored = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in (field.value as? String)?.isEmpty == false }, object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [restored], timeout: 10), .completed)
        let copied = try XCTUnwrap(field.value as? String)
        XCTAssertEqual(
            Data(copied.utf8), Data(text.utf8),
            "Native selected Copy and Paste must preserve exact logical scalar order, not only canonical equivalence")
        UIPasteboard.general.items = []
    }

    private func hasSystemTitle(_ label: String, title: String) -> Bool {
        guard label.contains(title) else { return false }
        let rest = label.replacingOccurrences(of: title, with: "")
        return rest.unicodeScalars.allSatisfy {
            CharacterSet.whitespacesAndNewlines.union(.punctuationCharacters).contains($0)
        }
    }

    @MainActor private func visibleEditAction(_ title: String, in app: XCUIApplication) -> XCUIElement? {
        let menu = app.menuItems.matching(NSPredicate(format: "label CONTAINS %@", title))
        let buttons = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", title))
        return (menu.allElementsBoundByIndex + buttons.allElementsBoundByIndex).first {
            $0.isHittable && hasSystemTitle($0.label, title: title)
        }
    }

    @MainActor private func editMenu(_ title: String, in app: XCUIApplication) throws -> XCUIElement {
        _ = app.menuItems.firstMatch.waitForExistence(timeout: 2)
        // Expanded native menus show one action per page. The actual failure AX
        // exposes Back Back/Forward Forward beside Select Select. Seek from the
        // first page, then forward, without substituting a different action.
        for direction in ["Back", "Forward"] {
            for _ in 0..<8 {
                if let action = visibleEditAction(title, in: app) { return action }
                let visibleItems = app.menuItems.allElementsBoundByIndex.filter { $0.isHittable }
                let bounds = visibleItems.reduce(CGRect.null) { $0.union($1.frame) }.intersection(windowFrame(in: app))
                guard !bounds.isNull, !bounds.isEmpty else { break }
                let band = CGRect(
                    x: windowFrame(in: app).minX, y: bounds.minY, width: windowFrame(in: app).width,
                    height: bounds.height)
                let arrows = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", direction))
                guard
                    let arrow = arrows.allElementsBoundByIndex.first(where: {
                        $0.isHittable && $0.isEnabled && hasSystemTitle($0.label, title: direction)
                            && !$0.frame.intersection(band).isEmpty
                    })
                else { break }
                arrow.tap()
            }
        }
        if let action = visibleEditAction(title, in: app) { return action }
        XCTFail("Native edit-menu action unavailable")
        throw NativeTextEntryIssue.selectionUnavailable
    }

    @MainActor private func capture(_ app: XCUIApplication, name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
