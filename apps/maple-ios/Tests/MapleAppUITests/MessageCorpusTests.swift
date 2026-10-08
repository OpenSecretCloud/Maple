#if DEBUG
    import Foundation
    import Testing
    @testable import MapleAppUI
    @testable import Textual

    @Suite struct MessageCorpusTests {
        @Test func fullNativeCorpusKeepsOnlyTheExplicitFenceAsCode() async throws {
            let prepared = try await MessageDocumentWorker().prepare(RendererAcceptanceCorpus.markdown)
            #expect(prepared.codeBlocks.count == 1)
            #expect(prepared.codeBlocks.values.first?.block.language == "swift")
            let text = Textual.Formatter(prepared.content).plainText()
            #expect(text.contains("Indented ordinary prose in the Research baseline."))
            #expect(text.contains("Soft first\nSoft second\n\nHard first\nHard second"))
        }

        @Test(arguments: ["\r\n", "\r"])
        func originalSourceLineLookupHandlesCommonMarkNewlines(_ newline: String) throws {
            let source = RendererAcceptanceCorpus.markdown.replacingOccurrences(of: "\n", with: newline)
            let parsed = try AttributedString(
                markdown: source, options: .init(interpretedSyntax: .full, appliesSourcePositionAttributes: true))
            let content = try MessageMarkdownParity.normalize(parsed, source: source)
            let codeRuns = content.runs.filter { run in
                run.presentationIntent?.components.contains { component in
                    if case .codeBlock = component.kind { return true }
                    return false
                } == true
            }
            #expect(codeRuns.count == 1)
            #expect(!codeRuns.contains { String(content[$0.range].characters).contains("Indented ordinary prose") })
            #expect(
                Textual.Formatter(content).plainText().contains("Indented ordinary prose in the Research baseline."))
        }

        @Test func plainCopySurvivesTheNativeAttributedStringBridge() async throws {
            let prepared = try await MessageDocumentWorker().prepare(
                "Soft first\nSoft second\n\nHard first  \nHard second")
            let native = try NSAttributedString(prepared.content, including: \.textual)
            #expect(Textual.Formatter(native).plainText() == "Soft first\nSoft second\n\nHard first\nHard second")
        }

        @Test func originalEquationAccessibilitySurvivesUnresolvedRasterization() async throws {
            let worker = MessageDocumentWorker()
            let prepared = try await worker.prepare(#"Original \(\boxed{x}\) and $y^2$."#)
            let original = try await worker.resolve(prepared, equations: [:])
            #expect(original.equationSources == [#"\(\boxed{x}\)"#, "$y^2$"])
            #expect(original.content.characters.contains("x"))
        }
    }
#endif
