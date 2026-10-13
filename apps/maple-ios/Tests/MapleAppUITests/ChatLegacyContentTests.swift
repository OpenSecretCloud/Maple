import Foundation
import Testing
@testable import MapleAppUI

@Suite struct ChatLegacyContentTests {
    @Test func documentSplitsEscapedPrefixFromOrdinaryUserText() throws {
        let source =
            #"Before {"document":{"filename":"notes}.txt","text_content":"A \"quoted\" {line}\nSecond line"}} After **prompt**"#
        let document = try #require(ChatLegacyContent.document(in: source))
        #expect(document.filename == "notes}.txt")
        #expect(document.text == "A \"quoted\" {line}\nSecond line")
        #expect(document.before == "Before")
        #expect(document.after == "After **prompt**")
    }

    @Test(arguments: [
        #"{"document":{"filename":"notes.txt","text_content":null}}"#,
        #"{"document":{"filename":7,"text_content":"notes"}}"#,
        #"{"document":{"filename":"notes.txt"}}"#,
        #"{"document":{"filename":"notes.txt","text_content":"unfinished"}"#,
        #"{"document" :{"filename":"notes.txt","text_content":"not the legacy prefix"}}"#,
    ])
    func invalidOrIncompleteDocumentRemainsOrdinaryText(_ source: String) {
        #expect(ChatLegacyContent.document(in: source) == nil)
    }

    @Test func emptyDocumentContentIsStillAValidPreview() throws {
        let document = try #require(
            ChatLegacyContent.document(in: #"{"document":{"filename":"empty.txt","text_content":""}}"#))
        #expect(document.text.isEmpty && document.before.isEmpty && document.after.isEmpty)
    }

    @Test func documentScannerReadsJSONQuotesBeforeACombiningFilenameScalar() throws {
        let document = try #require(
            ChatLegacyContent.document(
                in:
                    "{\"document\":{\"filename\":\"\u{0301}notes.txt\",\"text_content\":\"Text\"}} Prompt"))
        #expect(document.filename == "\u{0301}notes.txt")
        #expect(document.text == "Text" && document.after == "Prompt")
    }

    @Test func storedThinkingTagsRetainTextOrderAndStableOffsets() {
        let parts = ChatLegacyContent.assistantParts(
            "Before<think>Reasoning</think>After<think>More</think>End", streaming: false)
        #expect(parts.map(\.kind) == [.text, .thinking, .text, .thinking, .text])
        #expect(parts.map(\.text) == ["Before", "Reasoning", "After", "More", "End"])
        #expect(parts.map(\.id) == [0, 6, 30, 35, 54])
        #expect(parts.allSatisfy { !$0.isThinking })
    }

    @Test func streamingEmptyAndUnclosedThinkingRemainVisibleUntilSettled() {
        let empty = ChatLegacyContent.assistantParts("<think>", streaming: true)
        #expect(empty.count == 1 && empty[0].kind == .thinking && empty[0].isThinking)
        #expect(ChatLegacyContent.assistantParts("<think> \n", streaming: false).isEmpty)
        let unfinished = ChatLegacyContent.assistantParts("<think>Still thinking", streaming: true)
        #expect(unfinished.last?.isThinking == true)
        let recovered = ChatLegacyContent.assistantParts("<think>Still thinking", streaming: false)
        #expect(recovered.last?.text == "Still thinking" && recovered.last?.isThinking == false)
        let closed = ChatLegacyContent.assistantParts("<think>Done</think>Answer", streaming: true)
        #expect(closed.map(\.kind) == [.thinking, .text])
        #expect(closed.allSatisfy { !$0.isThinking })
    }

    @Test func ordinaryMarkdownAndPartialOpeningTagsStayUnchanged() {
        for text in ["**Answer**\n\n[Link](https://example.com)", "<thi", "Answer </think>"] {
            let parts = ChatLegacyContent.assistantParts(text, streaming: true)
            #expect(parts.count == 1 && parts[0].kind == .text && parts[0].text == text)
        }
    }

    @Test func completedStandaloneUnclosedThinkingTagRecoversTheAnswerAsText() {
        let completed = ChatLegacyContent.assistantParts("<think>\nAnswer", streaming: false)
        #expect(completed.count == 1 && completed[0].kind == .text && completed[0].text == "Answer")
        let streaming = ChatLegacyContent.assistantParts("<think>\nAnswer", streaming: true)
        #expect(streaming.count == 1 && streaming[0].kind == .thinking && streaming[0].isThinking)
        let nonemptyOpening = ChatLegacyContent.assistantParts("<think>Reasoning\nAnswer", streaming: false)
        #expect(nonemptyOpening.count == 1 && nonemptyOpening[0].kind == .thinking)
    }
}
