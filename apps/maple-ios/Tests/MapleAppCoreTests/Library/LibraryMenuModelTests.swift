import Foundation
import MapleSDK
import Testing
@testable import MapleAppCore

@MainActor @Suite struct LibraryMenuModelTests {
    private func fixture() -> (LibraryMenuModel, LibraryFixture) {
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:3000", principalID: "menu-fixture")
        let store = ChatFixtureStore(), service = LibraryFixture(), chat = ChatFixtureService(store: store)
        let deletion = ChatDeletionCoordinator(scope: scope, retained: [], service: chat, persistence: store)
        let root = LibraryModel(scope: scope, service: service, deletions: deletion)
        return (
            LibraryMenuModel(root: root) {
                LibraryModel(scope: scope, service: service, deletions: deletion)
            }, service
        )
    }

    private func folder(_ id: String) -> ConversationProjectListItem {
        ConversationProjectListItem(id: id, object: "project", name: id, createdAt: 0, updatedAt: 0)
    }

    @Test func launchPreloadsOnlyFirstPagesAndRestoredOpenFolders() async {
        let (menu, service) = fixture()
        await service.configureProjects([folder("open"), folder("closed")])
        let recent = (0..<45).map { libraryConversation("recent-\($0)") }
        let open = (0..<105).map { libraryConversation("open-\($0)", project: "open") }
        await service.configure(
            conversations: recent + open + [
                libraryConversation("closed-chat", project: "closed"), libraryConversation("pinned", pinned: true),
            ])
        await menu.prepare { $0 == "open" }
        #expect(menu.isPrepared && menu.root.projects.count == 2)
        #expect(menu.pinned.visibleConversations.map(\.id) == ["pinned"])
        #expect(menu.recent.conversations == Array(recent.prefix(20)) && menu.recent.hasMore)
        #expect(menu.projects["open"]?.conversations == Array(open.prefix(100)))
        #expect(menu.projects["closed"]?.hasLoadedConversations == false)
        #expect(await service.conversationListCalls.count == 3)
        #expect(await service.conversationListCalls.allSatisfy { $0.page.after == nil })
        await menu.prepare { _ in true }
        #expect(await service.conversationListCalls.count == 3)
    }

