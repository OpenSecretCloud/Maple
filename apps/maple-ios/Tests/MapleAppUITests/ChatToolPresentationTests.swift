import Foundation
import MapleSDK
import Testing
@testable import MapleAppUI

@Suite struct ChatToolPresentationTests {
    private let english = Locale(identifier: "en")

    @Test func titlesDecodeStringArgumentsAndKeepUserValuesVerbatim() {
        #expect(
            ChatToolPresentation.title(
                name: "web_search", arguments: .string(#"{"query":" garden "}"#), locale: english)
                == "Web Search: \" garden \"")
        #expect(
            ChatToolPresentation.title(
                name: "open_url", arguments: .object(["url": .string("https://example.com")]), locale: english)
                == "Open URL: https://example.com")
        #expect(
            ChatToolPresentation.title(
                name: "open_urls", arguments: .object(["urls": .array([.string(" "), .integer(3), .string("one")])]),
                locale: english) == "Open URL: one")
        #expect(
            ChatToolPresentation.title(
                name: "open_urls", arguments: .object(["urls": .array([.string("one"), .string("two")])]),
                locale: english) == "Open URLs: 2 pages")
        #expect(
            ChatToolPresentation.title(name: "read_image", arguments: .string(#"{"image_number":2}"#), locale: english)
                == "Read image: Image 2")
        #expect(
            ChatToolPresentation.title(name: "other", arguments: .object(["query": .string("term")]), locale: english)
                == "other: \"term\"")
        #expect(ChatToolPresentation.title(name: "function", arguments: nil, locale: english) == "Tool call")
        #expect(ChatToolPresentation.title(name: nil, arguments: nil, webSearch: true, locale: english) == "Web Search")
    }

    @Test func callStatusMatchesResearchPrecedenceAndOrphanOutputs() {
        var call = ConversationItem(id: "call", type: "function_call", status: "completed")
        var result = ConversationItem(id: "result", type: "function_call_output", status: "streaming")
        #expect(ChatToolPresentation.status(item: call, result: nil) == .active)
        #expect(ChatToolPresentation.status(item: call, result: result) == .active)
        result.status = "completed"
        #expect(ChatToolPresentation.status(item: call, result: result) == .completed)
        call.status = "incomplete"
        #expect(ChatToolPresentation.status(item: call, result: result) == .incomplete)
        result.status = "failed"
        #expect(ChatToolPresentation.status(item: call, result: result) == .failed)
        result.status = nil
        #expect(ChatToolPresentation.status(item: result, result: nil) == .completed)
        result.status = "streaming"
        #expect(ChatToolPresentation.status(item: result, result: nil) == .active)
        call.type = "web_search_call"; call.status = "searching"
        #expect(ChatToolPresentation.status(item: call, result: nil) == .active)
        call.status = nil
        #expect(ChatToolPresentation.status(item: call, result: nil) == .failed)
    }

    @Test func inputIsPrettyJSONAndOutputIsMarkdownWithoutJSONQuoting() {
        let call = ConversationItem(id: "call", type: "function_call", arguments: .string(#"{"query":"garden","n":2}"#))
        let result = ConversationItem(
            id: "result", type: "function_call_output", output: .string("**Results**\n[Garden](https://example.com)"))
        let details = ChatToolPresentation.details(item: call, result: result)
        #expect(details.input.contains("\n") && details.input.contains("\"query\" : \"garden\""))
        #expect(details.output == "**Results**\n[Garden](https://example.com)")
        #expect(ChatToolPresentation.details(item: result, result: nil).input.isEmpty)
        let incomplete = ConversationItem(id: "incomplete", type: "tool_call", arguments: .string("{\"query\":"))
        #expect(ChatToolPresentation.details(item: incomplete, result: nil).input == "{\"query\":")
    }

    @Test func previewKeepsFullPlainURLDestinationsAndAvoidsBrokenMarkdownLinks() {
        let url = "https://example.com/" + String(repeating: "long", count: 50)
        let text = "See " + url
        let preview = ChatToolPresentation.preview(text)
        #expect(preview.hasPrefix("See [https://example.com/"))
        #expect(preview.hasSuffix("](" + url + ")"))
        let linked = String(repeating: "x", count: 130) + " [Garden](" + url + ")"
        #expect(ChatToolPresentation.preview(linked) == String(repeating: "x", count: 130) + "...")
        #expect(
            ChatToolPresentation.preview("Small [link](https://example.com)") == "Small [link](https://example.com)")
        let unicode = String(repeating: "a", count: 149) + "😀tail"
        #expect(ChatToolPresentation.preview(unicode) == String(repeating: "a", count: 149) + "...")
    }

    @Test func blankArgumentsDoNotProduceAnInputDisclosure() {
        let blank = ConversationItem(id: "blank", type: "tool_call", arguments: .string(" \n\t "))
        let details = ChatToolPresentation.details(item: blank, result: nil)
        #expect(details.input.isEmpty && !details.hasDetails)
        let partial = ConversationItem(id: "partial", type: "tool_call", arguments: .string("  partial  "))
        #expect(ChatToolPresentation.details(item: partial, result: nil).input == "  partial  ")
    }
}
