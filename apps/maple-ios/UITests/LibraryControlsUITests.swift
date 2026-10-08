import XCTest

/// These cases never opt into the local account. The DEBUG fixture constructs
/// isolated in-memory services and invokes the shipping Library destinations.
@MainActor final class LibraryControlsUITests: XCTestCase {
    private var app: XCUIApplication!
    private func launch(_ extra: [String] = []) {
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-library-controls-fixture"] + extra; app.launch()
        label("library.fixture.ready", equals: "ready")
    }
    private func label(_ id: String, equals value: String, timeout: TimeInterval = 15) {
        let element = app.staticTexts[id]
        XCTAssertTrue(element.waitForExistence(timeout: timeout))
        let expectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", value), object: element)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: timeout), .completed)
    }
    private func tap(_ identifier: String) {
        let button = app.buttons[identifier].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        for _ in 0..<4 where !button.isHittable { app.swipeUp() }
        XCTAssertTrue(button.isHittable); button.tap()
    }
    private func openHistory() {
        tap("library.fixture.history")
        XCTAssertTrue(app.buttons["library.chat.alpha"].waitForExistence(timeout: 15))
    }
    private func selectAlphaAndBeta() {
        tap("Alpha chat"); tap("Beta chat")
    }
    private func openDeleteAll() { tap("library.actions"); tap("Delete all chats") }
    private func confirmTypedDelete() {
        let field = app.textFields["library.delete-confirmation"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["library.delete-confirm"].isEnabled)
        field.tap(); field.typeText("DELETE"); tap("library.delete-confirm")
    }
    private func openProjectDeletion() {
        tap("library.fixture.project")
        XCTAssertTrue(app.buttons["library.chat.active"].waitForExistence(timeout: 15))
        tap("Project actions"); tap("Delete project")
    }
    private func screenshot(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }

    func testHistoryPaginationFailureRetainsRowsAndRetryLoadsNextPageOnce() {
        launch(["--maple-library-controls-page-failure"]); openHistory()
        XCTAssertFalse(app.buttons["library.chat.outside"].exists)
        tap("Load more")
        XCTAssertTrue(app.staticTexts["library.error"].waitForExistence(timeout: 15))
        XCTAssertTrue(app.buttons["library.chat.alpha"].exists)
        XCTAssertTrue(app.buttons["library.chat.beta"].exists)
        tap("Load more")
        XCTAssertTrue(app.buttons["library.chat.outside"].waitForExistence(timeout: 15))
        XCTAssertEqual(app.buttons.matching(identifier: "library.chat.alpha").count, 1)
        XCTAssertEqual(app.buttons.matching(identifier: "library.chat.active").count, 1)
        XCTAssertFalse(app.buttons["Load more"].exists)
        label("library.fixture.pages", equals: "pages 3")
        XCTAssertFalse(app.staticTexts["library.error"].exists)
        screenshot("History pagination retry preserves previous rows")
    }

    func testMultiSelectionMovesAndDeletesOnlySelectedChats() {
        launch(); openHistory(); tap("library.actions"); tap("Select chats")
        selectAlphaAndBeta(); tap("Move selected chats"); tap("Synthetic project")
        label("library.fixture.moved", equals: "alpha,beta to fixture-project")
        selectAlphaAndBeta(); tap("Delete selected chats")
        XCTAssertTrue(app.staticTexts["Delete selected chats?"].waitForExistence(timeout: 10))
        let confirmationReady = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.buttons.matching(identifier: "Delete selected chats").allElementsBoundByIndex.contains {
                    $0.isHittable
                }
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [confirmationReady], timeout: 10), .completed)
        guard
            let confirm = app.buttons.matching(identifier: "Delete selected chats").allElementsBoundByIndex.first(
                where: { $0.isHittable })
        else {
            XCTFail("The native deletion confirmation is not hittable"); return
        }
        confirm.tap()
        label("library.fixture.deleted", equals: "alpha,beta")
        label("library.fixture.remaining", equals: "Remaining active,outside")
        label("library.fixture.project-state", equals: "project retained")
        screenshot("Bulk selection preserves unrelated chats and project")
    }

    func testDeleteAllRequiresTypedIntentAndIncludesUnloadedChatsButKeepsProjects() {
        launch(); openHistory(); openDeleteAll()
        let field = app.textFields["library.delete-confirmation"]
        XCTAssertTrue(field.waitForExistence(timeout: 10)); field.tap(); field.typeText("DELET")
        XCTAssertFalse(app.buttons["library.delete-confirm"].isEnabled)
        tap("Cancel")
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        openDeleteAll(); confirmTypedDelete()
        label("library.fixture.remaining", equals: "Remaining none")
        label("library.fixture.project-state", equals: "project retained")
        label("library.fixture.events", equals: "delete-all")
        XCTAssertTrue(app.staticTexts["No chats yet"].waitForExistence(timeout: 10))
        screenshot("Delete all covers unloaded chats and preserves projects")
    }

    func testProjectDeletionCancelsExactActiveResponseBeforeBackendMutation() {
        launch(["--maple-library-controls-active"]); openProjectDeletion(); confirmTypedDelete()
        label("library.fixture.events", equals: "post|cancel-request|cancel-confirm|delete-project")
        label("library.fixture.remaining", equals: "Remaining alpha,beta,outside")
        label("library.fixture.project-state", equals: "project deleted")
        label("library.fixture.owner", equals: "removed")
        label("library.fixture.draft", equals: "Draft empty")
        screenshot("Active project deletion waits for cancellation and retires owner")
    }

    func testFailedActiveProjectDeletionKeepsDraftAndQueuePausedForRetry() {
        launch(["--maple-library-controls-active", "--maple-library-controls-delete-failure"])
        openProjectDeletion(); confirmTypedDelete()
        XCTAssertTrue(app.staticTexts["library.error"].waitForExistence(timeout: 15))
        tap("Cancel")
        label("library.fixture.events", equals: "post|cancel-request|cancel-confirm|delete-failed")
        label("library.fixture.owner", equals: "retained")
        label("library.fixture.queue", equals: "paused")
        label("library.fixture.queue-items", equals: "Synthetic queued request")
        label("library.fixture.draft", equals: "Draft Unsent draft survives a failed deletion")
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        tap("Project actions"); tap("Delete project"); confirmTypedDelete()
        label("library.fixture.events", equals: "post|cancel-request|cancel-confirm|delete-failed|delete-project")
        label("library.fixture.owner", equals: "removed")
        label("library.fixture.remaining", equals: "Remaining alpha,beta,outside")
    }
}
