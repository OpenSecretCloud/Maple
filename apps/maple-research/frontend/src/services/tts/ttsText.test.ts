import { describe, expect, test } from "bun:test";
import {
  chunkTextForTTS,
  sanitizeTextForTTS,
  TTS_CHUNK_MAX_UNBROKEN_CHARACTERS,
  TTS_CHUNK_MAX_WORDS
} from "./ttsText";

describe("sanitizeTextForTTS", () => {
  test("removes hidden, code, markup, tags, and emoji while preserving readable text", () => {
    const input = [
      "# **Grüße** <em>friend</em> 😊",
      "<think>Do not speak this.</think>",
      "```ts",
      "console.log('hidden');",
      "```",
      "Email me@example.com — e.g., tomorrow."
    ].join("\n");

    expect(sanitizeTextForTTS(input)).toBe(
      "Grüße friend\n\nEmail me at example.com, for example, tomorrow."
    );
  });

  test("turns separator dashes into natural pauses", () => {
    expect(sanitizeTextForTTS("hey - what's up")).toBe("hey, what's up");
    expect(sanitizeTextForTTS("Wait—really – yes ‑ maybe")).toBe("Wait, really, yes, maybe");
  });

  test("speaks compact numeric en-dash ranges as to", () => {
    expect(sanitizeTextForTTS("Read pages 1–5.")).toBe("Read pages 1 to 5.");
  });

  test("speaks compact date en-dash ranges as to", () => {
    expect(sanitizeTextForTTS("Available 2026-07-01–2026-07-31.")).toBe(
      "Available 2026-07-01 to 2026-07-31."
    );
  });

  test("speaks compact word en-dash ranges as to", () => {
    expect(sanitizeTextForTTS("Open Monday–Friday.")).toBe("Open Monday to Friday.");
  });

  test("preserves nonbreaking and figure hyphens inside words and numbers", () => {
    expect(sanitizeTextForTTS("state‑of‑the‑art and 123‒456 remain intact")).toBe(
      "state‑of‑the‑art and 123‒456 remain intact"
    );
  });

  test("strips unordered Markdown and Unicode list markers", () => {
    const input = [
      "- Hyphen bullet",
      "+ Plus bullet",
      "* Asterisk bullet",
      "• Round bullet",
      "◦ Hollow bullet",
      "▪ Square bullet"
    ].join("\n");

    expect(sanitizeTextForTTS(input)).toBe(
      [
        "Hyphen bullet.",
        "Plus bullet.",
        "Asterisk bullet.",
        "Round bullet.",
        "Hollow bullet.",
        "Square bullet."
      ].join("\n")
    );
  });

  test("strips reasonable ordered-list markers", () => {
    expect(sanitizeTextForTTS("1. First\n2) Second\n(3) Third\n1000. Last")).toBe(
      "First.\nSecond.\nThird.\nLast."
    );
  });

  test("preserves a spoken pause between list items when chunks are packed", () => {
    expect(chunkTextForTTS("- First\n- Second")).toEqual(["First. Second."]);
    expect(chunkTextForTTS("1. Alpha\n2. Beta!\n• Gamma")).toEqual(["Alpha. Beta! Gamma."]);
  });

  test("removes horizontal-rule-only lines while preserving paragraph boundaries", () => {
    expect(sanitizeTextForTTS("Before\n---\nAfter\n\n* * *\n\nFinally\n_ _ _")).toBe(
      "Before\n\nAfter\n\nFinally"
    );
  });

  test("preserves negatives, hyphenated words, and meaningful plus signs", () => {
    expect(sanitizeTextForTTS("It is -5 outside. Use state-of-the-art C++. 2 + 2 = 4.")).toBe(
      "It is -5 outside. Use state-of-the-art C++. 2 + 2 = 4."
    );
  });

  test("removes an unclosed think block and an unclosed code fence", () => {
    expect(sanitizeTextForTTS("Visible.\n<think>hidden forever")).toBe("Visible.");
    expect(sanitizeTextForTTS("Visible.\n~~~js\nhidden forever")).toBe("Visible.");
  });

  test("removes complete emoji sequences without invisible speech chunks", () => {
    const englandFlag = "\u{1f3f4}\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}";

    expect(sanitizeTextForTTS("❤️ 👨‍👩‍👧‍👦 1️⃣")).toBe("1");
    expect(chunkTextForTTS("❤️ 👨‍👩‍👧‍👦")).toEqual([]);
    expect(chunkTextForTTS(englandFlag)).toEqual([]);
  });
});

describe("chunkTextForTTS", () => {
  test("returns no chunks for content with nothing speakable", () => {
    expect(chunkTextForTTS("<think>secret</think>\n```\ncode\n```")).toEqual([]);
  });

  test("packs short paragraphs and adds ending punctuation", () => {
    expect(chunkTextForTTS("One\n\nTwo. Three", 4)).toEqual(["One Two. Three."]);
  });

  test("prefers sentence and word boundaries for long text", () => {
    expect(chunkTextForTTS("Hello world. Goodbye friend.", 2)).toEqual([
      "Hello world.",
      "Goodbye friend."
    ]);
  });

  test("splits an overlong sentence by words", () => {
    expect(chunkTextForTTS("one two three four five six", 2)).toEqual([
      "one two.",
      "three four.",
      "five six."
    ]);
  });

  test("keeps an ordinary 60-word paragraph in one provider request", () => {
    expect(chunkTextForTTS("word ".repeat(60).trim())).toHaveLength(1);
  });

  test("uses the 300-word request limit by default", () => {
    const chunks = chunkTextForTTS("word ".repeat(350).trim());
    expect(TTS_CHUNK_MAX_WORDS).toBe(300);
    expect(chunks.map((chunk) => chunk.split(/\s+/).length)).toEqual([300, 50]);
  });

  test("retains a generous Unicode safety split for pathological unbroken tokens", () => {
    const chunks = chunkTextForTTS("é".repeat(TTS_CHUNK_MAX_UNBROKEN_CHARACTERS + 1));
    expect(chunks.map((chunk) => Array.from(chunk.replace(/\.$/, "")).length)).toEqual([
      TTS_CHUNK_MAX_UNBROKEN_CHARACTERS,
      1
    ]);
  });

  test("rejects invalid chunk sizes", () => {
    expect(chunkTextForTTS("hello", 0)).toEqual([]);
    expect(chunkTextForTTS("hello", 1.5)).toEqual([]);
  });
});
