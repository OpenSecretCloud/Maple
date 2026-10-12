import Foundation
import XCTest

/// Run only against the dedicated local account already signed into this exact
/// simulator. Each test changes its own uniquely named resources; never delete all.
@MainActor final class LibraryWorkflowUITests: XCTestCase {
    private var app: XCUIApplication!
    private var currentProjectID: String?
    private var currentChatID: String?
    private var cleanupRegistered = false
    private var observedCleanupOwner: String?

    private func launchLocal() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1")
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-library-local-cleanup"]; app.launch()
        XCTAssertTrue(app.buttons["chat.new"].waitForExistence(timeout: 30))
        observedCleanupOwner = try accountMarker()
        try writeJournal(ownedJournal())
        if !cleanupRegistered {
            cleanupRegistered = true
            let cleanup: @MainActor @Sendable () throws -> Void = { [weak self] in
                guard let self else { return }
                try self.cleanupJournaledEntities()
            }
            addTeardownBlock { try await cleanup() }
        }
        // A previous interrupted test must be cleaned before creating another resource.
        try cleanupJournaledEntities()
    }
    private func referenceScreenshot(_ surface: String) {
        let capture = XCTAttachment(screenshot: app.screenshot())
        capture.name = "Reference — actual local Library — " + surface
        capture.lifetime = .keepAlways; add(capture)
    }
    private func uniqueName(_ suffix: String) -> String {
        let value = "Native UI \(UUID().uuidString.prefix(8)) \(suffix)"
        let record = XCTAttachment(string: value)
        record.name = "Synthetic local resource for exact cleanup"; record.lifetime = .keepAlways; add(record)
        return value
    }
    private func tap(_ label: String) {
        let button = label == "chat.menu" ? app.mapleMenuButton : app.buttons[label].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 15))
        for _ in 0..<5 where !button.isHittable { app.swipeUp() }
        XCTAssertTrue(button.isHittable); button.tap()
    }
    private func replace(_ field: XCUIElement, with text: String, presentedAlert: XCUIElement? = nil) throws {
        try replaceSyntheticText(field, with: text, in: app, presentedAlert: presentedAlert)
    }
    private func openMenu() {
        tap("chat.menu"); XCTAssertTrue(app.buttons["library.new-project"].waitForExistence(timeout: 20))
    }
    private func createProject(_ name: String) throws {
        tap("library.new-project")
        let field = app.textFields["project-editor.name"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["project-editor.save"].isEnabled)
        field.tap(); field.typeText(name)
        referenceScreenshot("new-project form")
        tap("project-editor.save")
        let project = app.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "library.project.", name)
        ).firstMatch
        XCTAssertTrue(project.waitForExistence(timeout: 20))
        let id = String(project.identifier.dropFirst("library.project.".count))
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        try rememberProject(id); currentProjectID = id
        XCTAssertTrue(app.staticTexts[name].firstMatch.waitForExistence(timeout: 20))
    }
    private func openProject(_ name: String) {
        let owner = app.staticTexts["library.cleanup-account"]
        guard let currentProjectID, UUID(uuidString: currentProjectID) != nil,
            let observedCleanupOwner, owner.exists, owner.label == observedCleanupOwner
        else {
            XCTFail("The current project and account owner must remain available")
            return
        }
        let title = app.buttons.matching(identifier: "library.project." + currentProjectID)
            .matching(NSPredicate(format: "label == %@", name)).firstMatch
        XCTAssertTrue(title.waitForExistence(timeout: 20))
        for _ in 0..<4 where !title.isHittable { app.swipeDown() }
        XCTAssertTrue(title.isHittable)
        title.press(forDuration: 1)
        let manage = app.buttons["library.project.manage." + currentProjectID].firstMatch
        XCTAssertTrue(manage.waitForExistence(timeout: 15))
        for _ in 0..<5 where !manage.isHittable { app.swipeUp() }
        XCTAssertTrue(manage.isHittable); manage.tap()
        XCTAssertTrue(app.navigationBars[name].waitForExistence(timeout: 15))
    }
    private func deleteCurrentProject() {
        tap("project-editor.delete")
        let confirmation = app.textFields["library.delete-confirmation"]
        XCTAssertTrue(confirmation.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["library.delete-confirm"].isEnabled)
        referenceScreenshot("project deletion warning before confirmation")
        confirmation.tap(); confirmation.typeText("DELETE"); tap("library.delete-confirm")
        let deleted = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.buttons["library.new-project"].isHittable
                    && self.currentProjectID.map { !self.app.buttons["library.project." + $0].exists } == true
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [deleted], timeout: 30), .completed)
        XCTAssertFalse(app.staticTexts["library.error"].exists)
        if let currentProjectID {
            do { try forgetProject(currentProjectID) } catch {
                XCTFail("Could not update exact cleanup journal: \(error)")
            }
        }
    }
    private func row(named title: String) -> XCUIElement {
        app.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "library.chat.", title)
        ).firstMatch
    }

    private func dismissChatOptionsWithSelectedModel() throws {
        // Re-select the existing model to verify dismissal without changing the
        // account preference. The real-root acceptance tests outside dismissal.
        let models = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "settings.model."))
        guard let selected = models.allElementsBoundByIndex.first(where: { $0.isSelected && $0.isEnabled }) else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
        for _ in 0..<5 where !selected.isHittable { app.swipeUp() }
        guard selected.isHittable else { throw LibraryCleanupIssue.inaccessibleControl }
        selected.tap()
        try cleanupWait {
            !self.app.buttons["chat-options.project"].exists && self.app.buttons["chat.model"].isHittable
        }
    }

    private func openChatActions(_ id: String) throws {
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        let projects = try ownedJournal().projectIDs.sorted()
        guard try MenuChatUI.find(id, in: app, projectIDs: projects, pageLimit: 20) != nil else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
        try cleanupTap("library.chat-actions." + id)
    }

    private func requireEmptyProject(_ id: String) throws {
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        try MenuChatUI.expand(id, in: app, pageLimit: 20)
        let probe = app.staticTexts["library.cleanup-project." + id]
        guard probe.waitForExistence(timeout: 15) else { throw LibraryCleanupIssue.inaccessibleControl }
        try cleanupWait { probe.label != "loading" || self.app.staticTexts["library.error"].exists }
        guard probe.label != "error", !app.staticTexts["library.error"].exists else {
            throw LibraryCleanupIssue.loadingFailed
        }
        // The raw owner includes pinned children and all page state. An empty
        // filtered menu or collapsed folder alone is never deletion authority.
        guard probe.label == "empty" else { throw LibraryCleanupIssue.unjournaledProjectChat }
    }

    private func soleProjectChat(_ id: String) throws -> XCUIElement {
        try MenuChatUI.expand(id, in: app, pageLimit: 20)
        let probe = app.staticTexts["library.cleanup-project." + id]
        try cleanupWait { probe.exists && probe.label != "loading" }
        guard probe.label == "nonempty", !app.staticTexts["library.error"].exists else {
            throw LibraryCleanupIssue.loadingFailed
        }
        let root = app.collectionViews["library.sidebar-list"]
        let folder = root.buttons["library.project." + id]
        guard folder.exists, folder.isHittable else { throw LibraryCleanupIssue.inaccessibleControl }
        let followingBoundary = root.descendants(matching: .any).allElementsBoundByIndex.filter { item in
            let projectID = String(item.identifier.dropFirst("library.project.".count))
            let isFolder = item.identifier.hasPrefix("library.project.") && UUID(uuidString: projectID) != nil
            return item.exists
                && (isFolder || item.identifier == "library.drop.pinned"
                    || item.identifier == "library.drop.recent")
                && item.frame.minY >= folder.frame.maxY
        }.map(\.frame.minY).min()
        guard let followingBoundary else { throw LibraryCleanupIssue.inaccessibleControl }
        let children = root.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "library.chat.")
        ).allElementsBoundByIndex.filter {
            $0.exists && $0.frame.minY >= folder.frame.maxY && $0.frame.maxY <= followingBoundary
        }
        guard children.count == 1, let row = children.first,
            UUID(uuidString: String(row.identifier.dropFirst("library.chat.".count))) != nil
        else { throw LibraryCleanupIssue.unjournaledProjectChat }
        return row
    }

    func testLocalComposerProjectPickerCreatesSelectsClearsAndDeletesOwnedProject() throws {
        try launchLocal()
        let project = uniqueName("picker project")
        tap("chat.new")
        let composer = app.textViews["chat.composer"]
        XCTAssertTrue(composer.waitForExistence(timeout: 15))
        let draft = try XCTUnwrap(composer.value as? String)
        tap("chat.model"); tap("chat-options.project")
        let picker = app.navigationBars["Projects"]
        XCTAssertTrue(picker.waitForExistence(timeout: 15))
        tap("project-picker.new-project")
        let field = app.textFields["project-editor.name"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["project-editor.save"].isEnabled)
        field.tap(); field.typeText(project)
        referenceScreenshot("composer project picker create")
        tap("project-editor.save")
        let returned = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                !picker.exists && !field.exists && self.app.buttons["chat-options.project"].isHittable
            }, object: app)
        let creationReturned = XCTWaiter.wait(for: [returned], timeout: 20)
        // Record the returned UUID before asserting dismissal/selection, so an
        // observed created project remains owned by the existing teardown.
        if !picker.exists { tap("chat-options.project") }
        let createdRows = app.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@", "project-picker.project.", project))
        let created = createdRows.firstMatch
        XCTAssertTrue(created.waitForExistence(timeout: 20))
        XCTAssertEqual(createdRows.count, 1, "The unique synthetic project must resolve to exactly one picker row")
        let id = String(created.identifier.dropFirst("project-picker.project.".count))
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        try rememberProject(id); currentProjectID = id
        XCTAssertEqual(creationReturned, .completed, "Creating a project must return to Chat options")
        XCTAssertTrue(created.isSelected, "The returned project must be selected in the composer")
        referenceScreenshot("composer created project selected")
        tap("project-picker.project.none")
        try cleanupWait { !picker.exists && self.app.buttons["chat-options.project"].isHittable }
        tap("chat-options.project")
        let none = app.buttons["project-picker.project.none"]
        XCTAssertTrue(none.waitForExistence(timeout: 15))
        XCTAssertTrue(none.isSelected, "No project selection must be retained after reopening")
        XCTAssertFalse(created.isSelected)
        referenceScreenshot("composer no project selected")
        tap("project-picker.project.none")
        try cleanupWait { !picker.exists && self.app.buttons["chat-options.project"].isHittable }
        try dismissChatOptionsWithSelectedModel()
        XCTAssertEqual(composer.value as? String, draft)
        openMenu(); try requireEmptyProject(id); openProject(project); deleteCurrentProject()
        XCTAssertFalse(app.buttons["library.project." + id].exists)
    }

    func testLocalProjectInstructionsSaveDiscardRenameAndExactDeletion() throws {
        try launchLocal()
        let project = uniqueName("project"), renamed = project + " renamed"
        openMenu()
        referenceScreenshot("sidebar before creating the owned project")
        try createProject(project); openProject(project)
        tap("project-editor.instructions")
        let editor = app.textViews["library.instructions-text"]
        try replace(editor, with: "Use concise responses for this synthetic local project.")
        referenceScreenshot("project instructions editor before Save")
        tap("library.instructions-save")
        XCTAssertTrue(app.navigationBars[project].waitForExistence(timeout: 20))
        tap("project-editor.instructions")
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        XCTAssertEqual(editor.value as? String, "Use concise responses for this synthetic local project.")
        try replace(editor, with: "This edit must be discarded.")
        tap("library.instructions-cancel")
        XCTAssertTrue(app.buttons["Discard changes"].waitForExistence(timeout: 10))
        referenceScreenshot("unsaved instructions discard warning")
        tap("Discard changes")
        tap("project-editor.instructions")
        XCTAssertTrue(editor.waitForExistence(timeout: 15))
        XCTAssertEqual(editor.value as? String, "Use concise responses for this synthetic local project.")
        tap("library.instructions-cancel")
        tap("project-editor.rename")
        try replace(app.textFields["project-editor.name"], with: renamed)
        referenceScreenshot("rename-project form")
        tap("project-editor.save")
        XCTAssertTrue(app.navigationBars[renamed].waitForExistence(timeout: 20))
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Local native project and saved instructions"; screenshot.lifetime = .keepAlways;
        add(screenshot)
        deleteCurrentProject()
        XCTAssertFalse(app.staticTexts[project].exists); XCTAssertFalse(app.staticTexts[renamed].exists)
    }

    func testLocalProjectChatRenamePinMoveHistoryAndExactDeletion() throws {
        try launchLocal()
        let project = uniqueName("chat project"), chatName = uniqueName("chat")
        openMenu(); try createProject(project)
        guard let currentProjectID else { throw LibraryCleanupIssue.invalidJournal }
        try requireEmptyProject(currentProjectID)
        try rememberPendingCreation(currentProjectID)
        tap("library.project.new-chat." + currentProjectID)
        let composer = app.textViews["chat.composer"]
        try replace(composer, with: "Reply with exactly: Native library workflow passed.")
        tap("chat.send")
        // Capture only a state actually observed during this existing request.
        // Fast completion can omit this attachment; it is not a streaming pass.
        if app.buttons["chat.stop"].exists && app.buttons["chat.stop"].isHittable {
            referenceScreenshot("active local request with Stop visible")
        }
        let completed = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                self.app.descendants(matching: .any).matching(
                    NSPredicate(format: "identifier BEGINSWITH %@", "assistant.message.")
                ).count > 0
                    && !self.app.buttons["chat.stop"].exists
            }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [completed], timeout: 90), .completed)
        XCTAssertFalse(app.staticTexts["chat.error"].exists)
        openMenu()
        let initial = try soleProjectChat(currentProjectID)
        let chatID = String(initial.identifier.dropFirst("library.chat.".count))
        try rememberChat(chatID); currentChatID = chatID
        try openChatActions(chatID)
        XCTAssertTrue(app.buttons["Rename"].waitForExistence(timeout: 10))
        referenceScreenshot("chat context menu")
        tap("Rename")
        let rename = app.alerts["Rename chat"]
        try replace(rename.textFields.firstMatch, with: chatName, presentedAlert: rename)
        referenceScreenshot("rename-chat dialog")
        rename.buttons["Save"].tap()
        let chat = row(named: chatName)
        XCTAssertTrue(chat.waitForExistence(timeout: 20)); try openChatActions(chatID); tap("Pin")
        try openChatActions(chatID); XCTAssertTrue(app.buttons["Unpin"].waitForExistence(timeout: 15))
        referenceScreenshot("pinned chat context menu")
        tap("Unpin")
        try openChatActions(chatID); tap("Move to project")
        XCTAssertTrue(app.buttons["No project"].waitForExistence(timeout: 10))
        referenceScreenshot("move-chat destination chooser")
        tap("No project")
        try requireEmptyProject(currentProjectID)
        // The project now owns no chat, so its destructive cleanup is narrowly scoped.
        openProject(project); deleteCurrentProject()
        let search = app.textFields["library.search"]
        XCTAssertTrue(search.waitForExistence(timeout: 15)); search.tap(); search.typeText(chatName)
        XCTAssertTrue(chat.waitForExistence(timeout: 20))
        referenceScreenshot("filtered Recent menu with the exact owned chat")
        try openChatActions(chatID); tap("Delete")
        let confirm = app.buttons["Delete"].firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout: 10))
        referenceScreenshot("chat deletion confirmation")
        confirm.tap()
        let deleted = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !chat.exists }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [deleted], timeout: 30), .completed)
        XCTAssertFalse(app.staticTexts["library.error"].exists)
        if let currentChatID { try forgetChat(currentChatID) }
    }
}

