import Foundation
import MapleSDK
import Observation
import Synchronization
import Testing
@testable import MapleAppCore

private final class LibraryObservationChanges: Sendable {
    private let count = Mutex(0)
    var value: Int { count.withLock { $0 } }
    func record() { count.withLock { $0 += 1 } }
}

@MainActor @Suite struct LibraryModelTests {
    func fixture() -> (LibraryModel, LibraryFixture, ChatDeletionCoordinator, ChatFixtureService, ChatFixtureStore) {
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let store = ChatFixtureStore(), service = LibraryFixture(), chat = ChatFixtureService(store: store)
        let deletion = ChatDeletionCoordinator(scope: scope, retained: [], service: chat, persistence: store)
        return (LibraryModel(scope: scope, service: service, deletions: deletion), service, deletion, chat, store)
    }
    @Test func emptyFirstPageCompletionNotifiesReadinessObservers() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [], listGate: gate)
        let load = Task { await model.load(.project("project")) }
        await gate.waitForArrival()
        let changes = LibraryObservationChanges()
        let initiallyReady = withObservationTracking {
            model.hasLoadedConversations
        } onChange: {
            changes.record()
        }
        #expect(!initiallyReady && changes.value == 0)
        await gate.release(); await load.value
        #expect(model.hasLoadedConversations && changes.value == 1)
        #expect(model.conversations.isEmpty && !model.hasMore && model.issue == nil)
    }
    @Test func searchUsesLoadedTitlesAndPagingPreservesUniqueRows() async {
        let (model, service, _, _, _) = fixture()
        await service.configure(conversations: (0..<102).map { libraryConversation(String($0)) })
        await model.load()
        #expect(model.conversations.count == 100 && model.hasMore)
        model.searchText = "Title 101"
        #expect(model.visibleConversations.isEmpty)
        await model.loadMore()
        #expect(model.visibleConversations.map(\.id) == ["101"])
        #expect(!model.hasMore && model.conversations.count == 102)
    }
    @Test func renamePreservesCreationCorrelationAndOtherMetadata() async {
        let (model, service, _, _, _) = fixture()
        var original = libraryConversation("a")
        original.metadata?["internal_native_creation_id"] = .string(String(repeating: "a", count: 64))
        original.metadata?["preference"] = .bool(true)
        await service.configure(conversations: [original])
        await model.load()
        await model.renameConversation(id: "a", title: "Renamed")
        #expect(model.conversations.first?.metadata?["title"] == .string("Renamed"))
        #expect(
            await service.updateMetadata?["internal_native_creation_id"]
                == original.metadata?["internal_native_creation_id"])
        #expect(await service.updateMetadata?["preference"] == .bool(true))
    }
    @Test func bulkActionsUseTwentyIDContractAndKeepFailedSelection() async {
        let (model, service, _, _, _) = fixture()
        let rows = (0..<45).map { libraryConversation(String($0)) }, ids = Set(rows.map(\.id))
        await service.configure(conversations: rows, failedDeletes: ["3"])
        await model.load(); model.selectedIDs = ids
        let moved = await model.moveConversations(ids: ids, projectID: "project")
        #expect(moved)
        #expect(await service.moveCalls.map(\.count) == [20, 20, 5])
        #expect(model.conversations.allSatisfy { $0.projectId == "project" })
        #expect(model.selectedIDs.isEmpty)
        model.selectedIDs = ids
        let deleted = await model.deleteConversations(ids: ids)
        #expect(!deleted)
        #expect(await service.deleteCalls.map(\.count) == [20, 20, 5])
        #expect(model.conversations.map(\.id) == ["3"])
        #expect(model.selectedIDs == ["3"] && model.issue == .partialMutation)
    }
    @Test func bulkDeleteSignalsSuccessOnlyAfterEverySelectedChatIsDeleted() async {
        let (model, service, _, _, _) = fixture()
        let rows = (0..<25).map { libraryConversation(String($0)) }, ids = Set(rows.map(\.id))
        await service.configure(conversations: rows)
        await model.load(); model.selectedIDs = ids
        #expect(await model.deleteConversations(ids: ids))
        #expect(await service.deleteCalls.map(\.count) == [20, 5])
        #expect(model.selectedIDs.isEmpty && model.conversations.isEmpty && model.issue == nil)
    }
    @Test func failedBulkDeleteKeepsSelectionAndDoesNotSignalSuccess() async {
        let (model, service, _, _, _) = fixture()
        await service.configure(conversations: [libraryConversation("a")], failDelete: true)
        await model.load(); model.selectedIDs = ["a"]
        #expect(await model.deleteConversations(ids: ["a"]) == false)
        #expect(model.selectedIDs == ["a"] && model.conversations.map(\.id) == ["a"])
        #expect(model.issue == .connection)
        model.invalidate()
        #expect(await model.deleteConversations(ids: ["a"]) == false)
        #expect(await service.deleteCalls.count == 1)
    }
    @Test func projectDiscoveryFencesBeforeAwaitAndFailureReleasesWithoutDeleting() async throws {
        let (model, service, deletion, chat, store) = fixture(), gate = ChatTestGate()
        let runtime = ChatRuntime(scope: model.scope, service: chat, persistence: store, conversationID: "a")
        deletion.register(runtime, projectID: "unrelated")
        runtime.draftText = "still editable"
        await service.configure(
            conversations: [libraryConversation("a", project: "project")], failDelete: true, projectDiscoveryGate: gate)
        let request = Task { await model.deleteProject(id: "project") }
        await gate.waitForArrival()
        #expect(!runtime.canSend && !runtime.send())
        runtime.draftText = "edited during discovery"
        await gate.release(); await request.value
        #expect(runtime.canSend && runtime.draftText == "edited during discovery")
        #expect(await service.projectDeletes == ["project"])
        #expect(model.issue == .connection)
    }
    @Test func malformedRepeatedCursorBlocksProjectDelete() async {
        let (model, service, deletion, _, _) = fixture()
        await service.configure(conversations: [libraryConversation("a", project: "project")], repeatedCursor: true)
        await model.deleteProject(id: "project")
        #expect(model.issue == .invalidResponse)
        #expect(await service.projectDeletes.isEmpty)
        #expect(!deletion.hasActiveFence)
    }
    @Test func accountFenceBlocksNewLibraryMutations() async throws {
        let (model, service, deletion, _, _) = fixture()
        await service.configure(conversations: [libraryConversation("a")])
        let lease = try deletion.begin(.all)
        await model.renameConversation(id: "a", title: "late")
        #expect(model.issue == .busy)
        #expect(await service.updateMetadata == nil)
        deletion.release(lease)
    }

    @Test func projectMoveReturnsFalseForFailedOrRetiredAcknowledgements() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await service.configureMove(failedCall: 1)
        let failed = await model.moveConversations(ids: ["chat"], projectID: "project")
        #expect(!failed && model.issue == .connection)
        await service.configureMove(gate: gate)
        let moving = Task { await model.moveConversations(ids: ["chat"], projectID: "project") }
        await gate.waitForArrival(); model.invalidate(); await gate.release()
        #expect(await moving.value == false)
    }
    @Test func retiredHistoryResultCannotPublish() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")], listGate: gate)
        let loading = Task { await model.load() }
        await gate.waitForArrival(); model.invalidate(); await gate.release(); await loading.value
        #expect(model.conversations.isEmpty && !model.isLoading)
    }

    @Test func recentExcludesPinnedAndProjectChatsAtThePageBoundary() async {
        let (model, service, _, _, _) = fixture()
        await service.configure(conversations: [
            libraryConversation("pin", pinned: true), libraryConversation("recent"),
            libraryConversation("project", project: "folder"),
            libraryConversation("project-pin", project: "folder", pinned: true),
        ])
        await model.load(.recent)
        #expect(model.visibleConversations.map(\.id) == ["recent"])
        #expect(await service.conversationListCalls.last?.pinned == false)
        #expect(await service.conversationListCalls.last?.unassignedProject == true)
        var pinned = libraryConversation("recent", pinned: true)
        model.receiveConversationUpdate(pinned, from: model.scope)
        #expect(model.visibleConversations.isEmpty)
        pinned.pinned = false
        model.receiveConversationUpdate(pinned, from: model.scope)
        #expect(model.visibleConversations.map(\.id) == ["recent"])
    }

    @Test func recentMatchesWebDesktopTwentyThenTenCursorPagination() async {
        let (model, service, _, _, _) = fixture()
        let rows = (0..<37).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows + [libraryConversation("pin", pinned: true)])
        await model.load(.recent)
        #expect(model.conversations == Array(rows.prefix(20)) && model.hasMore)
        await model.loadMoreIfNeeded(visibleID: "0")
        #expect(await service.conversationListCalls.count == 1)
        await model.loadMoreIfNeeded(visibleID: "19")
        #expect(model.conversations == Array(rows.prefix(30)) && model.hasMore)
        await model.loadMoreIfNeeded(visibleID: "29")
        #expect(model.conversations == rows && !model.hasMore)
        #expect(await service.conversationListCalls.map(\.page.limit) == [20, 10, 10])
        #expect(await service.conversationListCalls.map(\.page.after) == [nil, "19", "29"])
        #expect(await service.conversationListCalls.allSatisfy { $0.pinned == false && $0.unassignedProject == true })
    }

    @Test func recentVisibleBoundaryDeduplicatesOverlapAndDoesNotAutoPageSearch() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        let rows = (0..<35).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows)
        await model.load(.recent)
        model.searchText = "19"
        await model.loadMoreIfNeeded(visibleID: "19")
        #expect(await service.conversationListCalls.count == 1)
        model.searchText = ""
        await service.configure(listGate: gate)
        let first = Task { await model.loadMoreIfNeeded(visibleID: "19") }
        await gate.waitForArrival()
        await model.loadMoreIfNeeded(visibleID: "18")
        await gate.release(); await first.value
        #expect(model.conversations.count == 30 && Set(model.conversations.map(\.id)).count == 30)
        #expect(await service.conversationListCalls.map(\.page.after) == [nil, "19"])
    }

    @Test func movingLastProjectChatRemovesItsCachedMembershipImmediately() async {
        let (model, service, _, _, _) = fixture()
        await service.configure(conversations: [libraryConversation("a", project: "folder")])
        await model.load(.project("folder")); model.selectedIDs = ["a"]
        await model.moveConversations(ids: ["a"], projectID: nil)
        #expect(model.conversations.isEmpty && model.selectedIDs.isEmpty && model.issue == nil)
        #expect(await service.conversations.first?.projectId == nil)
    }

    @Test func unchangedRefreshRetainsRowsSelectionAndAllLoadedPagesWithoutPublishing() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        let rows = (0..<205).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows)
        await model.load(); await model.loadMore()
        model.selectedIDs = ["150"]; model.searchText = "Title"
        let changes = LibraryObservationChanges()
        _ = withObservationTracking {
            model.conversations
        } onChange: {
            changes.record()
        }
        await service.configure(listGate: gate)
        let refresh = Task { await model.load() }
        await gate.waitForArrival()
        #expect(model.conversations == Array(rows.prefix(200)) && model.hasMore)
        await gate.release(); await refresh.value
        #expect(changes.value == 0)
        #expect(model.selectedIDs == ["150"] && model.searchText == "Title")
        #expect(await service.conversationListCalls.map(\.page.after) == [nil, "99", nil, "99"])
        await service.configure(listGate: nil)
        await model.loadMore()
        #expect(model.conversations == rows && !model.hasMore)
    }

    @Test func refreshReconcilesChangedLoadedPagesAtomically() async {
        let (model, service, _, _, _) = fixture()
        let rows = (0..<202).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows)
        await model.load(); await model.loadMore()
        var changed = Array(rows.dropFirst()); changed[120].metadata = ["title": .string("Renamed elsewhere")]
        changed.insert(libraryConversation("new"), at: 0)
        await service.configure(conversations: changed)
        await model.refresh()
        #expect(model.conversations == Array(changed.prefix(200)) && model.hasMore)
        await model.loadMore()
        #expect(model.conversations == changed)
    }

    @Test func recentsPollMergesServerOrderWithoutReloadingOlderPagesOrChangingSelection() async {
        let (model, service, _, _, _) = fixture()
        let rows = (0..<45).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows)
        await model.load(.recent); await model.loadMore()
        model.selectedIDs = ["25"]; model.searchText = "Title"
        var renamed = rows[1]; renamed.metadata = ["title": .string("Renamed elsewhere")]
        await service.configure(
            conversations: [libraryConversation("new"), renamed, rows[0]] + Array(rows.dropFirst(2)))
        await model.pollRecentConversations()
        #expect(model.conversations.map(\.id) == ["new", "1", "0"] + (2..<30).map(String.init))
        #expect(model.conversations[1] == renamed && model.selectedIDs == ["25"] && model.searchText == "Title")
        #expect(model.hasMore && !model.isLoading)
        await model.loadMore()
        #expect(model.conversations.count == 41)
        #expect(await service.conversationListCalls.map(\.page.limit) == [20, 10, 20, 10])
        #expect(await service.conversationListCalls.map(\.page.after) == [nil, "19", nil, "29"])
    }

    @Test func unchangedRecentsTitleKeepsCachedValueAndDoesNotInvalidateRows() async {
        let (model, service, _, _, _) = fixture(), original = libraryConversation("one")
        await service.configure(conversations: [original]); await model.load(.recent)
        let changes = LibraryObservationChanges()
        _ = withObservationTracking {
            model.conversations
        } onChange: {
            changes.record()
        }
        var sameTitle = original
        sameTitle.metadata?["other"] = .string("changed"); sameTitle.lastActivityAt = 20
        await service.configure(conversations: [sameTitle]); await model.pollRecentConversations()
        #expect(model.conversations == [original] && changes.value == 0)
        sameTitle.metadata?["title"] = .string("New title")
        await service.configure(conversations: [sameTitle]); await model.pollRecentConversations()
        #expect(model.conversations == [sameTitle] && changes.value == 1)
    }

    @Test(arguments: [true, false])
    func failedOrEmptyRecentsTickRetainsRowsAndExistingIssue(_ fails: Bool) async {
        let (model, service, _, _, _) = fixture(), rows = [libraryConversation("one")]
        await service.configure(conversations: rows); await model.load(.recent)
        model.selectedIDs = ["one"]; await model.renameConversation(id: "one", title: "")
        await service.configure(conversations: [], failList: fails); await model.pollRecentConversations()
        #expect(model.conversations == rows && model.selectedIDs == ["one"] && model.issue == .invalidInput)
    }

    @Test func manualRefreshSupersedesRecentsReadAlreadyInFlight() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("old")]); await model.load(.recent)
        let poll = Task {
            await service.configure(conversations: [libraryConversation("stale")], listGate: gate)
            await model.pollRecentConversations()
        }
        await gate.waitForArrival()
        await service.configure(conversations: [libraryConversation("fresh")])
        await model.refresh()
        await gate.release(); await poll.value
        #expect(model.conversations.map(\.id) == ["fresh"])
    }

    enum RecentsChange: CaseIterable, Sendable { case renamed, deleted, retired, cancelled }

    @Test(arguments: RecentsChange.allCases)
    func staleRecentsReadCannotReverseAccountChanges(_ change: RecentsChange) async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate(), original = libraryConversation("one")
        await service.configure(conversations: [original]); await model.load(.recent)
        await service.configure(conversations: [libraryConversation("stale"), original], listGate: gate)
        let poll = Task { await model.pollRecentConversations() }
        await gate.waitForArrival()
        switch change {
        case .renamed:
            var updated = original; updated.metadata?["title"] = .string("Renamed locally")
            model.receiveConversationUpdate(updated, from: model.scope)
        case .deleted: model.receiveDeletion(["one"], all: false, from: model.scope)
        case .retired: model.invalidate()
        case .cancelled: poll.cancel()
        }
        let expected = model.conversations
        await gate.release(); await poll.value
        #expect(model.conversations == expected)
    }

    @Test func failedOrCancelledRefreshKeepsLastSuccessfulPageAndCursor() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        let rows = (0..<102).map { libraryConversation(String($0)) }
        await service.configure(conversations: rows)
        await model.load()
        await service.configure(failList: true)
        await model.refresh()
        #expect(model.conversations == Array(rows.prefix(100)) && model.hasMore && model.issue == .connection)
        await service.configure(conversations: [libraryConversation("stale")], listGate: gate)
        let refresh = Task { await model.refresh() }
        await gate.waitForArrival(); refresh.cancel(); await gate.release(); await refresh.value
        #expect(model.conversations == Array(rows.prefix(100)) && model.hasMore && !model.isLoading)
        #expect(model.issue == nil)
        await service.configure(conversations: rows)
        await model.loadMore()
        #expect(model.conversations == rows && !model.hasMore)
    }

    @Test func unchangedProjectRefreshDoesNotPublishAndKeepsCacheWhileLoading() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await model.loadProjects()
        let projects = model.projects, changes = LibraryObservationChanges()
        _ = withObservationTracking {
            model.projects
        } onChange: {
            changes.record()
        }
        await service.configureProjects(projects, gate: gate)
        let refresh = Task { await model.loadProjects() }
        await gate.waitForArrival()
        #expect(model.projects == projects)
        await gate.release(); await refresh.value
        #expect(changes.value == 0 && !model.isLoadingProjects)
    }
}

