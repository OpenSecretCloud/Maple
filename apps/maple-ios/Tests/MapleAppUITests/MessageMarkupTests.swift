import Foundation
import Testing
@testable import MapleAppUI

@Suite struct MessageMarkupTests {
    @Test func extractsMathWithoutLosingBackslashesOrOriginalDelimiters() async throws {
        let source = #"Energy $E=mc^2$ and \(\{x\in\mathbb{R}\}\)."#
        let result = try await MessageMarkupParser().parse(source)
        #expect(result.original == source)
        #expect(result.formulas.map(\.latex) == [#"E=mc^2"#, #"\{x\in\mathbb{R}\}"#])
        #expect(result.formulas.map(\.original) == [#"$E=mc^2$"#, #"\(\{x\in\mathbb{R}\}\)"#])
    }

    @Test func currencyAndNumericInlinePreserveCurrentBehavior() async throws {
        let result = try await MessageMarkupParser().parse(#"Pay $12.50 or $7; $1+2=3$ stays literal, $x+y$ is math."#)
        #expect(result.formulas.map(\.latex) == ["x+y"])
        #expect(result.markdown.contains("$12.50"))
        #expect(result.markdown.contains("$1+2=3$"))
    }

    @Test func codeNeverEntersMathPathAndMathFencesUseOwnedCodeRenderer() async throws {
        let source = "`$x$`\n\n```math\n\\boxed{x}\n```\n\n~~~swift\nlet price = \"$5\"\n~~~\n$x+y$"
        let result = try await MessageMarkupParser().parse(source)
        #expect(result.formulas.map(\.latex) == ["x+y"])
        #expect(result.codeBlocks["maplecode0"]?.language == "math")
        #expect(result.codeBlocks["maplecode0"]?.source == #"\boxed{x}"#)
        #expect(!result.markdown.contains("```math"))
        #expect(result.markdown.contains("`$x$`"))
    }

    @Test func incompleteStreamRemainsVisible() async throws {
        for source in [#"Text \(\frac{1}{"#, #"Text $\frac{1}{"#, "`unfinished $x$"] {
            let result = try await MessageMarkupParser().parse(source)
            #expect(result.formulas.isEmpty)
            #expect(result.markdown == source || result.literals.first?.source == String(source.dropFirst(5)))
        }
    }

    @Test func unclosedFenceRetainsCompleteCode() async throws {
        let result = try await MessageMarkupParser().parse("```swift\nlet x = 1\n$x$")
        #expect(result.formulas.isEmpty)
        #expect(result.codeBlocks["maplecode0"]?.source == "let x = 1\n$x$")
    }

    @Test func formulaBudgetRejectsDeepAndMalformedInputWithoutDroppingSource() async throws {
        var limits = MessageRenderLimits(); limits.formulaDepth = 2
        let result = try await MessageMarkupParser().parse("${{{x}}}$ $\\frac{1}{$", limits: limits)
        #expect(result.formulas.count == 2)
        #expect(result.formulas.allSatisfy { !$0.isWithinBudget })
        #expect(result.formulas[0].original == "${{{x}}}$")
    }

    @Test func formattingLimitKeepsEntireMessage() async throws {
        var limits = MessageRenderLimits(); limits.documentBytes = 8
        let source = "Message 👩🏽‍💻 with more than eight bytes"
        let result = try await MessageMarkupParser().parse(source, limits: limits)
        #expect(result.isPlainText)
        #expect(result.original == source && result.markdown == source)
    }

    @Test func reservedPlaceholderCannotBecomeAnAttachment() async throws {
        let source = "\u{E000}maple-equation-0\u{E001} $x$"
        let result = try await MessageMarkupParser().parse(source)
        #expect(result.isPlainText && result.formulas.isEmpty)
        #expect(result.markdown == source)
    }

    @Test func displayMathAndEscapedCurrencyHaveDistinctTokens() async throws {
        let result = try await MessageMarkupParser().parse(#"Cost \$5. Display \[x^2\] and $$y^2$$."#)
        #expect(result.formulas.map(\.display) == [true, true])
        #expect(result.formulas.map(\.latex) == ["x^2", "y^2"])
    }

    @Test func formulaCountLimitKeepsUnformattedRemainder() async throws {
        var limits = MessageRenderLimits(); limits.formulaCount = 1
        let result = try await MessageMarkupParser().parse("$x$ then $y$", limits: limits)
        #expect(result.formulas.count == 1)
        #expect(result.literals.first?.source == "$y$")
    }

    @Test func highlighterPreservesEveryUnicodeCharacter() {
        let source = "// العربية 中文 👩🏽‍💻\nlet value = \"é\\\"\"; return 42\n"
        let result = MessageCodeHighlighter.tokens(source, language: "swift")
        #expect(result.map(\.text).joined() == source)
        #expect(result.contains { $0.kind == .keyword && $0.text == "let" })
        #expect(result.contains { $0.kind == .number && $0.text == "42" })
    }

    @Test func highlighterBoundsAndUnknownLanguageUseCompletePlainSource() {
        let source = String(repeating: "/#-", count: 5000)
        #expect(MessageCodeHighlighter.tokens(source, language: "unknown") == [.init(text: source, kind: .plain)])
        #expect(
            MessageCodeHighlighter.tokens(source, language: "swift", byteLimit: 100) == [
                .init(text: source, kind: .plain)
            ])
        #expect(MessageCodeHighlighter.tokens(source, language: "swift") == [.init(text: source, kind: .plain)])
    }
}
