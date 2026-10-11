import Testing
@testable import MapleAppCore

@MainActor struct DictationAdmissionOwnerTests {
    @Test func admissionPublishesExactOwnerOnceBeforePermissionAndActivity() async throws {
        let (runtime, coordinator, capture, _, _) = dictationFixture()
        runtime.draftText = "Owned B"
        let expected = DictationDraftOwner(
            scope: runtime.scope, storageOwnerID: runtime.snapshot.storageOwner, draftID: runtime.snapshot.draft.id)
        let permission = ChatTestGate(); capture.permissionGate = permission; capture.permission = false
        var owners: [DictationDraftOwner] = [], events: [String] = []
        coordinator.onCaptureAdmitted = {
            owners.append($0); events.append("owner")
        }
        coordinator.onActivityChange = { _ in events.append("activity") }
        #expect(coordinator.start(runtime: runtime))
        await permission.waitForArrival()
        #expect(owners == [expected] && events.prefix(2) == ["owner", "activity"])
        #expect(!coordinator.start(runtime: runtime))
        #expect(owners.count == 1)
        await permission.release()
        try await dictationWait { coordinator.statuses.isEmpty }
        #expect(capture.starts == 0 && owners == [expected])
        #expect(runtime.draftText == "Owned B")
        coordinator.invalidate(); runtime.invalidate()
    }
}
