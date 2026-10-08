#if DEBUG
    import Foundation
    import Testing
    @testable import Textual

    #if canImport(AppKit)
        import AppKit
    #endif

    /// Input reconstructed from the six public attributed runs in native selection.
    /// The extra native key keeps six input runs distinct; Formatter ignores that key.
    @Suite struct NativeCopyBoundaryTests {
        @Test func observedNativeRunsPreservePlainAndHTMLLineBreaks() throws {
            let native = try Self.observedSelection()
            var runs = 0
            native.enumerateAttributes(in: NSRange(location: 0, length: native.length)) { _, _, _ in
                runs += 1
            }
            let bridged = try AttributedString(native, including: \.textual)
            #expect(runs == 6)
            #expect(native.length == 44)
            #expect(native.string.filter { $0 == "\n" }.count == 2)
            #expect(bridged.runs.allSatisfy { ($0.inlinePresentationIntent ?? []).isEmpty })
            let formatter = Textual.Formatter(native)
            #expect(formatter.plainText() == "Soft first\nSoft second\n\nHard first\nHard second")
            #expect(formatter.html() == "<p>Soft first<br />Soft second</p>\n<p>Hard first<br />Hard second</p>")
        }

        @Test func literalBreaksDoNotUnescapeUserEntitiesOrUnicode() {
            var content = AttributedString("&<> &#10;\nCafé 👩🏽‍💻 e\u{301}")
            content.presentationIntent = PresentationIntent(.paragraph, identity: 1)
            let formatter = Textual.Formatter(content)
            #expect(formatter.plainText() == "&<> &#10;\nCafé 👩🏽‍💻 e\u{301}")
            #expect(
                formatter.html()
                    == "<p>&amp;&lt;&gt; &amp;#10;<br />Caf&#233; &#128105;&#127997;&#8205;&#128187; e&#769;</p>")
        }

        @Test func literalBreaksRetainInlineFormattingAndEscapedLinkDestinations() throws {
            var content = AttributedString("first\nsecond")
            content.presentationIntent = PresentationIntent(.paragraph, identity: 1)
            content.inlinePresentationIntent = [.stronglyEmphasized, .emphasized]
            content.link = URL(string: "https://example.test/?x=1&y=2")
            let formatter = Textual.Formatter(content)
            #expect(formatter.plainText() == "first\nsecond")
            #expect(
                formatter.html()
                    == "<p><a href=\"https://example.test/?x=1&amp;y=2\"><em><strong>first<br />second</strong></em></a></p>"
            )
        }

        @Test func codeBlocksKeepWhitespaceWithoutIntroducingHTMLBreakElements() throws {
            let content = try AttributedString(markdown: "```swift\n  first <&>\n    second\n```")
            let formatter = Textual.Formatter(content)
            #expect(formatter.plainText().contains("  first <&>\n    second"))
            #expect(formatter.html().contains("  first &lt;&amp;&gt;\n    second"))
            #expect(!formatter.html().contains("<br />"))
            #expect(formatter.html().hasPrefix("<pre><code class=\"language-swift\">"))
        }

        #if canImport(AppKit)
            @Test @MainActor func nativeHTMLImportKeepsVisibleIntraParagraphBreaks() throws {
                let formatter = Textual.Formatter(try Self.observedSelection())
                let imported = try NSAttributedString(
                    data: Data(formatter.html().utf8),
                    options: [.documentType: NSAttributedString.DocumentType.html], documentAttributes: nil)
                // AppKit represents <br> as LINE SEPARATOR. This tests rich-copy semantics;
                // the separate explicit UTF-8 representation retains the canonical plain LF.
                #expect(imported.string == "Soft first\u{2028}Soft second\nHard first\u{2028}Hard second\n")
            }
        #endif

        private static func observedSelection() throws -> NSAttributedString {
            let output = NSMutableAttributedString(string: "")
            let pieces: [(String, Int)] = [
                ("Soft first", 1), ("\n", 1), ("Soft second", 1),
                ("Hard first", 2), ("\n", 2), ("Hard second", 2),
            ]
            for (index, piece) in pieces.enumerated() {
                var content = AttributedString(piece.0)
                content.presentationIntent = PresentationIntent(.paragraph, identity: piece.1)
                let native = NSMutableAttributedString(
                    attributedString: try NSAttributedString(content, including: \.textual))
                native.addAttribute(
                    NSAttributedString.Key("MapleSyntheticCopyProbeRun"), value: index,
                    range: NSRange(location: 0, length: native.length))
                output.append(native)
            }
            return output
        }
    }
#endif
