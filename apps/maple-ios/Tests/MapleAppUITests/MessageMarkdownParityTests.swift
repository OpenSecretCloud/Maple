import Foundation
import Testing
@testable import MapleAppUI
@testable import Textual

@Suite struct MessageMarkdownParityTests {
    @Test func nativeCopyKeepsExplicitHardBreaksInPlainTextAndHTML() throws {
        let content = try AttributedString(markdown: "Hard first  \nHard second")
        let formatter = Textual.Formatter(content)
        #expect(formatter.plainText() == "Hard first\nHard second")
        #expect(formatter.html() == "<p>Hard first<br />Hard second</p>")
    }

    @Test func researchSoftBreaksRenderAndExportAsActualLineBreaks() async throws {
        let prepared = try await MessageDocumentWorker().prepare("Soft first\nSoft second\n\nHard first  \nHard second")
        let formatter = Textual.Formatter(prepared.content)
        #expect(formatter.plainText() == "Soft first\nSoft second\n\nHard first\nHard second")
        #expect(formatter.html() == "<p>Soft first<br />Soft second</p>\n<p>Hard first<br />Hard second</p>")
    }

    @Test func partialNativeSelectionRetainsBreaksAndInlineStyles() async throws {
        let prepared = try await MessageDocumentWorker().prepare("prefix **bold first**\n*italic second* suffix")
        let start = try #require(prepared.content.range(of: "first")?.lowerBound)
        let end = try #require(prepared.content.range(of: "second")?.upperBound)
        let formatter = Textual.Formatter(AttributedString(prepared.content[start..<end]))
        #expect(formatter.plainText() == "first\nitalic second")
        #expect(formatter.html().contains("<br />"))
        #expect(formatter.html().contains("<strong>first</strong>"))
        #expect(formatter.html().contains("<em>italic second</em>"))
    }

    @Test func indentedOrdinaryProseUsesInlineFormattingNotCode() async throws {
        let result = try await MessageDocumentWorker().prepare("    **Indented** ordinary\n    second line")
        #expect(result.codeBlocks.isEmpty)
        #expect(result.content.runs.contains { $0.inlinePresentationIntent?.contains(.stronglyEmphasized) == true })
        #expect(Textual.Formatter(result.content).plainText() == "Indented ordinary\nsecond line")
        #expect(!Textual.Formatter(result.content).html().contains("<pre>"))
    }

    @Test func indentedProseCanContainMathAndCannotIntroduceUnsafeLinks() async throws {
        let result = try await MessageDocumentWorker().prepare("    $x$ [unsafe](javascript:alert%281%29)")
        #expect(result.codeBlocks.isEmpty)
        #expect(result.formulas.count == 1)
        #expect(result.content.runs.allSatisfy { $0.link == nil && $0.imageURL == nil })
    }

    @Test(arguments: [
        "```\n**literal**\n    keep indentation\n```",
        "> ```\n> **literal**\n>     keep indentation\n> ```",
        "- item\n\n    ```\n    **literal**\n        keep indentation\n    ```",
        "> ~~~math\n> $\\boxed{x}$\n> ~~~",
    ])
    func fencedCodeRetainsLiteralSourceAndNeverEntersEquationPath(_ source: String) async throws {
        let result = try await MessageDocumentWorker().prepare(source)
        #expect(!result.codeBlocks.isEmpty)
        #expect(result.formulas.isEmpty)
        #expect(!result.content.runs.contains { $0.inlinePresentationIntent?.contains(.stronglyEmphasized) == true })
        if source.contains("keep indentation") {
            #expect(result.codeBlocks.values.contains { $0.block.source.contains("    keep indentation") })
        }
    }

    @Test func realTaskMarkersHaveCheckedStateWhileInlineCodeRemainsLiteral() async throws {
        let result = try await MessageDocumentWorker().prepare(
            "- [ ] Todo\n- [x] Done\n- [X]\tDone too\n- `[ ]` code\n\n[ ] ordinary")
        let text = Textual.Formatter(result.content).plainText()
        #expect(text.contains("☐ Todo"))
        #expect(text.contains("☑ Done"))
        #expect(text.contains("☑\tDone too"))
        #expect(text.contains("[ ] code"))
        #expect(text.contains("[ ] ordinary"))
    }

    @Test func ASTTaskRecognitionExcludesEscapedMarkersAndInlineCode() throws {
        // Test the Markdown AST contract directly: in a whole message, \[ is
        // also an explicit TeX opener and its incomplete suffix stays literal.
        let source = "- \\[ ] Escaped\n- `[ ]` code"
        let parsed = try AttributedString(
            markdown: source, options: .init(interpretedSyntax: .full, appliesSourcePositionAttributes: true))
        let content = try MessageMarkdownParity.normalize(parsed, source: source)
        let text = Textual.Formatter(content).plainText()
        #expect(text.contains("[ ] Escaped"))
        #expect(text.contains("[ ] code"))
        #expect(!text.contains("☐"))
    }

    @Test func multilineInlineCodeAndLinkDestinationsAreUnmodified() async throws {
        let source = "`first\nsecond` [safe](https://example.test/$x$/path?q=first%0Asecond)"
        let foundation = try AttributedString(markdown: source)
        let result = try await MessageDocumentWorker().prepare(source)
        let before = foundation.runs.filter { $0.inlinePresentationIntent?.contains(.code) == true }
            .map { String(foundation[$0.range].characters) }
        let after = result.content.runs.filter { $0.inlinePresentationIntent?.contains(.code) == true }
            .map { String(result.content[$0.range].characters) }
        #expect(after == before)
        #expect(
            result.content.runs.compactMap(\.link).map(\.absoluteString)
                == ["https://example.test/$x$/path?q=first%0Asecond"])
        #expect(result.formulas.isEmpty)
    }

    @Test func indentedLiteralFenceDoesNotBecomeAProtectedFencedBlock() async throws {
        let result = try await MessageDocumentWorker().prepare("    ```\n    ordinary\n    ```")
        #expect(result.codeBlocks.isEmpty)
    }
}
