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
    private func dragAcknowledgement(_ expected: String) {
        let state = app.staticTexts["library.fixture.alpha-state"]
        XCTAssertTrue(state.waitForExistence(timeout: 15))
        let expectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label == %@", expected), object: state)
        let result = XCTWaiter.wait(for: [expectation], timeout: 15)
        let trace = XCTAttachment(string: app.staticTexts["library.fixture.drag-trace"].label)
        trace.name = "Content-free native drag delegate events"; trace.lifetime = .keepAlways; add(trace)
        if result != .completed { screenshot("Native drag missing expected acknowledgement") }
        XCTAssertEqual(result, .completed)
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
        XCTAssertFalse(app.buttons["Move selected chats"].exists)
        XCTAssertFalse(app.buttons["library.selection-done"].exists)
        tap("library.actions"); tap("Select chats")
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
        XCTAssertFalse(app.buttons["Delete selected chats"].exists)
        XCTAssertFalse(app.buttons["library.selection-done"].exists)
        XCTAssertTrue(app.buttons["library.chat-actions.active"].isHittable)
        screenshot("Bulk selection preserves unrelated chats and project")
    }

    func testVisibleChatActionsSelectExactChatAndDoneClearsSelection() {
        launch(); openHistory(); tap("library.chat-actions.alpha")
        for action in ["Select", "Pin", "Move to project", "Rename", "Delete"] {
            XCTAssertTrue(app.buttons[action].waitForExistence(timeout: 10))
        }
        tap("Select")
        XCTAssertTrue(app.buttons["library.chat.alpha"].isSelected)
        XCTAssertFalse(app.buttons["library.chat.beta"].isSelected)
        XCTAssertTrue(app.buttons["Move selected chats"].isEnabled)
        tap("library.selection-done")
        XCTAssertFalse(app.buttons["Move selected chats"].exists)
        XCTAssertTrue(app.buttons["library.chat-actions.alpha"].isHittable)
        tap("library.actions"); tap("Select chats")
        XCTAssertFalse(app.buttons["library.chat.alpha"].isSelected)
        XCTAssertFalse(app.buttons["Move selected chats"].isEnabled)
        tap("library.selection-done")
        screenshot("Visible Done exits selection without retaining selected chats")
    }

    func testVisibleChatActionsPinAndUnpinRetainLongPressActions() {
        launch(); openHistory(); tap("library.chat-actions.alpha"); tap("Pin")
        let actions = app.buttons["library.chat-actions.alpha"]
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: actions)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed)
        tap("library.chat-actions.alpha")
        XCTAssertTrue(app.buttons["Unpin"].waitForExistence(timeout: 10)); tap("Unpin")
        let unpinned = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: actions)
        XCTAssertEqual(XCTWaiter.wait(for: [unpinned], timeout: 10), .completed)
        app.buttons["library.chat.alpha"].press(forDuration: 1)
        XCTAssertTrue(app.buttons["Pin"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["Select"].exists)
        screenshot("Ellipsis and long press share chat actions after pinning and unpinning")
    }

    func testChatSwipesNeverPinOrDeleteWithoutExplicitConfirmation() {
        launch(["--maple-library-menu-proof"])
        let alpha = app.buttons["library.chat.alpha"]
        XCTAssertTrue(alpha.waitForExistence(timeout: 15))
        alpha.swipeRight()
        XCTAssertFalse(app.buttons["Pin"].exists)
        label("library.fixture.deletion-count", equals: "deletions 0")

        // A full swipe may reveal controls, but must not invoke deletion.
        alpha.coordinate(withNormalizedOffset: CGVector(dx: 0.95, dy: 0.5))
            .press(
                forDuration: 0.02,
                thenDragTo: alpha.coordinate(withNormalizedOffset: CGVector(dx: 0.02, dy: 0.5)))
        let delete = app.buttons["library.swipe-delete.alpha"]
        XCTAssertTrue(delete.waitForExistence(timeout: 10))
        XCTAssertFalse(app.alerts["Delete chat?"].exists)
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        label("library.fixture.deletion-count", equals: "deletions 0")
        delete.tap()
        let confirmation = app.alerts["Delete chat?"]
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        XCTAssertTrue(confirmation.buttons["Cancel"].isHittable)
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        label("library.fixture.deletion-count", equals: "deletions 0")
        screenshot("Swipe Delete waits for an explicit confirmation")
        confirmation.buttons["Cancel"].tap()
        XCTAssertTrue(alpha.isHittable)
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        label("library.fixture.deletion-count", equals: "deletions 0")

        alpha.swipeLeft()
        XCTAssertTrue(delete.waitForExistence(timeout: 10)); delete.tap()
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        confirmation.buttons["Delete"].tap()
        label("library.fixture.deleted", equals: "alpha")
        label("library.fixture.remaining", equals: "Remaining beta,active,outside")
        label("library.fixture.events", equals: "delete-selected")
        label("library.fixture.deletion-count", equals: "deletions 1")
        XCTAssertFalse(alpha.exists)
        XCTAssertTrue(app.buttons["library.chat.beta"].isHittable)
        screenshot("Only the confirmed chat is removed after the swipe action")
    }

    func testEllipsisDeleteAlsoRequiresConfirmationAndCancelKeepsTheChat() {
        launch(["--maple-library-menu-proof"]); tap("library.chat-actions.alpha"); tap("Delete")
        let confirmation = app.alerts["Delete chat?"]
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        label("library.fixture.deletion-count", equals: "deletions 0")
        confirmation.buttons["Cancel"].tap()
        XCTAssertTrue(app.buttons["library.chat.alpha"].isHittable)
        label("library.fixture.remaining", equals: "Remaining alpha,beta,active,outside")
        label("library.fixture.deletion-count", equals: "deletions 0")
        tap("library.chat-actions.alpha"); tap("Delete")
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        confirmation.buttons["Delete"].tap()
        label("library.fixture.deleted", equals: "alpha")
        label("library.fixture.remaining", equals: "Remaining beta,active,outside")
        label("library.fixture.events", equals: "delete-selected")
        label("library.fixture.deletion-count", equals: "deletions 1")
    }

    func testNativeChatDragMovesBetweenProjectsPinnedAndRecent() {
        launch(["--maple-library-drag-proof"])
        let alpha = app.buttons["library.chat.alpha"]
        let first = app.buttons["library.project.fixture-project"]
        let second = app.buttons["library.project.fixture-project-2"]
        let pinned = app.staticTexts["library.drop.pinned"]
        XCTAssertTrue(alpha.waitForExistence(timeout: 15))
        XCTAssertTrue(first.isHittable && second.isHittable && alpha.isHittable)
        XCTAssertFalse(pinned.exists)
        // Establish a visible destination through the shipping chat actions.
        // XCTest resolves drag endpoints before a lift, so a target inserted
        // during the gesture cannot be located by its later accessibility frame.
        tap("library.chat-actions.beta"); tap("Pin")
        XCTAssertTrue(pinned.waitForExistence(timeout: 10) && pinned.isHittable && alpha.isHittable)
        alpha.press(forDuration: 0.8, thenDragTo: first, withVelocity: .slow, thenHoldForDuration: 0.3)
        dragAcknowledgement("alpha:fixture-project:unpinned")
        tap("library.project.fixture-project")
        XCTAssertTrue(alpha.waitForExistence(timeout: 10) && alpha.isHittable)
        XCTAssertGreaterThanOrEqual(alpha.frame.minX, first.frame.minX + 28)
        XCTAssertGreaterThanOrEqual(alpha.frame.height, 50 - 0.01)
        alpha.press(forDuration: 0.8, thenDragTo: second, withVelocity: .slow, thenHoldForDuration: 0.3)
        dragAcknowledgement("alpha:fixture-project-2:unpinned")
        tap("library.project.fixture-project-2")
        XCTAssertTrue(alpha.waitForExistence(timeout: 10) && alpha.isHittable)

        let recent = app.staticTexts["library.drop.recent"]
        XCTAssertTrue(pinned.isHittable)
        alpha.press(forDuration: 0.8, thenDragTo: pinned, withVelocity: .slow, thenHoldForDuration: 0.3)
        dragAcknowledgement("alpha:fixture-project-2:pinned")
        XCTAssertEqual(app.buttons.matching(identifier: "library.chat.alpha").count, 1)
        XCTAssertTrue(pinned.isHittable && recent.isHittable)
        alpha.press(forDuration: 0.8, thenDragTo: recent, withVelocity: .slow, thenHoldForDuration: 0.3)
        dragAcknowledgement("alpha:recent:unpinned")
        XCTAssertEqual(app.buttons.matching(identifier: "library.chat.alpha").count, 1)
        tap("library.chat-actions.beta"); tap("Unpin")
        let emptyPinnedGone = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"), object: pinned)
        XCTAssertEqual(XCTWaiter.wait(for: [emptyPinnedGone], timeout: 10), .completed)
        screenshot("Native drag moves exact chat between folders, Pinned and Recent")
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
