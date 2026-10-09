import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct LibraryOwnershipTests {
    @Test func changedFilterDropsOldPageAndDoesNotClearNewLoadingState() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), oldRead = ChatTestGate()
        await service.configure(
            conversations: [libraryConversation("a"), libraryConversation("b", pinned: true)], listGate: oldRead)
        let initial = Task { await model.load() }
        await oldRead.waitForArrival()
        await service.configure(listGate: nil)
        await model.load(.pinned)
        await oldRead.release(); await initial.value
        #expect(model.filter == .pinned)
        #expect(model.conversations.map(\.id) == ["b"] && !model.isLoading)
    }

    @Test func pageStartedBeforeRenameCannotRestoreOldMetadata() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")])
        await model.load()
        await service.configure(listGate: gate)
        let reading = Task { await model.load() }
        await gate.waitForArrival()
        await model.renameConversation(id: "a", title: "New title")
        await gate.release(); await reading.value
        #expect(!model.conversations.contains { LibraryModel.title($0) == "Title a" })
        await service.configure(listGate: nil)
        await model.loadMore()
        #expect(model.conversations.map(LibraryModel.title) == ["New title"])
    }

    @Test func readStartedDuringRenameIsInvalidatedWhenMutationSettles() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        let mutationGate = ChatTestGate(), readGate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")])
        await model.load()
        await service.holdUpdate(mutationGate)
        let mutating = Task { await model.renameConversation(id: "a", title: "New title") }
        await mutationGate.waitForArrival()
        await service.configure(listGate: readGate)
        let reading = Task { await model.load() }
        await readGate.waitForArrival()
        await mutationGate.release(); await mutating.value
        await readGate.release(); await reading.value
        #expect(!model.conversations.contains { LibraryModel.title($0) == "Title a" })
        await service.configure(listGate: nil)
        await model.loadMore()
        #expect(model.conversations.map(LibraryModel.title) == ["New title"])
    }

    @Test func accountRetirementDuringMutationCannotPublishMetadataOrCallbacks() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")])
        await model.load(); await service.holdUpdate(gate)
        var callbacks = 0
        model.searchText = "Private search query"
        model.onConversationUpdated = { _ in callbacks += 1 }
        let mutation = Task { await model.setPinned(id: "a", pinned: true) }
        await gate.waitForArrival(); model.invalidate()
        await gate.release(); await mutation.value
        #expect(model.conversations.isEmpty && !model.isMutating)
        #expect(model.searchText.isEmpty)
        #expect(callbacks == 0)
    }

    @Test func repeatedPaginationCursorFailsWithoutDuplicatingRows() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        await service.configure(conversations: [libraryConversation("a")], repeatedCursor: true)
        await model.load(); await model.loadMore()
        #expect(model.issue == .invalidResponse && !model.isLoading)
        #expect(model.conversations.map(\.id) == ["a"])
    }

    @Test func mixedSectionMovePublishesOnlyAcknowledgedBatchesWithoutLoadingAnotherList() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        let ids = Set((0..<25).map { String(format: "chat-%02d", $0) })
        await service.configureMove(failedCall: 2)
        var acknowledged = Set<String>()
        var destinations: [String?] = []
        model.onConversationProjectUpdated = { changed, destination in
            acknowledged.formUnion(changed); destinations.append(destination)
        }
        model.selectedIDs = ids
        await model.moveConversations(ids: ids, projectID: "destination")
        #expect(model.conversations.isEmpty)
        #expect(acknowledged == Set(ids.sorted().prefix(20)))
        #expect(destinations == ["destination"])
        #expect(model.selectedIDs == ids.subtracting(acknowledged))
        #expect(model.issue != nil)

        // Clearing the project must be published too; failed batches stay selected for retry.
        await service.configureMove()
        await model.moveConversations(ids: model.selectedIDs, projectID: nil)
        #expect(acknowledged == ids && destinations == ["destination", nil])
        #expect(model.selectedIDs.isEmpty && model.issue == nil)
    }

    @Test func retiredMixedSectionMoveCannotPublishMembershipToAnotherAccountGeneration() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configureMove(gate: gate)
        var callbacks = 0
        model.onConversationProjectUpdated = { _, _ in callbacks += 1 }
        model.selectedIDs = ["selected"]
        let moving = Task { await model.moveConversations(ids: ["selected"], projectID: "destination") }
        await gate.waitForArrival(); model.invalidate()
        await gate.release(); await moving.value
        #expect(callbacks == 0 && model.selectedIDs.isEmpty && !model.isMutating)
    }

    @Test func projectSearchUsesLoadedChatTitlesButDoesNotSearchUnloadedContent() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        await service.configure(conversations: [libraryConversation("needle", project: "project")])
        await model.loadProjects()
        model.searchText = "needle"
        #expect(model.visibleProjects.isEmpty)
        await model.load()
        #expect(model.visibleProjects.map(\.id) == ["project"])
        model.searchText = "  PrOjEcT  "
        #expect(model.visibleProjects.map(\.id) == ["project"])
        #expect(model.visibleConversations.isEmpty)
    }

    @Test func siblingPinAcknowledgementWinsOverPendingRecentRefresh() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a")])
        await model.load(.recent)
        await service.configure(listGate: gate)
        let refresh = Task { await model.refresh() }
        await gate.waitForArrival()
        model.receiveConversationUpdate(libraryConversation("a", pinned: true), from: model.scope)
        await gate.release(); await refresh.value
        #expect(model.conversations.isEmpty && model.visibleConversations.isEmpty && !model.isLoading)
    }

    @Test func siblingMoveAndDeleteCannotBeUndoneByPendingRefresh() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("a"), libraryConversation("b")])
        await model.load(.recent); model.selectedIDs = ["a", "b"]
        await service.configure(listGate: gate)
        let refresh = Task { await model.refresh() }
        await gate.waitForArrival()
        model.receiveProjectMembership(["a"], projectID: "folder", from: model.scope)
        model.receiveDeletion(["b"], all: false, from: model.scope)
        await gate.release(); await refresh.value
        #expect(model.visibleConversations.isEmpty && model.selectedIDs == ["a"])
        #expect(!model.isLoading)
    }

    @Test func previousAccountGenerationCannotBroadcastIntoCurrentCache() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        await service.configure(conversations: [libraryConversation("a")])
        await model.load(); await model.loadProjects()
        let oldOwner = AccountScope(apiOrigin: model.scope.apiOrigin, principalID: model.scope.principalID)
        model.receiveConversationUpdate(libraryConversation("other"), from: oldOwner)
        model.receiveProjectMembership(["a"], projectID: "foreign", from: oldOwner)
        model.receiveDeletion(["a"], all: true, from: oldOwner)
        model.receiveProjectDeletion("project", from: oldOwner)
        #expect(model.conversations == [libraryConversation("a")] && model.projects.count == 1)
        model.invalidate()
        model.receiveConversationUpdate(libraryConversation("late"), from: model.scope)
        #expect(model.conversations.isEmpty)
    }

    @Test func projectAcknowledgementInvalidatesOlderListAndRetirementDropsLateProjects() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await model.loadProjects()
        await service.configureProjects(model.projects, gate: gate)
        let refresh = Task { await model.loadProjects() }
        await gate.waitForArrival()
        model.receiveProjectUpdate(
            ConversationProject(id: "project", object: "project", name: "Renamed", createdAt: 0, updatedAt: 1),
            from: model.scope)
        await gate.release(); await refresh.value
        #expect(model.projects.first?.name == "Renamed" && !model.isLoadingProjects)
        let retiredGate = ChatTestGate()
        await service.configureProjects(model.projects, gate: retiredGate)
        let retiredRead = Task { await model.loadProjects() }
        await retiredGate.waitForArrival(); model.invalidate()
        await retiredGate.release(); await retiredRead.value
        #expect(model.projects.isEmpty && !model.isLoadingProjects)
    }
}
