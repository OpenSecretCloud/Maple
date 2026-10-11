import XCTest

/// A SwiftUI Form exposes the full toggle row as an accessibility switch.
/// Its center can be the label, so target the actual trailing native control.
@MainActor func tapNativeSwitch(_ row: XCUIElement, in app: XCUIApplication) {
    XCTAssertTrue(row.isEnabled && row.isHittable)
    let frame = row.frame
    if (20...100).contains(frame.width), (20...100).contains(frame.height) {
        row.tap(); return
    }
    let children = row.switches.allElementsBoundByIndex.filter {
        $0.isHittable && (20...100).contains($0.frame.width)
            && (20...100).contains($0.frame.height) && frame.contains($0.frame)
    }
    XCTAssertLessThanOrEqual(children.count, 1)
    if let control = children.first {
        control.tap()
    } else {
        XCTAssertGreaterThanOrEqual(frame.width, 160)
        let point = CGPoint(x: frame.maxX - 48, y: frame.midY)
        XCTAssertTrue(frame.contains(point) && app.windows.firstMatch.frame.contains(point))
        row.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 0.5))
            .withOffset(CGVector(dx: -48, dy: 0)).tap()
    }
}

enum NativeTextEntryIssue: Error { case unavailable, selectionUnavailable, clearFailed, replacementFailed }

/// Only synthetic fixture text belongs here. Credentials use the separate private
/// paste path. Hardware Command-A is not an iPhone text-replacement operation.
@MainActor func replaceSyntheticText(
    _ field: XCUIElement, with text: String, in app: XCUIApplication,
    presentedAlert: XCUIElement? = nil, centeredNativeFocus: Bool = false, beforeTyping: (() throws -> Void)? = nil,
    file: StaticString = #filePath, line: UInt = #line
) throws {
    guard field.waitForExistence(timeout: 20), text.utf16.count <= 4_096,
        let previous = field.value as? String, previous.utf16.count <= 4_096
    else {
        XCTFail("Synthetic text field unavailable or outside fixture bounds", file: file, line: line)
        throw NativeTextEntryIssue.unavailable
    }
    try interactWithSyntheticField(
        field, in: app, presentedAlert: presentedAlert, longPress: false, centeredNativeFocus: centeredNativeFocus)
    if !previous.isEmpty {
        if let presentedAlert {
            // Single-line alert fields may place a caret without opening an edit
            // menu. Use native input only, then prove the complete field is empty.
            try interactWithSyntheticField(
                field, in: app, presentedAlert: presentedAlert, longPress: false, trailingInsertion: true)
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.utf16.count))
        } else {
            try interactWithSyntheticField(field, in: app, presentedAlert: presentedAlert, longPress: true)
            let menu = app.menuItems["Select All"].firstMatch
            let button = app.buttons["Select All"].firstMatch
            if menu.waitForExistence(timeout: 2) {
                try activateSyntheticSelection(menu, field: field, in: app, presentedAlert: presentedAlert)
            } else if button.waitForExistence(timeout: 2) {
                try activateSyntheticSelection(button, field: field, in: app, presentedAlert: presentedAlert)
            } else {
                XCTFail("Native Select All action unavailable; refusing to append fixture text", file: file, line: line)
                throw NativeTextEntryIssue.selectionUnavailable
            }
            field.typeText(XCUIKeyboardKey.delete.rawValue)
        }
        let cleared = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let value = field.value as? String
                if value == "" { return true }
                // An empty alert field can expose its placeholder as its AX value.
                // Require the owning alert's disabled Save as independent evidence.
                guard let presentedAlert, presentedAlert.exists, presentedAlert.elementType == .alert,
                    app.alerts.count == 1, presentedAlert.textFields.count == 1,
                    field.exists, field.elementType == .textField,
                    let placeholder = field.placeholderValue, !placeholder.isEmpty, value == placeholder
                else { return false }
                let ownedField = presentedAlert.textFields.element(boundBy: 0)
                let saves = presentedAlert.buttons.matching(NSPredicate(format: "label == %@", "Save"))
                let frame = field.frame
                guard ownedField.exists, ownedField.identifier == field.identifier,
                    ownedField.frame == frame, frame.width > 0, frame.height > 0,
                    presentedAlert.frame.contains(frame), ownedField.placeholderValue == placeholder,
                    (ownedField.value as? String) == value, saves.count == 1
                else { return false }
                let save = saves.element(boundBy: 0)
                return save.exists && !save.isEnabled
            }, object: field)
        guard XCTWaiter.wait(for: [cleared], timeout: 5) == .completed else {
            XCTFail("Native selection did not clear the complete synthetic field", file: file, line: line)
            throw NativeTextEntryIssue.clearFailed
        }
    }
    try beforeTyping?()
    if !text.isEmpty { field.typeText(text) }
    let replaced = XCTNSPredicateExpectation(
        predicate: NSPredicate { _, _ in (field.value as? String) == text }, object: field)
    guard XCTWaiter.wait(for: [replaced], timeout: 10) == .completed else {
        XCTFail("Synthetic replacement did not match exactly; refusing to submit", file: file, line: line)
        throw NativeTextEntryIssue.replacementFailed
    }
}

