import Foundation
import Testing
@testable import MapleAppCore

@Suite struct DictationAnchorTests {
    @Test func editsBeforeAtAndAcrossMarkerRespectGraphemeBoundaries() {
        var anchor = DictationInsertionAnchor(text: "A👨‍👩‍👧‍👦B café", offsetUTF16: 12)
        anchor.rebase(to: "New A👨‍👩‍👧‍👦B café")
        #expect(anchor.inserting("voice") == "New A👨‍👩‍👧‍👦 voice B café")
        anchor.rebase(to: "New A👨‍👩‍👧‍👦typedB café")
        #expect(anchor.inserting("voice") == "New A👨‍👩‍👧‍👦typed voice B café")
        anchor.rebase(to: "New replacement café")
        #expect(anchor.inserting("voice") == "New replacement voice café")
    }

    @Test func invalidUTF16OffsetNeverSplitsSurrogateOrCombiningSequence() {
        #expect(DictationInsertionAnchor(text: "👩🏽‍💻x", offsetUTF16: 1).inserting("hello") == "hello 👩🏽‍💻x")
        #expect(DictationInsertionAnchor(text: "e\u{301}x", offsetUTF16: 1).inserting("hello") == "hello e\u{301}x")
        #expect(DictationInsertionAnchor(text: "Hi, there!", offsetUTF16: 2).inserting("friend") == "Hi friend, there!")
    }

    @Test func exactTransactionsDistinguishDeletingIdenticalCharacters() {
        var first = DictationInsertionAnchor(text: "aaaa", offsetUTF16: 2)
        var last = first
        let rebasedFirst = first.rebase(
            using: DictationTextEdit(beforeText: "aaaa", rangeUTF16: NSRange(location: 0, length: 1), replacement: ""))
        let rebasedLast = last.rebase(
            using: DictationTextEdit(beforeText: "aaaa", rangeUTF16: NSRange(location: 3, length: 1), replacement: ""))
        #expect(rebasedFirst && rebasedLast)
        #expect(first.text == last.text && first.offsetUTF16 == 1 && last.offsetUTF16 == 2)
    }

    @Test func wavIsBoundedMonoPCM16AndContainsNoExtraEnvelope() throws {
        let pcm = Data([1, 0, 255, 127])
        let wav = try DictationAudio.wav(pcm16Mono: pcm)
        #expect(wav.count == 48 && String(decoding: wav.prefix(4), as: UTF8.self) == "RIFF")
        #expect(Array(wav[22..<24]) == [1, 0])
        #expect(Array(wav[24..<28]) == [128, 62, 0, 0])
        #expect(Array(wav[34..<36]) == [16, 0] && wav.suffix(4) == pcm)
        #expect(throws: DictationIssue.emptyRecording) { try DictationAudio.wav(pcm16Mono: Data()) }
        #expect(throws: DictationIssue.recordingTooLarge) { try DictationAudio.wav(pcm16Mono: Data([1])) }
        #expect(throws: DictationIssue.recordingTooLarge) {
            try DictationAudio.wav(pcm16Mono: Data(repeating: 0, count: DictationAudio.maximumPCMBytes + 2))
        }
    }
}

