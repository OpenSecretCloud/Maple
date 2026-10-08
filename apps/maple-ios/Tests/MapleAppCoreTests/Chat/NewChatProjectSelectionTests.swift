import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct NewChatProjectSelectionTests {
    enum Replacement: CaseIterable, Sendable, Equatable { case generation, principal, origin }
    enum InvalidCurrent: CaseIterable, Sendable {
        case signedOut, missingRuntime, foreignRuntime, retiredRuntime, wrongKey, assignedConversation
    }

    @Test func winningSelectionKeepsExistingDraftAndQueueAndCanClearProject() async throws {
        let owner = Owner(), gate = ChatTestGate()
        let runtime = try #require(owner.runtime)
        runtime.isOnline = false
        runtime.draftText = "Queued synthetic draft"
        #expect(runtime.enqueueDraft())
        runtime.draftText = "Unsent synthetic draft"
        let draft = runtime.snapshot.draft, queue = runtime.queuedTurns
        let selecting = Task { await owner.select("project", gate: gate) }
        await gate.waitForArrival()
        #expect(owner.assignments == 0)
        await gate.release(); await selecting.value
        #expect(owner.projects[runtime.snapshot.storageOwner] == "project")
        #expect(owner.assignments == 1 && owner.runtime === runtime)
        #expect(runtime.snapshot.draft == draft && runtime.queuedTurns == queue)
        await owner.select(nil)
        #expect(owner.projects[runtime.snapshot.storageOwner] == nil && owner.assignments == 2)
        #expect(runtime.snapshot.draft == draft && runtime.queuedTurns == queue)
        runtime.invalidate()
    }

    @Test(arguments: Replacement.allCases)
    func suspendedSelectionCannotAdoptReplacementAccount(_ replacement: Replacement) async throws {
        let owner = Owner(), gate = ChatTestGate()
        let oldRuntime = try #require(owner.runtime), oldScope = try #require(owner.scope)
        let selecting = Task { await owner.select("old-project", gate: gate) }
        await gate.waitForArrival()
        let nextScope = AccountScope(
            apiOrigin: replacement == .origin ? "https://replacement.example" : oldScope.apiOrigin,
            principalID: replacement == .principal ? "replacement" : oldScope.principalID,
            generation: replacement == .generation ? UUID() : oldScope.generation)
        oldRuntime.invalidate()
        let nextRuntime = Owner.makeRuntime(scope: nextScope)
        nextRuntime.draftText = "Replacement draft"
        owner.scope = nextScope; owner.runtime = nextRuntime
        owner.projects[nextRuntime.snapshot.storageOwner] = "replacement-project"
        let draft = nextRuntime.snapshot.draft
        // Even an erroneously retained ticket cannot substitute a new generation.
        await gate.release(); await selecting.value
        #expect(owner.assignments == 0)
        #expect(owner.projects[nextRuntime.snapshot.storageOwner] == "replacement-project")
        #expect(nextRuntime.snapshot.draft == draft)
        nextRuntime.invalidate()
    }

    @Test func newerSameAccountSelectionWinsEvenWhenItReusesTheDraftRuntime() async throws {
        let owner = Owner(), gate = ChatTestGate()
        let runtime = try #require(owner.runtime)
        runtime.draftText = "Retained draft"
        let draft = runtime.snapshot.draft
        let oldSelection = Task { await owner.select("old-project", gate: gate) }
        await gate.waitForArrival()
        await owner.select("winning-project")
        #expect(owner.assignments == 1)
        await gate.release(); await oldSelection.value
        #expect(owner.assignments == 1 && owner.projects[runtime.snapshot.storageOwner] == "winning-project")
        #expect(owner.runtime === runtime && runtime.snapshot.draft == draft)
        runtime.invalidate()
    }

    @Test(arguments: InvalidCurrent.allCases)
    func incompleteOrForeignCurrentSelectionNeverReceivesProject(_ invalid: InvalidCurrent) async throws {
        let owner = Owner(), gate = ChatTestGate()
        let original = try #require(owner.runtime), scope = try #require(owner.scope)
        owner.projects[original.snapshot.storageOwner] = "keep-project"
        let projects = owner.projects
        let selecting = Task { await owner.select("wrong-project", gate: gate) }
        await gate.waitForArrival()
        switch invalid {
        case .signedOut: owner.scope = nil
        case .missingRuntime: owner.runtime = nil
        case .foreignRuntime:
            owner.runtime = Owner.makeRuntime(scope: AccountScope(apiOrigin: scope.apiOrigin, principalID: "other"))
        case .retiredRuntime: original.invalidate()
        case .wrongKey: owner.key = "another-selection"
        case .assignedConversation: owner.runtime = Owner.makeRuntime(scope: scope, conversationID: "conversation")
        }
        await gate.release(); await selecting.value
        #expect(owner.assignments == 0 && owner.projects == projects)
        owner.runtime?.invalidate(); original.invalidate()
    }

    @Test(arguments: [false, true])
    func abortedOrCancelledSelectionCannotAssignEvenWhenCurrentStateStillMatches(_ cancel: Bool) async throws {
        let owner = Owner(), gate = ChatTestGate()
        let runtime = try #require(owner.runtime)
        runtime.draftText = "Draft survives refusal"
        let draft = runtime.snapshot.draft
        owner.projects[runtime.snapshot.storageOwner] = "keep-project"
        let selecting = Task { await owner.select("wrong-project", gate: gate, completed: cancel) }
        await gate.waitForArrival()
        if cancel { selecting.cancel() }
        await gate.release(); await selecting.value
        #expect(owner.assignments == 0 && owner.projects[runtime.snapshot.storageOwner] == "keep-project")
        #expect(runtime.snapshot.draft == draft)
        runtime.invalidate()
    }

    @MainActor private final class Owner {
        var scope: AccountScope?
        var navigation = UUID()
        var key = ChatSnapshot.newChatKey
        var runtime: ChatRuntime?
        var projects: [UUID: String] = [:]
        var assignments = 0

        init() {
            let scope = AccountScope(apiOrigin: "https://fixture.example", principalID: "fixture")
            self.scope = scope; runtime = Self.makeRuntime(scope: scope)
        }

        static func makeRuntime(scope: AccountScope, conversationID: String? = nil) -> ChatRuntime {
            let store = ChatFixtureStore(), service = ChatFixtureService(store: store)
            return ChatRuntime(scope: scope, service: service, persistence: store, conversationID: conversationID)
        }

        func select(_ project: String?, gate: ChatTestGate? = nil, completed: Bool = true) async {
            guard let scope else { return }
            await NewChatProjectSelection.perform(
                scope: scope, projectID: project,
                navigate: { ticket in
                    self.navigation = ticket
                    if let gate { await gate.hold() }
                    return completed
                },
                current: {
                    .init(
                        accountScope: self.scope, navigation: self.navigation, selectedKey: self.key,
                        runtime: self.runtime)
                },
                assign: { runtime, project in
                    self.assignments += 1
                    self.projects[runtime.snapshot.storageOwner] = project
                })
        }
    }
}
