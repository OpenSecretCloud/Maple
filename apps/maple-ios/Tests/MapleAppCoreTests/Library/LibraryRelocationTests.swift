import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct LibraryRelocationTests {
    @Test func projectAndRecentDestinationsUnpinWithOneAcknowledgedUpdateEach() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        let original = libraryConversation("chat", project: "original", pinned: true)
        await service.configure(conversations: [original]); await model.load()
        var published: [Conversation] = []
        model.onConversationUpdated = { published.append($0) }
        model.selectedIDs = ["chat"]
        await model.relocateConversation(id: "chat", to: .project("destination"), from: model.scope)
        #expect(model.issue == nil && model.selectedIDs.isEmpty)
        #expect(await service.conversationUpdateIDs == ["chat"])
        #expect(published.count == 1 && published[0].projectId == "destination" && !published[0].pinned)
        #expect(published[0].metadata == original.metadata)
        await model.relocateConversation(id: "chat", to: .recent, from: model.scope)
        #expect(await service.conversationUpdateIDs == ["chat", "chat"])
        #expect(published.count == 2 && published[1].projectId == nil && !published[1].pinned)
        #expect(model.conversations == [published[1]])
    }

    @Test func pinningRetainsProjectAndPublishesToSiblingRecentAndProjectCaches() async {
        let (model, service, deletions, _, _) = LibraryModelTests().fixture()
        let project = LibraryModel(scope: model.scope, service: service, deletions: deletions)
        let pinned = LibraryModel(scope: model.scope, service: service, deletions: deletions)
        await service.configure(conversations: [libraryConversation("chat", project: "folder")])
        await model.load(.all); await project.load(.project("folder")); await pinned.load(.pinned)
        model.onConversationUpdated = {
            project.receiveConversationUpdate($0, from: model.scope)
            pinned.receiveConversationUpdate($0, from: model.scope)
        }
        await model.relocateConversation(id: "chat", to: .pinned, from: model.scope)
        #expect(await service.conversationUpdateIDs == ["chat"])
        #expect(pinned.visibleConversations.map(\.id) == ["chat"])
        #expect(pinned.conversations.first?.projectId == "folder")
        #expect(project.conversations.first?.pinned == true)
        await model.relocateConversation(id: "chat", to: .recent, from: model.scope)
        #expect(project.visibleConversations.isEmpty && pinned.visibleConversations.isEmpty)
        #expect(model.conversations.first?.projectId == nil && model.conversations.first?.pinned == false)
    }

    @Test func previousAccountGenerationAndRetiredSourceCannotStartRelocation() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        await service.configure(conversations: [libraryConversation("chat")]); await model.load()
        let previous = AccountScope(apiOrigin: model.scope.apiOrigin, principalID: model.scope.principalID)
        await model.relocateConversation(id: "chat", to: .pinned, from: previous)
        #expect(await service.conversationUpdateIDs.isEmpty)
        #expect(model.conversations.first?.pinned == false)
        model.invalidate()
        await model.relocateConversation(id: "chat", to: .project("folder"), from: model.scope)
        #expect(await service.conversationUpdateIDs.isEmpty && model.conversations.isEmpty)
        #expect(model.issue == .retired)
    }

    @Test func retirementDuringDropCannotPublishIntoTheNextGeneration() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("chat")]); await model.load()
        var callbacks = 0
        model.onConversationUpdated = { _ in callbacks += 1 }
        await service.holdUpdate(gate)
        let dropping = Task {
            await model.relocateConversation(id: "chat", to: .project("folder"), from: model.scope)
        }
        await gate.waitForArrival(); model.invalidate(); await gate.release(); await dropping.value
        #expect(callbacks == 0 && model.conversations.isEmpty && !model.isMutating)
    }

    @Test func deletionFenceAndInvalidDestinationDoNotSendUpdates() async throws {
        let (model, service, deletion, _, _) = LibraryModelTests().fixture()
        await service.configure(conversations: [libraryConversation("chat")]); await model.load()
        let fence = try deletion.begin(.all)
        await model.relocateConversation(id: "chat", to: .recent, from: model.scope)
        #expect(model.issue == .busy)
        #expect(await service.conversationUpdateIDs.isEmpty)
        deletion.release(fence)
        await model.relocateConversation(id: "chat", to: .project("  "), from: model.scope)
        #expect(model.issue == .invalidInput)
        #expect(await service.conversationUpdateIDs.isEmpty)
    }

    @Test func malformedAcknowledgementDoesNotPublishOrRemoveSelection() async {
        let (model, service, _, _, _) = LibraryModelTests().fixture()
        let original = libraryConversation("chat")
        await service.configure(conversations: [original]); await model.load()
        await service.returnMalformedConversationUpdate()
        var callbacks = 0
        model.onConversationUpdated = { _ in callbacks += 1 }
        model.selectedIDs = ["chat"]
        await model.relocateConversation(id: "chat", to: .pinned, from: model.scope)
        #expect(model.issue == .invalidResponse && callbacks == 0)
        #expect(model.conversations == [original] && model.selectedIDs == ["chat"])
    }
}
