import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct LibraryModelTests {
    func fixture() -> (LibraryModel, LibraryFixture, ChatDeletionCoordinator, ChatFixtureService, ChatFixtureStore) {
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "fixture")
        let store = ChatFixtureStore(), service = LibraryFixture(), chat = ChatFixtureService(store: store)
        let deletion = ChatDeletionCoordinator(scope: scope, retained: [], service: chat, persistence: store)
        return (LibraryModel(scope: scope, service: service, deletions: deletion), service, deletion, chat, store)
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
        await model.moveConversations(ids: ids, projectID: "project")
        #expect(await service.moveCalls.map(\.count) == [20, 20, 5])
        #expect(model.conversations.allSatisfy { $0.projectId == "project" })
        model.selectedIDs = ids
        await model.deleteConversations(ids: ids)
        #expect(await service.deleteCalls.map(\.count) == [20, 20, 5])
        #expect(model.conversations.map(\.id) == ["3"])
        #expect(model.selectedIDs == ["3"] && model.issue == .partialMutation)
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
    @Test func retiredHistoryResultCannotPublish() async {
        let (model, service, _, _, _) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")], listGate: gate)
        let loading = Task { await model.load() }
        await gate.waitForArrival(); model.invalidate(); await gate.release(); await loading.value
        #expect(model.conversations.isEmpty && !model.isLoading)
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
