import Foundation
import Testing
@testable import MapleAppCore

@MainActor @Suite struct InstructionOwnershipTests {
    @Test func typingDuringReloadPreservesTextAndItsOriginalInstructionOwner() async {
        let service = LibraryFixture(), gate = ChatTestGate()
        await service.configure(instructions: [libraryInstruction("original", prompt: "First")])
        let editor = InstructionEditor(scope: accountScope(), destination: .account, service: service)
        await editor.load()
        await service.configure(
            instructions: [libraryInstruction("replacement", prompt: "From reload")], listGate: gate)
        let reloading = Task { await editor.load() }
        await gate.waitForArrival()
        editor.text = "Text typed during reload"
        await gate.release(); await reloading.value
        #expect(editor.text == "Text typed during reload" && editor.isDirty && !editor.isLoading)
        await editor.save()
        #expect(await service.instructionUpdateIDs == ["original"])
    }

    @Test func failedSavePreservesDirtyDraftAndAllowsExplicitRetry() async {
        let service = LibraryFixture()
        await service.configure(instructions: [libraryInstruction("default")])
        let editor = InstructionEditor(scope: accountScope(), destination: .account, service: service)
        await editor.load(); editor.text = "Unsaved text"
        await service.holdInstructionSave(nil, fail: true)
        await editor.save()
        #expect(editor.text == "Unsaved text" && editor.isDirty && !editor.didSave)
        #expect(editor.issue == .connection && !editor.isSaving)
        await service.holdInstructionSave(nil)
        await editor.save()
        #expect(editor.didSave && !editor.isDirty)
    }

    @Test func invalidatingWhileSavingClearsTextAndRejectsLateSuccess() async {
        let service = LibraryFixture(), gate = ChatTestGate()
        await service.configure(instructions: [libraryInstruction("default")])
        await service.holdInstructionSave(gate)
        let editor = InstructionEditor(scope: accountScope(), destination: .account, service: service)
        await editor.load(); editor.text = "Submitted"
        let saving = Task { await editor.save() }
        await gate.waitForArrival()
        editor.text = "Attempt to mutate submission"
        #expect(editor.text == "Submitted")
        editor.invalidate()
        await gate.release(); await saving.value
        #expect(editor.text.isEmpty && !editor.isLoaded && !editor.isSaving && !editor.didSave)
    }

    @Test func multipleDefaultsFailClosedAndNeverChooseListOrder() async {
        let service = LibraryFixture()
        await service.configure(instructions: [libraryInstruction("a"), libraryInstruction("b")])
        let editor = InstructionEditor(scope: accountScope(), destination: .account, service: service)
        await editor.load(); editor.text = "Replacement"; await editor.save()
        #expect(editor.issue == .invalidResponse && !editor.isLoaded)
        #expect(await service.instructionChanges.isEmpty)
    }
}
