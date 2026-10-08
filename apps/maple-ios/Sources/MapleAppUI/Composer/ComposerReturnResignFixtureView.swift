#if os(iOS) && DEBUG
    import SwiftUI
    import UIKit
    import UniformTypeIdentifiers

    /// Isolated synthetic experiment. Resignation is measured, never treated as
    /// a documented input-flush guarantee or connected to application Send.
    struct ComposerReturnResignFixtureView: View {
        @State private var text = ""
        @State private var ledger = ReturnResignLedger()
        @State private var status = "No diagnostics read"

        var body: some View {
            VStack(alignment: .leading, spacing: 16) {
                Text("Synchronous Return resignation control").font(.headline)
                ReturnResignInput(text: $text, ledger: ledger).frame(height: 160)
                Button("Read resignation diagnostics") { status = ledger.snapshot }
                    .accessibilityIdentifier("composer.resign-read")
                Button("Copy resignation diagnostics") {
                    UIPasteboard.general.setItems(
                        [[UTType.utf8PlainText.identifier: Data(ledger.snapshot.utf8)]],
                        options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
                    status =
                        "Copied resignation diagnostics; recorded=\(ledger.entries.count); dropped=\(ledger.dropped)"
                }.accessibilityIdentifier("composer.resign-copy")
                Text(status).font(.caption2).lineLimit(6, reservesSpace: true)
                    .accessibilityLabel(status).accessibilityIdentifier("composer.resign-diagnostics")
                Spacer()
            }.padding()
        }
    }

    @MainActor private final class ReturnResignLedger {
        weak var view: ReturnResignTextView?
        private(set) var entries: [String] = []
        private(set) var dropped = 0

        func append(_ entry: String) {
            guard entries.count < 256 else { dropped = min(dropped + 1, 1_000_000); return }
            entries.append("\(entries.count + 1): \(entry)")
        }

        var snapshot: String {
            let native = view?.text ?? ""
            return "Return resignation diagnostics v1; recorded=\(entries.count); dropped=\(dropped)\n"
                + entries.joined(separator: "\n")
                + "\nfinal native=\(native.utf16.count); binding=\(view?.bindingLength?() ?? -1); "
                + "nativeIsABC=\(native == "abc" ? 1 : 0); nativeIsABCD=\(native == "abcd" ? 1 : 0); "
                + "firstResponder=\(view?.isFirstResponder == true ? 1 : 0)\n"
        }
    }

    private struct ReturnResignInput: UIViewRepresentable {
        @Binding var text: String
        let ledger: ReturnResignLedger

        func makeCoordinator() -> Coordinator { Coordinator(text: $text) }
        func makeUIView(context: Context) -> ReturnResignTextView {
            let view = ReturnResignTextView()
            view.font = .preferredFont(forTextStyle: .body)
            view.backgroundColor = .secondarySystemBackground
            view.accessibilityIdentifier = "composer.resign-control"
            view.delegate = context.coordinator
            view.ledger = ledger
            ledger.view = view
            view.bindingLength = { [weak coordinator = context.coordinator] in
                coordinator?.text.wrappedValue.utf16.count ?? -1
            }
            return view
        }

        func updateUIView(_ view: ReturnResignTextView, context: Context) {
            context.coordinator.text = $text
            // This control never reinstalls a model snapshot into native text.
            // The native input system alone owns its synthetic text mutations.
        }

        static func dismantleUIView(_ view: ReturnResignTextView, coordinator: Coordinator) {
            view.valid = false
            view.commandOwner = UUID()
            view.bindingLength = nil
            view.ledger?.view = nil
            view.ledger = nil
            view.delegate = nil
        }

        @MainActor final class Coordinator: NSObject, UITextViewDelegate {
            var text: Binding<String>
            init(text: Binding<String>) { self.text = text }

            func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String)
                -> Bool
            {
                (textView as? ReturnResignTextView)?.record(
                    .proposal, inputLength: text.utf16.count, range: range)
                return true
            }

            func textViewDidChange(_ textView: UITextView) {
                guard let view = textView as? ReturnResignTextView else { return }
                view.record(.changeBeforeBinding)
                text.wrappedValue = textView.text
                view.record(.changeAfterBinding)
            }

            func textViewDidEndEditing(_ textView: UITextView) {
                (textView as? ReturnResignTextView)?.record(.didEndEditing)
            }
        }
    }

    @MainActor private final class ReturnResignTextView: UITextView {
        enum Event: String {
            case proposal, changeBeforeBinding, changeAfterBinding, didEndEditing
            case insertBegin, insertEnd, commandEntry, resignBegin, resignEnd, wouldSubmit
            case pressBeganEntry, pressBeganExit, pressChangedEntry, pressChangedExit
            case pressEndedEntry, pressEndedExit, pressCancelledEntry, pressCancelledExit
        }

        var valid = true
        var commandOwner = UUID()
        var bindingLength: (() -> Int)?
        var ledger: ReturnResignLedger?
        private var operationDepth = 0
        private var resignDepth = 0
        private var insertions = 0
        private var insertionsDuringResign = 0
        private var rawOrdinal = 0
        private var commands = 0
        private var wouldSubmits = 0

        private var acceptsCommand: Bool {
            valid && isEditable && isFirstResponder && markedTextRange == nil
                && operationDepth == 0 && resignDepth == 0
                && !textStorage.editedMask.contains(.editedCharacters)
        }

        override var keyCommands: [UIKeyCommand]? {
            let inherited = super.keyCommands ?? []
            guard acceptsCommand else { return inherited }
            return ["\r", "\u{3}"].map { input in
                let command = UIKeyCommand(
                    title: "", action: #selector(measureReturn(_:)), input: input,
                    modifierFlags: [], propertyList: commandOwner.uuidString)
                command.wantsPriorityOverSystemBehavior = true
                return command
            } + inherited
        }

        private func owns(_ command: UIKeyCommand) -> Bool {
            command.action == #selector(measureReturn(_:))
                && command.propertyList as? String == commandOwner.uuidString
                && (command.input == "\r" || command.input == "\u{3}")
                && command.modifierFlags.subtracting(.numericPad).isEmpty
        }

        override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
            if action == #selector(measureReturn(_:)) {
                if sender == nil { return acceptsCommand }
                guard let command = sender as? UIKeyCommand else { return false }
                return owns(command) && acceptsCommand
            }
            return super.canPerformAction(action, withSender: sender)
        }

        @objc private func measureReturn(_ command: UIKeyCommand) {
            let eligible = owns(command) && acceptsCommand
            commands = min(commands + 1, 1_000_000)
            record(.commandEntry, mask: command.modifierFlags.rawValue, eligible: eligible ? 1 : 0)
            guard eligible, let admittedWindow = window else { return }
            let admittedOwner = commandOwner
            resignDepth += 1
            record(.resignBegin)
            let resigned = resignFirstResponder()
            record(.resignEnd, success: resigned ? 1 : 0)
            resignDepth -= 1
            guard resigned, valid, commandOwner == admittedOwner, window === admittedWindow,
                !isFirstResponder, markedTextRange == nil
            else { return }
            wouldSubmits = min(wouldSubmits + 1, 1_000_000)
            // Only a fixed synthetic comparator is exported. No text is submitted,
            // cleared, persisted, logged or passed into the application runtime.
            record(.wouldSubmit, success: 1, exactABC: text == "abc" ? 1 : 0)
        }

        override func insertText(_ text: String) {
            operationDepth += 1
            insertions = min(insertions + 1, 1_000_000)
            if resignDepth > 0 { insertionsDuringResign = min(insertionsDuringResign + 1, 1_000_000) }
            record(.insertBegin, inputLength: text.utf16.count)
            super.insertText(text)
            record(.insertEnd, inputLength: text.utf16.count)
            operationDepth -= 1
        }

        func record(
            _ event: Event, inputLength: Int = -1, range: NSRange? = nil,
            count: Int = -1, hid: Int = -1, mask: Int = 0,
            success: Int = -1, eligible: Int = -1, exactABC: Int = -1
        ) {
            guard valid else { return }
            ledger?.append(
                "event=\(event.rawValue); native=\((text ?? "").utf16.count); binding=\(bindingLength?() ?? -1); "
                    + "insertions=\(insertions); duringResign=\(insertionsDuringResign); resignDepth=\(resignDepth); "
                    + "operationDepth=\(operationDepth); firstResponder=\(isFirstResponder ? 1 : 0); "
                    + "marked=\(markedTextRange == nil ? 0 : 1); rawOrdinal=\(rawOrdinal); commands=\(commands); "
                    + "wouldSubmits=\(wouldSubmits); inputLength=\(inputLength); range=\(range?.location ?? -1):\(range?.length ?? -1); "
                    + "pressCount=\(count); returnHID=\(hid); mask=\(mask); success=\(success); eligible=\(eligible); exactABC=\(exactABC)"
            )
        }

        private func recordPress(_ event: Event, _ presses: Set<UIPress>) {
            let key = presses.compactMap(\.key).first {
                $0.keyCode == .keyboardReturnOrEnter || $0.keyCode == .keypadEnter
            }
            record(
                event, count: presses.count, hid: key.map { Int($0.keyCode.rawValue) } ?? -1,
                mask: key?.modifierFlags.rawValue ?? 0)
        }

        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            rawOrdinal = min(rawOrdinal + 1, 1_000_000)
            recordPress(.pressBeganEntry, presses)
            super.pressesBegan(presses, with: event)
            recordPress(.pressBeganExit, presses)
        }
        override func pressesChanged(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            rawOrdinal = min(rawOrdinal + 1, 1_000_000)
            recordPress(.pressChangedEntry, presses)
            super.pressesChanged(presses, with: event)
            recordPress(.pressChangedExit, presses)
        }
        override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            rawOrdinal = min(rawOrdinal + 1, 1_000_000)
            recordPress(.pressEndedEntry, presses)
            super.pressesEnded(presses, with: event)
            recordPress(.pressEndedExit, presses)
        }
        override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            rawOrdinal = min(rawOrdinal + 1, 1_000_000)
            recordPress(.pressCancelledEntry, presses)
            super.pressesCancelled(presses, with: event)
            recordPress(.pressCancelledExit, presses)
        }
    }
#endif