private struct LibraryCleanupJournal: Codable {
    var owner: String
    var projectIDs: Set<String> = []
    var chatIDs: Set<String> = []
    var pendingCreationProjectIDs: Set<String> = []
}
private enum LibraryCleanupIssue: Error {
    case noAccountMarker, invalidJournal, accountMismatch, inaccessibleControl, loadingFailed
    case paginationLimit, unjournaledProjectChat, deletionUnconfirmed
}

extension LibraryWorkflowUITests {
    private var journalURL: URL {
        get throws {
            let root = try FileManager.default.url(
                for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
            ).appendingPathComponent("NativeLibraryUITestCleanup", isDirectory: true)
            try FileManager.default.createDirectory(
                at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            var excluded = URLResourceValues(); excluded.isExcludedFromBackup = true
            var directory = root; try directory.setResourceValues(excluded)
            return root.appendingPathComponent("exact-entities-v1.json")
        }
    }
    private func accountMarker() throws -> String {
        let marker = app.staticTexts["library.cleanup-account"]
        guard marker.waitForExistence(timeout: 20), marker.label.count == 64,
            marker.label.allSatisfy({ $0.isHexDigit })
        else { throw LibraryCleanupIssue.noAccountMarker }
        return marker.label
    }
    private func readJournal() throws -> LibraryCleanupJournal? {
        let url = try journalURL
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        let data = try Data(contentsOf: url)
        guard data.count <= 64 * 1_024,
            let value = try? JSONDecoder().decode(LibraryCleanupJournal.self, from: data),
            value.owner.count == 64, value.owner.allSatisfy({ $0.isHexDigit }),
            value.projectIDs.count <= 64, value.chatIDs.count <= 64,
            value.pendingCreationProjectIDs.isSubset(of: value.projectIDs),
            (value.projectIDs.union(value.chatIDs)).allSatisfy({ UUID(uuidString: $0) != nil })
        else { throw LibraryCleanupIssue.invalidJournal }
        return value
    }
    private func writeJournal(_ journal: LibraryCleanupJournal) throws {
        // Retain an empty journal too: an interrupted process can reuse the same
        // file without broad filesystem cleanup or any account-owned app storage.
        let data = try JSONEncoder().encode(journal)
        try data.write(to: journalURL, options: [.atomic, .completeFileProtection])
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: journalURL.path)
    }
    private func ownedJournal() throws -> LibraryCleanupJournal {
        guard let owner = observedCleanupOwner else { throw LibraryCleanupIssue.noAccountMarker }
        guard let saved = try readJournal() else { return LibraryCleanupJournal(owner: owner) }
        guard saved.owner == owner || (saved.projectIDs.isEmpty && saved.chatIDs.isEmpty) else {
            throw LibraryCleanupIssue.accountMismatch
        }
        return saved.owner == owner ? saved : LibraryCleanupJournal(owner: owner)
    }
    private func rememberProject(_ id: String) throws {
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        var journal = try ownedJournal(); journal.projectIDs.insert(id)
        recordCleanupIDs(journal); try writeJournal(journal)
    }
    private func rememberChat(_ id: String) throws {
        guard UUID(uuidString: id) != nil else { throw LibraryCleanupIssue.invalidJournal }
        var journal = try ownedJournal(); journal.chatIDs.insert(id)
        if let currentProjectID { journal.pendingCreationProjectIDs.remove(currentProjectID) }
        recordCleanupIDs(journal); try writeJournal(journal)
    }
    private func rememberPendingCreation(_ projectID: String) throws {
        var journal = try ownedJournal()
        guard journal.projectIDs.contains(projectID) else { throw LibraryCleanupIssue.invalidJournal }
        journal.pendingCreationProjectIDs.insert(projectID); try writeJournal(journal)
    }
    private func forgetProject(_ id: String) throws {
        var journal = try ownedJournal(); journal.projectIDs.remove(id)
        journal.pendingCreationProjectIDs.remove(id); try writeJournal(journal)
    }
    private func forgetChat(_ id: String) throws {
        var journal = try ownedJournal(); journal.chatIDs.remove(id); try writeJournal(journal)
    }
    private func recordCleanupIDs(_ journal: LibraryCleanupJournal) {
        let value =
            "Projects: " + journal.projectIDs.sorted().joined(separator: ",")
            + "\nChats: " + journal.chatIDs.sorted().joined(separator: ",")
            + "\nUnresolved creation in projects: " + journal.pendingCreationProjectIDs.sorted().joined(separator: ",")
        let attachment = XCTAttachment(string: value)
        attachment.name = "Exact owned local entities for cleanup"; attachment.lifetime = .keepAlways; add(attachment)
    }
    private func cleanupTap(_ identifier: String) throws {
        let target = identifier == "chat.menu" ? app.mapleMenuButton : app.buttons[identifier].firstMatch
        guard target.waitForExistence(timeout: 10) else { throw LibraryCleanupIssue.inaccessibleControl }
        for _ in 0..<12 where !target.isHittable { app.swipeUp() }
        guard target.isHittable else { throw LibraryCleanupIssue.inaccessibleControl }
        target.tap()
    }
    private func cleanupWait(_ condition: @escaping @MainActor () -> Bool) throws {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: app)
        guard XCTWaiter.wait(for: [expectation], timeout: 15) == .completed else {
            throw LibraryCleanupIssue.loadingFailed
        }
    }
    private func cleanupRestart() throws {
        app.terminate(); app.launchArguments = ["--maple-library-local-cleanup"]; app.launch()
        guard app.buttons["chat.new"].waitForExistence(timeout: 30) else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
        observedCleanupOwner = try accountMarker()
        _ = try ownedJournal()
        try cleanupTap("chat.menu")
        guard app.textFields["library.search"].waitForExistence(timeout: 15) else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
    }
    private func cleanupChat(_ id: String) throws {
        try cleanupRestart()
        let projects = try ownedJournal().projectIDs.sorted()
        guard let row = try MenuChatUI.find(id, in: app, projectIDs: projects, pageLimit: 20) else {
            throw LibraryCleanupIssue.deletionUnconfirmed
        }
        XCTAssertEqual(row.identifier, "library.chat." + id)
        guard row.isHittable else { throw LibraryCleanupIssue.inaccessibleControl }
        try cleanupTap("library.chat-actions." + id); try cleanupTap("Delete")
        guard app.staticTexts["Delete chat?"].waitForExistence(timeout: 10) else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
        try cleanupTap("Delete")
        try cleanupWait { !row.exists || self.app.staticTexts["library.error"].exists }
        guard !row.exists, !app.staticTexts["library.error"].exists else {
            throw LibraryCleanupIssue.deletionUnconfirmed
        }
        try forgetChat(id)
    }
    private func cleanupProject(_ id: String) throws {
        try cleanupRestart()
        guard try !ownedJournal().pendingCreationProjectIDs.contains(id) else {
            throw LibraryCleanupIssue.unjournaledProjectChat
        }
        let target = app.buttons.matching(identifier: "library.project." + id).firstMatch
        // Missing or inaccessible IDs remain journaled. Only a confirmed UI
        // deletion removes an ID; a page load is not authority to forget it.
        try cleanupWait { target.exists || self.app.staticTexts["library.error"].exists }
        if app.staticTexts["library.error"].exists { throw LibraryCleanupIssue.loadingFailed }
        guard target.exists else { throw LibraryCleanupIssue.deletionUnconfirmed }
        for _ in 0..<20 where !target.isHittable { app.swipeUp() }
        guard target.isHittable else { throw LibraryCleanupIssue.inaccessibleControl }
        // Every recorded chat is cleaned first. Refuse cascade deletion if the
        // project now contains any other chat, including an unrecorded test result.
        try requireEmptyProject(id)
        target.press(forDuration: 1); try cleanupTap("library.project.manage." + id)
        try cleanupTap("project-editor.delete")
        let field = app.textFields["library.delete-confirmation"]
        guard field.waitForExistence(timeout: 10) else { throw LibraryCleanupIssue.inaccessibleControl }
        field.tap(); field.typeText("DELETE"); try cleanupTap("library.delete-confirm")
        try cleanupWait { self.app.buttons["library.new-project"].isHittable && !target.exists }
        guard !app.staticTexts["library.error"].exists, !target.exists else {
            throw LibraryCleanupIssue.deletionUnconfirmed
        }
        try forgetProject(id)
    }
    private func cleanupJournaledEntities() throws {
        guard ProcessInfo.processInfo.environment["MAPLE_RUN_LOCAL_UI"] == "1", let app else { return }
        guard let journal = try readJournal(), !journal.chatIDs.isEmpty || !journal.projectIDs.isEmpty else { return }
        try cleanupRestart()
        recordCleanupIDs(journal)
        for id in journal.chatIDs.sorted() { try cleanupChat(id) }
        for id in journal.projectIDs.sorted() { try cleanupProject(id) }
        // Restore the normal root before another test begins creating resources.
        app.terminate(); app.launchArguments = ["--maple-library-local-cleanup"]; app.launch()
        guard app.buttons["chat.new"].waitForExistence(timeout: 30) else {
            throw LibraryCleanupIssue.inaccessibleControl
        }
    }
}
