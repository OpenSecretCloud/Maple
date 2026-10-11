#if os(iOS)
    import MapleAppCore
    import SwiftUI
    import UIKit

    #if DEBUG
        @MainActor enum ComposerFocusTrace {
            static func record(_ state: String) {
                let arguments = ProcessInfo.processInfo.arguments
                guard arguments.contains("--maple-shipping-viewport-fixture"),
                    arguments.contains("--maple-shipping-focus-trace")
                else { return }
                // Only focus and lifecycle booleans supplied below. No account,
                // content, input ranges or service data belongs in this trace.
                NSLog("MapleComposerFocus %@", state)
            }
        }
    #endif

    /// A narrow native input surface: SwiftUI owns its chrome and account/draft lifecycle.
    /// UIKit supplies exact edits and marked-text ranges unavailable from TextField bindings.
    struct ComposerEditor: UIViewRepresentable {
        @Binding var text: String
        @Binding var focused: Bool
        let ownerID: UUID
        var accountGeneration: UUID? = nil
        var isCurrentOwner: () -> Bool = { true }
        var isCurrentPresentation: () -> Bool = { true }
        let placeholder: String
        var isCompact = false
        var accessibilityIdentifier = "chat.composer"
        var externalSelection: DictationEditorState? = nil
        var onChange: (DictationEditorState, [DictationTextEdit]) -> Void = { _, _ in }
        var onEditingInteraction: () -> Bool = { true }
        var refreshAfterSubmit: (() -> ComposerEditor?)? = nil
        var onSubmit: () -> Void

        func makeCoordinator() -> Coordinator { Coordinator(self) }

        func makeUIView(context: Context) -> ComposerTextView {
            let view = ComposerTextView(frame: .zero, textContainer: nil)
            view.backgroundColor = .clear
            view.font = .preferredFont(forTextStyle: .body)
            view.adjustsFontForContentSizeCategory = true
            view.textColor = .label
            view.textContainerInset = UIEdgeInsets(
                top: ComposerEditorMetrics.verticalInset, left: ComposerEditorMetrics.horizontalInset,
                bottom: ComposerEditorMetrics.verticalInset, right: ComposerEditorMetrics.horizontalInset)
            view.textContainer.lineFragmentPadding = 0
            view.autocapitalizationType = .sentences
            view.returnKeyType = .default
            view.isScrollEnabled = true
            view.delegate = context.coordinator
            let observer = ComposerStorageObserver { [weak coordinator = context.coordinator] in
                Task { @MainActor in coordinator?.publish() }
            }
            context.coordinator.storageObserver = observer
            view.textStorage.delegate = observer
            view.willReplace = { [weak coordinator = context.coordinator] before, range, replacement in
                coordinator?.recordNativeReplacement(before: before, range: range, replacement: replacement)
            }
            view.changed = { [weak coordinator = context.coordinator] in coordinator?.publish(completedInput: true) }
            view.submit = { [weak coordinator = context.coordinator] in coordinator?.submit() }
            #if DEBUG
                view.inputOrderObserver = { [weak coordinator = context.coordinator] event in
                    coordinator?.observeInputOrder(event)
                }
            #endif
            context.coordinator.view = view
            context.coordinator.install(self, in: view)
            return view
        }

        func updateUIView(_ view: ComposerTextView, context: Context) {
            context.coordinator.install(self, in: view)
        }

        func sizeThatFits(_ proposal: ProposedViewSize, uiView: ComposerTextView, context: Context) -> CGSize? {
            guard let width = proposal.width else { return nil }
            let line = uiView.font?.lineHeight ?? 22
            let size = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
            let placeholderHeight = uiView.placeholderLabel.sizeThatFits(
                CGSize(
                    width: max(0, width - ComposerEditorMetrics.horizontalInset * 2), height: .greatestFiniteMagnitude)
            ).height
            // Keep the normal tap surface and eight-line cap, then honor the
            // composer budget without reducing the native font. UITextView scrolls
            // its remaining content when chrome and the keyboard need more room.
            let preferred = ComposerEditorMetrics.preferredHeight(
                lineHeight: line,
                contentHeight: max(
                    size.height, text.isEmpty ? placeholderHeight + ComposerEditorMetrics.verticalInset * 2 : 0),
                compact: isCompact)
            let available = proposal.height.flatMap { $0.isFinite ? max(0, $0) : nil }
            let minimum = ComposerEditorMetrics.preferredHeight(lineHeight: line, contentHeight: 0, compact: true)
            return CGSize(width: width, height: min(preferred, max(minimum, available ?? preferred)))
        }

        static func dismantleUIView(_ view: ComposerTextView, coordinator: Coordinator) {
            // Finalize the old view through its old, owner-fenced callbacks. A held
            // draft must not retain a stale marked range after its input view leaves.
            view.unmarkText()
            coordinator.publish()
            coordinator.valid = false
            view.invalidateReturnCommands()
            view.removeKeyboardRestorationGesture()
            #if DEBUG
                coordinator.observeInputOrder("dismantle")
                coordinator.inputOrderObserver = nil
                coordinator.afterNativeTextAssignmentForProbe = nil
                view.beforeResignFirstResponderForProbe = nil
                view.beforeBecomeFirstResponderForProbe = nil
                view.beforeReloadInputViewsForProbe = nil
                view.inputOrderObserver = nil
            #endif
            view.changed = nil; view.submit = nil; view.willReplace = nil
            view.delegate = nil; view.textStorage.delegate = nil
            _ = view.resignFirstResponder()
            view.setSoftwareKeyboardSuppressed(false)
            view.text = ""
            coordinator.requests.removeAll()
            coordinator.reportedText = ""
            coordinator.storageObserver = nil
            coordinator.lastPublished = nil
        }

        @MainActor final class Coordinator: NSObject, UITextViewDelegate {
            struct Owner: Equatable { let draft: UUID; let generation: UUID? }
            var parent: ComposerEditor
            weak var view: ComposerTextView?
            var owner: Owner?
            var reportedText = ""
            var requests: [DictationTextEdit] = []
            var requestBytes = 0
            var requestOverflow = false
            var storageObserver: ComposerStorageObserver?
            var installing = false
            var valid = true
            private var installationRevision = UUID()
            private struct HardwareResignation {
                let owner: Owner?
                let installation: UUID
                let action: UUID
                let window: UIWindow
            }
            private var hardwareResignation: HardwareResignation?
            private struct NativeFocusAcquisition {
                let owner: Owner
                let installation: UUID
                let action: UUID
                let window: UIWindow
            }
            private var nativeFocusAcquisition: NativeFocusAcquisition?
            private final class SubmitInstallation {
                let owner: Owner
                let baseline: String
                let revision: UUID
                var expectedNativeText: String
                var acceptsNativeEdits = false
                var receivedNativeEdit = false
                var replacement: ComposerEditor?
                var replacementRevokesInput = false
                init(owner: Owner, baseline: String, revision: UUID) {
                    self.owner = owner; self.baseline = baseline; self.revision = revision
                    expectedNativeText = baseline
                }
            }
            private var submitInstallation: SubmitInstallation?
            #if DEBUG
                var afterNativeTextAssignmentForProbe: (() -> Void)?
            #endif
            var publicationScheduled = false
            var lastPublished: DictationEditorState?
            #if DEBUG
                // Opt-in synthetic fixture diagnostics: lengths/ranges/event names only.
                var inputOrderObserver: ((String) -> Void)?
                fileprivate func observeInputOrder(_ event: @autoclosure () -> String) {
                    guard let observer = inputOrderObserver else { return }
                    observer(
                        "\(event()); native=\((view?.text ?? "").utf16.count); binding=\(parent.text.utf16.count); "
                            + "reported=\(reportedText.utf16.count); same=\(view?.text == reportedText); "
                            + "valid=\(valid); installing=\(installing); hasView=\(view != nil); "
                            + "operation=\(view?.performingInputOperation == true); "
                            + "editingCharacters=\(view?.textStorage.editedMask.contains(.editedCharacters) == true); "
                            + "scheduled=\(publicationScheduled); "
                            + "pending=\(requests.count); overflow=\(requestOverflow); "
                            + "selection=\(view?.selectedRange.location ?? -1):\(view?.selectedRange.length ?? -1); "
                            + "marked=\(view?.markedTextRange != nil)")
                }
            #endif
            init(_ parent: ComposerEditor) { self.parent = parent }

            #if DEBUG
                fileprivate func traceFocus(_ event: String) {
                    ComposerFocusTrace.record(
                        "\(event); focused=\(parent.focused); first=\(view?.isFirstResponder == true); "
                            + "compact=\(view?.isCompact == true); suppressed=\(view?.softwareKeyboardSuppressed == true); "
                            + "attached=\(view?.window != nil); installing=\(installing); valid=\(valid)")
                }
            #endif

            func install(_ parent: ComposerEditor, in view: ComposerTextView) {
                #if DEBUG
                    traceFocus("install; incomingFocused=\(parent.focused); incomingCompact=\(parent.isCompact)")
                #endif
                let admittedRevision = installationRevision
                guard valid, self.view === view, parent.isCurrentOwner(), installationRevision == admittedRevision
                else { return }
                let nextOwner = Owner(draft: parent.ownerID, generation: parent.accountGeneration)
                if installing, let transaction = submitInstallation {
                    // SwiftUI may render the committed model while UIKit is still
                    // delivering input inside its native setter/focus callbacks.
                    if nextOwner != transaction.owner || parent.text != transaction.baseline {
                        transaction.replacement = parent
                        transaction.replacementRevokesInput = true
                        transaction.acceptsNativeEdits = false
                    } else if !transaction.replacementRevokesInput {
                        transaction.replacement = parent
                    }
                    return
                }
                #if DEBUG
                    observeInputOrder(
                        "install.begin; nextBinding=\(parent.text.utf16.count); ownerChanged=\(owner != nextOwner); "
                            + "generationChanged=\(owner?.generation != nextOwner.generation)")
                    defer { observeInputOrder("install.end; nextBinding=\(parent.text.utf16.count)") }
                #endif
                if refreshDuringHardwareResignation(parent, in: view, nextOwner: nextOwner) { return }
                if let owner, owner != nextOwner, owner.generation == nextOwner.generation {
                    // Commit/flush held draft B before replacing callbacks with edit A.
                    // Account-generation revocation instead discards old view work below.
                    view.unmarkText()
                    publish()
                }
                // Native callbacks normally publish synchronously. Flush any storage-only
                // transaction before an unrelated SwiftUI update can reapply an older binding.
                if owner == nextOwner, (!requests.isEmpty || requestOverflow), !view.performingInputOperation {
                    publish()
                }
                apply(parent, in: view)
            }

            private func refreshDuringHardwareResignation(
                _ parent: ComposerEditor, in view: ComposerTextView, nextOwner: Owner
            ) -> Bool {
                guard let resignation = hardwareResignation else { return false }
                let native = view.text ?? ""
                func selectionCompatible() -> Bool {
                    guard let requested = parent.externalSelection, requested.text == native,
                        requested.selectionUTF16.location != NSNotFound,
                        requested.selectionUTF16.location >= 0, requested.selectionUTF16.length >= 0,
                        requested.selectionUTF16.location <= native.utf16.count,
                        requested.selectionUTF16.length <= native.utf16.count - requested.selectionUTF16.location
                    else { return true }
                    return requested.selectionUTF16 == view.selectedRange
                }
                func matches() -> Bool {
                    valid && self.view === view && view.window === resignation.window
                        && owner == resignation.owner && nextOwner == resignation.owner
                        && installationRevision == resignation.installation
                        && view.returnActionRevision == resignation.action
                        && view.hardwareReturnInProgress && !installing && !view.performingInputOperation
                        && !view.textStorage.editedMask.contains(.editedCharacters)
                        && view.markedTextRange == nil && !requestOverflow
                        && parent.focused && self.parent.focused && selectionCompatible()
                        && parent.text == native && view.text == native && reportedText == native
                        && lastPublished?.text == native && lastPublished?.markedRangeUTF16 == nil
                        && requests.allSatisfy { $0.beforeText == native && $0.resultingText != nil }
                }
                guard matches() else { return false }
                let current = self.parent.isCurrentOwner()
                guard current, matches() else { return false }
                let incomingCurrent = parent.isCurrentOwner()
                guard incomingCurrent, matches() else {
                    if valid, self.view === view, owner == resignation.owner,
                        installationRevision == resignation.installation,
                        view.returnActionRevision == resignation.action
                    {
                        view.cancelHardwareReturn()
                    }
                    return false
                }
                // UIKit can finish a prior character while resigning, and SwiftUI
                // can synchronously render that exact published text. Refresh only
                // its current callbacks/labels: a no-op render must neither cancel
                // Return nor restore focus inside the outer native resignation.
                // The old Binding may still read a cached pre-publication value.
                // Unapplied, validated proposals remain owned by the normal ledger.
                self.parent = parent
                view.accessibilityIdentifier = parent.accessibilityIdentifier
                view.accessibilityLabel = parent.placeholder
                view.placeholderLabel.text = parent.placeholder
                return true
            }

            /// Install already-authoritative content without first publishing the
            /// retired native buffer. Ordinary updates flush the previous owner
            /// above; Send has already committed or retained that model content.
            private func apply(
                _ parent: ComposerEditor, in view: ComposerTextView, preservingPostSubmitInput: Bool = false,
                restoringHardwareFocus: Bool = false, suppressSoftwareKeyboard: Bool? = nil,
                reloadSoftwareKeyboard: Bool = false, restoringInteractionFocus: Bool = false
            ) {
                let admittedRevision = installationRevision
                guard valid, self.view === view, parent.isCurrentOwner(), installationRevision == admittedRevision
                else { return }
                installationRevision = UUID()
                let revision = installationRevision
                let nextOwner = Owner(draft: parent.ownerID, generation: parent.accountGeneration)
                let baseline = parent.text
                let focusWindow = view.window
                let priorInstalling = installing
                var transaction =
                    preservingPostSubmitInput
                    ? SubmitInstallation(owner: nextOwner, baseline: baseline, revision: revision) : nil
                submitInstallation = transaction
                installing = true
                defer {
                    installing = priorInstalling
                    submitInstallation = nil
                    // Focus cancellation/detachment ends presentation work, but
                    // exact input already received still belongs to this draft.
                    // finish rechecks model ownership and never submits/refocuses.
                    if let transaction { finish(transaction, in: view) }
                }
                let ownerChanged = owner != nextOwner
                let contentChanged = view.text != baseline
                // Retire composition only for actual authoritative replacement.
                // Ordinary same-owner renders must preserve ongoing IME input.
                if (ownerChanged || contentChanged), view.markedTextRange != nil { view.unmarkText() }
                guard valid, installationRevision == revision, self.view === view else { return }
                self.parent = parent
                view.accessibilityIdentifier = parent.accessibilityIdentifier
                view.accessibilityLabel = parent.placeholder
                view.placeholderLabel.text = parent.placeholder
                let previousSelection = view.selectedRange
                let maximumLines = parent.isCompact ? 1 : 0
                if view.textContainer.maximumNumberOfLines != maximumLines {
                    view.textContainer.maximumNumberOfLines = maximumLines
                    view.textContainer.lineBreakMode = parent.isCompact ? .byTruncatingTail : .byWordWrapping
                }
                view.placeholderLabel.numberOfLines = parent.isCompact ? 1 : 0
                view.isCompact = parent.isCompact
                view.updateKeyboardRestorationGesture()
                if ownerChanged {
                    view.invalidateReturnCommands()
                    owner = nextOwner
                    lastPublished = nil
                } else if contentChanged {
                    view.cancelHardwareReturn()
                }
                if ownerChanged || contentChanged {
                    // Establish the authoritative baseline before the native
                    // setter can reenter an owned input method.
                    clearRequests()
                    reportedText = baseline
                }
                transaction?.acceptsNativeEdits = transaction?.replacementRevokesInput != true
                if ownerChanged || contentChanged {
                    view.text = baseline
                    #if DEBUG
                        afterNativeTextAssignmentForProbe?()
                    #endif
                    // An actual native insertion owns its resulting selection.
                    if transaction?.receivedNativeEdit != true {
                        if !ownerChanged, let requested = parent.externalSelection, requested.text == baseline,
                            requested.selectionUTF16.location != NSNotFound,
                            requested.selectionUTF16.location >= 0, requested.selectionUTF16.length >= 0,
                            requested.selectionUTF16.location <= baseline.utf16.count,
                            requested.selectionUTF16.length <= baseline.utf16.count - requested.selectionUTF16.location
                        {
                            view.selectedRange = requested.selectionUTF16
                        } else {
                            let end =
                                ownerChanged
                                ? baseline.utf16.count : min(previousSelection.location, baseline.utf16.count)
                            view.selectedRange = NSRange(location: end, length: 0)
                        }
                    }
                }
                view.placeholderLabel.isHidden = !view.text.isEmpty
                let requestedPresentation: Bool? =
                    suppressSoftwareKeyboard
                    ?? (ownerChanged || (!restoringHardwareFocus && parent.focused && !view.isFirstResponder)
                        ? false : nil)
                if transaction == nil, requestedPresentation == false, view.softwareKeyboardSuppressed {
                    // Ordinary replacement has already retired old composition and
                    // installed the new owner/baseline. Only the ensuing keyboard
                    // reload/focus callbacks may record exact input for that owner.
                    let keyboardTransaction = SubmitInstallation(
                        owner: nextOwner, baseline: baseline, revision: revision)
                    keyboardTransaction.acceptsNativeEdits = true
                    transaction = keyboardTransaction
                    submitInstallation = keyboardTransaction
                }
                let presentationActionRevision = view.returnActionRevision
                let presentationChanged = requestedPresentation.map { view.setSoftwareKeyboardSuppressed($0) } ?? false
                let reloadsPresentation =
                    reloadSoftwareKeyboard && !presentationChanged && view.isFirstResponder
                    && !view.softwareKeyboardSuppressed
                if reloadsPresentation { view.reloadInputViews() }
                if presentationChanged || reloadsPresentation {
                    let current = parent.isCurrentOwner()
                    guard current, valid, self.view === view, view.window === focusWindow, owner == nextOwner,
                        installationRevision == revision, view.returnActionRevision == presentationActionRevision,
                        transaction?.replacementRevokesInput != true
                    else { return }
                }
                if restoringHardwareFocus || restoringInteractionFocus {
                    // The successor's owner and baseline are already installed.
                    // Keep the native installation transaction open while focus
                    // resumes, so received input cannot target the retired draft.
                    if !parent.focused { parent.focused = true }
                    let current = parent.isCurrentOwner()
                    guard current, valid, self.view === view, installationRevision == revision,
                        focusWindow != nil, view.window === focusWindow,
                        transaction?.replacementRevokesInput != true, owner == nextOwner,
                        view.markedTextRange == nil
                    else { return }
                }
                if (restoringHardwareFocus || restoringInteractionFocus || parent.focused), !view.isFirstResponder {
                    if let window = focusWindow, view.window === window {
                        let previous = nativeFocusAcquisition
                        nativeFocusAcquisition = NativeFocusAcquisition(
                            owner: nextOwner, installation: revision, action: view.returnActionRevision, window: window)
                        defer { nativeFocusAcquisition = previous }
                        view.becomeFirstResponder()
                    }
                } else if !restoringHardwareFocus, !restoringInteractionFocus, !parent.focused, view.isFirstResponder {
                    _ = view.resignFirstResponder()
                }
                #if DEBUG
                    traceFocus("apply.afterFocus")
                #endif
                if transaction == nil { schedulePublication() }
            }

            private func finish(_ transaction: SubmitInstallation, in view: ComposerTextView) {
                guard valid, self.view === view, owner == transaction.owner,
                    installationRevision == transaction.revision
                else { return }
                if transaction.replacementRevokesInput, let replacement = transaction.replacement {
                    // A new owner/account or changed authoritative value wins.
                    // Do not flush this transaction through either replacement.
                    clearRequests()
                    apply(replacement, in: view)
                    return
                }
                guard parent.isCurrentOwner(), installationRevision == transaction.revision else { return }
                if transaction.receivedNativeEdit {
                    // A nested render of the unchanged committed baseline predates
                    // this publication. Its cached value cannot erase exact new input;
                    // the binding write supplies the next ordinary SwiftUI render.
                    publish(completedInput: true)
                } else if let replacement = transaction.replacement {
                    apply(replacement, in: view)
                } else {
                    schedulePublication()
                }
            }

            func recordNativeReplacement(before: String, range: NSRange, replacement: String) {
                guard installing else {
                    recordReplacement(before: before, range: range, replacement: replacement)
                    return
                }
                guard valid, let transaction = submitInstallation, transaction.acceptsNativeEdits,
                    owner == transaction.owner, installationRevision == transaction.revision
                else { return }
                let request = DictationTextEdit(beforeText: before, rangeUTF16: range, replacement: replacement)
                transaction.receivedNativeEdit = true
                if requests.last == request, request.resultingText == transaction.expectedNativeText { return }
                guard before == transaction.expectedNativeText, let result = request.resultingText else {
                    // Preserve the existing explicit unknown-edit boundary. No
                    // character diff is used to recover a dictation marker.
                    requests.removeAll(); requestBytes = 0; requestOverflow = true
                    return
                }
                transaction.expectedNativeText = result
                append(request)
            }

            func recordReplacement(before: String, range: NSRange, replacement: String) {
                guard valid, !installing, !requestOverflow else { return }
                let request = DictationTextEdit(beforeText: before, rangeUTF16: range, replacement: replacement)
                append(request)
            }

            private func append(_ request: DictationTextEdit) {
                guard !requestOverflow, requests.last != request else { return }
                let bytes = request.beforeText.utf8.count + request.replacement.utf8.count
                guard requests.count < 64, bytes <= 8 * 1024 * 1024 - requestBytes else {
                    requests.removeAll(); requestBytes = 0; requestOverflow = true
                    return
                }
                requests.append(request); requestBytes += bytes
            }

            func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String)
                -> Bool
            {
                recordReplacement(before: textView.text ?? "", range: range, replacement: text)
                #if DEBUG
                    observeInputOrder("proposed")
                #endif
                return true
            }

            private func clearRequests() {
                requests.removeAll(); requestBytes = 0; requestOverflow = false
            }

            private func exactChanges(endingAt text: String) -> [DictationTextEdit]? {
                if !requestOverflow {
                    // Prefer the innermost verified transitions when an input method
                    // calls another public input method or its delegate recursively.
                    var target = text
                    var reversed: [DictationTextEdit] = []
                    for request in requests.reversed() where request.resultingText == target {
                        reversed.append(request); target = request.beforeText
                    }
                    if target == reportedText, !reversed.isEmpty { return Array(reversed.reversed()) }
                    if requests.isEmpty, text == reportedText { return [] }
                }
                return nil
            }

            private func unknownChange() -> [DictationTextEdit] {
                // A storage notification is an aggregate, not edit intent. Never
                // infer a marker from a diff or an attribute-expanded storage span.
                [
                    DictationTextEdit(
                        beforeText: reportedText,
                        rangeUTF16: NSRange(location: NSNotFound, length: 0), replacement: "")
                ]
            }

            func textViewDidChange(_ textView: UITextView) {
                #if DEBUG
                    observeInputOrder("didChange")
                #endif
                publish(completedInput: true)
            }
            func textViewDidChangeSelection(_ textView: UITextView) {
                #if DEBUG
                    observeInputOrder("selection")
                #endif
                publish()
            }

            private func admitsNativeFocusCallback(_ textView: UITextView, firstResponder: Bool) -> Bool {
                let revision = installationRevision
                let destinationOwner = owner
                let actionRevision = view?.returnActionRevision
                let window = textView.window
                func matches() -> Bool {
                    valid && !installing && view === textView && window != nil && textView.window === window
                        && owner == destinationOwner && parent.ownerID == destinationOwner?.draft
                        && parent.accountGeneration == destinationOwner?.generation
                        && installationRevision == revision && view?.returnActionRevision == actionRevision
                        && textView.isFirstResponder == firstResponder
                }
                return matches() && parent.isCurrentOwner() && matches()
                    && (!firstResponder || (parent.isCurrentPresentation() && matches()))
            }

            func textViewShouldBeginEditing(_ textView: UITextView) -> Bool {
                let revision = installationRevision
                let destinationOwner = owner
                let actionRevision = view?.returnActionRevision
                let window = textView.window
                func matches() -> Bool {
                    guard valid, view === textView, let window, textView.window === window,
                        owner == destinationOwner, parent.ownerID == destinationOwner?.draft,
                        parent.accountGeneration == destinationOwner?.generation,
                        installationRevision == revision, view?.returnActionRevision == actionRevision,
                        submitInstallation?.replacementRevokesInput != true
                    else { return false }
                    if parent.focused { return true }
                    // An admitted native touch may write SwiftUI's focus binding
                    // before its cached getter sees that write. Only the exact
                    // synchronous acquisition transaction may bridge that render.
                    guard let acquisition = nativeFocusAcquisition else { return false }
                    return acquisition.owner == destinationOwner && acquisition.installation == revision
                        && acquisition.action == actionRevision && acquisition.window === window
                }
                let admitted =
                    matches() && parent.isCurrentOwner() && matches()
                    && parent.isCurrentPresentation() && matches()
                #if DEBUG
                    traceFocus("native.shouldBegin=\(admitted)")
                #endif
                return admitted
            }

            func textViewDidBeginEditing(_ textView: UITextView) {
                #if DEBUG
                    traceFocus("native.didBegin.before")
                #endif
                // UIKit can deliver a queued Begin after navigation has already
                // resigned this exact view. Only its current native focus may
                // publish editing intent and reopen keyboard presentation.
                if admitsNativeFocusCallback(textView, firstResponder: true) { parent.focused = true }
                #if DEBUG
                    traceFocus("native.didBegin.after")
                #endif
                schedulePublication()
            }
            func textViewDidEndEditing(_ textView: UITextView) {
                #if DEBUG
                    traceFocus("native.didEnd.before")
                #endif
                if (textView as? ComposerTextView)?.hardwareReturnInProgress != true,
                    admitsNativeFocusCallback(textView, firstResponder: false)
                {
                    parent.focused = false
                }
                #if DEBUG
                    traceFocus("native.didEnd.after")
                #endif
                schedulePublication()
            }

            private func schedulePublication() {
                guard valid, !publicationScheduled else { return }
                publicationScheduled = true
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.publicationScheduled = false
                    self.publish()
                }
            }

            func publish(completedInput: Bool = false) {
                #if DEBUG
                    observeInputOrder(completedInput ? "complete" : "observe")
                #endif
                guard valid, !installing, let view, !view.performingInputOperation,
                    !view.textStorage.editedMask.contains(.editedCharacters)
                else {
                    #if DEBUG
                        observeInputOrder("publish.guard-return")
                    #endif
                    return
                }
                let text = view.text ?? ""
                let selection = view.selectedRange
                let marked = view.markedTextRange.map {
                    NSRange(
                        location: view.offset(from: view.beginningOfDocument, to: $0.start),
                        length: view.offset(from: $0.start, to: $0.end))
                }
                let changes: [DictationTextEdit]
                if text == reportedText, !requests.isEmpty || requestOverflow {
                    if completedInput {
                        // An owned input operation has returned, or UIKit delivered
                        // didChange. A same-text exact replacement still carries edit
                        // intent; a proposal that never changed storage is rejected.
                        changes = requestOverflow ? unknownChange() : exactChanges(endingAt: text) ?? []
                        clearRequests()
                    } else {
                        // UIKit may deliver a selection callback after shouldChange
                        // but before applying its proposed replacement. Publish the
                        // caret/composition observation without consuming that proposal.
                        changes = []
                    }
                } else {
                    changes = exactChanges(endingAt: text) ?? unknownChange()
                    clearRequests()
                }
                reportedText = text
                #if DEBUG
                    observeInputOrder(
                        "publish.resolved; changes=\(changes.count); unknown=\(changes.filter { $0.resultingText == nil }.count)"
                    )
                #endif
                let state = DictationEditorState(text: text, selectionUTF16: selection, markedRangeUTF16: marked)
                view.placeholderLabel.isHidden = !text.isEmpty
                guard state != lastPublished || !changes.isEmpty else {
                    #if DEBUG
                        observeInputOrder("publish.unchanged")
                    #endif
                    return
                }
                lastPublished = state
                // A binding mutation can synchronously trigger a SwiftUI owner update.
                // Keep this event attached to the callbacks that owned its native text.
                let destination = parent
                #if DEBUG
                    observeInputOrder("publish.before-binding; destinationBinding=\(destination.text.utf16.count)")
                #endif
                if destination.text != text { destination.text = text }
                #if DEBUG
                    observeInputOrder("publish.after-binding; destinationBinding=\(destination.text.utf16.count)")
                #endif
                destination.onChange(state, changes)
                #if DEBUG
                    observeInputOrder("publish.after-callback; destinationBinding=\(destination.text.utf16.count)")
                #endif
                view.invalidateIntrinsicContentSize()
            }

            /// A measured native resignation boundary, not a delayed callback
            /// or an inferred relationship between press and text-input events.
            fileprivate func hardwareReturn(shifted: Bool) {
                guard valid, !installing, let view, let window = view.window,
                    view.isFirstResponder, view.markedTextRange == nil, !view.performingInputOperation
                else { return }
                let destinationOwner = owner
                let revision = installationRevision
                let actionRevision = view.returnActionRevision
                func admitted() -> Bool {
                    guard valid, self.view === view, view.window === window, owner == destinationOwner,
                        installationRevision == revision, view.returnActionRevision == actionRevision,
                        !installing, !view.performingInputOperation, view.markedTextRange == nil,
                        !view.textStorage.editedMask.contains(.editedCharacters)
                    else { return false }
                    let current = parent.isCurrentOwner()
                    return current && valid && self.view === view && view.window === window
                        && owner == destinationOwner && installationRevision == revision
                        && view.returnActionRevision == actionRevision && !installing
                        && view.markedTextRange == nil && !view.performingInputOperation
                        && !view.textStorage.editedMask.contains(.editedCharacters)
                }
                guard admitted() else { return }
                defer {
                    // A refused/cancelled barrier must not leave a stale focus
                    // binding. A replacement installation owns its own focus.
                    if valid, self.view === view, owner == destinationOwner, installationRevision == revision,
                        parent.isCurrentOwner(), installationRevision == revision,
                        parent.focused != view.isFirstResponder
                    {
                        parent.focused = view.isFirstResponder
                    }
                }
                #if DEBUG
                    observeInputOrder("barrier.begin")
                #endif
                let resigned: Bool = {
                    hardwareResignation = HardwareResignation(
                        owner: destinationOwner, installation: revision, action: actionRevision, window: window)
                    defer { hardwareResignation = nil }
                    return view.resignForHardwareReturn()
                }()
                #if DEBUG
                    observeInputOrder("barrier.end; resigned=\(resigned)")
                #endif
                if installationRevision != revision {
                    // An authoritative replacement can install during UIKit's
                    // resignation callbacks. The outer native resignation may
                    // finish afterward; only that replacement can reclaim focus.
                    reconcileReplacementFocus(in: view, window: window, generation: destinationOwner?.generation)
                    return
                }
                guard resigned, !view.isFirstResponder, admitted() else { return }
                publish(completedInput: true)
                guard !view.isFirstResponder, admitted(), reportedText == view.text else { return }
                if shifted {
                    // Commit the newline before restoring the input target.
                    // Later received characters therefore follow this exact edit.
                    view.insertText("\n")
                    guard !view.isFirstResponder, admitted() else { return }
                    apply(parent, in: view, preservingPostSubmitInput: true, restoringHardwareFocus: true)
                } else {
                    submit(afterHardwareResignation: true)
                }
            }

            private func reconcileReplacementFocus(in view: ComposerTextView, window: UIWindow, generation: UUID?) {
                guard let replacementOwner = owner, replacementOwner.generation == generation else { return }
                let revision = installationRevision
                let actionRevision = view.returnActionRevision
                func matches() -> Bool {
                    valid && self.view === view && view.window === window && owner == replacementOwner
                        && installationRevision == revision && view.returnActionRevision == actionRevision
                        && parent.ownerID == replacementOwner.draft
                        && parent.accountGeneration == replacementOwner.generation && parent.focused
                        && !view.isFirstResponder && !installing && !view.performingInputOperation
                        && view.markedTextRange == nil && !view.textStorage.editedMask.contains(.editedCharacters)
                }
                func admitted() -> Bool {
                    guard matches() else { return false }
                    let current = parent.isCurrentOwner()
                    return current && matches()
                }
                guard admitted() else { return }
                // Publish any exact input received by the replacement before
                // touching focus. A stale binding must not erase native text.
                publish()
                guard admitted(), parent.text == view.text, reportedText == view.text, matches() else { return }
                // The existing transaction captures input delivered by focus
                // restoration under the replacement's callbacks and baseline.
                apply(parent, in: view, preservingPostSubmitInput: true)
            }

            func submit() { submit(afterHardwareResignation: false) }

            private func submit(afterHardwareResignation: Bool) {
                #if DEBUG
                    observeInputOrder("submit.entry")
                #endif
                guard valid, let view, view.markedTextRange == nil else { return }
                let destination = parent
                let destinationOwner = owner
                let actionRevision = view.returnActionRevision
                let publicationRevision = installationRevision
                let destinationWindow = view.window
                publish()
                let stillCurrent = destination.isCurrentOwner()
                guard stillCurrent, valid, owner == destinationOwner, self.view === view,
                    view.window === destinationWindow, installationRevision == publicationRevision,
                    view.returnActionRevision == actionRevision,
                    !installing, (afterHardwareResignation ? !view.isFirstResponder : view.isFirstResponder),
                    view.markedTextRange == nil, !view.performingInputOperation
                else { return }
                #if DEBUG
                    observeInputOrder("submit.after-publish; destinationBinding=\(destination.text.utf16.count)")
                #endif
                let submittedInstallation = installationRevision
                destination.onSubmit()
                #if DEBUG
                    observeInputOrder("submit.after-callback; destinationBinding=\(destination.text.utf16.count)")
                #endif
                // Core may synchronously detach the sent draft while SwiftUI's
                // view update is still pending. Install its authoritative successor
                // now, so a queued native insertion cannot republish the sent text
                // or reach the retired draft's callbacks.
                guard valid, owner == destinationOwner, self.view === view, view.window === destinationWindow,
                    installationRevision == submittedInstallation, view.returnActionRevision == actionRevision,
                    !installing, !view.performingInputOperation, view.markedTextRange == nil
                else { return }
                let next: ComposerEditor
                if let refresh = destination.refreshAfterSubmit {
                    guard let refreshed = refresh() else {
                        if valid, owner == destinationOwner, self.view === view,
                            installationRevision == submittedInstallation
                        {
                            view.cancelHardwareReturn()
                        }
                        return
                    }
                    next = refreshed
                } else {
                    next = parent
                }
                guard valid, owner == destinationOwner, self.view === view, view.window === destinationWindow,
                    installationRevision == submittedInstallation, view.returnActionRevision == actionRevision
                else { return }
                guard next.accountGeneration == destinationOwner?.generation else {
                    view.cancelHardwareReturn()
                    return
                }
                // Accepted/reserved Send requests dismissal through its focus
                // binding. Retain hardware input with a custom empty input view;
                // failed Send and Shift do not introduce that presentation.
                let dismissSoftwareKeyboard = afterHardwareResignation && !next.focused
                apply(
                    next, in: view, preservingPostSubmitInput: true,
                    restoringHardwareFocus: afterHardwareResignation,
                    suppressSoftwareKeyboard: dismissSoftwareKeyboard ? true : nil)
            }

            @discardableResult fileprivate func showSoftwareKeyboard(in view: ComposerTextView) -> Bool {
                #if DEBUG
                    traceFocus("interaction.begin")
                #endif
                view.cancelHardwareReturn()
                var revision = installationRevision
                let destinationOwner = owner
                let actionRevision = view.returnActionRevision
                let window = view.window
                func matches() -> Bool {
                    valid && self.view === view && view.window === window && window != nil
                        && owner == destinationOwner && installationRevision == revision
                        && view.returnActionRevision == actionRevision
                        && !installing && !view.performingInputOperation && !view.hardwareReturnInProgress
                        && view.markedTextRange == nil && !view.textStorage.editedMask.contains(.editedCharacters)
                }
                func refreshAfterInteraction() -> Bool {
                    // Rendering an admitted focus/presentation change may
                    // synchronously refresh this same editor. Its new callbacks
                    // can continue only with the exact installed owner/baseline.
                    let nextRevision = installationRevision
                    func sameInput() -> Bool {
                        valid && self.view === view && view.window === window && window != nil
                            && owner == destinationOwner && parent.ownerID == destinationOwner?.draft
                            && parent.accountGeneration == destinationOwner?.generation
                            && installationRevision == nextRevision && view.returnActionRevision == actionRevision
                            && !installing && !view.performingInputOperation && !view.hardwareReturnInProgress
                            && view.markedTextRange == nil && !view.textStorage.editedMask.contains(.editedCharacters)
                            && parent.text == view.text && reportedText == view.text
                    }
                    guard sameInput(), parent.isCurrentOwner(), sameInput(), parent.isCurrentPresentation(), sameInput()
                    else { return false }
                    revision = nextRevision
                    return true
                }
                guard matches(), parent.isCurrentOwner(), matches(), parent.isCurrentPresentation(), matches()
                else { return false }
                publish()
                guard matches(), parent.isCurrentOwner(), matches(), parent.isCurrentPresentation(), matches(),
                    parent.text == view.text,
                    reportedText == view.text
                else { return false }
                // A keyboard hide can retain the native first responder while
                // SwiftUI collapses this existing-chat editor. An explicit tap
                // begins editing again even when custom hardware suppression
                // was never installed. Presentation admission can revoke it.
                let reloadsPresentation =
                    view.isCompact && !view.softwareKeyboardSuppressed && view.isFirstResponder
                let interactionAdmitted = parent.onEditingInteraction()
                #if DEBUG
                    traceFocus("interaction.admitted=\(interactionAdmitted)")
                #endif
                guard interactionAdmitted, refreshAfterInteraction() else { return false }
                if !parent.focused { parent.focused = true }
                #if DEBUG
                    traceFocus("interaction.focusWritten")
                #endif
                guard refreshAfterInteraction() else { return false }
                // Keep reload callbacks in the same exact-input transaction used
                // for successor focus, without committing marked text.
                apply(
                    parent, in: view, preservingPostSubmitInput: true, suppressSoftwareKeyboard: false,
                    reloadSoftwareKeyboard: reloadsPresentation, restoringInteractionFocus: true)
                return view.isFirstResponder && refreshAfterInteraction()
            }
        }
    }

    @MainActor final class ComposerTextView: UITextView {
        let placeholderLabel = UILabel()
        var isCompact = false {
            didSet {
                if isCompact != oldValue {
                    updateKeyboardRestorationGesture()
                    updateVerticalAlignment()
                    setNeedsLayout()
                    #if DEBUG
                        (delegate as? ComposerEditor.Coordinator)?.traceFocus("compact.changed")
                    #endif
                }
            }
        }
        private var placeholderTopConstraint: NSLayoutConstraint?
        var changed: (() -> Void)?
        var submit: (() -> Void)?
        var willReplace: ((String, NSRange, String) -> Void)?
        private var operationDepth = 0
        var performingInputOperation: Bool { operationDepth > 0 }
        private var returnCommandIdentity = UUID()
        private(set) var hardwareReturnInProgress = false
        private var expectedHardwareResignation = false
        private(set) var returnActionRevision = UUID()

        private lazy var suppressedInputView: UIView = {
            let view = UIView(frame: .zero)
            view.autoresizingMask = [.flexibleWidth]
            view.isUserInteractionEnabled = false
            view.accessibilityElementsHidden = true
            return view
        }()
        private(set) var softwareKeyboardSuppressed = false

        private final class KeyboardRestorationDelegate: NSObject, UIGestureRecognizerDelegate {
            weak var view: ComposerTextView?
            func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
                guard let view else { return false }
                #if DEBUG
                    (view.delegate as? ComposerEditor.Coordinator)?.traceFocus("tap.shouldBegin")
                #endif
                guard gestureRecognizer === view.keyboardRestorationTap, view.needsEditingInteraction else {
                    return false
                }
                // Admit this real touch before UIKit requests native focus. A
                // retained NavigationStack can otherwise restore its old text
                // responder without a touch even after the app dismissed it.
                return view.showSoftwareKeyboardForInteraction()
            }
            func gestureRecognizer(
                _ gestureRecognizer: UIGestureRecognizer,
                shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
            ) -> Bool {
                guard let view else { return false }
                return gestureRecognizer === view.keyboardRestorationTap
                    || otherGestureRecognizer === view.keyboardRestorationTap
            }
        }
        private let keyboardRestorationDelegate = KeyboardRestorationDelegate()
        private lazy var keyboardRestorationTap = UITapGestureRecognizer(
            target: self, action: #selector(restoreKeyboardFromTap(_:)))

        @objc private func restoreKeyboardFromTap(_ recognizer: UITapGestureRecognizer) {
            guard recognizer === keyboardRestorationTap, recognizer.view === self,
                recognizer.state == .ended
            else { return }
            #if DEBUG
                (delegate as? ComposerEditor.Coordinator)?.traceFocus("tap.ended")
            #endif
        }

        fileprivate func removeKeyboardRestorationGesture() {
            keyboardRestorationTap.isEnabled = false
            keyboardRestorationTap.delegate = nil
            keyboardRestorationTap.removeTarget(self, action: #selector(restoreKeyboardFromTap(_:)))
            removeGestureRecognizer(keyboardRestorationTap)
            keyboardRestorationDelegate.view = nil
        }

        @discardableResult fileprivate func setSoftwareKeyboardSuppressed(_ suppressed: Bool) -> Bool {
            guard softwareKeyboardSuppressed != suppressed else { return false }
            softwareKeyboardSuppressed = suppressed
            updateKeyboardRestorationGesture()
            inputView = suppressed ? suppressedInputView : nil
            if isFirstResponder { reloadInputViews() }
            return true
        }

        fileprivate var needsEditingInteraction: Bool {
            softwareKeyboardSuppressed || isCompact
                || (delegate as? ComposerEditor.Coordinator)?.parent.focused == false
        }

        fileprivate func updateKeyboardRestorationGesture() {
            keyboardRestorationTap.isEnabled = needsEditingInteraction
        }

        @discardableResult func showSoftwareKeyboardForInteraction() -> Bool {
            guard needsEditingInteraction else { return false }
            return (delegate as? ComposerEditor.Coordinator)?.showSoftwareKeyboard(in: self) ?? false
        }

        override func accessibilityActivate() -> Bool {
            showSoftwareKeyboardForInteraction()
            return super.accessibilityActivate()
        }

        func cancelHardwareReturn() {
            #if DEBUG
                observeInputOperation("command.cancel; hadAction=\(hardwareReturnInProgress)")
            #endif
            returnActionRevision = UUID()
        }

        func invalidateReturnCommands() {
            cancelHardwareReturn()
            returnCommandIdentity = UUID()
        }

        private var acceptsReturnCommand: Bool {
            guard let coordinator = delegate as? ComposerEditor.Coordinator, coordinator.valid, !coordinator.installing
            else {
                return false
            }
            return !hardwareReturnInProgress && submit != nil && isEditable && isFirstResponder
                && markedTextRange == nil
                && !performingInputOperation
                && !textStorage.editedMask.contains(.editedCharacters)
        }

        override var keyCommands: [UIKeyCommand]? {
            let inherited = super.keyCommands ?? []
            guard acceptsReturnCommand else { return inherited }
            let commands = ["\r", "\u{3}"].flatMap { input in
                [UIKeyModifierFlags(), .shift].map { modifiers in
                    let command = UIKeyCommand(
                        title: "", action: #selector(handleComposerReturn(_:)), input: input,
                        modifierFlags: modifiers, propertyList: returnCommandIdentity.uuidString)
                    // Keep Return and keypad Enter on the same ownership fence.
                    // Software text input never invokes this hardware command action.
                    command.wantsPriorityOverSystemBehavior = true
                    return command
                }
            }
            return commands + inherited
        }

        override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
            if action == #selector(handleComposerReturn(_:)) {
                guard acceptsReturnCommand else { return false }
                guard let sender else { return true }
                guard let command = sender as? UIKeyCommand else { return false }
                return ownsReturnCommand(command)
            }
            return super.canPerformAction(action, withSender: sender)
        }

        private func ownsReturnCommand(_ command: UIKeyCommand) -> Bool {
            command.action == #selector(handleComposerReturn(_:))
                && (command.input == "\r" || command.input == "\u{3}")
                && (command.propertyList as? String) == returnCommandIdentity.uuidString
                && (command.modifierFlags.isEmpty || command.modifierFlags == .shift)
        }

        @objc func handleComposerReturn(_ sender: Any?) {
            let command = sender as? UIKeyCommand
            #if DEBUG
                let kind =
                    command.map {
                        $0.modifierFlags == .shift ? "newline" : $0.modifierFlags.isEmpty ? "submit" : "invalid"
                    } ?? "invalid"
                observeInputOperation(
                    "command.entry; kind=\(kind); modifierMask=\(command?.modifierFlags.rawValue ?? 0); "
                        + "ownerAccepted=\(command.map { ownsReturnCommand($0) } ?? false); eligible=\(acceptsReturnCommand)"
                )
            #endif
            guard acceptsReturnCommand, let command, ownsReturnCommand(command) else {
                return
            }
            guard let coordinator = delegate as? ComposerEditor.Coordinator else { return }
            hardwareReturnInProgress = true
            defer {
                hardwareReturnInProgress = false
                #if DEBUG
                    observeInputOperation("command.exit")
                #endif
            }
            coordinator.hardwareReturn(shifted: command.modifierFlags == .shift)
        }

        fileprivate func resignForHardwareReturn() -> Bool {
            guard hardwareReturnInProgress, !expectedHardwareResignation else { return false }
            expectedHardwareResignation = true
            defer { expectedHardwareResignation = false }
            return resignFirstResponder()
        }

        override func paste(_ sender: Any?) {
            cancelHardwareReturn()
            super.paste(sender)
        }
        override func resignFirstResponder() -> Bool {
            #if DEBUG
                (delegate as? ComposerEditor.Coordinator)?.traceFocus("native.resign")
            #endif
            let expected = expectedHardwareResignation
            expectedHardwareResignation = false
            let revision = returnActionRevision
            if !expected { cancelHardwareReturn() }
            #if DEBUG
                beforeResignFirstResponderForProbe?()
                if refuseResignationForProbe { return false }
            #endif
            if expected, (returnActionRevision != revision || markedTextRange != nil) { return false }
            return super.resignFirstResponder()
        }
        #if DEBUG
            var beforeResignFirstResponderForProbe: (() -> Void)?
            var beforeBecomeFirstResponderForProbe: (() -> Void)?
            var refuseResignationForProbe = false
            override func becomeFirstResponder() -> Bool {
                (delegate as? ComposerEditor.Coordinator)?.traceFocus("native.become")
                beforeBecomeFirstResponderForProbe?()
                return super.becomeFirstResponder()
            }
            var beforeReloadInputViewsForProbe: (() -> Void)?
            override func reloadInputViews() {
                beforeReloadInputViewsForProbe?()
                super.reloadInputViews()
            }
        #endif
        #if DEBUG
            var inputOrderObserver: ((String) -> Void)?
            private func observeInputOperation(_ event: String, inputLength: Int = 0) {
                inputOrderObserver?("\(event); inputLength=\(inputLength); depth=\(operationDepth)")
            }
            private func observePresses(_ phase: String, _ presses: Set<UIPress>) {
                guard let inputOrderObserver else { return }
                // Never record character keys or their characters/scalars. Only
                // the two submit/newline HID usages and numeric modifier masks.
                let keys = presses.compactMap(\.key)
                let returns = keys.filter { $0.keyCode == .keyboardReturnOrEnter || $0.keyCode == .keypadEnter }
                let codes = returns.prefix(16).map { "\($0.keyCode.rawValue):\($0.modifierFlags.rawValue)" }
                    .sorted().joined(separator: "|")
                let modifiers = Set(keys.map { $0.modifierFlags.rawValue }).sorted().prefix(16)
                    .map { String($0) }.joined(separator: "|")
                inputOrderObserver(
                    "press.\(phase); count=\(presses.count); returnCount=\(returns.count); "
                        + "returnHID=\(codes); modifierMasks=\(modifiers); depth=\(operationDepth)")
            }
            override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
                observePresses("ended", presses)
                super.pressesEnded(presses, with: event)
            }
            override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
                observePresses("cancelled", presses)
                super.pressesCancelled(presses, with: event)
            }
        #endif

        override init(frame: CGRect, textContainer: NSTextContainer?) {
            super.init(frame: frame, textContainer: textContainer)
            // Observe an explicit tap alongside UIKit's text-selection gestures.
            // Do not cancel/delay their touches or replace their delegates.
            keyboardRestorationDelegate.view = self
            keyboardRestorationTap.delegate = keyboardRestorationDelegate
            keyboardRestorationTap.cancelsTouchesInView = false
            keyboardRestorationTap.delaysTouchesBegan = false
            keyboardRestorationTap.delaysTouchesEnded = false
            keyboardRestorationTap.isEnabled = false
            addGestureRecognizer(keyboardRestorationTap)
            placeholderLabel.font = .preferredFont(forTextStyle: .body)
            placeholderLabel.adjustsFontForContentSizeCategory = true
            placeholderLabel.textColor = .placeholderText
            placeholderLabel.isAccessibilityElement = false
            placeholderLabel.translatesAutoresizingMaskIntoConstraints = false
            addSubview(placeholderLabel)
            let placeholderTop = placeholderLabel.topAnchor.constraint(equalTo: topAnchor)
            placeholderTopConstraint = placeholderTop
            NSLayoutConstraint.activate([
                placeholderLabel.leadingAnchor.constraint(
                    equalTo: leadingAnchor, constant: ComposerEditorMetrics.horizontalInset),
                placeholderTop,
                placeholderLabel.trailingAnchor.constraint(
                    lessThanOrEqualTo: trailingAnchor, constant: -ComposerEditorMetrics.horizontalInset),
            ])
        }
        required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

        override func layoutSubviews() {
            updateVerticalAlignment()
            super.layoutSubviews()
        }

        private func updateVerticalAlignment() {
            // The collapsed editor keeps its 44-point tap surface, with its
            // single native line and placeholder centered beside the controls.
            // Expanded drafts keep UITextView's normal top-aligned layout.
            let lineHeight = font?.lineHeight ?? placeholderLabel.font.lineHeight
            let inset = ComposerEditorMetrics.topInset(
                height: bounds.height, lineHeight: lineHeight, compact: isCompact)
            let insets = UIEdgeInsets(
                top: inset, left: ComposerEditorMetrics.horizontalInset,
                bottom: inset, right: ComposerEditorMetrics.horizontalInset)
            if textContainerInset != insets { textContainerInset = insets }
            placeholderTopConstraint?.constant = ComposerEditorMetrics.topInset(
                height: bounds.height, lineHeight: placeholderLabel.intrinsicContentSize.height, compact: isCompact)
        }

        override func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
            cancelHardwareReturn()
            #if DEBUG
                observeInputOperation("mark.begin", inputLength: markedText?.utf16.count ?? 0)
            #endif
            if let markedText { captureReplacement(range: markedReplacementRange, text: markedText) }
            operationDepth += 1
            super.setMarkedText(markedText, selectedRange: selectedRange)
            operationDepth -= 1
            #if DEBUG
                observeInputOperation("mark.end", inputLength: markedText?.utf16.count ?? 0)
            #endif
            if operationDepth == 0 { changed?() }
        }
        override func unmarkText() {
            #if DEBUG
                observeInputOperation("unmark.begin")
            #endif
            operationDepth += 1
            super.unmarkText()
            operationDepth -= 1
            #if DEBUG
                observeInputOperation("unmark.end")
            #endif
            if operationDepth == 0 { changed?() }
        }
        override func replace(_ range: UITextRange, withText text: String) {
            #if DEBUG
                observeInputOperation("replace.begin", inputLength: text.utf16.count)
            #endif
            captureReplacement(
                range: NSRange(
                    location: offset(from: beginningOfDocument, to: range.start),
                    length: offset(from: range.start, to: range.end)), text: text)
            operationDepth += 1
            super.replace(range, withText: text)
            operationDepth -= 1
            #if DEBUG
                observeInputOperation("replace.end", inputLength: text.utf16.count)
            #endif
            if operationDepth == 0 { changed?() }
        }
        override func insertText(_ text: String) {
            #if DEBUG
                observeInputOperation("insert.begin", inputLength: text.utf16.count)
            #endif
            captureReplacement(range: markedReplacementRange, text: text)
            operationDepth += 1
            super.insertText(text)
            operationDepth -= 1
            #if DEBUG
                observeInputOperation("insert.end", inputLength: text.utf16.count)
            #endif
            if operationDepth == 0 { changed?() }
        }
        override func deleteBackward() {
            #if DEBUG
                observeInputOperation("delete.begin")
            #endif
            var range = selectedRange
            if range.length == 0, range.location > 0, range.location <= (text ?? "").utf16.count {
                range = ((text ?? "") as NSString).rangeOfComposedCharacterSequence(at: range.location - 1)
            }
            captureReplacement(range: range, text: "")
            operationDepth += 1
            super.deleteBackward()
            operationDepth -= 1
            #if DEBUG
                observeInputOperation("delete.end")
            #endif
            if operationDepth == 0 { changed?() }
        }

        private var markedReplacementRange: NSRange {
            guard let markedTextRange else { return selectedRange }
            return NSRange(
                location: offset(from: beginningOfDocument, to: markedTextRange.start),
                length: offset(from: markedTextRange.start, to: markedTextRange.end))
        }
        private func captureReplacement(range: NSRange, text replacement: String) {
            willReplace?(text ?? "", range, replacement)
        }

        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            #if DEBUG
                observePresses("began", presses)
            #endif
            // Let UIKit sequence text input and key-command dispatch. Submitting
            // from raw press delivery can overtake an earlier text insertion.
            super.pressesBegan(presses, with: event)
        }
    }

    /// NSTextStorage is not actor-isolated. This observer moves only a notification
    /// onto the main actor; all text, ranges and UI state stay in their UIKit owner.
    final class ComposerStorageObserver: NSObject, NSTextStorageDelegate {
        let processed: @Sendable () -> Void
        init(processed: @escaping @Sendable () -> Void) { self.processed = processed }
        func textStorage(
            _ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorage.EditActions,
            range editedRange: NSRange, changeInLength delta: Int
        ) {
            if editedMask.contains(.editedCharacters) { processed() }
        }
    }
#endif
