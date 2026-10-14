import Foundation
import MapleAppCore
import Testing
@testable import MapleAppUI

@Suite struct ComposerListContinuationTests {
    private func edit(_ text: String, selection: NSRange? = nil) -> (text: String, selection: NSRange)? {
        guard
            let edit = ComposerListContinuation.edit(
                text: text, selectionUTF16: selection ?? NSRange(location: text.utf16.count, length: 0)),
            let output = DictationTextEdit(
                beforeText: text, rangeUTF16: edit.rangeUTF16, replacement: edit.replacement
            ).resultingText
        else { return nil }
        return (output, edit.selectionUTF16)
    }

    @Test(arguments: ["-", "+", "*"])
    func unorderedMarkersContinueWithTheSameStyle(_ marker: String) {
        let text = marker + " first item"
        #expect(edit(text)?.text == text + "\n" + marker + " ")
        #expect(edit(text)?.selection == NSRange(location: text.utf16.count + 3, length: 0))
    }

    @Test(arguments: [
        ("- [ ] todo", "- [ ] "), ("* [x] done", "* [ ] "),
        ("\t+  [X]\tfinished", "\t+  [ ]\t"),
        ("  9) ninth item", "  10) "), ("\t99. ninety-ninth", "\t100. "),
        ("0009. leading zeroes", "10. "),
        ("999999999999999999999. large", "1000000000000000000000. "),
        ("- [x]not a task", "- "),
    ])
    func indentationSpacingTasksAndDecimalMarkersMatchResearch(_ text: String, _ prefix: String) {
        #expect(edit(text)?.text == text + "\n" + prefix)
        #expect(edit(text)?.selection == NSRange(location: text.utf16.count + prefix.utf16.count + 1, length: 0))
    }

    @Test func theCurrentMixedListAndTextAfterTheCaretArePreserved() {
        let mixed = "- first\n\n1. second\n\n* third"
        #expect(edit(mixed)?.text == mixed + "\n* ")
        #expect(edit("- item", selection: NSRange(location: 2, length: 0))?.text == "- \n- item")
        #expect(
            edit("- item", selection: NSRange(location: 2, length: 0))?.selection == NSRange(location: 5, length: 0))
    }

    @Test func selectedTextIncludingLaterLinesIsReplacedByOneExactNativeEdit() {
        let text = "1. first selected\nsecond\nthird", source = "1. first"
        let selection = NSRange(location: source.utf16.count, length: " selected\nsecond\n".utf16.count)
        let continuation = ComposerListContinuation.edit(text: text, selectionUTF16: selection)
        #expect(continuation?.rangeUTF16 == selection)
        #expect(continuation?.replacement == "\n2. ")
        #expect(edit(text, selection: selection)?.text == "1. first\n2. third")
        #expect(edit(text, selection: selection)?.selection == NSRange(location: source.utf16.count + 4, length: 0))
    }

    @Test(arguments: ["First\n  - ", "First\n\t3)  ", "First\n  * [x]  "])
    func anEmptyItemRemovesItsMarkerAndRetainsIndentation(_ text: String) {
        let line = text.components(separatedBy: "\n").last ?? ""
        let indentation = String(line.prefix(while: { $0 == " " || $0 == "\t" }))
        let expected = "First\n" + indentation
        #expect(edit(text)?.text == expected)
        #expect(edit(text)?.selection == NSRange(location: expected.utf16.count, length: 0))
    }

    @Test func anEmptyMarkerRemovesSelectedFollowingTextButPreservesUnselectedSuffix() {
        let text = "- \nselected\nsuffix"
        let selection = NSRange(location: 2, length: "\nselected".utf16.count)
        #expect(edit(text, selection: selection)?.text == "\nsuffix")
        #expect(edit(text, selection: selection)?.selection == NSRange(location: 0, length: 0))
        #expect(edit("- \nselected", selection: NSRange(location: 2, length: 9))?.text == "")
    }

    @Test(arguments: [
        "ordinary text", "prefix - item", "\\- escaped", "-not a list", "- - -", "* * * *",
        "```\n- code item", "~~~ts\n1. code item", "before\n````js\n* code item", "before\n~~~\n+ code item",
        "~~~~\n1. code\n~~~\n2. still code", "```\n~~~\n- still code", "```\n``` trailing\n- still code",
    ])
    func ordinaryTextHorizontalRulesAndOpenFencesDoNotContinue(_ text: String) {
        #expect(edit(text) == nil)
    }

    @Test func matchingOrLongerClosingFenceRestoresListContinuation() {
        let text = "```\n- code\n````\n- real"
        #expect(edit(text)?.text == text + "\n- ")
        let tildes = "   ~~~swift\n1. code\n   ~~~\t\n1. real"
        #expect(edit(tildes)?.text == tildes + "\n2. ")
    }

    @Test(arguments: ["\r", "\u{2028}", "\u{2029}"])
    func trailingLineTerminatorsDoNotPartiallyMatchListMarkers(_ terminator: String) {
        for prefix in ["- item", "- [x]", "3. item"] {
            #expect(edit(prefix + terminator) == nil)
        }
    }

    @Test(arguments: ["\r", "\u{2028}", "\u{2029}"])
    func aClosingFenceMustMatchTheWholeLineLikeResearch(_ terminator: String) {
        #expect(edit("```\ncode\n```" + terminator + "\n- still fenced") == nil)
    }

    @Test func nativeRangesUseUTF16AndInvalidOrSplitSurrogatesFailClosed() {
        let text = "Intro 🐝\n- café 🐝"
        let emoji = (text as NSString).range(of: "🐝", options: .backwards)
        #expect(edit(text)?.text == text + "\n- ")
        #expect(edit(text)?.selection == NSRange(location: text.utf16.count + 3, length: 0))
        for range in [
            NSRange(location: NSNotFound, length: 0), NSRange(location: -1, length: 0),
            NSRange(location: text.utf16.count + 1, length: 0), NSRange(location: 0, length: Int.max),
            NSRange(location: emoji.location + 1, length: 0),
            NSRange(location: emoji.location, length: 1), NSRange(location: emoji.location + 1, length: 1),
        ] {
            #expect(ComposerListContinuation.edit(text: text, selectionUTF16: range) == nil)
        }
    }
}
