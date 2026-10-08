#if os(macOS)
    import Foundation
    import JavaScriptCore
    import Testing
    @testable import MapleAppUI

    @MainActor
    @Suite struct EquationJavaScriptTests {
        private func context() throws -> JSContext {
            let url = try #require(EquationResourcePolicy.resource("katex.min.js"))
            let script = try String(contentsOf: url, encoding: .utf8)
            let context = try #require(JSContext())
            context.evaluateScript(script)
            #expect(context.exception == nil)
            return context
        }

        private func render(_ formula: String, display: Bool = false, context: JSContext) -> String? {
            context.setObject(formula, forKeyedSubscript: "input" as NSString)
            context.setObject(display, forKeyedSubscript: "display" as NSString)
            context.exception = nil
            let result = context.evaluateScript(
                """
                katex.renderToString(input, { displayMode: display, output: 'html',
                    throwOnError: true, trust: false, strict: 'error', maxExpand: 1000,
                    maxSize: 16, macros: Object.create(null), globalGroup: false })
                """)
            return context.exception == nil ? result?.toString() : nil
        }

        @Test func pinnedRendererSupportsNativeCandidateGaps() throws {
            let js = try context()
            for source in [
                #"\operatorname{argmax}_{x} f(x)"#, #"\Bigl(\frac{a}{b}\Bigr)"#,
                #"\boxed{E=mc^2}"#, #"\newcommand{\RR}{\mathbb{R}}\RR^2"#,
                #"E=mc^2\tag{1}"#,
            ] {
                let output = try #require(render(source, display: true, context: js))
                #expect(output.contains("katex"))
                #expect(!output.contains("katex-error"))
            }
        }

        @Test func trustCommandsCannotCreateLinksImagesOrHTML() throws {
            let js = try context()
            for source in [
                #"\href{https://example.test/private}{x}"#,
                #"\url{https://example.test/private}"#,
                #"\includegraphics{https://example.test/private}"#,
                #"\htmlStyle{background:url(https://example.test/private)}{x}"#,
                #"\htmlClass{private}{x}"#,
            ] {
                if let output = render(source, context: js) {
                    #expect(!output.contains("<a "))
                    #expect(!output.contains("<img"))
                    #expect(!output.contains("background:url"))
                    #expect(!output.contains("class=\"private\""))
                }
            }
        }

        @Test func macroRecursionIsBoundedAndDefinitionsDoNotLeak() throws {
            let js = try context()
            #expect(render(#"\def\a{\a}\a"#, context: js) == nil)
            #expect(render(#"\newcommand{\private}{x}\private"#, context: js) != nil)
            #expect(render(#"\private"#, context: js) == nil)
        }

        @Test func hostileTextIsEscaped() throws {
            let js = try context()
            if let output = render(#"\text{</script><img src='https://example.test'>}"#, context: js) {
                #expect(!output.contains("</script>"))
                #expect(!output.contains("<img"))
            }
        }
    }
#endif
