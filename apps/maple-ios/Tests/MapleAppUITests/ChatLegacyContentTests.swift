import Foundation
import Testing
@testable import MapleAppUI

@Suite struct ChatLegacyContentTests {
    @Test func pendingDocumentNeverFallsThroughToRawJSONAndInvalidContentRecovers() throws {
        let generation = UUID()
        let source = ChatLegacyContent.Source(
            text: #"{"document":{"filename":"notes.txt","text_content":"Private document"}} Prompt"#,
            user: true, assistant: false, streaming: false, generation: generation)
        #expect(ChatLegacyContent.presentation(source: source, prepared: nil) == .pendingDocument)
        let prepared = ChatLegacyContent.prepare(source)
        let document = try #require(prepared.document)
        #expect(ChatLegacyContent.presentation(source: source, prepared: prepared) == .document(document))

        let incomplete = ChatLegacyContent.Source(
            text: #"{"document":{"filename":"notes.txt","text_content":"unfinished"}"#,
            user: true, assistant: false, streaming: false, generation: generation)
        #expect(ChatLegacyContent.presentation(source: incomplete, prepared: prepared) == .pendingDocument)
        #expect(
            ChatLegacyContent.presentation(source: incomplete, prepared: ChatLegacyContent.prepare(incomplete))
                == .ordinary)
    }

    @Test func oldAccountDocumentPreparationCannotRenderForTheNewGeneration() {
        let text = #"{"document":{"filename":"notes.txt","text_content":"Private document"}}"#
        let old = ChatLegacyContent.Source(
            text: text, user: true, assistant: false, streaming: false, generation: UUID())
        let current = ChatLegacyContent.Source(
            text: text, user: true, assistant: false, streaming: false, generation: UUID())
        #expect(
            ChatLegacyContent.presentation(source: current, prepared: ChatLegacyContent.prepare(old))
                == .pendingDocument)
    }

    @Test func firstThinkingTagWaitsWithAVisibleStateInsteadOfEmptyOrdinaryParts() {
        let generation = UUID()
        let ordinary = ChatLegacyContent.Source(
            text: "", user: false, assistant: false, streaming: true, generation: generation)
        let thinking = ChatLegacyContent.Source(
            text: "<think>", user: false, assistant: true, streaming: true, generation: generation)
        #expect(
            ChatLegacyContent.presentation(source: thinking, prepared: ChatLegacyContent.prepare(ordinary))
                == .pendingThinking)
        let prepared = ChatLegacyContent.prepare(thinking)
        #expect(prepared.parts.count == 1 && prepared.parts[0].kind == .thinking)
        #expect(ChatLegacyContent.presentation(source: thinking, prepared: prepared) == .assistant(prepared.parts))
    }

    @Test func streamingAppendKeepsRichPartsUntilTheExactNewSourcePublishes() {
        let generation = UUID()
        let previous = ChatLegacyContent.Source(
            text: "<think>Reasoning</think>**Answer**", user: false, assistant: true, streaming: true,
            generation: generation)
        let appended = ChatLegacyContent.Source(
            text: previous.text + " with [source](https://example.com)", user: false, assistant: true,
            streaming: true, generation: generation)
        let retained = ChatLegacyContent.prepare(previous)
        #expect(ChatLegacyContent.presentation(source: appended, prepared: retained) == .assistant(retained.parts))
        let current = ChatLegacyContent.prepare(appended)
        #expect(ChatLegacyContent.presentation(source: appended, prepared: current) == .assistant(current.parts))
        #expect(current.parts.last?.text == "**Answer** with [source](https://example.com)")
        #expect(current.parts.first?.kind == .thinking && current.parts.first?.text == "Reasoning")
    }

    @Test func rewrittenSettledAndNewAccountThinkingCannotReuseStreamingParts() {
        let generation = UUID()
        let previous = ChatLegacyContent.Source(
            text: "<think>Old reasoning", user: false, assistant: true, streaming: true, generation: generation)
        let retained = ChatLegacyContent.prepare(previous)
        let rewritten = ChatLegacyContent.Source(
            text: "<think>Different reasoning", user: false, assistant: true, streaming: true, generation: generation)
        let settled = ChatLegacyContent.Source(
            text: previous.text, user: false, assistant: true, streaming: false, generation: generation)
        let nextAccount = ChatLegacyContent.Source(
            text: previous.text + " continues", user: false, assistant: true, streaming: true, generation: UUID())
        for source in [rewritten, settled, nextAccount] {
            #expect(ChatLegacyContent.presentation(source: source, prepared: retained) == .pendingThinking)
        }
        let current = ChatLegacyContent.prepare(settled)
        #expect(ChatLegacyContent.presentation(source: settled, prepared: current) == .assistant(current.parts))
        #expect(current.parts.first?.isThinking == false)
    }

    @Test func ordinaryTextBypassesLegacyPreparationAndDoesNotReuseOldContent() {
        let generation = UUID()
        let old = ChatLegacyContent.Source(
            text: "<think>Reasoning</think>Answer", user: false, assistant: true, streaming: false,
            generation: generation)
        let source = ChatLegacyContent.Source(
            text: "**Plain answer**", user: false, assistant: false, streaming: false, generation: generation)
        #expect(!source.requiresPreparation)
        #expect(ChatLegacyContent.presentation(source: source, prepared: ChatLegacyContent.prepare(old)) == .ordinary)
    }

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
