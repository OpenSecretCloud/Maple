import Foundation
import Testing
import SwiftUI
import Textual
@testable import MapleAppUI

@Suite struct MessageDocumentTests {
    @Test func equationEnrichmentChangesPresentationKeyWithoutRandomRefreshes() async throws {
        let worker = MessageDocumentWorker()
        let draft = try await worker.prepare("Equation $x$.")
        let id = try #require(draft.formulas.first?.id)
        let plain = try await worker.resolve(draft, equations: [:])
        let equations = [id: AnyAttachment(TestEquation())]
        let rendered = try await worker.resolve(draft, equations: equations)
        let repeated = try await worker.resolve(draft, equations: equations)
        #expect(plain.revision == rendered.revision)
        #expect(plain.presentationKey != rendered.presentationKey)
        #expect(rendered.presentationKey == repeated.presentationKey)
        #expect(rendered.content.runs.contains { $0.textual.attachment?.description == "$x$" })
    }

    @Test func incompleteTeXKeepsBackslashesAfterMarkdownParsing() async throws {
        for source in [#"Text \(\frac{1}{"#, #"Text \[\alpha +"#, #"Text $\frac{1}{"#] {
            let worker = MessageDocumentWorker()
            let draft = try await worker.prepare(source)
            let result = try await worker.resolve(draft, equations: [:])
            #expect(String(result.content.characters) == source)
            #expect(result.formulas.isEmpty)
        }
    }

    @Test func quotedAndNestedFencedCodeNeverReachesEquationRenderer() async throws {
        for source in [
            "> ~~~math\n> $\\boxed{x}$\n> ~~~", "- code\n\n    ```math\n    $x$\n    ```",
        ] {
            let result = try await MessageDocumentWorker().prepare(source)
            #expect(result.formulas.isEmpty)
            #expect(!result.codeBlocks.isEmpty)
            for run in result.content.runs {
                for intent in run.presentationIntent?.components ?? [] {
                    if case .codeBlock(let hint) = intent.kind {
                        let key = try #require(hint)
                        #expect(result.codeBlocks[key] != nil)
                        #expect(key != "math")
                    }
                }
            }
            #expect(!String(result.content.characters).contains("\u{E000}"))
        }
    }

    @Test func URLsRemainIntactAndUnsafeSchemesAndImagesHaveNoCapability() async throws {
        let source =
            #"[safe](https://example.test/$x$/path) [bad](javascript:alert%281%29) [private](https://user:password@example.test) ![alt](https://example.test/image)"#
        let result = try await MessageDocumentWorker().prepare(source)
        #expect(result.formulas.isEmpty)
        let links = result.content.runs.compactMap(\.link)
        #expect(links.map(\.absoluteString) == ["https://example.test/$x$/path"])
        #expect(result.content.runs.allSatisfy { $0.imageURL == nil })
    }

    @Test func gfmKeepsTableListAndStrikethroughStructure() async throws {
        let result = try await MessageDocumentWorker().prepare(
            "~~strike~~\n\n- item\n\n| A | B |\n| --- | --- |\n| one | two |")
        #expect(result.content.runs.contains { $0.inlinePresentationIntent?.contains(.strikethrough) == true })
        #expect(
            result.content.runs.contains { run in
                run.presentationIntent?.components.contains {
                    if case .table = $0.kind { return true }; return false
                } == true
            })
        #expect(
            result.content.runs.contains { run in
                run.presentationIntent?.components.contains {
                    if case .unorderedList = $0.kind { return true }; return false
                } == true
            })
    }
}

private struct TestEquation: Textual.Attachment {
    var description: String { "$x$" }
    var body: some View { Text("x") }
    func sizeThatFits(_ proposal: ProposedViewSize, in environment: TextEnvironmentValues) -> CGSize {
        CGSize(width: 10, height: 10)
    }
}
