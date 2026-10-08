#if os(iOS) && DEBUG
    import SwiftUI
    import UIKit
    import Accessibility
    import Textual
    import UniformTypeIdentifiers

    /// Synthetic fixtures only. Available solely through an explicit debug launch argument.
    struct RendererFixtureView: View {
        @State private var suffix = ""
        @State private var generation = UUID()
        @State private var pasted = ""
        @State private var links = 0
        @State private var dark = false
        @State private var revision = 0
        @State private var accessibilityChecked = false
        @State private var accessibilityCounts = "AX not checked"
        @State private var selectionStatus = "Selection not checked"
        var body: some View {
            if ProcessInfo.processInfo.arguments.contains("--maple-presentation-cover-fixture") {
                PresentationCoverFixtureView()
            } else if ProcessInfo.processInfo.arguments.contains("--maple-renderer-lifecycle") {
                MessageEquationLifecycleFixtureView()
            } else if ProcessInfo.processInfo.arguments.contains("--maple-renderer-corpus") {
                RendererCorpusFixtureView()
            } else if ProcessInfo.processInfo.arguments.contains("--maple-renderer-layout") {
                RendererLayoutFixtureView()
            } else {
                standardContent
            }
        }

        private var standardContent: some View {
            VStack(alignment: .leading) {
                HStack {
                    Button("Append after delay") {
                        Task { @MainActor in
                            var before: FixtureSelection?
                            for _ in 0..<100 {
                                if let current = Self.selectedFixtureText() { before = current }
                                try? await Task.sleep(for: .milliseconds(100))
                            }
                            suffix += " Appended synthetic stream text."
                            revision += 1
                            try? await Task.sleep(for: .seconds(1))
                            let after = Self.selectedFixtureText()
                            selectionStatus =
                                before != nil && before == after
                                ? "Selection preserved" : "Selection changed"
                        }
                    }.accessibilityIdentifier("renderer.append")
                    Button("Revoke") {
                        generation = UUID(); suffix = " Revoked generation."
                    }
                    .accessibilityIdentifier("renderer.revoke")
                    Toggle("Dark", isOn: $dark).fixedSize()
                }
                Text("Opened links: \(links)").accessibilityIdentifier("renderer.links")
                Text("Revision \(revision)").accessibilityIdentifier("renderer.revision")
                Text(selectionStatus).font(.caption).accessibilityIdentifier("renderer.selection-status")
                HStack {
                    Button("Check equation accessibility") {
                        let result = Self.nativeEquationContent()
                        accessibilityChecked = result.found
                        accessibilityCounts = result.counts
                    }.accessibilityIdentifier("renderer.ax-check")
                    Text(accessibilityChecked ? "Equation AX present" : "Equation AX not checked")
                        .accessibilityIdentifier("renderer.ax-status")
                }.font(.caption)
                Text(accessibilityCounts).font(.caption).accessibilityIdentifier("renderer.ax-counts")
                TextField("Paste selection", text: $pasted, axis: .vertical)
                    .accessibilityIdentifier("renderer.paste").textFieldStyle(.roundedBorder)
                ScrollView {
                    EquationDiagnosticView()
                    MessageView(
                        text: Self.fixture + suffix, theme: dark ? .dark : .light,
                        accountGeneration: generation
                    ) { _ in links += 1 }
                    .accessibilityIdentifier("renderer.message")
                    .padding()
                }
            }.padding()
        }

        // DEBUG fixture inspects only its own rendered native accessibility tree.
        // Uses public container/provider APIs and returns no content to the test.
        @MainActor private static func nativeEquationContent() -> (found: Bool, counts: String) {
            var pending: [NSObject] = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                .flatMap(\.windows).map { $0 as NSObject }
            var seen = Set<ObjectIdentifier>()
            var providers = 0
            var contents = 0
            var matches = 0
            while let item = pending.popLast(), seen.count < 1024 {
                guard seen.insert(ObjectIdentifier(item)).inserted else { continue }
                var nativeContent: [AXCustomContent] = []
                if let provider = item as? any AXCustomContentProvider {
                    providers += 1
                    let block = provider.accessibilityCustomContentBlock ?? nil
                    nativeContent = block?() ?? provider.accessibilityCustomContent ?? []
                } else {
                    // Some framework-owned accessibility nodes implement this
                    // public Objective-C accessor without declaring the protocol.
                    let selector = NSSelectorFromString("accessibilityCustomContent")
                    if item.responds(to: selector) {
                        providers += 1
                        nativeContent = item.perform(selector)?.takeUnretainedValue() as? [AXCustomContent] ?? []
                    }
                }
                for content in nativeContent {
                    contents += 1
                    matches = max(
                        matches, EquationDiagnosticView.formulas.filter { content.value.contains($0) }.count)
                    if content.importance == .high,
                        EquationDiagnosticView.formulas.allSatisfy({ content.value.contains($0) })
                    {
                        return (
                            true,
                            "AX nodes \(seen.count), providers \(providers), contents \(contents), matches \(matches)"
                        )
                    }
                }
                var children = (item.accessibilityElements ?? []).compactMap { $0 as? NSObject }
                let count = item.accessibilityElementCount()
                if children.isEmpty, (1...1024).contains(count) {
                    children = (0..<count).compactMap { item.accessibilityElement(at: $0) as? NSObject }
                }
                if let view = item as? UIView { children += view.subviews.map { $0 as NSObject } }
                pending.append(contentsOf: children.prefix(max(0, 1024 - seen.count - pending.count)))
            }
            return (false, "AX nodes \(seen.count), providers \(providers), contents \(contents), matches \(matches)")
        }

        private struct FixtureSelection: Equatable {
            let text: String
            let start: Int
            let length: Int
        }

        // Public UITextInput observation only, bounded and synthetic. Comparing the
        // actual range distinguishes selection loss from an edit menu dismissing.
        @MainActor private static func selectedFixtureText() -> FixtureSelection? {
            var pending = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
                .map { $0 as UIView }
            var visited = 0
            while let view = pending.popLast(), visited < 1024 {
                visited += 1
                if view.isFirstResponder, let input = view as? any UITextInput,
                    let range = input.selectedTextRange, !range.isEmpty,
                    let text = input.text(in: range), !text.isEmpty
                {
                    return FixtureSelection(
                        text: text, start: input.offset(from: input.beginningOfDocument, to: range.start),
                        length: input.offset(from: range.start, to: range.end))
                }
                pending.append(contentsOf: view.subviews.prefix(max(0, 1024 - visited - pending.count)))
            }
            return nil
        }

        static let fixture = #"""
            Start alpha **bold beta** and *italic gamma*, [delta link](https://example.test) and end epsilon.

            Arabic العربية Hebrew עברית CJK 中文 emoji 👩🏽‍💻 and combining é.

            ```swift
            let synthetic = "copy code" // preserved
            print(synthetic)
            ```

            | Name | Formula | Detail |
            | :--- | ---: | :---: |
            | row alpha | $x^2$ | beta cell |
            | row gamma | $\frac{1}{2}$ | delta cell |

            Final omega after code and table.

            Control $\frac{1}{2}+\sqrt{x^2+1}$.
            Operator $\operatorname{argmax}_{x} f(x)$.
            Delimiters $\Bigl(\frac{a}{b}\Bigr)$.
            Boxed $\boxed{E=mc^2}$.
            Macro $\newcommand{\RR}{\mathbb{R}}\RR^2$.

            \[E=mc^2\tag{1}\]

            [Blocked script](javascript:alert%281%29) ![Remote image](https://example.test/never.png)
            """#
    }
    private struct RendererCorpusFixtureView: View {
        @State private var copyStatus = "Not run"
        @State private var copyDiagnostics = "Not inspected"
        @State private var copiedScalars = "Not copied"
        @State private var multilingualStatus = "Not run"
        @State private var representationStatus = "Not inspected"
        @State private var linesReady = false
        @State private var multilingualReady = false
        @State private var readiness = "Waiting for native selection spans"
        var body: some View {
            VStack(alignment: .leading) {
                HStack {
                    Button("Copy synthetic line probe") {
                        guard
                            let result = Self.copySpan(
                                from: "Soft first", through: "Hard second", diagnostics: &copyDiagnostics)
                        else { copyStatus = "Native copy unavailable"; return }
                        copiedScalars = Self.scalars(result.copied)
                        copyStatus =
                            "Soft newline \(result.copied.contains("Soft first\nSoft second") ? 1 : 0); "
                            + "hard newline \(result.copied.contains("Hard first\nHard second") ? 1 : 0)"
                        let expected = Data("Soft first\nSoft second\n\nHard first\nHard second".utf8)
                        representationStatus =
                            result.rawUTF8 == expected && Data(result.copied.utf8) == expected
                            ? "Canonical LF and raw UTF8 preserved" : "Copy representation differs"
                    }.accessibilityIdentifier("renderer.copy-lines").disabled(!linesReady)
                    Button("Copy multilingual span") {
                        guard
                            let result = Self.copySpan(
                                from: Self.multilingual, through: Self.multilingual, diagnostics: &copyDiagnostics)
                        else { multilingualStatus = "Native copy unavailable"; return }
                        copiedScalars = Self.scalars(result.copied)
                        multilingualStatus =
                            Array(result.selected.utf8) == Array(Self.multilingual.utf8)
                                && Array(result.copied.utf8) == Array(Self.multilingual.utf8)
                            ? "Multilingual bytes preserved" : "Multilingual bytes changed"
                        representationStatus =
                            result.rawUTF8 == Data(Self.multilingual.utf8)
                            ? "Raw UTF8 preserved" : "Raw UTF8 changed"
                    }.accessibilityIdentifier("renderer.copy-multilingual").disabled(!multilingualReady)
                }
                Text(readiness).font(.caption2).lineLimit(3, reservesSpace: true)
                    .accessibilityLabel(readiness).accessibilityIdentifier("renderer.copy-readiness")
                Text(copyStatus).accessibilityIdentifier("renderer.line-status")
                Text(multilingualStatus).accessibilityIdentifier("renderer.multilingual-status")
                Text(representationStatus).font(.caption).accessibilityIdentifier("renderer.representation-status")
                Text(copyDiagnostics).font(.caption2).lineLimit(8)
                    .accessibilityLabel(copyDiagnostics).accessibilityIdentifier("renderer.line-diagnostics")
                Text(copiedScalars).font(.caption2).accessibilityIdentifier("renderer.copy-scalars")
                ScrollView {
                    MessageView(text: Self.corpus) { _ in }.padding()
                }.accessibilityIdentifier("renderer.corpus-scroll")
            }.padding()
                .task { await observeNativeSpans() }
        }

        @MainActor private func observeNativeSpans() async {
            let clock = ContinuousClock()
            let deadline = clock.now.advanced(by: .seconds(15))
            while !Task.isCancelled {
                let lines = Self.findSpan(from: "Soft first", through: "Hard second")
                let multilingual = Self.findSpan(from: Self.multilingual, through: Self.multilingual)
                linesReady = lines.target != nil
                multilingualReady = multilingual.target != nil
                readiness =
                    "Lines \(linesReady ? "ready" : "waiting"); multilingual \(multilingualReady ? "ready" : "waiting"); "
                    + "lines {\(lines.metadata)}; multilingual {\(multilingual.metadata)}"
                if linesReady && multilingualReady { return }
                guard clock.now < deadline else {
                    readiness = "Native span deadline reached; " + readiness
                    return
                }
                // Poll only the concrete public input condition, never replay Copy.
                do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
            }
        }

        private static let multilingual = "العربية עברית 中文 👩🏽‍💻 é."
        private static func scalars(_ text: String) -> String {
            text.unicodeScalars.prefix(128).map { String($0.value, radix: 16) }.joined(separator: ",")
        }

        private static func rawText(_ value: Any?) -> String {
            if let text = value as? String {
                return "String:" + Self.scalars(text)
            }
            if let data = value as? Data {
                guard data.count <= 8192 else { return "Data exceeds synthetic bound" }
                guard let text = String(data: data, encoding: .utf8) else { return "Data is not UTF8" }
                return "Data:" + Self.scalars(text)
            }
            return value == nil ? "absent" : "other representation"
        }

        // DEBUG-only exact synthetic span. Calls the rendered native responder's
        // Copy action; neither explicit whole-message Copy nor a replacement exporter.
        @MainActor private static func copySpan(
            from first: String, through last: String, diagnostics: inout String
        ) -> (selected: String, copied: String, rawUTF8: Data?)? {
            let found = findSpan(from: first, through: last)
            guard let target = found.target else {
                diagnostics = "Synthetic input unavailable; " + found.metadata
                return nil
            }
            let view = target.view
            let input = target.input
            guard let a = input.position(from: input.beginningOfDocument, offset: target.lower),
                let b = input.position(from: input.beginningOfDocument, offset: target.upper),
                let range = input.textRange(from: a, to: b)
            else { diagnostics = "Range unavailable; " + found.metadata; return nil }
            _ = view.becomeFirstResponder()
            input.selectedTextRange = range
            let selected = input.selectedTextRange.flatMap { input.text(in: $0) } ?? ""
            var nativeRuns: [String] = []
            if let native = input.attributedText?(in: range),
                let bridged = try? AttributedString(native, including: \.textual)
            {
                for run in bridged.runs.prefix(64) {
                    let text = String(bridged[run.range].characters)
                    let inline = String(run.inlinePresentationIntent?.rawValue ?? 0)
                    let paragraph = String(String(describing: run.presentationIntent).prefix(128))
                    nativeRuns.append(Self.scalars(text) + ":i=" + inline + ":p=" + paragraph)
                }
            }
            let before = UIPasteboard.general.changeCount
            view.copy(nil)
            guard UIPasteboard.general.changeCount > before else {
                diagnostics = "Selected \(selected.utf16.count); copy did not change clipboard"
                return nil
            }
            let copied = UIPasteboard.general.string ?? ""
            let retained = input.selectedTextRange.flatMap { input.text(in: $0) } ?? ""
            let item = UIPasteboard.general.items.first ?? [:]
            let advertised = item.keys.sorted().prefix(8).joined(separator: ",")
            let plain = Self.rawText(item[UTType.plainText.identifier])
            let utf8 = Self.rawText(item[UTType.utf8PlainText.identifier])
            let rawUTF8 = UIPasteboard.general.data(forPasteboardType: UTType.utf8PlainText.identifier)
            diagnostics =
                "Selected \(selected.utf16.count); LF \(selected.filter { $0 == "\n" }.count); "
                + "copied \(copied.utf16.count); LF \(copied.filter { $0 == "\n" }.count); "
                + "retained \(retained == selected && !selected.isEmpty ? 1 : 0); "
                + "responder \(String(reflecting: type(of: view))); "
                + "selected scalars \(Self.scalars(selected)); advertised \(advertised); "
                + "raw plain \(plain); raw utf8 \(utf8); native runs \(nativeRuns.joined(separator: "|"))"
            return (selected, copied, rawUTF8)
        }

        @MainActor private struct NativeCopyTarget {
            let view: UIView
            let input: any UITextInput
            let lower: Int
            let upper: Int
        }

        // Nonmutating bounded readiness: no first-responder, selection or clipboard action.
        @MainActor private static func findSpan(from first: String, through last: String)
            -> (target: NativeCopyTarget?, metadata: String)
        {
            let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
                .flatMap(\.windows).map { $0 as UIView }
            var pending = Array(windows.prefix(1024))
            var visited = 0
            var omitted = max(0, windows.count - 1024)
            var inputCount = 0
            var documentCount = 0
            var firstCount = 0
            var lastCount = 0
            var samples: [String] = []
            func metadata() -> String {
                "visited=\(visited), pending=\(pending.count), truncated=\(omitted > 0 || (visited >= 1024 && !pending.isEmpty)), "
                    + "inputs=\(inputCount), documents=\(documentCount), first=\(firstCount), last=\(lastCount), "
                    + "samples=\(samples.joined(separator: "|"))"
            }
            while visited < 1024, let view = pending.popLast() {
                visited += 1
                if let input = view as? any UITextInput {
                    inputCount += 1
                    if let document = input.textRange(from: input.beginningOfDocument, to: input.endOfDocument),
                        let full = input.text(in: document)
                    {
                        documentCount += 1
                        let start = full.range(of: first)
                        let end = full.range(of: last)
                        if start != nil { firstCount += 1 }
                        if end != nil { lastCount += 1 }
                        if samples.count < 8 {
                            samples.append("len:\(full.utf16.count),first:\(start != nil),last:\(end != nil)")
                        }
                        if let start, let end, start.lowerBound < end.upperBound {
                            return (
                                NativeCopyTarget(
                                    view: view, input: input, lower: full[..<start.lowerBound].utf16.count,
                                    upper: full[..<end.upperBound].utf16.count), metadata()
                            )
                        }
                    }
                }
                let capacity = max(0, 1024 - visited - pending.count)
                let children = view.subviews
                omitted += max(0, children.count - capacity)
                pending.append(contentsOf: children.prefix(capacity))
            }
            return (nil, metadata())
        }

        private static let corpus = RendererAcceptanceCorpus.markdown
    }

    private struct RendererLayoutFixtureView: View {
        private var arguments: [String] { ProcessInfo.processInfo.arguments }
        private var dark: Bool { arguments.contains("--maple-renderer-dark") }
        private var large: Bool { arguments.contains("--maple-renderer-large") }
        private var rtl: Bool { arguments.contains("--maple-renderer-rtl") }
        var body: some View {
            ScrollView {
                MessageView(text: Self.corpus, theme: dark ? .dark : .light) { _ in }
                    .padding()
            }
            .accessibilityIdentifier("renderer.layout-scroll")
            .environment(\.dynamicTypeSize, large ? .accessibility3 : .large)
            .environment(\.legibilityWeight, large ? .bold : .regular)
            .environment(\.layoutDirection, rtl ? .rightToLeft : .leftToRight)
            .preferredColorScheme(dark ? .dark : .light)
        }

        private static let corpus = #"""
            | Primary choice | Secondary choice | Third choice | Fourth choice | Fifth choice | Sixth choice |
            | :--- | :--- | :--- | :--- | :--- | :--- |
            | table first marker | Longer prose wraps inside this table cell while columns retain their own scrolling surface. | العربية עברית | 中文 👩🏽‍💻 é | **bold value** | table final marker |

            After wide table reading anchor.

            Multilingual العربية עברית 中文 👩🏽‍💻 é. **Bold** and *italic* preserve their native text styles.

            ```swift
            let synthetic = "wide code with preserved indentation and unbroken_long_identifier_0123456789"
            print(synthetic)
            ```

            Original equation \(\frac{1}{2}+\sqrt{x^2+1}\) follows native text.

            Final reading anchor after code and equation.
            """#
    }
#endif