    @Test func overlappingLaunchAndMenuPreparationShareRequestsAndSnapshots() async {
        let (menu, service) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("chat")], listGate: gate)
        let first = Task { await menu.prepare { _ in false } }
        await gate.waitForArrival()
        let second = Task { await menu.prepare { _ in false } }
        await Task.yield(); await gate.release()
        await first.value; await second.value
        #expect(await service.conversationListCalls.count == 2)
        #expect(await service.projectListCalls.count == 1)
        let retained = menu.recent
        await menu.prepare { _ in false }
        #expect(menu.recent === retained && menu.recent.visibleConversations.map(\.id) == ["chat"])
    }

    @Test func retirementDuringPreloadCannotRepopulateMenu() async {
        let (menu, service) = fixture(), gate = ChatTestGate()
        await service.configure(conversations: [libraryConversation("private")], listGate: gate)
        let loading = Task { await menu.prepare { _ in true } }
        await gate.waitForArrival(); menu.invalidate(); menu.root.invalidate()
        await gate.release(); await loading.value
        #expect(menu.projects.isEmpty && menu.recent.conversations.isEmpty && menu.pinned.conversations.isEmpty)
        #expect(!menu.isPrepared)
        await menu.prepare { _ in true }
        #expect(menu.projects.isEmpty)
    }

    @Test func refreshRetainsLoadedDepthAndFolderOwnersWithoutFetchingClosedFolders() async {
        let (menu, service) = fixture()
        await service.configureProjects([folder("open"), folder("closed")])
        await service.configure(conversations: (0..<45).map { libraryConversation(String($0)) })
        await menu.prepare { _ in false }
        let recent = menu.recent, closed = menu.project("closed")
        await recent.loadMore()
        await menu.refresh { _ in false }
        #expect(menu.recent === recent && menu.project("closed") === closed)
        #expect(recent.conversations.count == 30 && recent.hasMore)
        #expect(!closed.hasLoadedConversations)
        #expect(await service.conversationListCalls.filter { $0.projectId != nil }.isEmpty)
        #expect(
            await service.conversationListCalls.filter { $0.unassignedProject == true }.map(\.page.limit)
                == [20, 10, 20, 10])
    }

    @Test func searchChangedDuringFolderReadPublishesOnlyCurrentFilter() async {
        let (menu, service) = fixture(), gate = ChatTestGate()
        await service.configureProjects([folder("closed")])
        await service.configure(conversations: [libraryConversation("one", project: "closed")])
        await menu.prepare { _ in false }
        await service.configure(listGate: gate)
        menu.setSearch("Title one")
        await gate.waitForArrival()
        menu.setSearch("")
        await gate.release()
        await menu.refresh { _ in true }
        #expect(menu.searchText.isEmpty && menu.project("closed").searchText.isEmpty)
        #expect(menu.project("closed").visibleConversations.map(\.id) == ["one"])
        #expect(await service.conversationListCalls.filter { $0.projectId == "closed" }.count <= 2)
    }

    @Test func hiddenMenuCancelsSearchReadsAndResumesWithoutDiscardingQuery() async {
        let (menu, service) = fixture(), gate = ChatTestGate()
        await service.configureProjects([folder("first"), folder("second")])
        await service.configure(conversations: [
            libraryConversation("one", project: "first"), libraryConversation("two", project: "second"),
        ])
        await menu.prepare { _ in false }
        await service.configure(listGate: gate)
        menu.setSearch("Title")
        await gate.waitForArrival(); menu.cancelSearchLoading(); await gate.release()
        await menu.refresh { _ in false }
        #expect(menu.searchText == "Title" && menu.project("first").searchText == "Title")
        #expect(await service.conversationListCalls.filter { $0.projectId == "first" }.count == 1)
        #expect(await service.conversationListCalls.filter { $0.projectId == "second" }.isEmpty)
        await service.configure(listGate: nil)
        menu.setSearch(menu.searchText)
        await menu.refresh { _ in false }
        #expect(menu.project("first").visibleConversations.map(\.id) == ["one"])
        #expect(menu.project("second").visibleConversations.map(\.id) == ["two"])
    }

    @Test func visibleMenuPollsOnlyRecentsOnceWithoutRecreatingItsOwners() async {
        let (menu, service) = fixture(), first = ChatTestGate(), next = ChatTestGate()
        await service.configure(conversations: [libraryConversation("old")])
        await menu.prepare { _ in false }
        let recent = menu.recent, pinned = menu.pinned
        let clock = LibraryRecentsSleepRecorder(gates: [first, next])
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await clock.sleep($0) })
        await first.waitForArrival()
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await clock.sleep($0) })
        await service.configure(conversations: [libraryConversation("new"), libraryConversation("old")])
        await first.release(); await next.waitForArrival()
        #expect(menu.recent === recent && menu.pinned === pinned)
        #expect(menu.recent.conversations.map(\.id) == ["new", "old"])
        #expect(await service.conversationListCalls.count == 3)
        #expect(await service.projectListCalls.count == 1)
        #expect(await clock.delays == [.seconds(60), .seconds(60)])
        menu.updateRecentsPolling(visible: false, foreground: true)
        await next.release()
    }

    @Test func failedPollingClockCanRestartWithoutHidingTheMenu() async {
        let (menu, service) = fixture(), sleeping = ChatTestGate()
        let failedClock = LibraryRecentsSleepRecorder(fails: true)
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await failedClock.sleep($0) })
        let restartedClock = LibraryRecentsSleepRecorder(gates: [sleeping])
        // Actor turns make progress without depending on an elapsed wall-clock
        // delay. A retained completed task would prevent every restart attempt.
        for _ in 0..<100 {
            menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await restartedClock.sleep($0) })
            if !(await restartedClock.delays).isEmpty { break }
            await Task.yield()
        }
        #expect(await failedClock.delays == [.seconds(60)])
        #expect(await restartedClock.delays == [.seconds(60)])
        #expect(await service.conversationListCalls.isEmpty)
        menu.updateRecentsPolling(visible: false, foreground: true)
        await sleeping.release(); menu.invalidate()
    }

    @Test func oldPollCompletionCannotClearItsVisibleReplacement() async {
        let (menu, service) = fixture(), oldSleep = ChatTestGate(), newSleep = ChatTestGate()
        let oldClock = LibraryRecentsSleepRecorder(gates: [oldSleep])
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await oldClock.sleep($0) })
        await oldSleep.waitForArrival()
        menu.updateRecentsPolling(visible: false, foreground: true)
        let newClock = LibraryRecentsSleepRecorder(gates: [newSleep])
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await newClock.sleep($0) })
        await newSleep.waitForArrival(); await oldSleep.release()
        let unexpectedClock = LibraryRecentsSleepRecorder(fails: true)
        for _ in 0..<20 {
            menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await unexpectedClock.sleep($0) })
            _ = await unexpectedClock.delays
            await Task.yield()
        }
        #expect(await newClock.delays == [.seconds(60)])
        #expect(await unexpectedClock.delays.isEmpty)
        #expect(await service.conversationListCalls.isEmpty)
        menu.updateRecentsPolling(visible: false, foreground: true)
        await newSleep.release(); menu.invalidate()
    }

    enum RecentsOwnerChange: CaseIterable, Sendable, Equatable { case hidden, backgrounded, retired }

    @Test(arguments: RecentsOwnerChange.allCases)
    func recentsOwnershipLossDuringReadCannotPublishLateRows(_ change: RecentsOwnerChange) async {
        let (menu, service) = fixture(), request = ChatTestGate(), sleeping = ChatTestGate()
        await service.configure(conversations: [libraryConversation("old")]); await menu.prepare { _ in false }
        let clock = LibraryRecentsSleepRecorder(gates: [sleeping])
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await clock.sleep($0) })
        await sleeping.waitForArrival()
        await service.configure(conversations: [libraryConversation("late")], listGate: request)
        let refreshing = Task { await menu.refreshRecents() }
        await request.waitForArrival()
        switch change {
        case .hidden: menu.updateRecentsPolling(visible: false, foreground: true)
        case .backgrounded: menu.updateRecentsPolling(visible: true, foreground: false)
        case .retired: menu.invalidate()
        }
        await request.release(); await refreshing.value; await sleeping.release()
        #expect(menu.recent.conversations.map(\.id) == (change == .retired ? [] : ["old"]))
    }

    @Test func conversationCreatedRefreshesHiddenRecentsButNotBackgroundedOrRetiredMenu() async {
        let (menu, service) = fixture()
        await service.configure(conversations: [libraryConversation("old")]); await menu.prepare { _ in false }
        await service.configure(conversations: [libraryConversation("created"), libraryConversation("old")])
        await menu.refreshRecents()
        #expect(menu.recent.conversations.map(\.id) == ["created", "old"])
        menu.updateRecentsPolling(visible: false, foreground: false)
        await menu.refreshRecents()
        menu.invalidate(); await menu.refreshRecents()
        #expect(await service.conversationListCalls.count == 3)
    }

    @Test func hidingMenuDoesNotCancelExplicitLoadedWindowRefresh() async {
        let (menu, service) = fixture(), request = ChatTestGate(), sleeping = ChatTestGate()
        await service.configure(conversations: [libraryConversation("old")]); await menu.prepare { _ in false }
        let clock = LibraryRecentsSleepRecorder(gates: [sleeping])
        menu.updateRecentsPolling(visible: true, foreground: true, sleep: { try await clock.sleep($0) })
        await sleeping.waitForArrival()
        await service.configure(conversations: [libraryConversation("fresh")], listGate: request)
        let refreshing = Task { await menu.refresh { _ in false } }
        await request.waitForArrival(); menu.updateRecentsPolling(visible: false, foreground: true)
        await request.release(); await refreshing.value; await sleeping.release()
        #expect(menu.recent.conversations.map(\.id) == ["fresh"])
    }
}
