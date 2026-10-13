#if os(iOS) && DEBUG
    import MapleAppCore
    import SwiftUI
    import UIKit
    import UniformTypeIdentifiers

    @MainActor
    enum ComposerEditOrderingProbe {
        private static var failureDetails: [String] = []

        private static func labeledCheck(_ result: Bool, label: String, check: (Bool) -> Void) {
            check(result)
            if !result { failureDetails.append(label) }
        }

        // Same production Coordinator and native UITextView; each case controls
        // callback ordering without a real delegate silently repairing the sequence.
        static func run() async -> String {
            let cases: [(String, ((Bool) -> Void) -> Void)] = [
                (
                    "proposedEditSurvivesPreMutationSelectionAndPublishesExactlyOnce",
                    proposedEditSurvivesPreMutationSelectionAndPublishesExactlyOnce
                ),
                (
                    "sameTextReplacementWaitsForCompletionThenCarriesExactIntent",
                    sameTextReplacementWaitsForCompletionThenCarriesExactIntent
                ),
                (
                    "rejectedProposalDoesNotCreateAnEditOrPoisonTheNextKnownEdit",
                    rejectedProposalDoesNotCreateAnEditOrPoisonTheNextKnownEdit
                ),
                ("unmatchedActualMutationStillFailsClosed", unmatchedActualMutationStillFailsClosed),
                ("mutationWithoutAProposalStillFailsClosed", mutationWithoutAProposalStillFailsClosed),
                (
                    "oldOwnerFlushRetainsExactEditAndRoutesItOnlyToOldOwner",
                    oldOwnerFlushRetainsExactEditAndRoutesItOnlyToOldOwner
                ),
                ("overflowingActualInputStillFailsClosed", overflowingActualInputStillFailsClosed),
                (
                    "ownedInputCompletionPublishesAfterNestedCallbacks",
                    ownedInputCompletionPublishesAfterNestedCallbacks
                ),
                (
                    "authoritativeSameOwnerUpdateRetiresPendingProposal",
                    authoritativeSameOwnerUpdateRetiresPendingProposal
                ),
                (
                    "resignationIncludesPriorInputBeforeSubmit",
                    resignationIncludesPriorInputBeforeSubmit
                ),
                ("resignationOrdersPriorInputBeforeShiftNewline", resignationOrdersPriorInputBeforeShiftNewline),
                (
                    "hardwareReturnAndKeypadCommandsUseSameGuardedPath",
                    hardwareReturnAndKeypadCommandsUseSameGuardedPath
                ),
                (
                    "ordinaryTextInputPreservesNewlinesWithoutSubmitting",
                    ordinaryTextInputPreservesNewlinesWithoutSubmitting
                ),
                (
                    "markdownNewlineRetainsExactDictationAnchorAndComposition",
                    markdownNewlineRetainsExactDictationAnchorAndComposition
                ),
                ("markedInputDoesNotExposeOrExecuteReturnCommands", markedInputDoesNotExposeOrExecuteReturnCommands),
                ("retiredOwnerCannotExecuteCachedReturnCommand", retiredOwnerCannotExecuteCachedReturnCommand),
                (
                    "reentrantHardwareCommandCannotSubmitTwice",
                    reentrantHardwareCommandCannotSubmitTwice
                ),
                ("distinctShiftActionsEachInsertOneNewline", distinctShiftActionsEachInsertOneNewline),
                ("refusedResignationDoesNotSendOrClear", refusedResignationDoesNotSendOrClear),
                ("compositionDuringResignationCancelsWithoutCommit", compositionDuringResignationCancelsWithoutCommit),
                ("revokedOwnerWithoutInstallationCannotSend", revokedOwnerWithoutInstallationCannotSend),
                ("successorIsInstalledBeforeHardwareFocusResumes", successorIsInstalledBeforeHardwareFocusResumes),
                (
                    "replacementDuringResignationCancelsSend",
                    replacementDuringResignationCancelsSend
                ),
                ("nestedFocusLossDuringResignationCancelsSend", nestedFocusLossDuringResignationCancelsSend),
                ("pasteDuringResignationCancelsSend", pasteDuringResignationCancelsSend),
                ("publicationRevocationCancelsHardwareSend", publicationRevocationCancelsHardwareSend),
                (
                    "postReturnTextStaysWithAuthoritativeSuccessor",
                    postReturnTextStaysWithAuthoritativeSuccessor
                ),
                (
                    "sameOwnerSubmitClearsBeforePendingNativeInsertion",
                    sameOwnerSubmitClearsBeforePendingNativeInsertion
                ),
                ("rotatedSubmitRoutesPendingInputOnlyToSuccessor", rotatedSubmitRoutesPendingInputOnlyToSuccessor),
                ("rejectedAndReservedSubmitRetainCurrentDraft", rejectedAndReservedSubmitRetainCurrentDraft),
                ("reservedOwnerAppliesLaterAuthoritativeSuccessor", reservedOwnerAppliesLaterAuthoritativeSuccessor),
                ("submitRefreshCannotReplaceReentrantOwner", submitRefreshCannotReplaceReentrantOwner),
                ("submitRefreshRejectsNilAndDifferentAccount", submitRefreshRejectsNilAndDifferentAccount),
                (
                    "postSubmitPreservesExactInputDuringNativeAssignment",
                    postSubmitPreservesExactInputDuringNativeAssignment
                ),
                ("postSubmitPreservesExactInputDuringFocusChange", postSubmitPreservesExactInputDuringFocusChange),
                (
                    "postSubmitReplacementRevokesUnpublishedNativeInput",
                    postSubmitReplacementRevokesUnpublishedNativeInput
                ),
                (
                    "postSubmitUnknownNativeMutationKeepsMarkerFailure",
                    postSubmitUnknownNativeMutationKeepsMarkerFailure
                ),
                ("postSubmitNestedRenderWithoutInputStillUpdates", postSubmitNestedRenderWithoutInputStillUpdates),
                ("postSubmitStaleOwnerRenderCannotReclaimSuccessor", postSubmitStaleOwnerRenderCannotReclaimSuccessor),
                ("postSubmitEqualBaselineRuleEndsAtOuterUnwind", postSubmitEqualBaselineRuleEndsAtOuterUnwind),
                (
                    "ordinarySameOwnerInstallPreservesMarkedComposition",
                    ordinarySameOwnerInstallPreservesMarkedComposition
                ),
                (
                    "hardwareSendDismissalPreservesSuccessorKeyboardInput",
                    hardwareSendDismissalPreservesSuccessorKeyboardInput
                ),
                ("keyboardPresentationPreservesSendDisposition", keyboardPresentationPreservesSendDisposition),
                (
                    "explicitEditingRestoresSystemKeyboardWithExactInput",
                    explicitEditingRestoresSystemKeyboardWithExactInput
                ),
                (
                    "ordinaryOwnerAndTeardownResetKeyboardPresentation",
                    ordinaryOwnerAndTeardownResetKeyboardPresentation
                ),
                (
                    "keyboardPresentationReentryCannotReclaimReplacement",
                    keyboardPresentationReentryCannotReclaimReplacement
                ),
                (
                    "keyboardRestorationPreservesMarkedAndRevokedOwners",
                    keyboardRestorationPreservesMarkedAndRevokedOwners
                ),
                ("compactInteractionReclaimsRetainedKeyboardFocus", compactInteractionReclaimsRetainedKeyboardFocus),
                ("compactInteractionSurvivesSameOwnerRender", compactInteractionSurvivesSameOwnerRender),
                (
                    "compactInteractionStartsUnfocusedEditorWithExactInput",
                    compactInteractionStartsUnfocusedEditorWithExactInput
                ),
                (
                    "compactInteractionCannotReclaimRevokedPresentation",
                    compactInteractionCannotReclaimRevokedPresentation
                ),
                (
                    "lateNativeFocusCallbacksCannotReopenOrDismissEditor",
                    lateNativeFocusCallbacksCannotReopenOrDismissEditor
                ),
                (
                    "unrequestedNativeFocusCannotReopenDismissedComposer",
                    unrequestedNativeFocusCannotReopenDismissedComposer
                ),
                ("nativeTextInsetsKeepCaretAndPlaceholderAligned", nativeTextInsetsKeepCaretAndPlaceholderAligned),
            ]
            var lines = cases.map { name, exercise in
                var failures = 0
                failureDetails.removeAll(keepingCapacity: true)
                exercise { if !$0 { failures += 1 } }
                let details = failureDetails.isEmpty ? "" : "; " + failureDetails.joined(separator: ",")
                return "\(name): \(failures == 0 ? "passed" : "FAILED(\(failures)\(details))")"
            }
            var failures = 0
            failureDetails.removeAll(keepingCapacity: true)
            await nativeImagePasteRoutesProvidersAndPreservesTextFallback { if !$0 { failures += 1 } }
            let details = failureDetails.isEmpty ? "" : "; " + failureDetails.joined(separator: ",")
            lines.append(
                "nativeImagePasteRoutesProvidersAndPreservesTextFallback: \(failures == 0 ? "passed" : "FAILED(\(failures)\(details))")"
            )
            return lines.joined(separator: "\n")
        }

        private static func proposedEditSurvivesPreMutationSelectionAndPublishesExactlyOnce(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            let range = NSRange(location: 0, length: 1)
            check(harness.coordinator.textView(harness.view, shouldChangeTextIn: range, replacementText: ""))
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            check(harness.capture.events.last?.0.selectionUTF16 == NSRange(location: 1, length: 0))
            check(harness.capture.edits.isEmpty)
            check(harness.capture.marker?.offsetUTF16 == 2)
            check(harness.coordinator.requests.count == 1)

            harness.view.text = "aaa"
            harness.view.selectedRange = NSRange(location: 0, length: 0)
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits == [DictationTextEdit(beforeText: "aaaa", rangeUTF16: range, replacement: "")])
            check(harness.capture.marker?.offsetUTF16 == 1)
            check(harness.capture.text == "aaa")
            check(harness.coordinator.requests.isEmpty)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            harness.coordinator.publish()
            check(harness.capture.edits.count == 1)
        }

        private static func sameTextReplacementWaitsForCompletionThenCarriesExactIntent(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 0)
            defer { harness.close() }
            let range = NSRange(location: 0, length: 1)
            _ = harness.coordinator.textView(harness.view, shouldChangeTextIn: range, replacementText: "a")
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            check(harness.capture.edits.isEmpty)
            check(harness.capture.marker?.offsetUTF16 == 0)
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits == [DictationTextEdit(beforeText: "aaaa", rangeUTF16: range, replacement: "a")])
            check(harness.capture.marker?.offsetUTF16 == 1)
            harness.coordinator.publish(completedInput: true)
            check(harness.capture.edits.count == 1)
        }

        private static func rejectedProposalDoesNotCreateAnEditOrPoisonTheNextKnownEdit(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            _ = harness.coordinator.textView(
                harness.view, shouldChangeTextIn: NSRange(location: 0, length: 1), replacementText: "x")
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            harness.coordinator.publish(completedInput: true)
            check(harness.capture.edits.isEmpty)
            check(harness.capture.marker?.offsetUTF16 == 2)
            check(harness.coordinator.requests.isEmpty)

            let range = NSRange(location: 3, length: 1)
            _ = harness.coordinator.textView(harness.view, shouldChangeTextIn: range, replacementText: "z")
            harness.view.text = "aaaz"
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits == [DictationTextEdit(beforeText: "aaaa", rangeUTF16: range, replacement: "z")])
            check(harness.capture.marker?.offsetUTF16 == 2)
        }

        private static func unmatchedActualMutationStillFailsClosed(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            _ = harness.coordinator.textView(
                harness.view, shouldChangeTextIn: NSRange(location: 0, length: 1), replacementText: "x")
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            harness.view.text = "azaa"
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits.count == 1)
            check(harness.capture.edits.first?.rangeUTF16.location == NSNotFound)
            check(harness.capture.edits.first?.resultingText == nil)
            check(harness.capture.rebaseFailures == 1)
        }

        private static func mutationWithoutAProposalStillFailsClosed(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            harness.view.text = "aaa"
            harness.coordinator.publish()
            check(harness.capture.edits.count == 1)
            check(harness.capture.edits.first?.resultingText == nil)
            check(harness.capture.rebaseFailures == 1)
        }

        private static func oldOwnerFlushRetainsExactEditAndRoutesItOnlyToOldOwner(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            let range = NSRange(location: 0, length: 1)
            _ = harness.coordinator.textView(harness.view, shouldChangeTextIn: range, replacementText: "")
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            harness.view.text = "aaa"
            var nextEvents: [DictationEditorState] = []
            let next = ComposerEditor(
                text: .constant("next owner"), focused: .constant(false), ownerID: UUID(),
                placeholder: "Synthetic next owner", onChange: { state, _ in nextEvents.append(state) }, onSubmit: {})
            harness.coordinator.install(next, in: harness.view)
            check(harness.capture.text == "aaa")
            check(harness.capture.edits == [DictationTextEdit(beforeText: "aaaa", rangeUTF16: range, replacement: "")])
            check(nextEvents.isEmpty)
            check(harness.view.text == "next owner")
            check(harness.coordinator.requests.isEmpty)
        }

        private static func overflowingActualInputStillFailsClosed(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            for index in 0..<65 {
                harness.coordinator.recordReplacement(
                    before: "aaaa", range: NSRange(location: 0, length: 1), replacement: String(index))
            }
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            check(harness.capture.edits.isEmpty)
            check(harness.coordinator.requestOverflow)
            harness.view.text = "unknown"
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits.count == 1)
            check(harness.capture.edits.first?.resultingText == nil)
            check(harness.capture.rebaseFailures == 1)
        }

        private static func ownedInputCompletionPublishesAfterNestedCallbacks(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            let coordinator = harness.coordinator
            harness.view.delegate = coordinator
            harness.view.pasteDelegate = coordinator
            harness.view.willReplace = { [weak coordinator] before, range, replacement in
                coordinator?.recordNativeReplacement(before: before, range: range, replacement: replacement)
            }
            harness.view.changed = { [weak coordinator] in coordinator?.publish(completedInput: true) }
            harness.view.selectedRange = NSRange(location: 0, length: 0)
            harness.view.insertText("b")
            check(!harness.view.performingInputOperation)
            check(harness.capture.text == "baaaa")
            check(
                harness.capture.edits == [
                    DictationTextEdit(beforeText: "aaaa", rangeUTF16: NSRange(location: 0, length: 0), replacement: "b")
                ])
            check(harness.capture.marker?.offsetUTF16 == 3)
            check(harness.capture.rebaseFailures == 0)
        }

        private static func authoritativeSameOwnerUpdateRetiresPendingProposal(_ check: (Bool) -> Void) {
            let harness = Harness(text: "aaaa", marker: 2)
            defer { harness.close() }
            var inserted = false
            harness.capture.afterReceive = { state, edits in
                guard !inserted, state.text == "aaaa", state.selectionUTF16.location == 1, edits.isEmpty else { return }
                inserted = true
                // The real dictation path may insert a waiting result when a marked
                // range clears. Recreate only its same-owner authoritative update.
                harness.capture.text = "aaaa transcript"
                harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            }
            _ = harness.coordinator.textView(
                harness.view, shouldChangeTextIn: NSRange(location: 0, length: 1), replacementText: "")
            harness.view.selectedRange = NSRange(location: 1, length: 0)
            harness.coordinator.textViewDidChangeSelection(harness.view)
            check(inserted)
            check(harness.view.text == "aaaa transcript")
            check(harness.coordinator.reportedText == "aaaa transcript")
            check(harness.coordinator.requests.isEmpty)
            check(harness.capture.edits.isEmpty)

            // A later mutation from the retired proposal must not be reinterpreted
            // as a valid deletion in the authoritative replacement text.
            harness.view.text = "aaa"
            harness.coordinator.textViewDidChange(harness.view)
            check(harness.capture.edits.count == 1)
            check(harness.capture.edits.first?.beforeText == "aaaa transcript")
            check(harness.capture.edits.first?.resultingText == nil)
        }

        private static func returnCommand(
            _ view: ComposerTextView, modifiers: UIKeyModifierFlags = [], input: String = "\r"
        ) -> UIKeyCommand? {
            view.keyCommands?.first {
                $0.action == #selector(ComposerTextView.handleComposerReturn(_:)) && $0.modifierFlags == modifiers
                    && $0.input == input
            }
        }

        private static func wireNativeInput(_ harness: Harness) {
            let coordinator = harness.coordinator
            harness.view.delegate = coordinator
            harness.view.pasteDelegate = coordinator
            harness.view.willReplace = { [weak coordinator] before, range, replacement in
                coordinator?.recordNativeReplacement(before: before, range: range, replacement: replacement)
            }
            harness.view.changed = { [weak coordinator] in coordinator?.publish(completedInput: true) }
            harness.view.submit = { [weak coordinator] in coordinator?.submit() }
        }

        // These cases exercise command ownership and text semantics. Only an
        // actual hardware burst can prove UIKit's command/text delivery ordering.
        private static func resignationIncludesPriorInputBeforeSubmit(_ check: (Bool) -> Void) {
            let harness = Harness(text: "ab", marker: 2, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            harness.coordinator.installing = true
            check(!harness.view.canPerformAction(action, withSender: command))
            _ = harness.view.perform(action, with: command)
            check(harness.capture.submitted.isEmpty)
            harness.coordinator.installing = false
            var barriers = 0
            var unsentAtBarrier = false
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                barriers += 1
                unsentAtBarrier = harness.capture.submitted.isEmpty
                harness.view.insertText("c")
            }
            _ = harness.view.perform(action, with: command)
            check(unsentAtBarrier)
            check(barriers == 1 && harness.capture.submitted == ["abc"])
            check(harness.capture.text == "abc" && harness.view.isFirstResponder)
            check(harness.capture.edits.last?.replacement == "c")
            check(harness.capture.rebaseFailures == 0)
            ordinaryRenderInsideResignation(check)
        }

        private static func ordinaryRenderInsideResignation(_ check: (Bool) -> Void) {
            for variant in [
                "ordinary", "pending", "unfocused", "cancelled", "invalid-proposal", "overflow", "selection",
                "authority",
            ] {
                let harness = Harness(text: "ab", marker: 2, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                let original = harness.coordinator.parent
                var staleSnapshot = false
                var incomingCurrent = true
                var revokeIncomingOnOldAdmission = false
                var obsoleteSubmits = 0
                let oldProjection = ComposerEditor(
                    text: Binding(
                        get: { staleSnapshot ? "" : harness.capture.text },
                        set: { harness.capture.text = $0 }),
                    focused: original.$focused, ownerID: original.ownerID,
                    accountGeneration: original.accountGeneration,
                    isCurrentOwner: {
                        if revokeIncomingOnOldAdmission { incomingCurrent = false }
                        return true
                    },
                    placeholder: "Old projection",
                    onChange: original.onChange, onSubmit: { obsoleteSubmits += 1 })
                harness.coordinator.install(oldProjection, in: harness.view)
                guard let command = returnCommand(harness.view), let action = command.action else {
                    labeledCheck(false, label: "refresh.\(variant).command", check: check); continue
                }
                var renders = 0
                var proposalRetained = false
                var requestsAfterRefresh = 0
                let fresh = ComposerEditor(
                    text: Binding(get: { harness.capture.text }, set: { harness.capture.text = $0 }),
                    focused: variant == "unfocused" ? .constant(false) : original.$focused,
                    ownerID: original.ownerID, accountGeneration: original.accountGeneration,
                    isCurrentOwner: { incomingCurrent },
                    placeholder: "Current projection", accessibilityIdentifier: "composer.current-projection",
                    externalSelection: variant == "selection"
                        ? DictationEditorState(
                            text: "abc", selectionUTF16: NSRange(location: 0, length: 0), markedRangeUTF16: nil) : nil,
                    onChange: original.onChange,
                    onSubmit: {
                        harness.capture.submitted.append(harness.capture.text)
                        harness.capture.text = ""; harness.capture.focused = false
                        harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
                    })
                harness.view.beforeResignFirstResponderForProbe = {
                    harness.view.beforeResignFirstResponderForProbe = nil
                    // Real native replacement/publication, then the reentrant
                    // SwiftUI projection observed before outer resignation ends.
                    harness.view.insertText("c")
                    staleSnapshot = true
                    if variant == "pending" {
                        _ = harness.coordinator.textView(
                            harness.view, shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d")
                    } else if variant == "invalid-proposal" {
                        harness.coordinator.recordReplacement(
                            before: "abc", range: NSRange(location: 30, length: 0), replacement: "d")
                    } else if variant == "overflow" {
                        for index in 0..<65 {
                            harness.coordinator.recordReplacement(
                                before: "abc", range: NSRange(location: 3, length: 0), replacement: String(index))
                        }
                    } else if variant == "cancelled" {
                        harness.view.cancelHardwareReturn()
                    }
                    revokeIncomingOnOldAdmission = variant == "authority"
                    let priorRequests = harness.coordinator.requests
                    renders += 1
                    harness.coordinator.install(fresh, in: harness.view)
                    requestsAfterRefresh = harness.coordinator.requests.count
                    proposalRetained = harness.coordinator.requests == priorRequests
                }
                _ = harness.view.perform(action, with: command)
                labeledCheck(renders == 1, label: "refresh.\(variant).oneRender", check: check)
                labeledCheck(obsoleteSubmits == 0, label: "refresh.\(variant).currentCallback", check: check)
                if variant == "ordinary" || variant == "pending" {
                    labeledCheck(
                        harness.capture.submitted == ["abc"], label: "refresh.\(variant).exactSubmit", check: check)
                    labeledCheck(
                        harness.capture.text.isEmpty && harness.view.text.isEmpty,
                        label: "refresh.\(variant).emptySuccessor", check: check)
                    labeledCheck(
                        harness.view.accessibilityIdentifier == "composer.current-projection"
                            && harness.view.placeholderLabel.text == "Current projection",
                        label: "refresh.\(variant).metadata", check: check)
                    labeledCheck(proposalRetained, label: "refresh.\(variant).proposalUntouched", check: check)
                    labeledCheck(
                        requestsAfterRefresh == (variant == "pending" ? 1 : 0),
                        label: "refresh.\(variant).requestCount", check: check)
                    if variant == "pending" {
                        // Actual reception follows the installed empty successor;
                        // never infer that an earlier delegate proposal was applied.
                        harness.view.insertText("d")
                        labeledCheck(
                            harness.capture.text == "d" && harness.view.text == "d",
                            label: "refresh.pending.successorInput", check: check)
                        labeledCheck(
                            harness.capture.edits.last
                                == DictationTextEdit(
                                    beforeText: "", rangeUTF16: NSRange(location: 0, length: 0), replacement: "d"),
                            label: "refresh.pending.exactSuccessorEdit", check: check)
                    }
                    labeledCheck(
                        harness.capture.rebaseFailures == 0, label: "refresh.\(variant).exactEdits", check: check)
                } else {
                    labeledCheck(
                        harness.capture.submitted.isEmpty, label: "refresh.\(variant).revoked", check: check)
                    labeledCheck(
                        harness.capture.text == "abc" && harness.view.text == "abc",
                        label: "refresh.\(variant).textRetained", check: check)
                }
            }
        }

        private static func resignationOrdersPriorInputBeforeShiftNewline(_ check: (Bool) -> Void) {
            let harness = Harness(text: "ab", marker: 2, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view, modifiers: .shift), let action = command.action else {
                check(false); return
            }
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                harness.view.insertText("c")
            }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "abc\n")
            check(harness.capture.submitted.isEmpty && harness.view.isFirstResponder)
            check(harness.capture.edits.map(\.replacement) == ["c", "\n"])
            check(harness.capture.rebaseFailures == 0)
        }

        private static func ordinaryTextInputPreservesNewlinesWithoutSubmitting(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            _ = returnCommand(harness.view)
            harness.view.insertText("\n")
            harness.view.insertText("left\nright")
            check(harness.capture.text == "abc\nleft\nright")
            check(harness.capture.submitted.isEmpty)
            check(harness.capture.edits.map(\.replacement) == ["\n", "left\nright"])
            check(harness.capture.rebaseFailures == 0)
        }

        private static func markdownNewlineRetainsExactDictationAnchorAndComposition(_ check: (Bool) -> Void) {
            for delegatePath in [false, true] {
                let text = "- 🐝", caret = text.utf16.count
                let harness = Harness(text: text, marker: caret, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                harness.view.selectedRange = NSRange(location: caret, length: 0)
                if delegatePath {
                    check(
                        !harness.coordinator.textView(
                            harness.view, shouldChangeTextIn: harness.view.selectedRange, replacementText: "\n"))
                } else {
                    harness.view.insertText("\n")
                }
                check(harness.capture.text == text + "\n- ")
                check(harness.view.selectedRange == NSRange(location: caret + 3, length: 0))
                check(
                    harness.capture.edits == [
                        DictationTextEdit(
                            beforeText: text, rangeUTF16: NSRange(location: caret, length: 0), replacement: "\n- ")
                    ])
                check(harness.capture.marker?.offsetUTF16 == caret + 3 && harness.capture.rebaseFailures == 0)
                check(harness.capture.submitted.isEmpty)
            }
            let composing = Harness(text: "- item", marker: 6, focus: true)
            defer { composing.close() }
            wireNativeInput(composing)
            composing.view.selectedRange = NSRange(location: 6, length: 0)
            composing.view.setMarkedText("pending", selectedRange: NSRange(location: 7, length: 0))
            let before = composing.view.text
            check(composing.view.markedTextRange != nil)
            check(
                composing.coordinator.textView(
                    composing.view, shouldChangeTextIn: composing.view.selectedRange, replacementText: "\n"))
            check(composing.view.text == before && composing.view.markedTextRange != nil)
            check(composing.capture.submitted.isEmpty)
        }

        private static func hardwareReturnAndKeypadCommandsUseSameGuardedPath(_ check: (Bool) -> Void) {
            // Controlled command matching only; actual keypad HID delivery is a separate native check.
            for input in ["\r", "\u{3}"] {
                for modifiers in [UIKeyModifierFlags(), .shift] {
                    let harness = Harness(text: "abc", marker: 3, focus: true)
                    defer { harness.close() }
                    wireNativeInput(harness)
                    let commands =
                        harness.view.keyCommands?.filter {
                            $0.action == #selector(ComposerTextView.handleComposerReturn(_:))
                        } ?? []
                    check(commands.count == 4)
                    check(Set(commands.compactMap(\.input)) == Set(["\r", "\u{3}"]))
                    guard let command = returnCommand(harness.view, modifiers: modifiers, input: input),
                        let action = command.action
                    else { check(false); continue }
                    check(harness.view.canPerformAction(action, withSender: command))
                    let unsupported = UIKeyCommand(
                        title: "", action: action, input: "\n", modifierFlags: modifiers,
                        propertyList: command.propertyList)
                    check(!harness.view.canPerformAction(action, withSender: unsupported))
                    _ = harness.view.perform(action, with: unsupported)
                    check(harness.capture.text == "abc" && harness.capture.submitted.isEmpty)
                    _ = harness.view.perform(action, with: command)

                    if modifiers == .shift {
                        check(harness.capture.text == "abc\n" && harness.capture.submitted.isEmpty)
                        check(
                            harness.capture.edits == [
                                DictationTextEdit(
                                    beforeText: "abc", rangeUTF16: NSRange(location: 3, length: 0), replacement: "\n")
                            ])
                    } else {
                        check(harness.capture.text == "abc" && harness.capture.submitted == ["abc"])
                        check(harness.capture.edits.isEmpty)
                    }
                    check(harness.capture.rebaseFailures == 0)
                }
            }
        }

        private static func markedInputDoesNotExposeOrExecuteReturnCommands(_ check: (Bool) -> Void) {
            let harness = Harness(text: "", marker: 0, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let saved = returnCommand(harness.view), let savedAction = saved.action else { check(false); return }
            harness.view.setMarkedText("にほん", selectedRange: NSRange(location: 3, length: 0))
            check(harness.view.markedTextRange != nil)
            check(returnCommand(harness.view) == nil)
            check(!harness.view.canPerformAction(savedAction, withSender: saved))
            _ = harness.view.perform(savedAction, with: saved)

            check(harness.capture.submitted.isEmpty)
            check(harness.view.markedTextRange != nil)
            check(harness.view.text == "にほん")
            harness.view.unmarkText()
            check(returnCommand(harness.view) != nil)
            check(harness.capture.submitted.isEmpty)
        }

        private static func retiredOwnerCannotExecuteCachedReturnCommand(_ check: (Bool) -> Void) {
            let harness = Harness(text: "old draft", marker: 0, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let old = returnCommand(harness.view), let oldAction = old.action else { check(false); return }
            var nextSends = 0
            let next = ComposerEditor(
                text: .constant("replacement"), focused: .constant(true), ownerID: UUID(),
                accountGeneration: UUID(), placeholder: "Synthetic replacement",
                onSubmit: { nextSends += 1 })
            harness.coordinator.install(next, in: harness.view)

            check(!harness.view.canPerformAction(oldAction, withSender: old))
            _ = harness.view.perform(oldAction, with: old)
            check(nextSends == 0 && harness.capture.submitted.isEmpty)
            guard let current = returnCommand(harness.view), let currentAction = current.action else {
                check(false); return
            }
            check((current.propertyList as? String) != (old.propertyList as? String))
            check(harness.view.canPerformAction(currentAction, withSender: current))
            _ = harness.view.perform(currentAction, with: current)

            check(nextSends == 1)
            ComposerEditor.dismantleUIView(harness.view, coordinator: harness.coordinator)

            check(!harness.view.canPerformAction(currentAction, withSender: current))
            _ = harness.view.perform(currentAction, with: current)
            check(nextSends == 1)
        }

        private static func reentrantHardwareCommandCannotSubmitTwice(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            var reentryRejected = false
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                reentryRejected = !harness.view.canPerformAction(action, withSender: command)
                _ = harness.view.perform(action, with: command)
            }
            _ = harness.view.perform(action, with: command)
            check(reentryRejected)
            check(harness.capture.submitted == ["abc"] && harness.view.isFirstResponder)
        }

        private static func distinctShiftActionsEachInsertOneNewline(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view, modifiers: .shift), let action = command.action else {
                check(false); return
            }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "abc\n")
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "abc\n\n" && harness.capture.submitted.isEmpty)
            check(harness.capture.edits.map(\.replacement) == ["\n", "\n"])
            check(harness.capture.rebaseFailures == 0)
        }

        private static func replacementDuringResignationCancelsSend(_ check: (Bool) -> Void) {
            for sameOwner in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                guard let command = returnCommand(harness.view), let action = command.action else {
                    check(false); continue
                }
                var replacementSends = 0
                let replacement = ComposerEditor(
                    text: .constant("authoritative replacement"), focused: .constant(false),
                    ownerID: sameOwner ? harness.coordinator.parent.ownerID : UUID(),
                    accountGeneration: harness.coordinator.parent.accountGeneration,
                    placeholder: "Replacement", onSubmit: { replacementSends += 1 })
                harness.view.beforeResignFirstResponderForProbe = {
                    harness.view.beforeResignFirstResponderForProbe = nil
                    harness.coordinator.install(replacement, in: harness.view)
                }
                _ = harness.view.perform(action, with: command)
                check(harness.view.text == "authoritative replacement")
                check(harness.capture.submitted.isEmpty && replacementSends == 0)
                check(!harness.view.isFirstResponder)
            }
        }

        private static func nestedFocusLossDuringResignationCancelsSend(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            var resigned = false
            var resumed = false
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                resigned = harness.view.resignFirstResponder()
                resumed = harness.view.becomeFirstResponder()
            }
            _ = harness.view.perform(action, with: command)
            check(resigned)
            check(resumed)
            check(harness.capture.submitted.isEmpty)
            _ = harness.view.perform(action, with: command)
            check(harness.capture.submitted == ["abc"])
        }

        private static func pasteDuringResignationCancelsSend(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            let previous = UIPasteboard.general.items
            defer { UIPasteboard.general.setItems(previous, options: [.localOnly: true]) }
            UIPasteboard.general.items = []
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                harness.view.paste(nil)
            }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "abc" && harness.capture.submitted.isEmpty)
        }

        private static func nativeImagePasteRoutesProvidersAndPreservesTextFallback(_ check: (Bool) -> Void) async {
            let board = UIPasteboard.general, previous = board.items
            defer { board.setItems(previous, options: [.localOnly: true]) }
            let text = "- 🐝", caret = text.utf16.count
            let harness = Harness(text: text, marker: caret, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.view.selectedRange = NSRange(location: caret, length: 0)
            var callbacks = 0, expectedProvider = false
            harness.coordinator.parent.onPasteImages = { providers in
                callbacks += 1
                expectedProvider =
                    providers.count == 1 && providers[0].hasItemConformingToTypeIdentifier(UTType.png.identifier)
            }
            let image = UIGraphicsImageRenderer(size: CGSize(width: 1, height: 1)).pngData { context in
                context.cgContext.setFillColor(UIColor.black.cgColor)
                context.cgContext.fill(CGRect(x: 0, y: 0, width: 1, height: 1))
            }
            board.setItems([[UTType.png.identifier: image]], options: [.localOnly: true])
            labeledCheck(
                harness.view.canPerformAction(#selector(ComposerTextView.paste(_:)), withSender: nil),
                label: "paste.image.action", check: check)
            harness.view.paste(nil)
            labeledCheck(
                callbacks == 1 && expectedProvider && harness.capture.text == text && harness.capture.edits.isEmpty,
                label: "paste.image.route[callbacks=\(callbacks);expectedType=\(expectedProvider)]", check: check)
            board.setItems([[UTType.utf8PlainText.identifier: Data("\n".utf8)]], options: [.localOnly: true])
            harness.view.paste(nil)
            // UIKit may finish text-provider decoding after paste(_:) returns.
            // Keep this exact view and pasteboard alive until the Coordinator
            // publishes an actual edit; the bound is only a test failure deadline.
            let clock = ContinuousClock(), deadline = clock.now.advanced(by: .seconds(2))
            while harness.capture.edits.isEmpty, clock.now < deadline, !Task.isCancelled {
                do { try await Task.sleep(for: .milliseconds(10)) } catch { break }
            }
            labeledCheck(
                callbacks == 1 && harness.capture.text == text + "\n",
                label:
                    "paste.text.literal[length=\(harness.capture.text.utf16.count);continued=\(harness.capture.text.hasSuffix("\n- "))]",
                check: check)
            labeledCheck(
                harness.capture.edits == [
                    DictationTextEdit(
                        beforeText: text, rangeUTF16: NSRange(location: caret, length: 0), replacement: "\n")
                ], label: "paste.text.exact[edits=\(harness.capture.edits.count)]", check: check)
            labeledCheck(
                harness.capture.marker?.offsetUTF16 == caret + 1 && harness.capture.rebaseFailures == 0,
                label:
                    "paste.text.anchor[offset=\(harness.capture.marker?.offsetUTF16 ?? -1);failures=\(harness.capture.rebaseFailures)]",
                check: check)
            labeledCheck(
                harness.view.selectedRange == NSRange(location: caret + 1, length: 0),
                label: "paste.text.caret", check: check)
            labeledCheck(harness.capture.submitted.isEmpty, label: "paste.noSubmit", check: check)

            for boundary in ["replacement", "reinstalled", "retired", "composition"] {
                let candidate = Harness(text: "old", marker: 3, focus: true)
                defer { candidate.close() }
                wireNativeInput(candidate)
                candidate.view.selectedRange = NSRange(location: 3, length: 0)
                board.setItems([[UTType.utf8PlainText.identifier: Data("late".utf8)]], options: [.localOnly: true])
                candidate.view.paste(nil)
                labeledCheck(
                    candidate.coordinator.pendingTextPasteReads > 0,
                    label: "paste.\(boundary).initiatingOwnerCaptured", check: check)
                let original = candidate.coordinator.parent, next = Capture(text: "Successor", marker: 9)
                if boundary == "replacement" || boundary == "reinstalled" {
                    let successor = ComposerEditor(
                        text: Binding(get: { next.text }, set: { next.text = $0 }),
                        focused: .constant(true), ownerID: UUID(), accountGeneration: UUID(),
                        placeholder: "Synthetic successor", onChange: { next.receive($0, $1) }, onSubmit: {})
                    candidate.coordinator.install(successor, in: candidate.view)
                    if boundary == "reinstalled" { candidate.coordinator.install(original, in: candidate.view) }
                } else if boundary == "retired" {
                    candidate.coordinator.valid = false
                } else {
                    candidate.view.setMarkedText("IME", selectedRange: NSRange(location: 3, length: 0))
                }
                let expectedText = candidate.view.text
                let ownerDeadline = clock.now.advanced(by: .seconds(2))
                while candidate.coordinator.pendingTextPasteReads > 0, clock.now < ownerDeadline,
                    !Task.isCancelled
                {
                    do { try await Task.sleep(for: .milliseconds(10)) } catch { break }
                }
                labeledCheck(
                    candidate.coordinator.pendingTextPasteReads == 0 && candidate.view.text == expectedText,
                    label: "paste.\(boundary).lateReadRejected", check: check)
                labeledCheck(
                    next.text == "Successor" && next.edits.isEmpty, label: "paste.\(boundary).successorUnchanged",
                    check: check)
                if boundary == "composition" {
                    labeledCheck(
                        candidate.view.markedTextRange != nil, label: "paste.composition.retained", check: check)
                }
                guard let start = candidate.view.position(from: candidate.view.beginningOfDocument, offset: 0),
                    let end = candidate.view.position(from: start, offset: 1),
                    let selected = candidate.view.textRange(from: start, to: end)
                else { check(false); continue }
                let previousSelection = candidate.view.selectedRange
                let returnedSelection = candidate.coordinator.textPasteConfigurationSupporting(
                    candidate.view, performPasteOf: NSAttributedString(string: ""), to: selected)
                labeledCheck(
                    candidate.view.text == expectedText, label: "paste.\(boundary).emptyResultIgnored", check: check)
                labeledCheck(
                    candidate.view.selectedRange == previousSelection
                        && candidate.view.offset(from: candidate.view.beginningOfDocument, to: returnedSelection.start)
                            == previousSelection.location
                        && candidate.view.offset(from: returnedSelection.start, to: returnedSelection.end)
                            == previousSelection.length,
                    label: "paste.\(boundary).selectionRetained", check: check)
            }
        }

        private static func publicationRevocationCancelsHardwareSend(_ check: (Bool) -> Void) {
            for revocation in [
                "focus", "composition", "replacement", "replacement-input", "replacement-unfocused",
                "replacement-account", "replacement-cancel",
            ] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                guard let command = returnCommand(harness.view), let action = command.action else {
                    labeledCheck(false, label: "\(revocation).command", check: check); continue
                }
                // A known native mutation waits for the submit-time publish. Its
                // callback revokes the action without changing the draft owner.
                labeledCheck(
                    harness.coordinator.textView(
                        harness.view, shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d"),
                    label: "\(revocation).proposal", check: check)
                harness.view.delegate = nil
                harness.view.text = "abcd"
                harness.view.selectedRange = NSRange(location: 4, length: 0)
                harness.view.delegate = harness.coordinator
                var focusInputCount = 0
                var canceledDuringAdmission = false
                var baselineBeforeFocus = false
                let priorGeneration = harness.coordinator.parent.accountGeneration
                harness.capture.afterReceive = { _, _ in
                    harness.capture.afterReceive = nil
                    if revocation == "composition" {
                        harness.view.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0))
                    } else if revocation.hasPrefix("replacement") {
                        harness.capture.text = "authoritative replacement"
                        var replacement = harness.coordinator.parent
                        if revocation == "replacement-unfocused" { harness.capture.focused = false }
                        if revocation == "replacement-account" { replacement.accountGeneration = UUID() }
                        if revocation == "replacement-input" {
                            harness.capture.marker = DictationInsertionAnchor(
                                text: harness.capture.text, offsetUTF16: harness.capture.text.utf16.count)
                            harness.view.beforeBecomeFirstResponderForProbe = {
                                harness.view.beforeBecomeFirstResponderForProbe = nil
                                focusInputCount += 1
                                baselineBeforeFocus =
                                    harness.coordinator.reportedText == "authoritative replacement"
                                    && harness.view.text == "authoritative replacement"
                                    && harness.coordinator.owner?.draft == harness.coordinator.parent.ownerID
                                harness.view.selectedRange = NSRange(location: harness.view.text.utf16.count, length: 0)
                                harness.view.insertText("e")
                            }
                        }
                        harness.coordinator.install(replacement, in: harness.view)
                        if revocation == "replacement-cancel" {
                            harness.coordinator.parent.isCurrentOwner = {
                                if !canceledDuringAdmission {
                                    canceledDuringAdmission = true
                                    // A new cancellation must win even if native
                                    // focus is already absent and binding is true.
                                    harness.view.resignFirstResponder()
                                }
                                return true
                            }
                        }
                    } else {
                        harness.view.resignFirstResponder()
                    }
                }

                _ = harness.view.perform(action, with: command)
                labeledCheck(harness.capture.submitted.isEmpty, label: "\(revocation).noSubmit", check: check)
                labeledCheck(harness.capture.rebaseFailures == 0, label: "\(revocation).exactEdits", check: check)
                if revocation.hasPrefix("replacement") {
                    let expected = "authoritative replacement" + (revocation == "replacement-input" ? "e" : "")
                    labeledCheck(
                        harness.capture.text == expected && harness.view.text == expected,
                        label: "\(revocation).replacementText", check: check)
                    if revocation == "replacement-unfocused" || revocation == "replacement-account"
                        || revocation == "replacement-cancel"
                    {
                        labeledCheck(!harness.view.isFirstResponder, label: "\(revocation).unfocused", check: check)
                    } else {
                        labeledCheck(harness.view.isFirstResponder, label: "\(revocation).focused", check: check)
                    }
                    labeledCheck(harness.view.markedTextRange == nil, label: "\(revocation).unmarked", check: check)
                    if revocation == "replacement-cancel" {
                        labeledCheck(canceledDuringAdmission, label: "\(revocation).cancellationObserved", check: check)
                    }
                    if revocation == "replacement-account" {
                        labeledCheck(
                            harness.coordinator.owner?.generation != priorGeneration,
                            label: "\(revocation).newGeneration", check: check)
                    }
                    if revocation == "replacement-input" {
                        labeledCheck(focusInputCount == 1, label: "\(revocation).focusInputOnce", check: check)
                        labeledCheck(baselineBeforeFocus, label: "\(revocation).baselineBeforeFocus", check: check)
                        labeledCheck(
                            harness.capture.edits.last
                                == DictationTextEdit(
                                    beforeText: "authoritative replacement",
                                    rangeUTF16: NSRange(location: "authoritative replacement".utf16.count, length: 0),
                                    replacement: "e"),
                            label: "\(revocation).exactInput", check: check)
                    }
                } else {
                    labeledCheck(harness.capture.text.hasPrefix("abcd"), label: "\(revocation).prefix", check: check)
                    if revocation == "composition" {
                        labeledCheck(harness.view.markedTextRange != nil, label: "\(revocation).marked", check: check)
                    } else {
                        labeledCheck(!harness.view.isFirstResponder, label: "\(revocation).unfocused", check: check)
                    }
                }
            }
        }

        private static func postReturnTextStaysWithAuthoritativeSuccessor(_ check: (Bool) -> Void) {
            let harness = Harness(text: "ab", marker: 2, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            var nextText = ""
            var nextSends = 0
            let next = ComposerEditor(
                text: Binding(get: { nextText }, set: { nextText = $0 }), focused: .constant(true), ownerID: UUID(),
                accountGeneration: harness.coordinator.parent.accountGeneration, placeholder: "Synthetic next draft",
                onSubmit: { nextSends += 1 })
            harness.capture.afterSubmit = { harness.coordinator.install(next, in: harness.view) }
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                harness.view.insertText("c")
            }
            _ = harness.view.perform(action, with: command)

            check(harness.capture.submitted == ["abc"] && harness.view.text.isEmpty)
            harness.view.insertText("d")

            check(nextText == "d" && nextSends == 0)
            check(harness.capture.submitted == ["abc"])
        }

        private static func sameOwnerSubmitClearsBeforePendingNativeInsertion(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            harness.capture.afterSubmit = {
                harness.capture.text = ""; harness.capture.focused = false
                harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
            }
            // Actual fifteenth ordering: trailing d is proposed before Send's
            // callback, but its insertText is delivered after the callback.
            check(
                harness.coordinator.textView(
                    harness.view, shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d"))
            _ = harness.view.perform(action, with: command)

            check(harness.capture.submitted == ["abc"])
            check(harness.view.text.isEmpty && harness.coordinator.reportedText.isEmpty)
            check(harness.coordinator.requests.isEmpty && harness.view.isFirstResponder)
            harness.view.insertText("d")
            check(harness.capture.text == "d" && harness.view.text == "d")
            check(
                harness.capture.edits == [
                    DictationTextEdit(beforeText: "", rangeUTF16: NSRange(location: 0, length: 0), replacement: "d")
                ])
            check(harness.capture.rebaseFailures == 0)
        }

        private static func rotatedSubmitRoutesPendingInputOnlyToSuccessor(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            let successor = Capture(text: "", marker: 0)
            let nextOwner = UUID()
            let next = ComposerEditor(
                text: Binding(get: { successor.text }, set: { successor.text = $0 }),
                focused: Binding(get: { harness.capture.focused }, set: { harness.capture.focused = $0 }),
                ownerID: nextOwner, accountGeneration: harness.coordinator.parent.accountGeneration,
                placeholder: "Synthetic next draft", onChange: { successor.receive($0, $1) },
                onSubmit: { successor.submitted.append(successor.text) })
            harness.coordinator.parent.refreshAfterSubmit = { next }
            harness.capture.afterSubmit = {
                // Shipping's captured old UUID no longer resolves once Core
                // detaches it. Do not make its getter dynamically adopt a new UUID.
                harness.capture.retired = true; harness.capture.focused = false
            }
            check(
                harness.coordinator.textView(
                    harness.view, shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d"))
            _ = harness.view.perform(action, with: command)

            check(harness.capture.submitted == ["abc"] && harness.capture.text == "abc")
            check(harness.coordinator.owner?.draft == nextOwner && harness.view.text.isEmpty)
            check(harness.view.isFirstResponder && harness.coordinator.requests.isEmpty)
            harness.view.insertText("d")
            check(successor.text == "d" && successor.submitted.isEmpty)
            check(harness.capture.text == "abc" && harness.capture.edits.isEmpty)
            check(
                successor.edits == [
                    DictationTextEdit(beforeText: "", rangeUTF16: NSRange(location: 0, length: 0), replacement: "d")
                ])
            check(successor.rebaseFailures == 0 && harness.capture.rebaseFailures == 0)
        }

        private static func rejectedAndReservedSubmitRetainCurrentDraft(_ check: (Bool) -> Void) {
            for reserved in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                let owner = harness.coordinator.owner
                guard let command = returnCommand(harness.view), let action = command.action else {
                    check(false); continue
                }
                harness.capture.afterSubmit = { if reserved { harness.capture.focused = false } }
                harness.coordinator.parent.refreshAfterSubmit = { harness.coordinator.parent }
                check(
                    harness.coordinator.textView(
                        harness.view, shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d"))
                _ = harness.view.perform(action, with: command)

                check(harness.coordinator.owner == owner && harness.capture.text == "abc" && harness.view.text == "abc")
                check(harness.view.isFirstResponder)
                harness.view.insertText("d")
                check(harness.capture.text == "abcd" && harness.capture.submitted == ["abc"])
                check(
                    harness.capture.edits == [
                        DictationTextEdit(
                            beforeText: "abc", rangeUTF16: NSRange(location: 3, length: 0), replacement: "d")
                    ])
                check(harness.capture.rebaseFailures == 0)
            }
        }

        private static func reservedOwnerAppliesLaterAuthoritativeSuccessor(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            let oldOwner = harness.coordinator.owner
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            harness.capture.afterSubmit = { harness.capture.focused = false }
            _ = harness.view.perform(action, with: command)

            check(harness.coordinator.owner == oldOwner && harness.view.text == "abc")
            let successor = Capture(text: "", marker: 0)
            let next = ComposerEditor(
                text: Binding(get: { successor.text }, set: { successor.text = $0 }), focused: .constant(false),
                ownerID: UUID(), accountGeneration: harness.coordinator.parent.accountGeneration,
                placeholder: "Synthetic asynchronously validated draft", onChange: { successor.receive($0, $1) },
                onSubmit: {})
            harness.capture.retired = true
            // This is the ordinary SwiftUI install once exact-body validation
            // really detaches the draft; reserved admission itself never clears it.
            harness.coordinator.install(next, in: harness.view)
            check(harness.view.text.isEmpty && harness.capture.text == "abc")
            harness.view.insertText("d")
            check(successor.text == "d" && successor.rebaseFailures == 0)
            check(harness.capture.submitted == ["abc"])
        }

        private static func submitRefreshCannotReplaceReentrantOwner(_ check: (Bool) -> Void) {
            for sameOwner in [false, true] {
                for duringRefresh in [false, true] {
                    let harness = Harness(text: "abc", marker: 3, focus: true)
                    defer { harness.close() }
                    wireNativeInput(harness)
                    let replacement = ComposerEditor(
                        text: .constant("replacement"), focused: .constant(false),
                        ownerID: sameOwner ? harness.coordinator.parent.ownerID : UUID(),
                        placeholder: "Replacement", onSubmit: {})
                    let stale = harness.coordinator.parent
                    var refreshCalls = 0
                    harness.coordinator.parent.refreshAfterSubmit = {
                        refreshCalls += 1
                        if duringRefresh { harness.coordinator.install(replacement, in: harness.view) }
                        return stale
                    }
                    harness.capture.afterSubmit = {
                        if !duringRefresh { harness.coordinator.install(replacement, in: harness.view) }
                    }
                    guard let command = returnCommand(harness.view), let action = command.action else {
                        check(false); continue
                    }
                    _ = harness.view.perform(action, with: command)
                    check(refreshCalls == (duringRefresh ? 1 : 0))
                    check(harness.coordinator.owner?.draft == replacement.ownerID && harness.view.text == "replacement")
                    check(harness.capture.submitted == ["abc"])
                }
            }
        }

        private static func submitRefreshRejectsNilAndDifferentAccount(_ check: (Bool) -> Void) {
            for differentAccount in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                let owner = harness.coordinator.owner
                let foreign = ComposerEditor(
                    text: .constant("foreign"), focused: .constant(false), ownerID: UUID(), accountGeneration: UUID(),
                    placeholder: "Foreign account", onSubmit: {})
                harness.coordinator.parent.refreshAfterSubmit = { differentAccount ? foreign : nil }
                guard let command = returnCommand(harness.view), let action = command.action else {
                    check(false); continue
                }
                _ = harness.view.perform(action, with: command)
                _ = harness.view.perform(action, with: command)

                check(harness.coordinator.owner == owner && harness.view.text == "abc")
                check(harness.capture.submitted == ["abc"])
            }
        }

        private static func postSubmitPreservesExactInputDuringNativeAssignment(_ check: (Bool) -> Void) {
            checkPostSubmitReentrantInput(duringFocus: false, check)
        }

        private static func postSubmitPreservesExactInputDuringFocusChange(_ check: (Bool) -> Void) {
            checkPostSubmitReentrantInput(duringFocus: true, check)
        }

        private static func checkPostSubmitReentrantInput(duringFocus: Bool, _ check: (Bool) -> Void) {
            for rotateOwner in [false, true] {
                for nestedBaselineRender in [false, true] {
                    let harness = Harness(text: "abc", marker: 3, focus: true)
                    defer { harness.close() }
                    wireNativeInput(harness)
                    let successor = rotateOwner ? Capture(text: "", marker: 0) : harness.capture
                    let next = ComposerEditor(
                        text: Binding(get: { successor.text }, set: { successor.text = $0 }),
                        focused: .constant(false), ownerID: rotateOwner ? UUID() : harness.coordinator.parent.ownerID,
                        accountGeneration: harness.coordinator.parent.accountGeneration,
                        placeholder: "Synthetic successor", onChange: { successor.receive($0, $1) }, onSubmit: {})
                    harness.coordinator.parent.refreshAfterSubmit = { next }
                    harness.capture.afterSubmit = {
                        harness.capture.focused = false
                        if rotateOwner {
                            harness.capture.retired = true
                        } else {
                            harness.capture.text = "";
                            harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
                        }
                    }
                    var observations: [Bool] = []
                    let insertion = {
                        harness.coordinator.afterNativeTextAssignmentForProbe = nil
                        harness.view.beforeBecomeFirstResponderForProbe = nil
                        observations.append(harness.coordinator.installing && harness.view.text.isEmpty)
                        harness.view.insertText("d")
                        // The complete owned operation arrived inside the setter/focus
                        // transaction; no model callback runs before the outer unwind.
                        observations.append(successor.text.isEmpty && harness.view.text == "d")
                        if nestedBaselineRender {
                            let stale = ComposerEditor(
                                text: .constant(""), focused: .constant(false),
                                ownerID: next.ownerID, accountGeneration: next.accountGeneration,
                                placeholder: "Older committed render", onSubmit: {})
                            harness.coordinator.install(stale, in: harness.view)
                            observations.append(harness.view.text == "d")
                        }
                    }
                    if duringFocus {
                        harness.view.beforeBecomeFirstResponderForProbe = insertion
                    } else {
                        harness.coordinator.afterNativeTextAssignmentForProbe = insertion
                    }
                    guard let command = returnCommand(harness.view), let action = command.action else {
                        check(false); continue
                    }
                    // The old draft's d proposal precedes Submit; its actual native
                    // insertion is delivered only inside authoritative installation.
                    check(
                        harness.coordinator.textView(
                            harness.view,
                            shouldChangeTextIn: NSRange(location: 3, length: 0), replacementText: "d"))
                    _ = harness.view.perform(action, with: command)

                    check(observations.count == (nestedBaselineRender ? 3 : 2))
                    for observation in observations { check(observation) }
                    check(harness.capture.submitted == ["abc"])
                    check(successor.text == "d" && harness.view.text == "d" && harness.coordinator.reportedText == "d")
                    check(harness.view.selectedRange == NSRange(location: 1, length: 0))
                    check(
                        successor.edits == [
                            DictationTextEdit(
                                beforeText: "", rangeUTF16: NSRange(location: 0, length: 0), replacement: "d")
                        ])
                    check(successor.rebaseFailures == 0 && !harness.coordinator.installing)
                    check(harness.view.isFirstResponder)
                    if rotateOwner { check(harness.capture.text == "abc" && harness.capture.edits.isEmpty) }
                }
            }
        }

        private static func postSubmitReplacementRevokesUnpublishedNativeInput(_ check: (Bool) -> Void) {
            for replacementKind in ["model", "owner", "account"] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                harness.capture.afterSubmit = {
                    harness.capture.text = ""; harness.capture.focused = false
                    harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
                }
                let replacementText = replacementKind == "model" ? "authoritative" : ""
                let replacementCapture = Capture(text: replacementText, marker: replacementText.utf16.count)
                let replacement = ComposerEditor(
                    text: Binding(get: { replacementCapture.text }, set: { replacementCapture.text = $0 }),
                    focused: .constant(false),
                    ownerID: replacementKind == "model" ? harness.coordinator.parent.ownerID : UUID(),
                    accountGeneration: replacementKind == "account"
                        ? UUID() : harness.coordinator.parent.accountGeneration,
                    placeholder: "Replacement", onChange: { replacementCapture.receive($0, $1) }, onSubmit: {})
                harness.coordinator.afterNativeTextAssignmentForProbe = {
                    harness.coordinator.afterNativeTextAssignmentForProbe = nil
                    harness.view.insertText("d")
                    let oldBaselineRender = harness.coordinator.parent
                    harness.coordinator.install(replacement, in: harness.view)
                    harness.coordinator.install(oldBaselineRender, in: harness.view)
                    harness.view.insertText("e")
                }
                guard let command = returnCommand(harness.view), let action = command.action else {
                    check(false); continue
                }
                _ = harness.view.perform(action, with: command)
                check(harness.capture.submitted == ["abc"] && harness.capture.text.isEmpty)
                check(harness.capture.edits.isEmpty && replacementCapture.edits.isEmpty)
                check(replacementCapture.text == replacementText && harness.view.text == replacementText)
                check(harness.coordinator.owner?.draft == replacement.ownerID)
            }
        }

        private static func postSubmitUnknownNativeMutationKeepsMarkerFailure(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.capture.afterSubmit = {
                harness.capture.text = ""; harness.capture.focused = false
                harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
            }
            harness.coordinator.afterNativeTextAssignmentForProbe = {
                harness.coordinator.afterNativeTextAssignmentForProbe = nil
                harness.view.text = "unknown"
                harness.view.selectedRange = NSRange(location: 7, length: 0)
                harness.view.insertText("d")
            }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "unknownd" && harness.capture.submitted == ["abc"])
            check(harness.capture.rebaseFailures == 1 && harness.capture.edits.count == 1)
            check(harness.capture.edits.first?.resultingText == nil)
        }

        private static func postSubmitNestedRenderWithoutInputStillUpdates(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.capture.afterSubmit = {
                harness.capture.text = ""; harness.capture.focused = false
            }
            harness.coordinator.afterNativeTextAssignmentForProbe = {
                harness.coordinator.afterNativeTextAssignmentForProbe = nil
                let current = harness.coordinator.parent
                let latest = ComposerEditor(
                    text: current.$text, focused: current.$focused,
                    ownerID: current.ownerID, accountGeneration: current.accountGeneration,
                    placeholder: "Latest render", onChange: current.onChange, onSubmit: current.onSubmit)
                harness.coordinator.install(latest, in: harness.view)
            }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.submitted == ["abc"] && harness.view.text.isEmpty)
            check(harness.view.placeholderLabel.text == "Latest render")
        }

        private static func postSubmitStaleOwnerRenderCannotReclaimSuccessor(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            let oldID = harness.coordinator.parent.ownerID
            let nextID = UUID()
            var visibleID = oldID
            harness.coordinator.parent.isCurrentOwner = { visibleID == oldID }
            let stale = harness.coordinator.parent
            let successor = Capture(text: "", marker: 0)
            let next = ComposerEditor(
                text: Binding(get: { successor.text }, set: { successor.text = $0 }), focused: .constant(false),
                ownerID: nextID, accountGeneration: harness.coordinator.parent.accountGeneration,
                isCurrentOwner: { visibleID == nextID }, placeholder: "Successor",
                onChange: { successor.receive($0, $1) }, onSubmit: {})
            harness.coordinator.parent.refreshAfterSubmit = { next }
            harness.capture.afterSubmit = {
                visibleID = nextID; harness.capture.retired = true; harness.capture.focused = false
            }
            harness.coordinator.afterNativeTextAssignmentForProbe = {
                harness.coordinator.afterNativeTextAssignmentForProbe = nil
                harness.view.insertText("d")
                harness.coordinator.install(stale, in: harness.view)
            }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.submitted == ["abc"] && harness.capture.text == "abc")
            check(successor.text == "d" && harness.coordinator.owner?.draft == nextID)
            harness.coordinator.install(stale, in: harness.view)
            check(successor.text == "d" && harness.view.text == "d")
            harness.view.insertText("e")
            check(successor.text == "de" && successor.rebaseFailures == 0 && harness.capture.edits.isEmpty)
        }

        private static func postSubmitEqualBaselineRuleEndsAtOuterUnwind(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.capture.afterSubmit = {
                harness.capture.text = ""; harness.capture.focused = false
                harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
            }
            harness.coordinator.afterNativeTextAssignmentForProbe = {
                harness.coordinator.afterNativeTextAssignmentForProbe = nil
                harness.view.insertText("d")
                harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.text == "d" && harness.view.text == "d")
            // A later same-owner external clear is authoritative. The special
            // linearization rule never survives the synchronous Submit apply.
            harness.capture.text = ""
            harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            check(harness.view.text.isEmpty && harness.coordinator.reportedText.isEmpty)
            check(harness.capture.submitted == ["abc"] && harness.capture.rebaseFailures == 0)
        }

        private static func ordinarySameOwnerInstallPreservesMarkedComposition(_ check: (Bool) -> Void) {
            for replaceOwner in [false, true] {
                let harness = Harness(text: "a", marker: 1, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                harness.view.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0))
                let markedText = harness.view.text
                let selectedRange = harness.view.selectedRange
                check(harness.view.markedTextRange != nil && harness.capture.text == markedText)
                harness.coordinator.install(harness.coordinator.parent, in: harness.view)
                check(harness.view.markedTextRange != nil)
                check(harness.view.text == markedText && harness.view.selectedRange == selectedRange)
                if replaceOwner {
                    let replacement = ComposerEditor(
                        text: .constant("external"), focused: .constant(false),
                        ownerID: UUID(), accountGeneration: harness.coordinator.parent.accountGeneration,
                        placeholder: "Replacement", onSubmit: {})
                    harness.coordinator.install(replacement, in: harness.view)
                } else {
                    harness.capture.text = "external"
                    harness.coordinator.install(harness.coordinator.parent, in: harness.view)
                }
                check(harness.view.markedTextRange == nil && harness.view.text == "external")
                check(harness.capture.submitted.isEmpty)
            }
        }

        private static func refusedResignationDoesNotSendOrClear(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            harness.view.refuseResignationForProbe = true
            _ = harness.view.perform(action, with: command)
            check(harness.view.isFirstResponder && harness.capture.focused)
            check(harness.capture.text == "abc" && harness.capture.submitted.isEmpty)
            check(harness.capture.edits.isEmpty && harness.capture.rebaseFailures == 0)
            harness.view.refuseResignationForProbe = false
        }

        private static func compositionDuringResignationCancelsWithoutCommit(_ check: (Bool) -> Void) {
            for modifiers in [UIKeyModifierFlags(), .shift] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                guard let command = returnCommand(harness.view, modifiers: modifiers), let action = command.action
                else { check(false); continue }
                harness.view.beforeResignFirstResponderForProbe = {
                    harness.view.beforeResignFirstResponderForProbe = nil
                    harness.view.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0))
                }
                _ = harness.view.perform(action, with: command)
                check(harness.view.markedTextRange != nil && harness.view.isFirstResponder)
                check(harness.view.text == "abcに" && harness.capture.text == "abcに")
                check(harness.capture.submitted.isEmpty && !harness.view.text.contains("\n"))
                check(harness.capture.rebaseFailures == 0)
            }
        }

        private static func revokedOwnerWithoutInstallationCannotSend(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            var active = true
            harness.coordinator.parent.isCurrentOwner = { active }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            harness.view.beforeResignFirstResponderForProbe = {
                harness.view.beforeResignFirstResponderForProbe = nil
                active = false
            }
            _ = harness.view.perform(action, with: command)
            check(harness.capture.submitted.isEmpty && harness.capture.text == "abc")
            check(!harness.view.isFirstResponder)
        }

        private static func successorIsInstalledBeforeHardwareFocusResumes(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            let successor = Capture(text: "", marker: 0)
            let successorID = UUID()
            let next = ComposerEditor(
                text: Binding(get: { successor.text }, set: { successor.text = $0 }),
                focused: Binding(get: { successor.focused }, set: { successor.focused = $0 }),
                ownerID: successorID, accountGeneration: harness.coordinator.parent.accountGeneration,
                placeholder: "Successor", onChange: { successor.receive($0, $1) }, onSubmit: {})
            harness.capture.afterSubmit = { harness.capture.retired = true }
            harness.coordinator.parent.refreshAfterSubmit = { next }
            var resumeCount = 0
            var successorInstalled = false
            var baselineEmpty = false
            var submittedBeforeFocus = false
            harness.view.beforeBecomeFirstResponderForProbe = {
                harness.view.beforeBecomeFirstResponderForProbe = nil
                resumeCount += 1
                successorInstalled = harness.coordinator.owner?.draft == successorID
                baselineEmpty = harness.coordinator.reportedText.isEmpty && harness.view.text.isEmpty
                submittedBeforeFocus = harness.capture.submitted == ["abc"]
                harness.view.insertText("d")
            }
            guard let command = returnCommand(harness.view), let action = command.action else { check(false); return }
            _ = harness.view.perform(action, with: command)
            check(successorInstalled)
            check(baselineEmpty)
            check(submittedBeforeFocus)
            check(resumeCount == 1 && harness.view.isFirstResponder && successor.focused)
            check(successor.text == "d" && successor.edits.map(\.replacement) == ["d"])
            check(successor.rebaseFailures == 0 && harness.capture.edits.isEmpty)
            check(harness.capture.text == "abc" && harness.capture.submitted == ["abc"])
        }

        private static func sendAndDismissForPresentation(_ harness: Harness) -> Bool {
            wireNativeInput(harness)
            harness.capture.afterSubmit = {
                harness.capture.text = ""
                harness.capture.marker = DictationInsertionAnchor(text: "", offsetUTF16: 0)
                harness.capture.focused = false
            }
            guard let command = returnCommand(harness.view), let action = command.action else { return false }
            _ = harness.view.perform(action, with: command)
            return harness.capture.submitted == ["abc"] && harness.view.softwareKeyboardSuppressed
        }

        private static func hardwareSendDismissalPreservesSuccessorKeyboardInput(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            var suppressedBeforeFocus = false
            harness.view.beforeBecomeFirstResponderForProbe = {
                harness.view.beforeBecomeFirstResponderForProbe = nil
                suppressedBeforeFocus =
                    harness.view.softwareKeyboardSuppressed
                    && harness.view.inputView?.frame.height == 0 && harness.view.text.isEmpty
                harness.view.insertText("d")
            }
            check(sendAndDismissForPresentation(harness))
            check(suppressedBeforeFocus && harness.view.isFirstResponder)
            check(harness.capture.text == "d" && harness.capture.edits.map(\.replacement) == ["d"])
            harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            check(harness.view.softwareKeyboardSuppressed && harness.view.inputView != nil)
            harness.view.insertText("e")
            check(harness.capture.text == "de" && harness.capture.submitted == ["abc"])
            check(harness.capture.rebaseFailures == 0)
        }

        private static func keyboardPresentationPreservesSendDisposition(_ check: (Bool) -> Void) {
            for disposition in ["rejected", "reserved", "shift", "refused"] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                if disposition == "reserved" { harness.capture.afterSubmit = { harness.capture.focused = false } }
                if disposition == "refused" { harness.view.refuseResignationForProbe = true }
                guard let command = returnCommand(harness.view, modifiers: disposition == "shift" ? .shift : []),
                    let action = command.action
                else { check(false); continue }
                _ = harness.view.perform(action, with: command)
                check(harness.view.softwareKeyboardSuppressed == (disposition == "reserved"))
                check((harness.view.inputView != nil) == (disposition == "reserved"))
                check(harness.capture.text == (disposition == "shift" ? "abc\n" : "abc"))
                check(harness.capture.rebaseFailures == 0)
            }
        }

        private static func explicitEditingRestoresSystemKeyboardWithExactInput(_ check: (Bool) -> Void) {
            for accessibility in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                check(sendAndDismissForPresentation(harness))
                var reloads = 0
                var baselineBeforeInput = false
                harness.view.beforeReloadInputViewsForProbe = {
                    harness.view.beforeReloadInputViewsForProbe = nil
                    reloads += 1
                    baselineBeforeInput =
                        harness.view.inputView == nil
                        && harness.coordinator.reportedText.isEmpty && harness.view.text.isEmpty
                    harness.view.insertText("d")
                }
                if accessibility {
                    _ = harness.view.accessibilityActivate()
                } else {
                    harness.view.showSoftwareKeyboardForInteraction()
                }
                check(reloads == 1 && baselineBeforeInput)
                check(!harness.view.softwareKeyboardSuppressed && harness.view.inputView == nil)
                check(harness.capture.text == "d" && harness.view.text == "d")
                check(harness.capture.edits.map(\.replacement) == ["d"])
                check(harness.capture.rebaseFailures == 0 && harness.capture.submitted == ["abc"])
            }
        }

        private static func ordinaryOwnerAndTeardownResetKeyboardPresentation(_ check: (Bool) -> Void) {
            for newGeneration in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                check(sendAndDismissForPresentation(harness))
                let next = ComposerEditor(
                    text: .constant("new draft"), focused: .constant(true), ownerID: UUID(),
                    accountGeneration: newGeneration ? UUID() : harness.coordinator.parent.accountGeneration,
                    placeholder: "Replacement", onSubmit: {})
                harness.coordinator.install(next, in: harness.view)
                check(harness.view.inputView == nil && !harness.view.softwareKeyboardSuppressed)
                check(harness.view.text == "new draft" && harness.capture.text.isEmpty)
            }
            for replaceDuringReload in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                check(sendAndDismissForPresentation(harness))
                let nextCapture = Capture(text: "new draft", marker: 9)
                nextCapture.focused = true
                let nextID = UUID()
                let revokedID = UUID()
                let next = ComposerEditor(
                    text: Binding(get: { nextCapture.text }, set: { nextCapture.text = $0 }),
                    focused: Binding(get: { nextCapture.focused }, set: { nextCapture.focused = $0 }),
                    ownerID: nextID, accountGeneration: UUID(), placeholder: "Next owner",
                    onChange: { nextCapture.receive($0, $1) }, onSubmit: {})
                var baselineBeforeReload = false
                harness.view.beforeReloadInputViewsForProbe = {
                    harness.view.beforeReloadInputViewsForProbe = nil
                    baselineBeforeReload =
                        harness.coordinator.owner?.draft == nextID
                        && harness.coordinator.reportedText == "new draft" && harness.view.text == "new draft"
                    harness.view.insertText("d")
                    if replaceDuringReload {
                        let revoked = ComposerEditor(
                            text: .constant("authoritative replacement"), focused: .constant(false), ownerID: revokedID,
                            accountGeneration: UUID(), placeholder: "Replacement", onSubmit: {})
                        harness.coordinator.install(revoked, in: harness.view)
                    }
                }
                harness.coordinator.install(next, in: harness.view)
                check(baselineBeforeReload)
                check(harness.view.inputView == nil && !harness.view.softwareKeyboardSuppressed)
                check(harness.capture.text.isEmpty && harness.capture.edits.isEmpty)
                if replaceDuringReload {
                    check(harness.coordinator.owner?.draft == revokedID && !harness.view.isFirstResponder)
                    check(harness.view.text == "authoritative replacement" && nextCapture.text == "new draft")
                    check(nextCapture.edits.isEmpty)
                } else {
                    check(harness.coordinator.owner?.draft == nextID)
                    check(nextCapture.text == "new draftd" && harness.view.text == "new draftd")
                    check(
                        nextCapture.edits == [
                            DictationTextEdit(
                                beforeText: "new draft", rangeUTF16: NSRange(location: 9, length: 0), replacement: "d")
                        ])
                    check(nextCapture.rebaseFailures == 0)
                }
            }
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            check(sendAndDismissForPresentation(harness))
            ComposerEditor.dismantleUIView(harness.view, coordinator: harness.coordinator)
            check(harness.view.inputView == nil && !harness.view.softwareKeyboardSuppressed)
            check(!harness.view.isFirstResponder && !harness.coordinator.valid)
        }

        private static func keyboardPresentationReentryCannotReclaimReplacement(_ check: (Bool) -> Void) {
            for detach in [false, true] {
                let retained = Harness(text: "abc", marker: 3, focus: true)
                defer { retained.close() }
                check(sendAndDismissForPresentation(retained))
                retained.view.beforeReloadInputViewsForProbe = {
                    retained.view.beforeReloadInputViewsForProbe = nil
                    retained.view.insertText("d")
                    retained.capture.focused = false
                    if detach {
                        retained.view.removeFromSuperview()
                    } else {
                        retained.view.resignFirstResponder()
                    }
                }
                retained.view.showSoftwareKeyboardForInteraction()
                check(retained.capture.text == "d" && retained.view.text == "d")
                check(
                    retained.capture.edits == [
                        DictationTextEdit(beforeText: "", rangeUTF16: NSRange(location: 0, length: 0), replacement: "d")
                    ])
                check(retained.capture.rebaseFailures == 0 && retained.capture.submitted == ["abc"])
                check(!retained.view.isFirstResponder && !retained.capture.focused)
                if detach { check(retained.view.window == nil) }
            }
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            check(sendAndDismissForPresentation(harness))
            let nextID = UUID()
            var replaced = false
            harness.view.beforeReloadInputViewsForProbe = {
                harness.view.beforeReloadInputViewsForProbe = nil
                replaced = true
                let next = ComposerEditor(
                    text: .constant("replacement"), focused: .constant(false), ownerID: nextID,
                    accountGeneration: UUID(), placeholder: "Replacement", onSubmit: {})
                harness.coordinator.install(next, in: harness.view)
            }
            harness.view.showSoftwareKeyboardForInteraction()
            check(replaced && harness.coordinator.owner?.draft == nextID)
            check(harness.view.text == "replacement" && !harness.view.isFirstResponder)
            check(harness.view.inputView == nil && !harness.view.softwareKeyboardSuppressed)
            check(harness.capture.text.isEmpty && harness.capture.submitted == ["abc"])
        }

        private static func keyboardRestorationPreservesMarkedAndRevokedOwners(_ check: (Bool) -> Void) {
            for revoked in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                check(sendAndDismissForPresentation(harness))
                if revoked {
                    harness.coordinator.parent.isCurrentOwner = { false }
                } else {
                    harness.view.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0))
                    check(harness.view.markedTextRange != nil)
                }
                let text = harness.view.text
                let selection = harness.view.selectedRange
                harness.view.showSoftwareKeyboardForInteraction()
                check(harness.view.softwareKeyboardSuppressed && harness.view.inputView != nil)
                check(harness.view.text == text && harness.view.selectedRange == selection)
                if !revoked { check(harness.view.markedTextRange != nil) }
                check(harness.capture.submitted == ["abc"])
            }
        }

        private static func compactInteractionReclaimsRetainedKeyboardFocus(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            var interactions = 0
            var editor = harness.coordinator.parent
            editor.isCompact = true
            editor.onEditingInteraction = {
                interactions += 1; return true
            }
            harness.coordinator.install(editor, in: harness.view)
            check(harness.view.isFirstResponder && !harness.view.softwareKeyboardSuppressed)
            var reloads = 0
            harness.view.beforeReloadInputViewsForProbe = {
                harness.view.beforeReloadInputViewsForProbe = nil
                reloads += 1
                harness.view.insertText("d")
            }
            harness.view.showSoftwareKeyboardForInteraction()
            check(interactions == 1 && reloads == 1 && harness.capture.focused)
            check(harness.view.text == "abcd" && harness.capture.text == "abcd")
            check(harness.capture.edits.map(\.replacement) == ["d"] && harness.capture.rebaseFailures == 0)
            check(harness.capture.submitted.isEmpty && harness.view.markedTextRange == nil)
        }

        private static func compactInteractionStartsUnfocusedEditorWithExactInput(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.view.resignFirstResponder()
            check(!harness.capture.focused && !harness.view.isFirstResponder)
            var interactions = 0
            var editor = harness.coordinator.parent
            editor.isCompact = true
            editor.onEditingInteraction = {
                interactions += 1; return true
            }
            harness.coordinator.install(editor, in: harness.view)
            harness.view.beforeBecomeFirstResponderForProbe = {
                harness.view.beforeBecomeFirstResponderForProbe = nil
                harness.view.insertText("d")
            }
            harness.view.showSoftwareKeyboardForInteraction()
            check(interactions == 1 && harness.capture.focused && harness.view.isFirstResponder)
            check(harness.view.text == "abcd" && harness.capture.text == "abcd")
            check(harness.capture.edits.map(\.replacement) == ["d"] && harness.capture.rebaseFailures == 0)
            check(harness.capture.submitted.isEmpty && !harness.view.softwareKeyboardSuppressed)
        }

        private static func compactInteractionSurvivesSameOwnerRender(_ check: (Bool) -> Void) {
            for startsFocused in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                if !startsFocused { harness.view.resignFirstResponder() }
                var interactions = 0
                var editor = harness.coordinator.parent
                editor.isCompact = true
                editor.onEditingInteraction = {
                    interactions += 1
                    var refreshed = harness.coordinator.parent
                    refreshed.isCompact = false
                    harness.coordinator.install(refreshed, in: harness.view)
                    return true
                }
                editor = ComposerEditor(
                    text: editor.$text,
                    focused: Binding(
                        get: { harness.capture.focused },
                        set: { value in
                            harness.capture.focused = value
                            harness.coordinator.install(harness.coordinator.parent, in: harness.view)
                        }),
                    ownerID: editor.ownerID, accountGeneration: editor.accountGeneration,
                    isCurrentOwner: editor.isCurrentOwner, isCurrentPresentation: editor.isCurrentPresentation,
                    placeholder: editor.placeholder,
                    isCompact: editor.isCompact, accessibilityIdentifier: editor.accessibilityIdentifier,
                    externalSelection: editor.externalSelection, onChange: editor.onChange,
                    onEditingInteraction: editor.onEditingInteraction,
                    refreshAfterSubmit: editor.refreshAfterSubmit, onSubmit: editor.onSubmit)
                harness.coordinator.install(editor, in: harness.view)
                var reloads = 0
                harness.view.beforeReloadInputViewsForProbe = { reloads += 1 }
                harness.view.showSoftwareKeyboardForInteraction()
                check(interactions == 1 && harness.capture.focused && harness.view.isFirstResponder)
                check(reloads == (startsFocused ? 1 : 0))
                check(harness.view.text == "abc" && harness.capture.text == "abc")
                check(harness.capture.edits.isEmpty && harness.capture.submitted.isEmpty)
            }
        }

        private static func compactInteractionCannotReclaimRevokedPresentation(_ check: (Bool) -> Void) {
            for replacesOwner in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                harness.view.resignFirstResponder()
                let nextID = UUID()
                var editor = harness.coordinator.parent
                editor.isCompact = true
                editor.onEditingInteraction = {
                    if replacesOwner {
                        let next = ComposerEditor(
                            text: .constant("next"), focused: .constant(false), ownerID: nextID,
                            accountGeneration: UUID(), placeholder: "Next", onSubmit: {})
                        harness.coordinator.install(next, in: harness.view)
                        return true
                    }
                    return false
                }
                harness.coordinator.install(editor, in: harness.view)
                var reloads = 0
                harness.view.beforeReloadInputViewsForProbe = { reloads += 1 }
                harness.view.showSoftwareKeyboardForInteraction()
                check(reloads == 0 && !harness.view.isFirstResponder && !harness.capture.focused)
                check(harness.capture.submitted.isEmpty && harness.capture.edits.isEmpty)
                check(harness.view.text == (replacesOwner ? "next" : "abc"))
                if replacesOwner { check(harness.coordinator.owner?.draft == nextID) }
            }
        }

        private static func lateNativeFocusCallbacksCannotReopenOrDismissEditor(_ check: (Bool) -> Void) {
            let harness = Harness(text: "abc", marker: 3, focus: true)
            defer { harness.close() }
            wireNativeInput(harness)
            harness.view.resignFirstResponder()
            check(!harness.capture.focused && !harness.view.isFirstResponder)

            // Model the actual queued Begin observed after menu navigation had
            // already resigned the retained native editor. It must not refocus.
            harness.coordinator.textViewDidBeginEditing(harness.view)
            check(!harness.capture.focused && !harness.view.isFirstResponder)
            harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            check(!harness.capture.focused && !harness.view.isFirstResponder)

            harness.capture.focused = true
            harness.coordinator.install(harness.coordinator.parent, in: harness.view)
            check(harness.capture.focused && harness.view.isFirstResponder)
            // A queued End from that earlier resignation also cannot dismiss
            // a native editor that the current owned projection reacquired.
            harness.coordinator.textViewDidEndEditing(harness.view)
            check(harness.capture.focused && harness.view.isFirstResponder)

            harness.coordinator.parent.isCurrentOwner = { false }
            harness.capture.focused = false
            harness.coordinator.textViewDidBeginEditing(harness.view)
            check(!harness.capture.focused && harness.view.isFirstResponder)
            harness.view.resignFirstResponder()
            harness.capture.focused = true
            harness.coordinator.textViewDidEndEditing(harness.view)
            check(harness.capture.focused && !harness.view.isFirstResponder)
            check(harness.view.text == "abc" && harness.capture.text == "abc")
            check(harness.capture.edits.isEmpty && harness.capture.submitted.isEmpty)
        }

        private static func unrequestedNativeFocusCannotReopenDismissedComposer(_ check: (Bool) -> Void) {
            for cachedFocusRead in [false, true] {
                let harness = Harness(text: "abc", marker: 3, focus: true)
                defer { harness.close() }
                wireNativeInput(harness)
                harness.view.resignFirstResponder()
                check(!harness.capture.focused && !harness.view.isFirstResponder)
                var interactions = 0
                let previous = harness.coordinator.parent
                let focused: Binding<Bool> =
                    cachedFocusRead
                    ? Binding(get: { false }, set: { harness.capture.focused = $0 }) : previous.$focused
                let editor = ComposerEditor(
                    text: previous.$text, focused: focused, ownerID: previous.ownerID,
                    accountGeneration: previous.accountGeneration, isCurrentOwner: previous.isCurrentOwner,
                    isCurrentPresentation: previous.isCurrentPresentation,
                    placeholder: previous.placeholder, isCompact: true,
                    accessibilityIdentifier: previous.accessibilityIdentifier,
                    externalSelection: previous.externalSelection, onChange: previous.onChange,
                    onEditingInteraction: {
                        interactions += 1; return true
                    },
                    refreshAfterSubmit: previous.refreshAfterSubmit, onSubmit: previous.onSubmit)
                harness.coordinator.install(editor, in: harness.view)

                // Model the same UIKit acquisition that occurred on retained
                // navigation reattachment, without a user touch or focus request.
                check(!harness.view.becomeFirstResponder())
                check(!harness.view.isFirstResponder && !harness.capture.focused && interactions == 0)

                // The real restoration gesture calls this exact production
                // admission before native acquisition. A SwiftUI binding whose
                // cached getter still says false must not reject the owned tap.
                check(harness.view.showSoftwareKeyboardForInteraction())
                check(harness.view.isFirstResponder && harness.capture.focused && interactions == 1)
                check(harness.view.text == "abc" && harness.capture.text == "abc")
                check(harness.capture.edits.isEmpty && harness.capture.submitted.isEmpty)

                harness.view.resignFirstResponder()
                check(!harness.view.isFirstResponder)
                harness.capture.focused = true
                harness.coordinator.parent.isCurrentPresentation = { false }
                check(!harness.view.becomeFirstResponder())
                check(!harness.view.isFirstResponder && interactions == 1)
                harness.coordinator.parent.isCurrentPresentation = { true }
                harness.coordinator.parent.isCurrentOwner = { false }
                check(!harness.view.becomeFirstResponder())
                check(!harness.view.isFirstResponder && interactions == 1)
            }
        }

        private static func nativeTextInsetsKeepCaretAndPlaceholderAligned(_ check: (Bool) -> Void) {
            // Caret geometry must come from the same owned, hosted native editor
            // as real input. A detached UITextView can defer its text layout.
            let harness = Harness(text: "", marker: 0, focus: true)
            defer { harness.close() }
            let view = harness.view
            labeledCheck(view.window != nil, label: "geometry.hostedWindow", check: check)
            labeledCheck(view.isFirstResponder, label: "geometry.nativeFocus", check: check)
            view.frame.size = CGSize(width: 240, height: 44)
            view.font = .preferredFont(forTextStyle: .body)
            view.textContainer.lineFragmentPadding = 0
            view.placeholderLabel.text = "Message Maple"
            view.placeholderLabel.font = view.font
            let lineHeight = view.font?.lineHeight ?? 22

            func layoutNativeText() {
                view.setNeedsLayout()
                view.layoutIfNeeded()
                // Prefer TextKit 2: accessing layoutManager would otherwise
                // force UIKit's production editor to fall back to TextKit 1.
                if let manager = view.textLayoutManager, let range = manager.textContentManager?.documentRange {
                    manager.ensureLayout(for: range)
                } else {
                    view.layoutManager.ensureLayout(for: view.textContainer)
                }
                view.layoutIfNeeded()
            }

            for (iteration, compact) in [true, false, true].enumerated() {
                view.isCompact = compact
                view.frame.size.height = compact ? 44 : 110
                view.text = ""
                view.selectedRange = NSRange(location: 0, length: 0)
                layoutNativeText()
                let caret = view.caretRect(for: view.beginningOfDocument)
                let geometry =
                    "iteration=\(iteration);compact=\(compact);bounds=\(String(describing: view.bounds));"
                    + "placeholder=\(String(describing: view.placeholderLabel.frame));caret=\(String(describing: caret));"
                    + "insets=\(String(describing: view.textContainerInset))"
                labeledCheck(
                    view.textContainerInset.left == 8 && view.textContainerInset.right == 8,
                    label: "geometry.horizontalInsets[\(geometry)]", check: check)
                labeledCheck(
                    abs(view.placeholderLabel.frame.minX - 8) < 0.5,
                    label: "geometry.placeholderLeading[\(geometry)]", check: check)
                labeledCheck(
                    view.placeholderLabel.frame.maxX <= view.bounds.width - 8 + 0.5,
                    label: "geometry.placeholderTrailing[\(geometry)]", check: check)
                labeledCheck(
                    // UIKit centers its two-point caret stroke on the glyph's
                    // insertion position; its left edge is one point earlier.
                    abs(caret.midX - view.placeholderLabel.frame.minX) < 0.5,
                    label: "geometry.caretInsertionPosition[\(geometry)]", check: check)
                if compact {
                    labeledCheck(
                        abs(view.placeholderLabel.frame.midY - view.bounds.midY) < 1,
                        label: "geometry.compactPlaceholderCentered[\(geometry)]", check: check)
                    labeledCheck(
                        caret.minY >= 6 && caret.maxY <= view.bounds.height,
                        label: "geometry.compactCaretBounds[\(geometry)]", check: check)
                } else {
                    labeledCheck(
                        view.textContainerInset.top == 6 && view.textContainerInset.bottom == 6,
                        label: "geometry.expandedVerticalInsets[\(geometry)]", check: check)
                    labeledCheck(
                        abs(view.placeholderLabel.frame.minY - 6) < 0.5,
                        label: "geometry.expandedPlaceholderTop[\(geometry)]", check: check)
                }
                view.text = "Synthetic text"
                view.selectedRange = NSRange(location: 0, length: 0)
                layoutNativeText()
                view.setMarkedText("に", selectedRange: NSRange(location: 1, length: 0))
                labeledCheck(
                    view.markedTextRange != nil, label: "geometry.\(iteration).markedInputRetained", check: check)
                labeledCheck(
                    view.textContainerInset.left == 8 && view.textContainerInset.right == 8,
                    label: "geometry.\(iteration).markedHorizontalInsets", check: check)
                view.unmarkText()
                labeledCheck(
                    view.text == "にSynthetic text" && view.selectedRange.location == 1,
                    label: "geometry.\(iteration).committedInputAndSelection", check: check)
            }
            view.font = .systemFont(ofSize: 44)
            view.placeholderLabel.font = view.font
            view.isCompact = true
            view.frame.size.height = ComposerEditorMetrics.preferredHeight(
                lineHeight: view.font?.lineHeight ?? lineHeight, contentHeight: 0, compact: true)
            layoutNativeText()
            let largeGeometry =
                "bounds=\(String(describing: view.bounds));placeholder=\(String(describing: view.placeholderLabel.frame));"
                + "insets=\(String(describing: view.textContainerInset))"
            labeledCheck(
                view.textContainerInset.top >= 6 && view.textContainerInset.bottom >= 6,
                label: "geometry.largeFontVerticalInsets[\(largeGeometry)]", check: check)
            labeledCheck(
                abs(view.placeholderLabel.frame.midY - view.bounds.midY) < 1,
                label: "geometry.largeFontPlaceholderCentered[\(largeGeometry)]", check: check)
        }

        @MainActor private final class Capture {
            var text: String
            var focused = false
            var retired = false
            var events: [(DictationEditorState, [DictationTextEdit])] = []
            var edits: [DictationTextEdit] { events.flatMap(\.1) }
            var marker: DictationInsertionAnchor?
            var rebaseFailures = 0
            var afterReceive: ((DictationEditorState, [DictationTextEdit]) -> Void)?
            var submitted: [String] = []
            var afterSubmit: (() -> Void)?
            init(text: String, marker: Int) {
                self.text = text
                self.marker = DictationInsertionAnchor(text: text, offsetUTF16: marker)
            }
            func receive(_ state: DictationEditorState, _ edits: [DictationTextEdit]) {
                events.append((state, edits))
                for edit in edits where marker?.rebase(using: edit) != true { rebaseFailures += 1 }
                afterReceive?(state, edits)
            }
        }

        @MainActor private struct Harness {
            let capture: Capture
            let view: ComposerTextView
            let coordinator: ComposerEditor.Coordinator
            init(text: String, marker: Int, focus: Bool = false) {
                let capture = Capture(text: text, marker: marker)
                capture.focused = focus
                let editor = ComposerEditor(
                    text: Binding(
                        get: { capture.retired ? "" : capture.text }, set: { if !capture.retired { capture.text = $0 } }
                    ),
                    focused: Binding(get: { capture.focused }, set: { capture.focused = $0 }), ownerID: UUID(),
                    placeholder: "Synthetic ordering probe",
                    onChange: { capture.receive($0, $1) },
                    onSubmit: {
                        capture.submitted.append(capture.text); capture.afterSubmit?()
                    })
                let view = ComposerTextView(frame: .zero, textContainer: nil)
                let coordinator = editor.makeCoordinator()
                // Direct coordinator calls control the order; no real delegate or
                // storage callbacks can silently repair the injected sequence.
                coordinator.view = view
                coordinator.install(editor, in: view)
                coordinator.publish()
                capture.events.removeAll()
                if focus,
                    let window = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene })
                        .flatMap(\.windows).first(where: \.isKeyWindow)
                {
                    view.frame = CGRect(x: 8, y: 8, width: 220, height: 44)
                    window.addSubview(view)
                    view.becomeFirstResponder()
                }
                self.capture = capture
                self.view = view
                self.coordinator = coordinator
            }
            func close() {
                coordinator.valid = false
                coordinator.parent.refreshAfterSubmit = nil
                coordinator.afterNativeTextAssignmentForProbe = nil
                view.beforeResignFirstResponderForProbe = nil
                view.beforeBecomeFirstResponderForProbe = nil
                view.beforeReloadInputViewsForProbe = nil
                view.refuseResignationForProbe = false
                view.resignFirstResponder()
                view.removeFromSuperview()
                view.delegate = nil
                view.pasteDelegate = nil
                view.changed = nil
                view.willReplace = nil
                view.submit = nil
                coordinator.view = nil
                capture.afterReceive = nil
                capture.afterSubmit = nil
            }
        }
    }
#endif
