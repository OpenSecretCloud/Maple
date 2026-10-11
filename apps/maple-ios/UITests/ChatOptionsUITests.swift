import XCTest

/// Real sheet/navigation and account-owned LibraryModel over an isolated
/// in-memory service. These tests never construct a client or network request.
@MainActor final class ChatOptionsUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launch() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--maple-library-controls-fixture", "--maple-chat-options-proof"]
        app.launch()
        waitLabel("library.fixture.ready", "ready")
    }

    private func tap(_ identifier: String) {
        let button = app.buttons[identifier].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        XCTAssertTrue(button.isEnabled, "Disabled control: " + identifier)
        for _ in 0..<4 where !button.isHittable { app.swipeUp() }
        XCTAssertTrue(button.isHittable); button.tap()
    }

    private func waitLabel(_ identifier: String, _ value: String) {
        let element = app.staticTexts[identifier]
        XCTAssertTrue(element.waitForExistence(timeout: 15))
        let expected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", value), object: element)
        XCTAssertEqual(XCTWaiter.wait(for: [expected], timeout: 15), .completed)
    }

    private func optionsRoot() {
        XCTAssertTrue(app.buttons["chat-options.project"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.switches["model.web-search"].exists)
        XCTAssertTrue(app.buttons["settings.model.auto:quick"].exists)
        XCTAssertFalse(app.staticTexts["Default model"].exists)
        XCTAssertFalse(app.buttons["Done"].exists)
    }

    private func back() {
        let button = app.navigationBars.buttons.firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 10)); button.tap()
    }

    private func editName(_ name: String, replacing previous: String? = nil) {
        let field = app.textFields["project-editor.name"]
        XCTAssertTrue(field.waitForExistence(timeout: 10)); field.tap()
        if let previous { field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count)) }
        field.typeText(name); tap("project-editor.save")
    }

    private func assertSheetFrame(
        _ actual: CGRect, equals expected: CGRect, file: StaticString = #filePath, line: UInt = #line
    ) {
        XCTAssertGreaterThan(actual.width, 0, file: file, line: line)
        XCTAssertGreaterThan(actual.height, 0, file: file, line: line)
        XCTAssertEqual(actual.minX, expected.minX, accuracy: 1, file: file, line: line)
        XCTAssertEqual(actual.minY, expected.minY, accuracy: 1, file: file, line: line)
        XCTAssertEqual(actual.width, expected.width, accuracy: 1, file: file, line: line)
        XCTAssertEqual(actual.maxY, expected.maxY, accuracy: 1, file: file, line: line)
    }

    private func recordFrames(_ name: String, _ frames: String) {
        let attachment = XCTAttachment(string: frames)
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testProjectSelectionAndWebPreferenceKeepOptionsOpenUntilModelChoice() {
        launch(); tap("chat-options.fixture.open"); optionsRoot()
        let models = app.collectionViews["settings.models"]
        XCTAssertTrue(models.waitForExistence(timeout: 10))
        let rootScreenshot = XCTAttachment(screenshot: app.screenshot())
        rootScreenshot.name = "Options root for native sheet spacing review"
        rootScreenshot.lifetime = .keepAlways; add(rootScreenshot)
        let outerSheet = app.otherElements.matching(identifier: "chat.options-sheet").allElementsBoundByIndex.first {
            let frame = $0.frame
            return frame.width.isFinite && frame.height.isFinite && frame.width > 0 && frame.height > 0
        }
        let boundaryKind = outerSheet == nil ? "content canvas (root list / destination list and bar)" : "outer sheet"
        let initialSheetFrame = outerSheet?.frame ?? models.frame
        let project = app.buttons["chat-options.project"]
        recordFrames(
            "Options root geometry boundary",
            "boundary=\(boundaryKind)\ninitial=\(initialSheetFrame)\nmodels=\(models.frame)\nproject=\(project.frame)\nwindow=\(app.windows.firstMatch.frame)"
        )
        XCTAssertGreaterThan(initialSheetFrame.width, 0)
        XCTAssertGreaterThan(initialSheetFrame.height, 0)
        XCTAssertGreaterThanOrEqual(project.frame.minY, initialSheetFrame.minY)
        XCTAssertLessThanOrEqual(
            project.frame.minY - initialSheetFrame.minY, 64,
            "Project must stay near the measured \(boundaryKind) top")
        XCTAssertLessThanOrEqual(project.frame.maxY, initialSheetFrame.maxY)
        let toggle = app.switches["model.web-search"]
        XCTAssertEqual(toggle.value as? String, "1"); tapNativeSwitch(toggle, in: app)
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", "0"), object: toggle)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 10), .completed); optionsRoot()
        tap("chat-options.project")
        XCTAssertTrue(app.buttons["project-picker.project.none"].waitForExistence(timeout: 10))
        let projectList = app.collectionViews["project-picker.screen"]
        let projectBar = app.navigationBars["Projects"]
        XCTAssertTrue(projectList.waitForExistence(timeout: 10))
        XCTAssertTrue(projectBar.waitForExistence(timeout: 10))
        let projectSheetFrame: CGRect
        if let outerSheet {
            XCTAssertTrue(outerSheet.exists)
            projectSheetFrame = outerSheet.frame
        } else {
            projectSheetFrame = projectList.frame.union(projectBar.frame)
        }
        recordFrames(
            "Project destination geometry boundary",
            "boundary=\(boundaryKind)\ninitial=\(initialSheetFrame)\ndestination=\(projectSheetFrame)\nlist=\(projectList.frame)\nbar=\(projectBar.frame)"
        )
        assertSheetFrame(projectSheetFrame, equals: initialSheetFrame)
        back(); optionsRoot()
        if let outerSheet { XCTAssertTrue(outerSheet.exists) }
        let returnedSheetFrame = outerSheet?.frame ?? models.frame
        assertSheetFrame(returnedSheetFrame, equals: initialSheetFrame)
        tap("chat-options.project"); tap("project-picker.project.fixture-project")
        optionsRoot()
        tap("chat-options.project"); tap("project-picker.project.none")
        optionsRoot(); tap("settings.model.auto:powerful")
        waitLabel("chat-options.fixture.project", "none")
        waitLabel("chat-options.fixture.model", "auto:powerful")
        waitLabel("chat-options.fixture.web", "web off")
        XCTAssertFalse(app.buttons["chat-options.project"].exists)
        tap("chat-options.fixture.open"); optionsRoot()
        XCTAssertEqual(app.switches["model.web-search"].value as? String, "0")
    }

    func testCreateRenameInstructionsAndConfirmedDeleteStayInsideProjectFlow() {
        launch(); tap("chat-options.fixture.open"); tap("chat-options.project")
        tap("project-picker.new-project"); editName("Options project")
        optionsRoot()
        tap("chat-options.project")
        let existingProjectActions = app.buttons["project-picker.actions.fixture-project"]
        XCTAssertTrue(existingProjectActions.waitForExistence(timeout: 10))
        XCTAssertGreaterThanOrEqual(existingProjectActions.frame.width, 44)
        XCTAssertGreaterThanOrEqual(existingProjectActions.frame.height, 44)
        tap("project-picker.actions.fixture-project")
        XCTAssertTrue(app.navigationBars["Synthetic project"].waitForExistence(timeout: 10))
        back()
        XCTAssertTrue(
            app.buttons["project-picker.project.created-project-1"].isSelected,
            "Opening project management must preserve the selected destination")
        tap("project-picker.actions.created-project-1")
        tap("project-editor.rename"); editName("Renamed options", replacing: "Options project")
        XCTAssertTrue(app.navigationBars["Renamed options"].waitForExistence(timeout: 10))
        tap("project-editor.instructions")
        let editor = app.textViews["library.instructions-text"]
        XCTAssertTrue(editor.waitForExistence(timeout: 10)); editor.tap(); editor.typeText("Synthetic instructions")
        tap("library.instructions-save")
        XCTAssertTrue(app.buttons["project-editor.instructions"].waitForExistence(timeout: 10))
        tap("project-editor.instructions")
        XCTAssertTrue(editor.waitForExistence(timeout: 10))
        XCTAssertEqual(editor.value as? String, "Synthetic instructions"); tap("library.instructions-cancel")
        tap("project-editor.delete")
        let confirmation = app.textFields["library.delete-confirmation"]
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["library.delete-confirm"].isEnabled)
        confirmation.tap(); confirmation.typeText("DELETE"); tap("library.delete-confirm")
        XCTAssertTrue(app.buttons["project-picker.project.none"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.buttons["project-picker.project.created-project-1"].exists)
        XCTAssertTrue(app.buttons["project-picker.project.fixture-project"].exists)
        back(); optionsRoot(); tap("settings.model.auto:quick")
        waitLabel("chat-options.fixture.project", "none")
    }

    func testMainMenuManagementUsesSameProjectScreensWithoutChangingChatSelection() {
        launch(); tap("chat-options.fixture.manage")
        tap("project-picker.new-project"); editName("Menu project")
        XCTAssertTrue(app.buttons["project-picker.manage.created-project-1"].waitForExistence(timeout: 10))
        tap("project-picker.manage.created-project-1"); tap("project-editor.rename")
        editName("Menu renamed", replacing: "Menu project")
        XCTAssertTrue(app.navigationBars["Menu renamed"].waitForExistence(timeout: 10))
        back()
        XCTAssertTrue(app.buttons["project-picker.manage.created-project-1"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["project-picker.project.none"].exists)
    }
}