@MainActor @Suite struct DictationWorkflowTests {
    @Test func pendingTextUsesStopMarkerDespiteLaterCaretAndEditsAndNeverSends() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Hello world"; dictationEditor(runtime, dictation, offset: 5)
        #expect(dictation.start(runtime: runtime)); #expect(!runtime.canSend)
        await dictationWait { capture.starts == 1 }
        runtime.draftText = "Hello great world"; dictationEditor(runtime, dictation, offset: 11)
        dictation.stop(runtime: runtime); #expect(capture.stops == 1)
        await service.waitForCalls()
        runtime.draftText = "Dear Hello great world"; dictationEditor(runtime, dictation, offset: 0)
        await service.complete(text: "voice")
        await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.draftText == "Dear Hello great voice world")
        #expect(runtime.canSend && !runtime.hasUnresolvedRun)
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func completedResultWaitsForIMECommitAndRebasesCommittedSelectionReplacement() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Before after"; dictationEditor(runtime, dictation, offset: 6)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        runtime.draftText = "前 after"
        dictationEditor(runtime, dictation, offset: 1, marked: NSRange(location: 0, length: 1))
        await service.complete(text: "voice")
        await dictationWait { dictation.status(for: runtime)?.phase == .waitingForComposition }
        #expect(runtime.draftText == "前 after" && !runtime.canSend)
        runtime.draftText = "前文 after"; dictationEditor(runtime, dictation, offset: 2)
        #expect(runtime.draftText == "前文 voice after" && runtime.canSend)
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func cancellationRejectsLateResultWithoutChangingTextImagesOrAnotherBusyOwner() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.updateDraft(
            ChatDraft(
                id: runtime.snapshot.draft.id, text: "Keep",
                images: [ChatImageAttachment(data: Data([1]), contentType: "image/png")]))
        let before = runtime.snapshot.draft; let unrelated = UUID(); runtime.busyDraftIDs.insert(unrelated)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        dictation.cancel(runtime: runtime); await service.complete()
        for _ in 0..<20 { await Task.yield() }
        #expect(runtime.snapshot.draft == before && runtime.busyDraftIDs == [unrelated])
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func permissionDenialAndNavigationDuringPromptNeverStartCapture() async {
        let (runtime, dictation, capture, _, _) = dictationFixture()
        let gate = ChatTestGate(); capture.permissionGate = gate; runtime.draftText = "Keep"
        #expect(dictation.start(runtime: runtime)); await gate.waitForArrival()
        dictation.stopCaptureForNavigation(); await gate.release()
        await dictationWait { dictation.statuses.isEmpty }
        #expect(capture.starts == 0 && runtime.canSend)
        capture.permissionGate = nil; capture.permission = false
        #expect(dictation.start(runtime: runtime)); await dictationWait { dictation.statuses.isEmpty }
        #expect(dictation.lastIssue == .permissionDenied && capture.starts == 0 && runtime.draftText == "Keep")
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func retryUsesSameMemoryClipAndDiscardReleasesOwner() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Keep"
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls(); await service.complete(fails: true)
        await dictationWait { dictation.status(for: runtime)?.phase == .failed }
        #expect(runtime.draftText == "Keep" && !runtime.canSend)
        dictation.retry(runtime: runtime); await service.waitForCalls(2)
        let clips = await service.clips; #expect(clips[0] == clips[1] && capture.starts == 1)
        dictation.cancel(runtime: runtime); await service.complete(1)
        #expect(runtime.canSend && runtime.draftText == "Keep")
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func limitStopsOnceAndLogoutRejectsPendingResultAndSameUserRelogin() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Old account draft"
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        capture.limits?(); capture.limits?(); #expect(capture.stops == 1)
        await service.waitForCalls()
        dictation.invalidate(); runtime.invalidate()
        let (replacement, _, _) = ChatRuntimeTests().fixture()
        replacement.draftText = "New account generation"
        #expect(!dictation.start(runtime: replacement))
        await service.complete()
        for _ in 0..<20 { await Task.yield() }
        #expect(runtime.draftText.isEmpty && replacement.draftText == "New account generation")
        replacement.invalidate()
    }

    @Test func protectedSaveFailureKeepsInsertedResultInMemoryWithoutSavingAudio() async {
        let (runtime, dictation, capture, service, store) = dictationFixture()
        runtime.draftText = "Keep"; await runtime.saveDraft()
        await store.configure(fail: true)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls(); await service.complete(text: "voice")
        await dictationWait { dictation.statuses.isEmpty }
        await runtime.saveDraft()
        #expect(runtime.draftText == "Keep voice" && runtime.lastIssue == .storageUnavailable)
        #expect(await store.recorded("conversation")?.draft.text == "Keep")
        await store.configure(); await runtime.saveDraft()
        #expect(await store.recorded("conversation")?.draft.text == "Keep voice")
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func staleEditorSnapshotsCannotMoveStopMarker() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Before after"; dictationEditor(runtime, dictation, offset: 6)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        dictation.updateEditor(
            runtime: runtime, draftID: runtime.snapshot.draft.id,
            state: DictationEditorState(text: "Outdated", selectionUTF16: NSRange(location: 0, length: 0)))
        await service.complete(text: "voice"); await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.draftText == "Before voice after")
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func retiredRuntimeCannotStartMicrophoneAfterPermissionFinishes() async {
        let (runtime, dictation, capture, _, _) = dictationFixture()
        let gate = ChatTestGate(); capture.permissionGate = gate
        runtime.draftText = "Retired"
        #expect(dictation.start(runtime: runtime)); await gate.waitForArrival()
        runtime.invalidate(); await gate.release()
        await dictationWait { dictation.statuses.isEmpty }
        #expect(capture.starts == 0 && !dictation.start(runtime: runtime))
        dictation.invalidate()
    }

    @Test func runtimeRetirementSynchronouslyStopsCaptureAndMatchedDeletionCancelsOnlyItsOwner() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Local draft"
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        let fence = UUID(); runtime.beginDeletionFence(fence)
        #expect(capture.active != nil)
        runtime.onQuiesceForDeletion = {
            for owner in runtime.snapshot.ownedDrafts { dictation.cancel(runtime: runtime, draftID: owner.id) }
        }
        runtime.requestDeletionQuiescence(fence)
        #expect(capture.active == nil && dictation.statuses.isEmpty)
        #expect(await service.clips.isEmpty)
        runtime.releaseDeletionFence(fence)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 2 }
        runtime.invalidate()
        #expect(capture.active == nil && dictation.statuses.isEmpty)
        dictation.invalidate()
    }

    @Test func exactCoalescedEditsCommitCompositionBeforeResultInsertion() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "aaaa"; dictationEditor(runtime, dictation, offset: 2)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        runtime.draftText = "aaa"
        dictation.updateEditor(
            runtime: runtime, draftID: runtime.snapshot.draft.id,
            state: DictationEditorState(
                text: "aaa", selectionUTF16: NSRange(location: 0, length: 0),
                markedRangeUTF16: NSRange(location: 0, length: 1)),
            edits: [DictationTextEdit(beforeText: "aaaa", rangeUTF16: NSRange(location: 0, length: 1), replacement: "")]
        )
        await service.complete(text: "voice")
        await dictationWait { dictation.status(for: runtime)?.phase == .waitingForComposition }
        runtime.draftText = "前aaa"
        dictation.updateEditor(
            runtime: runtime, draftID: runtime.snapshot.draft.id,
            state: DictationEditorState(text: "前aaa", selectionUTF16: NSRange(location: 1, length: 0)),
            edits: [
                DictationTextEdit(beforeText: "aaa", rangeUTF16: NSRange(location: 0, length: 0), replacement: "まえ"),
                DictationTextEdit(beforeText: "まえaaa", rangeUTF16: NSRange(location: 0, length: 2), replacement: "前"),
            ])
        #expect(runtime.draftText == "前a voice aa" && dictation.statuses.isEmpty)
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func unmatchedInteractiveTransactionRequiresExplicitPositionInsteadOfGuessing() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "aaaa"; dictationEditor(runtime, dictation, offset: 2)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        runtime.draftText = "aaa"
        dictation.updateEditor(
            runtime: runtime, draftID: runtime.snapshot.draft.id,
            state: DictationEditorState(text: "aaa", selectionUTF16: NSRange(location: 0, length: 0)),
            edits: [
                DictationTextEdit(
                    beforeText: "aaaa", rangeUTF16: NSRange(location: NSNotFound, length: 0), replacement: "")
            ])
        await service.complete(text: "voice")
        await dictationWait { dictation.status(for: runtime)?.phase == .waitingForInsertionPoint }
        #expect(runtime.draftText == "aaa" && !runtime.canSend)
        dictation.insertAtCurrentPosition(runtime: runtime)
        #expect(runtime.draftText == "voice aaa" && runtime.canSend)
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func retainedClipsAreBoundedAndForeignAccountCannotUseTheCapture() async {
        let (first, dictation, capture, service, store) = dictationFixture()
        var runtimes = [first]
        for index in 1...4 {
            runtimes.append(
                ChatRuntime(
                    scope: first.scope, service: ChatFixtureService(store: store), persistence: store,
                    conversationID: "conversation-\(index)"))
        }
        for index in 0..<4 {
            runtimes[index].draftText = "Preserve \(index)"
            #expect(dictation.start(runtime: runtimes[index]))
            await dictationWait { capture.starts == index + 1 }
            dictation.stop(runtime: runtimes[index]); await service.waitForCalls(index + 1)
        }
        runtimes[4].draftText = "Keep fifth draft"
        #expect(!dictation.start(runtime: runtimes[4]))
        #expect(dictation.issue(for: runtimes[4]) == .tooManyOperations)
        #expect(dictation.issue(for: first) == nil && runtimes[4].canSend)
        let (foreign, _, _) = ChatRuntimeTests().fixture()
        foreign.draftText = "Other account generation"
        #expect(!dictation.start(runtime: foreign))
        for index in 0..<4 { await service.complete(index, fails: true) }
        await dictationWait { dictation.statuses.values.allSatisfy { $0.phase == .failed } }
        dictation.invalidate()
        #expect(runtimes.allSatisfy { $0.busyDraftIDs.isEmpty } && capture.active == nil)
        for runtime in runtimes { runtime.invalidate() }; foreign.invalidate()
    }

    @Test func insertionPreservesTheCaretMovedAfterStopRelativeToItsSurroundingText() async {
        let (runtime, dictation, capture, service, _) = dictationFixture()
        runtime.draftText = "Hello world"; let owner = runtime.snapshot.draft.id
        dictationEditor(runtime, dictation, offset: 5)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        dictationEditor(runtime, dictation, offset: 11)
        await service.complete(text: "voice"); await dictationWait { dictation.statuses.isEmpty }
        let editor = dictation.editorState(runtime: runtime, draftID: owner)
        #expect(runtime.draftText == "Hello voice world")
        #expect(editor?.text == runtime.draftText && editor?.selectionUTF16 == NSRange(location: 17, length: 0))
        dictation.invalidate(); runtime.invalidate()
    }

    @Test func anotherRuntimeWithClonedDraftCannotRedirectTheEditorOrItsMarker() async {
        let (runtime, dictation, capture, service, store) = dictationFixture()
        runtime.draftText = "Hello world"; let owner = runtime.snapshot.draft.id
        dictationEditor(runtime, dictation, offset: 5)
        #expect(dictation.start(runtime: runtime)); await dictationWait { capture.starts == 1 }
        dictation.stop(runtime: runtime); await service.waitForCalls()
        let clone = ChatRuntime(scope: runtime.scope, service: ChatFixtureService(store: store), persistence: store)
        #expect(clone.loadRetainedSnapshot(runtime.snapshot))
        dictation.updateEditor(
            runtime: clone, draftID: owner,
            state: DictationEditorState(
                text: runtime.draftText, selectionUTF16: NSRange(location: 0, length: 0),
                markedRangeUTF16: NSRange(location: 0, length: 1)))
        #expect(dictation.editorState(runtime: clone, draftID: owner) == nil)
        await service.complete(text: "voice"); await dictationWait { dictation.statuses.isEmpty }
        #expect(runtime.draftText == "Hello voice world" && clone.draftText == "Hello world")
        dictation.invalidate(); runtime.invalidate(); clone.invalidate()
    }
}