@MainActor @Suite struct InstructionEditorTests {
    @Test func defaultBeyondFirstPageLoadsAndWhitespaceSaveDeletesOnlyThatDefault() async {
        let service = LibraryFixture(), scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let rows =
            (0..<100).map { libraryInstruction(String($0), isDefault: false) } + [
                libraryInstruction("default", prompt: "Original")
            ]
        await service.configure(instructions: rows)
        let editor = InstructionEditor(scope: scope, destination: .account, service: service)
        await editor.load()
        #expect(editor.text == "Original" && !editor.isDirty)
        editor.text = "   \n"
        #expect(editor.isDirty)
        await editor.save()
        #expect(await service.instructionChanges == ["delete"])
        #expect(editor.didSave && !editor.isDirty && editor.text.isEmpty)
        #expect(await service.instructions.count == 100)
    }
    @Test func failedLoadDoesNotCreateAnEmptyReplacement() async {
        let service = LibraryFixture(), scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        await service.configure(failList: true)
        let editor = InstructionEditor(scope: scope, destination: .account, service: service)
        await editor.load(); editor.text = "replacement"; await editor.save()
        #expect(!editor.isLoaded && !editor.didSave)
        #expect(await service.instructionChanges.isEmpty)
    }
    @Test func projectEditorUsesExplicitSaveAndNullClearing() async {
        let service = LibraryFixture(), scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let editor = InstructionEditor(scope: scope, destination: .project("project"), service: service)
        await editor.load(); editor.text = "draft only"
        #expect(await service.projectInstructions == "project original")
        editor.discardChanges(); #expect(editor.text == "project original" && !editor.isDirty)
        editor.text = " \n "; await editor.save()
        #expect(await service.projectInstructions == nil)
        #expect(editor.didSave)
    }
    @Test func retiredEditorDropsLateLoad() async {
        let service = LibraryFixture(),
            scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture"), gate = ChatTestGate()
        await service.configure(instructions: [libraryInstruction("private")], listGate: gate)
        let editor = InstructionEditor(scope: scope, destination: .account, service: service)
        let loading = Task { await editor.load() }
        await gate.waitForArrival(); editor.invalidate(); await gate.release(); await loading.value
        #expect(editor.text.isEmpty && !editor.isLoaded)
    }
}
