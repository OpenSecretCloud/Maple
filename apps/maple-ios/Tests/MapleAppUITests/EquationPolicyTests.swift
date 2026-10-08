import Foundation
import Testing
@testable import MapleAppUI

@Suite struct EquationPolicyTests {
    @Test func admitsOnlyPackagedAssets() throws {
        for path in EquationResourcePolicy.paths {
            let url = try #require(URL(string: EquationResourcePolicy.origin + "/" + path))
            #expect(EquationResourcePolicy.path(for: url) == path)
        }
    }

    @Test func rejectsExternalCredentialAndTraversalPaths() throws {
        let rejected = [
            "https://example.test/katex.min.js", "http://127.0.0.1:12345/index.html",
            "file:///etc/passwd", "data:text/html,hello", "javascript:alert(1)",
            "maple-equation://other/index.html", "maple-equation://bundle:80/index.html",
            "maple-equation://user@bundle/index.html", "maple-equation://bundle/index.html?private=value",
            "maple-equation://bundle/index.html#private", "maple-equation://bundle/%69ndex.html",
            "maple-equation://bundle/fonts/../index.html", "maple-equation://bundle//index.html",
            "maple-equation://bundle/unknown.js", "maple-equation://bundle/fonts/KaTeX_Main-Regular.ttf",
        ]
        for string in rejected {
            let url = try #require(URL(string: string))
            #expect(EquationResourcePolicy.path(for: url) == nil)
        }
    }

    @Test func contentBlockerHasOnlyOneExactAssetOriginException() throws {
        let data = try #require(EquationResourcePolicy.contentRules.data(using: .utf8))
        let rules = try #require(JSONSerialization.jsonObject(with: data) as? [[String: [String: String]]])
        #expect(rules.count == 2)
        #expect(rules[0]["trigger"]?["url-filter"] == ".*")
        #expect(rules[0]["action"]?["type"] == "block")
        #expect(rules[1]["trigger"]?["url-filter"] == "^maple-equation://bundle/")
    }

    @Test func bootstrapNavigationRequiresOneExactMainDocument() throws {
        var policy = EquationBootstrapPolicy()
        let bootstrap = EquationResourcePolicy.bootstrap
        #expect(!policy.allowsResponse(bootstrap, mainFrame: true))
        let missingURLAdmitted = policy.admit(nil, mainFrame: true, other: true)
        #expect(!missingURLAdmitted)
        for string in [
            "about:blank#fragment", "about:blank?query", "about:srcdoc", "about:BLANK",
            "about://blank", "about:%62lank", "https://example.test/", "file:///index.html",
            "data:text/html,hello", "javascript:alert(1)", "maple-equation://bundle/index.html",
        ] {
            let url = try #require(URL(string: string))
            let invalidURLAdmitted = policy.admit(url, mainFrame: true, other: true)
            #expect(!invalidURLAdmitted)
            #expect(!policy.allowsResponse(url, mainFrame: true))
        }
        let childFrameAdmitted = policy.admit(bootstrap, mainFrame: false, other: true)
        #expect(!childFrameAdmitted)
        let otherTypeAdmitted = policy.admit(bootstrap, mainFrame: true, other: false)
        #expect(!otherTypeAdmitted)
        #expect(!policy.started)
        let firstDocumentAdmitted = policy.admit(bootstrap, mainFrame: true, other: true)
        #expect(firstDocumentAdmitted)
        #expect(policy.started)
        let repeatedDocumentAdmitted = policy.admit(bootstrap, mainFrame: true, other: true)
        #expect(!repeatedDocumentAdmitted)
        #expect(policy.allowsResponse(bootstrap, mainFrame: true))
        #expect(!policy.allowsResponse(bootstrap, mainFrame: false))
        #expect(!policy.allowsResponse(nil, mainFrame: true))
        #expect(!policy.allowsResponse(URL(string: "https://example.test/"), mainFrame: true))
    }

    @Test func bootstrapDocumentUsesOnlyAbsolutePackagedAssets() throws {
        let html = try #require(EquationResourcePolicy.bootstrapHTML())
        let pattern = try NSRegularExpression(pattern: #"(?:src|href)="([^"]+)""#)
        let range = NSRange(html.startIndex..., in: html)
        let references = try pattern.matches(in: html, range: range).map { match in
            let valueRange = try #require(Range(match.range(at: 1), in: html))
            let url = try #require(URL(string: String(html[valueRange])))
            return try #require(EquationResourcePolicy.path(for: url))
        }
        #expect(references == ["katex.min.css", "renderer.css", "katex.min.js", "renderer.js"])
        #expect(!html.contains("<base"))
        #expect(html.contains("base-uri 'none'"))
        #expect(html.contains("connect-src 'none'"))
        #expect(html.contains("form-action 'none'"))
        #expect(EquationResourcePolicy.path(for: EquationResourcePolicy.bootstrap) == nil)
    }
}