/// Alert-owned field events stay anchored to the expected alert. Like the edit
/// menu action below, a direct element event may otherwise trigger XCTest's
/// interruption path before UIKit receives the intended field gesture.
@MainActor private func interactWithSyntheticField(
    _ field: XCUIElement, in app: XCUIApplication, presentedAlert: XCUIElement?, longPress: Bool,
    trailingInsertion: Bool = false, centeredNativeFocus: Bool = false
) throws {
    guard let presentedAlert else {
        if longPress {
            field.press(forDuration: 1.2)
        } else if centeredNativeFocus {
            guard field.exists, field.isEnabled, field.isHittable else { throw NativeTextEntryIssue.unavailable }
            let fieldFrame = field.frame, appFrame = app.frame
            guard
                [fieldFrame, appFrame].allSatisfy({
                    $0.minX.isFinite && $0.minY.isFinite && $0.maxX.isFinite && $0.maxY.isFinite
                        && $0.width.isFinite && $0.height.isFinite && $0.width > 0 && $0.height > 0
                }), appFrame.contains(fieldFrame)
            else { throw NativeTextEntryIssue.unavailable }
            // One ordinary event inside the observed editor, without a second focus tap.
            app.coordinate(withNormalizedOffset: .zero).withOffset(
                CGVector(dx: fieldFrame.midX - appFrame.minX, dy: fieldFrame.midY - appFrame.minY)
            ).tap()
        } else {
            field.tap()
        }
        return
    }
    guard presentedAlert.exists, presentedAlert.elementType == .alert,
        app.alerts.count == 1, presentedAlert.textFields.count == 1,
        field.exists, field.elementType == .textField, field.isEnabled, field.isHittable
    else { throw NativeTextEntryIssue.selectionUnavailable }
    let ownedField = presentedAlert.textFields.element(boundBy: 0)
    let ownerFrame = presentedAlert.frame, fieldFrame = field.frame, appFrame = app.frame
    let frames = [ownerFrame, fieldFrame, appFrame]
    guard ownedField.exists, ownedField.identifier == field.identifier, ownedField.frame == fieldFrame,
        frames.allSatisfy({
            $0.minX.isFinite && $0.minY.isFinite && $0.width.isFinite && $0.height.isFinite
                && $0.width > 0 && $0.height > 0
        }), ownerFrame.contains(fieldFrame), appFrame.contains(ownerFrame)
    else { throw NativeTextEntryIssue.selectionUnavailable }
    // The trailing point is inside the observed field. Scrolled or RTL text can
    // still leave a suffix; the unchanged empty-value gate must reject that.
    let insertionX = trailingInsertion ? fieldFrame.maxX - min(8, fieldFrame.width / 4) : fieldFrame.midX
    let coordinate = presentedAlert.coordinate(withNormalizedOffset: .zero).withOffset(
        CGVector(dx: insertionX - ownerFrame.minX, dy: fieldFrame.midY - ownerFrame.minY))
    if longPress { coordinate.press(forDuration: 1.2) } else { coordinate.tap() }
    guard presentedAlert.exists, field.exists else { throw NativeTextEntryIssue.selectionUnavailable }
}

/// An edit menu appears outside its alert's accessibility subtree. Element.tap()
/// treats that expected alert as an interruption and dismisses it. Anchor the
/// native event to the observed owning alert and the current action frame.
@MainActor private func activateSyntheticSelection(
    _ action: XCUIElement, field: XCUIElement, in app: XCUIApplication, presentedAlert: XCUIElement?
) throws {
    guard let presentedAlert else {
        action.tap()
        return
    }
    guard presentedAlert.exists, app.alerts.count == 1, presentedAlert.textFields.count == 1,
        field.exists, action.exists, action.isEnabled, action.isHittable
    else { throw NativeTextEntryIssue.selectionUnavailable }
    let ownerFrame = presentedAlert.frame, fieldFrame = field.frame, actionFrame = action.frame
    let appFrame = app.frame
    let frames = [ownerFrame, fieldFrame, actionFrame, appFrame]
    guard
        frames.allSatisfy({
            $0.minX.isFinite && $0.minY.isFinite && $0.width.isFinite && $0.height.isFinite
                && $0.width > 0 && $0.height > 0
        }), ownerFrame.contains(fieldFrame), appFrame.contains(actionFrame)
    else { throw NativeTextEntryIssue.selectionUnavailable }
    presentedAlert.coordinate(withNormalizedOffset: .zero).withOffset(
        CGVector(dx: actionFrame.midX - ownerFrame.minX, dy: actionFrame.midY - ownerFrame.minY)
    ).tap()
    guard presentedAlert.exists, field.exists else { throw NativeTextEntryIssue.selectionUnavailable }
}
