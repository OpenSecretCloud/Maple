#if os(iOS) && DEBUG
    import SwiftUI
    import UIKit
    import MapleAppCore
    import UniformTypeIdentifiers

    // Records callback delivery without mutating SwiftUI State while UIKit is
    // synchronously finalizing an old owner from updateUIView. The test explicitly
    // reads this content-free ledger after the view update has returned.
    @MainActor private final class ComposerFixtureOwnerEvents {
        var latest: [UUID: Bool] = [:]
        var counts: [UUID: Int] = [:]
        func record(owner: UUID, marked: Bool) {
            latest[owner] = marked
            counts[owner, default: 0] += 1
        }
    }

    @MainActor private final class ComposerFixtureInputEvents {
        var entries: [String] = []
        private(set) var dropped = 0
        func reset() {
            entries.removeAll()
            dropped = 0
        }
        func record(_ event: String) {
            guard entries.count < 256 else {
                if dropped < Int.max { dropped += 1 }
                return
            }
            entries.append("\(entries.count + 1): \(event.prefix(1024))")
        }
        var snapshot: String {
            "Composer input diagnostics v1; recorded=\(entries.count); dropped=\(dropped)\n"
                + entries.joined(separator: "\n")
        }
    }

    /// Compares the documented SwiftUI hardware-key path before adopting any
    /// shipping UIKit composer integration. All content is synthetic and in memory.
    struct ComposerKeyboardFixtureView: View {
        @State private var text = ""
        @State private var sends = 0
        @State private var marked = false
        @State private var nativeMarkStatus = "Native marked 0"
        @State private var nativeText = ""
        @State private var keyEvents = 0
        @State private var returnEvents = 0
        @State private var ownerID = UUID()
        @State private var nativeFocused = true
        @State private var editSummary = "Edits 0; invalid 0"
        @State private var editCount = 0
        @State private var invalidEdits = 0
        @State private var previousOwner: UUID?
        @State private var ownerEvents = ComposerFixtureOwnerEvents()
        @State private var previousOwnerStatus = "No committed owner"
        @State private var insertionMarker: DictationInsertionAnchor?
        @State private var transactionSummary = ""
        @State private var reentrantStatus = "Not checked"
        @State private var orderingStatus = "Not checked"
        @State private var inputEvents = ComposerFixtureInputEvents()
        @State private var inputOrderStatus = "Not recording"
        @State private var presentationSetup = "Not prepared"
        @FocusState private var focused: Bool
        private var owned: Bool { ProcessInfo.processInfo.arguments.contains("--maple-owned-composer") }
        var body: some View {
            if ProcessInfo.processInfo.arguments.contains("--maple-composer-order-check") {
                VStack {
                    Button("Check input ordering") { orderingStatus = ComposerEditOrderingProbe.run() }
                        .accessibilityIdentifier("composer.order-check")
                    Text(orderingStatus).font(.caption2).lineLimit(12)
                        .accessibilityLabel(orderingStatus).accessibilityIdentifier("composer.order-status")
                }.padding()
            } else if ProcessInfo.processInfo.arguments.contains("--maple-return-resign-control") {
                ComposerReturnResignFixtureView()
            } else if ProcessInfo.processInfo.arguments.contains("--maple-native-keyboard-control") {
                ComposerStandaloneNativeControl()
            } else {
                mixedFixture
            }
        }
        private var mixedFixture: some View {
            let renderedOwner = ownerID
            return VStack(alignment: .leading, spacing: 16) {
                Text("SwiftUI keyboard probe").font(.title2)
                Text("Sends \(sends)").accessibilityIdentifier("composer.sends")
                Text("Newlines \(text.filter { $0 == "\n" }.count)").accessibilityIdentifier("composer.newlines")
                Text("Marked \(marked ? 1 : 0)").accessibilityIdentifier("composer.marked")
                if owned && ProcessInfo.processInfo.arguments.contains("--maple-composer-presentation-check") {
                    Button("Send with owned Return command") {
                        guard let view = Self.fixtureInput() as? ComposerTextView,
                            let command = view.keyCommands?.first(where: {
                                $0.action == #selector(ComposerTextView.handleComposerReturn(_:))
                                    && $0.input == "\r" && $0.modifierFlags.isEmpty
                            }), let action = command.action,
                            view.canPerformAction(action, withSender: command)
                        else { presentationSetup = "Command unavailable"; return }
                        // Controlled command dispatch sets up the presentation test.
                        // Actual hardware receipt remains a separate runtime proof.
                        _ = view.perform(action, with: command)
                        presentationSetup = "Owned command dispatched"
                    }.accessibilityIdentifier("composer.presentation-send")
                    Text(presentationSetup).accessibilityIdentifier("composer.presentation-status")
                }
                Text(nativeMarkStatus).accessibilityIdentifier("composer.native-marked")
                Text("Key events \(keyEvents); Return events \(returnEvents)")
                    .accessibilityIdentifier("composer.events")
                if owned {
                    ComposerEditor(
                        text: $text, focused: $nativeFocused, ownerID: ownerID,
                        placeholder: "Synthetic composer", accessibilityIdentifier: "composer.probe",
                        onChange: { state, edits in
                            ownerEvents.record(owner: renderedOwner, marked: state.markedRangeUTF16 != nil)
                            marked = state.markedRangeUTF16 != nil
                            editCount += edits.count
                            invalidEdits += edits.filter { $0.resultingText == nil }.count
                            if !edits.isEmpty {
                                transactionSummary = edits.map {
                                    "\($0.rangeUTF16.location):\($0.rangeUTF16.length):\($0.replacement.utf16.count)"
                                }.joined(separator: "|")
                                for edit in edits { _ = insertionMarker?.rebase(using: edit) }
                            }
                            if let edit = edits.last {
                                editSummary =
                                    "Edit \(edit.rangeUTF16.location):\(edit.rangeUTF16.length):\(edit.replacement.utf16.count); invalid \(invalidEdits)"
                            }
                        },
                        onSubmit: {
                            guard ownerID == renderedOwner else { return }
                            sends += 1; text = ""; nativeFocused = false
                        }
                    )
                    .padding(8).background(.secondary.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                    Text(editSummary).font(.caption).accessibilityIdentifier("composer.edit-summary")
                    HStack {
                        Button("Repeated text") {
                            text = "aaaa"; insertionMarker = DictationInsertionAnchor(text: "aaaa", offsetUTF16: 2)
                        }.accessibilityIdentifier("composer.repeated")
                        Button("Delete first") {
                            guard let input = Self.fixtureInput(),
                                let end = input.position(from: input.beginningOfDocument, offset: 1),
                                let range = input.textRange(from: input.beginningOfDocument, to: end)
                            else { return }
                            input.replace(range, withText: "")
                        }.accessibilityIdentifier("composer.delete-first")
                        Button("Commit composition") { Self.fixtureInput()?.unmarkText() }
                            .accessibilityIdentifier("composer.unmark")
                        Button("Clear") { text = "" }.accessibilityIdentifier("composer.clear")
                    }.font(.caption)
                    HStack {
                        Button("Batch edits") { Self.batchFixtureEdits() }.accessibilityIdentifier("composer.batch")
                        Text("Marker \(insertionMarker?.offsetUTF16 ?? -1)").accessibilityIdentifier("composer.marker")
                        Text(transactionSummary).accessibilityIdentifier("composer.transactions")
                    }.font(.caption)
                    HStack {
                        Button("Switch draft") {
                            previousOwner = ownerID; ownerID = UUID()
                        }
                        .accessibilityIdentifier("composer.switch-owner")
                        Button("Read previous owner") {
                            if let previousOwner {
                                previousOwnerStatus =
                                    ownerEvents.latest[previousOwner] == false
                                    ? "Previous owner committed"
                                    : "Previous owner uncommitted; events \(ownerEvents.counts[previousOwner, default: 0])"
                            }
                        }.accessibilityIdentifier("composer.read-previous-owner")
                        Text(previousOwnerStatus).accessibilityIdentifier("composer.previous-owner")
                    }.font(.caption)
                    HStack {
                        Button("Check owner callback") { reentrantStatus = Self.checkReentrantOwner() }
                            .accessibilityIdentifier("composer.reentrant-check")
                        Text(reentrantStatus).accessibilityIdentifier("composer.reentrant-status")
                    }.font(.caption)
                    if ProcessInfo.processInfo.arguments.contains("--maple-composer-order-events") {
                        HStack {
                            Button("Record input order") {
                                guard let view = Self.fixtureInput() as? ComposerTextView,
                                    let coordinator = view.delegate as? ComposerEditor.Coordinator
                                else { inputOrderStatus = "Owned coordinator unavailable"; return }
                                inputEvents.reset()
                                let recorder = inputEvents
                                coordinator.inputOrderObserver = { [weak recorder] in recorder?.record($0) }
                                inputOrderStatus = "Recording synthetic input order"
                            }.accessibilityIdentifier("composer.order-record")
                            Button("Read input order") {
                                inputOrderStatus = inputEvents.snapshot
                            }
                            .accessibilityIdentifier("composer.order-read")
                        }.font(.caption)
                        Button("Copy input diagnostics") {
                            // Explicit synthetic-only export, separate from the app's
                            // message Copy action. No text, account or owner ID is copied.
                            let snapshot =
                                inputEvents.snapshot
                                + "\nfixture sends=\(sends); newlines=\(text.filter { $0 == "\n" }.count); "
                                + "invalidEdits=\(invalidEdits)\n"
                            UIPasteboard.general.setItems(
                                [[UTType.utf8PlainText.identifier: Data(snapshot.utf8)]],
                                options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
                            inputOrderStatus =
                                "Copied input diagnostics; recorded=\(inputEvents.entries.count); dropped=\(inputEvents.dropped)"
                        }
                        .font(.caption).accessibilityIdentifier("composer.order-copy")
                        Text(inputOrderStatus).font(.caption2).lineLimit(4, reservesSpace: true)
                            .accessibilityLabel(inputOrderStatus).accessibilityIdentifier("composer.order-events")
                    }
                } else {
                    TextField("Synthetic composer", text: $text, axis: .vertical)
                        .lineLimit(1...8).focused($focused).textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("composer.probe")
                        .onKeyPress(phases: .down) { press in
                            keyEvents += 1
                            guard press.key == .return else { return .ignored }
                            returnEvents += 1
                            guard press.modifiers.isEmpty else { return .ignored }
                            sends += 1
                            text = ""
                            focused = false
                            return .handled
                        }
                }
                HStack {
                    Button("Mark Japanese text") {
                        guard let input = Self.fixtureInput(activateOwned: owned) else {
                            nativeMarkStatus = "Owned input unavailable"; marked = false; return
                        }
                        input.setMarkedText("にほん", selectedRange: NSRange(location: 3, length: 0))
                        nativeMarkStatus = input.markedTextRange != nil ? "Native marked 1" : "Native marked 0"
                        marked = input.markedTextRange != nil
                    }.accessibilityIdentifier("composer.mark")
                    Button("Read composition") { marked = Self.fixtureInput()?.markedTextRange != nil }
                        .accessibilityIdentifier("composer.read-mark")
                }
                Text("Native UITextView control").font(.caption)
                ComposerNativeInputControl(text: $nativeText).frame(height: 64)
                Text("Native newlines \(nativeText.filter { $0 == "\n" }.count)")
                    .accessibilityIdentifier("composer.native-newlines")
                Spacer()
            }
            .padding()
            .onAppear { focused = !owned }
        }

        // Debug probe only. This uses public responder/text-input protocols to set
        // controlled marked text; product code must not depend on SwiftUI hierarchy.
        @MainActor private static func fixtureInput(activateOwned: Bool = false) -> (any UITextInput)? {
            var pending = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                .flatMap(\.windows).filter(\.isKeyWindow).map { $0 as UIView }
            var firstResponder: (any UITextInput)?
            var visited = 0
            while let view = pending.popLast(), visited < 1024 {
                visited += 1
                if let owned = view as? ComposerTextView, owned.accessibilityIdentifier == "composer.probe" {
                    if activateOwned && !owned.isFirstResponder && !owned.becomeFirstResponder() { return nil }
                    return owned
                }
                if view.isFirstResponder, let input = view as? any UITextInput { firstResponder = input }
                pending.append(contentsOf: view.subviews.prefix(max(0, 1024 - visited - pending.count)))
            }
            return activateOwned ? nil : firstResponder
        }

        // Exercises the production Coordinator with a binding that synchronously
        // installs another owner. No account data or global view hooks are involved.
        @MainActor private static func checkReentrantOwner() -> String {
            let oldOwner = UUID(), newOwner = UUID()
            var activeOwner = oldOwner
            var oldText = ""
            var oldReceived = "", newReceived = ""
            var oldSends = 0, newSends = 0, staleDropped = 0
            let view = ComposerTextView(frame: .zero, textContainer: nil)
            var coordinator: ComposerEditor.Coordinator!
            let next = ComposerEditor(
                text: .constant("new owner"), focused: .constant(false), ownerID: newOwner,
                placeholder: "Synthetic new owner",
                onChange: { state, _ in newReceived = state.text },
                onSubmit: { if activeOwner == newOwner { newSends += 1 } })
            let old = ComposerEditor(
                text: Binding(
                    get: { oldText },
                    set: { value in
                        oldText = value
                        activeOwner = newOwner
                        coordinator.install(next, in: view)
                    }),
                focused: .constant(false), ownerID: oldOwner, placeholder: "Synthetic old owner",
                onChange: { state, _ in oldReceived = state.text },
                onSubmit: {
                    guard activeOwner == oldOwner else { staleDropped += 1; return }
                    oldSends += 1
                })
            coordinator = old.makeCoordinator()
            coordinator.view = view
            coordinator.install(old, in: view)
            defer {
                coordinator.valid = false
                coordinator.parent = next
                coordinator.view = nil
            }
            coordinator.publish()
            oldReceived = ""
            view.text = "synthetic old input"
            coordinator.submit()
            return oldReceived == "synthetic old input" && newReceived != "synthetic old input"
                && oldSends == 0 && newSends == 0 && staleDropped == 0
                ? "Owner callbacks fenced" : "Owner callbacks failed"
        }

        @MainActor private static func batchFixtureEdits() {
            guard let view = fixtureInput() as? ComposerTextView, view.text == "aaaa" else { return }
            view.textStorage.beginEditing()
            defer { view.textStorage.endEditing() }
            for (offset, replacement) in [(0, "b"), (3, "c")] {
                guard let start = view.position(from: view.beginningOfDocument, offset: offset),
                    let end = view.position(from: start, offset: 1), let range = view.textRange(from: start, to: end)
                else { return }
                view.replace(range, withText: replacement)
            }
            // Attribute fixing broadens the aggregate storage span, but must not
            // broaden the independently captured character replacement events.
            view.textStorage.addAttribute(
                .foregroundColor, value: UIColor.label,
                range: NSRange(location: 0, length: view.textStorage.length))
        }
    }

    /// Positive control with no sibling editor, SwiftUI focus binding, key handler,
    /// or production ComposerEditor that could intercept native Return events.
    private struct ComposerStandaloneNativeControl: View {
        @State private var text = ""
        @State private var diagnostics = "Not observed"
        var body: some View {
            VStack(alignment: .leading, spacing: 20) {
                Text("Standalone native UITextView control").font(.headline)
                ComposerNativeInputControl(text: $text, diagnostics: $diagnostics).frame(height: 160)
                Text(diagnostics).font(.caption2).accessibilityIdentifier("composer.native-diagnostics")
                Text("Native newlines \(text.filter { $0 == "\n" }.count)")
                    .accessibilityIdentifier("composer.native-newlines")
                Spacer()
            }.padding()
        }
    }

    private struct ComposerNativeInputControl: UIViewRepresentable {
        @Binding var text: String
        @Binding var diagnostics: String
        init(text: Binding<String>, diagnostics: Binding<String> = .constant("")) {
            _text = text; _diagnostics = diagnostics
        }
        func makeCoordinator() -> Coordinator { Coordinator(text: $text, diagnostics: $diagnostics) }
        func makeUIView(context: Context) -> ComposerObservedNativeControl {
            let view = ComposerObservedNativeControl()
            view.accessibilityIdentifier = "composer.native-control"
            view.font = .preferredFont(forTextStyle: .body)
            view.backgroundColor = .secondarySystemBackground
            view.delegate = context.coordinator
            view.observed = { [weak view, weak coordinator = context.coordinator] in
                guard let view else { return }
                coordinator?.publish(view)
            }
            return view
        }
        func updateUIView(_ view: ComposerObservedNativeControl, context: Context) {
            context.coordinator.text = $text
            context.coordinator.diagnostics = $diagnostics
            if view.text != text, view.markedTextRange == nil { view.text = text }
        }
        static func dismantleUIView(_ view: ComposerObservedNativeControl, coordinator: Coordinator) {
            view.observed = nil
            view.delegate = nil
            view.clearRetainedReturn()
        }
        @MainActor final class Coordinator: NSObject, UITextViewDelegate {
            var text: Binding<String>
            var diagnostics: Binding<String>
            var changeCount = 0
            init(text: Binding<String>, diagnostics: Binding<String>) {
                self.text = text; self.diagnostics = diagnostics
            }
            func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String)
                -> Bool
            {
                (textView as? ComposerObservedNativeControl)?.observeNewlineProposal(text, range: range)
                return true
            }
            func textViewDidChange(_ textView: UITextView) {
                changeCount += 1
                text.wrappedValue = textView.text
                if let view = textView as? ComposerObservedNativeControl { publish(view) }
            }
            func publish(_ view: ComposerObservedNativeControl) {
                let scalars = (view.text ?? "").unicodeScalars.prefix(64).map { String($0.value, radix: 16) }
                    .joined(separator: ",")
                let value =
                    "Scalars \(scalars); selection \(view.selectedRange.location):\(view.selectedRange.length); "
                    + "changes \(changeCount); presses \(view.presses); keys \(view.keys); inserted \(view.inserted); "
                    + view.returnScopeDiagnostics
                if diagnostics.wrappedValue != value { diagnostics.wrappedValue = value }
            }
        }
    }

    /// Observation-only native control. Overrides always forward the original
    /// inputs unchanged; neither key commands nor Return behavior is implemented.
    @MainActor private final class ComposerObservedNativeControl: UITextView {
        var observed: (() -> Void)?
        var presses = 0
        var keys = ""
        var inserted = ""
        private var returnScopeDepth = 0
        private var returnScopes = 0
        private var proposalInside = 0
        private var proposalOutside = 0
        private var insertionInside = 0
        private var insertionOutside = 0
        private var scopeDropped = 0
        private var proposalScope = false
        private var insertionScope = false
        private var proposalBefore = -1
        private var insertionBefore = -1
        private var proposalRange = NSRange(location: NSNotFound, length: 0)
        private var retainedReturn: UIPress?
        private var returnEnded = false
        private var returnCancelled = false
        private var lifetimeEvents: [String] = []
        private var lifetimeDropped = 0
        private var retainedReplacements = 0
        private enum LifetimeEvent: Int {
            case beganEntry = 1, beganExit, changedEntry, changedExit
            case endedEntry, endedExit, cancelledEntry, cancelledExit
            case newlineProposal, newlineInsertionEntry, newlineInsertionExit
        }
        var returnScopeDiagnostics: String {
            "returnScopes \(returnScopes); scopeDepth \(returnScopeDepth); "
                + "newlineProposal \(proposalInside):\(proposalOutside); proposalScope \(proposalScope); "
                + "proposalBefore \(proposalBefore); proposalRange \(proposalRange.location):\(proposalRange.length); "
                + "newlineInsertion \(insertionInside):\(insertionOutside); insertionScope \(insertionScope); "
                + "insertionBefore \(insertionBefore); scopeDropped \(scopeDropped); "
                + "lifetimeDropped \(lifetimeDropped); retainedReplacements \(retainedReplacements); "
                + "lifetime[ordinal:kind:hid:mask:phase:ended:cancelled:matched] \(lifetimeEvents.joined(separator: ";"))"
        }
        func clearRetainedReturn() {
            retainedReturn = nil
            returnEnded = false
            returnCancelled = false
        }
        private func isReturn(_ press: UIPress) -> Bool {
            press.key?.keyCode == .keyboardReturnOrEnter || press.key?.keyCode == .keypadEnter
        }
        private func matchesRetained(_ presses: Set<UIPress>) -> Bool {
            guard let retainedReturn else { return false }
            return presses.contains { $0 === retainedReturn }
        }
        private func recordLifetime(_ kind: LifetimeEvent, matched: Int = -1) {
            guard lifetimeEvents.count < 64 else {
                lifetimeDropped = min(lifetimeDropped + 1, 256)
                return
            }
            let hid = retainedReturn?.key.map { Int($0.keyCode.rawValue) } ?? -1
            let mask = retainedReturn?.key.map { Int($0.modifierFlags.rawValue) } ?? -1
            let phase = retainedReturn.map { Int($0.phase.rawValue) } ?? -1
            lifetimeEvents.append(
                "\(lifetimeEvents.count + 1):\(kind.rawValue):\(hid):\(mask):\(phase):"
                    + "\(returnEnded ? 1 : 0):\(returnCancelled ? 1 : 0):\(matched)")
        }
        private func increment(_ value: inout Int) {
            if value < 256 { value += 1 } else { scopeDropped = min(scopeDropped + 1, 256) }
        }
        func observeNewlineProposal(_ text: String, range: NSRange) {
            guard text == "\n" || text == "\r" else { return }
            proposalScope = returnScopeDepth > 0
            proposalBefore = (self.text ?? "").utf16.count
            proposalRange = range
            if proposalScope { increment(&proposalInside) } else { increment(&proposalOutside) }
            recordLifetime(.newlineProposal)
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            self.presses += presses.count
            let hardwareKeys = presses.compactMap { $0.key }
            keys = hardwareKeys.map {
                "\($0.keyCode.rawValue):\($0.modifierFlags.rawValue)"
            }.sorted().joined(separator: ",")
            let hasReturn = hardwareKeys.contains {
                $0.keyCode == .keyboardReturnOrEnter || $0.keyCode == .keypadEnter
            }
            if let press = presses.first(where: { isReturn($0) }) {
                if retainedReturn != nil { retainedReplacements = min(retainedReplacements + 1, 256) }
                retainedReturn = press
                returnEnded = false
                returnCancelled = false
                recordLifetime(.beganEntry, matched: 1)
            }
            let enteredScope = hasReturn && returnScopeDepth < 256
            if enteredScope {
                returnScopeDepth += 1
                increment(&returnScopes)
            } else if hasReturn {
                scopeDropped = min(scopeDropped + 1, 256)
            }
            // Observe only this synchronous super call. No scope survives it,
            // and every original input is forwarded unchanged.
            super.pressesBegan(presses, with: event)
            if enteredScope { returnScopeDepth -= 1 }
            if hasReturn { recordLifetime(.beganExit, matched: matchesRetained(presses) ? 1 : 0) }
            observed?()
        }
        override func pressesChanged(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            let hasReturn = presses.contains(where: { isReturn($0) })
            if hasReturn { recordLifetime(.changedEntry, matched: matchesRetained(presses) ? 1 : 0) }
            super.pressesChanged(presses, with: event)
            if hasReturn {
                recordLifetime(.changedExit, matched: matchesRetained(presses) ? 1 : 0)
                observed?()
            }
        }
        override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            let hasReturn = presses.contains(where: { isReturn($0) })
            if matchesRetained(presses) { returnEnded = true }
            if hasReturn { recordLifetime(.endedEntry, matched: matchesRetained(presses) ? 1 : 0) }
            super.pressesEnded(presses, with: event)
            if hasReturn {
                recordLifetime(.endedExit, matched: matchesRetained(presses) ? 1 : 0)
                observed?()
            }
        }
        override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            let hasReturn = presses.contains(where: { isReturn($0) })
            if matchesRetained(presses) { returnCancelled = true }
            if hasReturn { recordLifetime(.cancelledEntry, matched: matchesRetained(presses) ? 1 : 0) }
            super.pressesCancelled(presses, with: event)
            if hasReturn {
                recordLifetime(.cancelledExit, matched: matchesRetained(presses) ? 1 : 0)
                observed?()
            }
        }
        override func insertText(_ text: String) {
            inserted = text.unicodeScalars.prefix(64).map { String($0.value, radix: 16) }.joined(separator: ",")
            if text == "\n" || text == "\r" {
                insertionScope = returnScopeDepth > 0
                insertionBefore = (self.text ?? "").utf16.count
                if insertionScope { increment(&insertionInside) } else { increment(&insertionOutside) }
                recordLifetime(.newlineInsertionEntry)
            }
            super.insertText(text)
            if text == "\n" || text == "\r" { recordLifetime(.newlineInsertionExit) }
            observed?()
        }
    }
#endif
