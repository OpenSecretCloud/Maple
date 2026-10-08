import Foundation
import Testing
@testable import MapleAppCore

@Suite struct SpeechTextPlannerTests {
    @Test(arguments: [
        (
            "# **Grüße** <em>friend</em> 😊\n<think>Do not speak this.</think>\n```ts\nconsole.log('hidden');\n```\nEmail me@example.com — e.g., tomorrow.",
            "Grüße friend\n\nEmail me at example.com, for example, tomorrow."
        ),
        ("hey - what's up", "hey, what's up"),
        ("Wait—really – yes ‑ maybe", "Wait, really, yes, maybe"),
        ("Read pages 1–5.", "Read pages 1 to 5."),
        ("Ranges 1–5–10.", "Ranges 1 to 5 to 10."),
        ("Already, — paused; — again.", "Already, paused; again."),
        ("— Leading and trailing —", "Leading and trailing"),
        ("Available 2026-07-01–2026-07-31.", "Available 2026-07-01 to 2026-07-31."),
        ("Open Monday–Friday.", "Open Monday to Friday."),
        ("state‑of‑the‑art and 123‒456 remain intact", "state‑of‑the‑art and 123‒456 remain intact"),
        (
            "- Hyphen bullet\n+ Plus bullet\n* Asterisk bullet\n• Round bullet\n◦ Hollow bullet\n▪ Square bullet",
            "Hyphen bullet.\nPlus bullet.\nAsterisk bullet.\nRound bullet.\nHollow bullet.\nSquare bullet."
        ),
        ("1. First\n2) Second\n(3) Third\n1000. Last", "First.\nSecond.\nThird.\nLast."),
        ("Before\n---\nAfter\n\n* * *\n\nFinally\n_ _ _", "Before\n\nAfter\n\nFinally"),
        (
            "It is -5 outside. Use state-of-the-art C++. 2 + 2 = 4.",
            "It is -5 outside. Use state-of-the-art C++. 2 + 2 = 4."
        ),
        ("Visible.\n<think>hidden forever", "Visible."),
        ("Visible.\n~~~js\nhidden forever", "Visible."),
        ("❤️ 👨‍👩‍👧‍👦 1️⃣", "1"),
    ])
    func matchesResearchSanitization(input: String, expected: String) throws {
        let sanitized = try SpeechTextPlanner.sanitize(input)
        #expect(sanitized == expected)
    }

    @Test func emptyHiddenAndEmojiContentNeverProducesSpeech() throws {
        for text in [
            "", "❤️ 👨‍👩‍👧‍👦", "<think>secret</think>\n```\ncode\n```",
            "\u{1f3f4}\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}",
        ] {
            #expect(try SpeechTextPlanner.chunks(text).isEmpty)
        }
    }
    @Test func chunksAtParagraphSentenceAndWordBoundaries() throws {
        #expect(try SpeechTextPlanner.chunks("One\n\nTwo. Three", maximumWords: 4) == ["One Two. Three."])
        #expect(
            try SpeechTextPlanner.chunks("Hello world. Goodbye friend.", maximumWords: 2) == [
                "Hello world.", "Goodbye friend.",
            ])
        #expect(
            try SpeechTextPlanner.chunks("one two three four five six", maximumWords: 2) == [
                "one two.", "three four.", "five six.",
            ])
        #expect(try SpeechTextPlanner.chunks("- First\n- Second") == ["First. Second."])
        #expect(try SpeechTextPlanner.chunks("word ".repeating(60)).count == 1)
        #expect(
            try SpeechTextPlanner.chunks("word ".repeating(350)).map { $0.split(separator: " ").count } == [300, 50])
    }
    @Test func unicodeSafetySplitKeepsComposedCharactersAndBoundsProviderCharacters() throws {
        let chunks = try SpeechTextPlanner.chunks("é".repeating(1_001))
        #expect(chunks.map { $0.dropLast().unicodeScalars.count } == [1_000, 1])
        let grouped = try SpeechTextPlanner.chunks(("a".repeating(999) + " ").repeating(300))
        #expect(grouped.allSatisfy { $0.unicodeScalars.count <= 100_000 && $0.split(separator: " ").count <= 300 })
        let pathological = "a" + "\u{301}".repeating(1_002)
        #expect(throws: SpeechIssue.capacity) { try SpeechTextPlanner.chunks(pathological) }
    }
    @Test func limitsRejectRatherThanTruncateText() throws {
        #expect(throws: SpeechIssue.capacity) {
            try SpeechTextPlanner.chunks("a".repeating(SpeechLimits.maximumSourceBytes + 1))
        }
        #expect(throws: SpeechIssue.capacity) { try SpeechTextPlanner.chunks("word ".repeating(77_100)) }
        #expect(try SpeechTextPlanner.chunks("hello", maximumWords: 0).isEmpty)
        #expect(try SpeechTextPlanner.chunks("hello", maximumWords: 301).isEmpty)
    }
}

private extension String { func repeating(_ count: Int) -> String { String(repeating: self, count: count) } }
