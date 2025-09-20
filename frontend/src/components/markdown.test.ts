import { describe, it, expect } from "bun:test";
import { stripMarkdownForTTS } from "./markdown";

describe("stripMarkdownForTTS", () => {
  describe("emphasis markers", () => {
    it("should remove bold markers with asterisks", () => {
      expect(stripMarkdownForTTS("This is **bold text** here")).toBe("This is bold text here");
      expect(stripMarkdownForTTS("**Bold at start** of sentence")).toBe(
        "Bold at start of sentence"
      );
      expect(stripMarkdownForTTS("End with **bold**")).toBe("End with bold");
    });

    it("should remove bold markers with underscores", () => {
      expect(stripMarkdownForTTS("This is __bold text__ here")).toBe("This is bold text here");
      expect(stripMarkdownForTTS("__Bold at start__ of sentence")).toBe(
        "Bold at start of sentence"
      );
    });

    it("should remove italic markers with asterisks", () => {
      expect(stripMarkdownForTTS("This is *italic text* here")).toBe("This is italic text here");
      expect(stripMarkdownForTTS("*Italic at start* of sentence")).toBe(
        "Italic at start of sentence"
      );
      expect(stripMarkdownForTTS("End with *italic*")).toBe("End with italic");
    });

    it("should remove italic markers with underscores", () => {
      expect(stripMarkdownForTTS("This is _italic text_ here")).toBe("This is italic text here");
      expect(stripMarkdownForTTS("_Italic at start_ of sentence")).toBe(
        "Italic at start of sentence"
      );
    });

    it("should remove bold italic markers", () => {
      expect(stripMarkdownForTTS("This is ***bold italic*** text")).toBe(
        "This is bold italic text"
      );
      expect(stripMarkdownForTTS("This is ___bold italic___ text")).toBe(
        "This is bold italic text"
      );
    });

    it("should remove strikethrough markers", () => {
      expect(stripMarkdownForTTS("This is ~~strikethrough~~ text")).toBe(
        "This is strikethrough text"
      );
    });

    it("should handle multiple emphasis markers in one line", () => {
      expect(stripMarkdownForTTS("**Bold** and *italic* and ~~strike~~ text")).toBe(
        "Bold and italic and strike text"
      );
    });

    it("should handle nested emphasis (even though not standard markdown)", () => {
      expect(stripMarkdownForTTS("**Bold with *italic* inside**")).toBe("Bold with italic inside");
    });
  });

  describe("code blocks and inline code", () => {
    it("should remove inline code with single backticks", () => {
      expect(stripMarkdownForTTS("Use `console.log()` to debug")).toBe("Use  to debug");
      expect(stripMarkdownForTTS("The `variable` is undefined")).toBe("The  is undefined");
    });

    it("should remove code blocks with triple backticks", () => {
      const input = `Here is some code:
\`\`\`javascript
function hello() {
  console.log("Hello");
}
\`\`\`
And text continues`;
      expect(stripMarkdownForTTS(input)).toBe(
        "Here is some code:\ncode omitted\nAnd text continues"
      );
    });

    it("should remove code blocks with language specifier", () => {
      const input = `\`\`\`python
def hello():
    print("Hello")
\`\`\``;
      expect(stripMarkdownForTTS(input)).toBe("code omitted");
    });

    it("should handle multiple code blocks", () => {
      const input = `First block:
\`\`\`
code1
\`\`\`
Second block:
\`\`\`
code2
\`\`\``;
      expect(stripMarkdownForTTS(input)).toBe(
        "First block:\ncode omitted\nSecond block:\ncode omitted"
      );
    });
  });

  describe("headings", () => {
    it("should remove h1 heading markers", () => {
      expect(stripMarkdownForTTS("# Main Title")).toBe("Main Title");
    });

    it("should remove h2 heading markers", () => {
      expect(stripMarkdownForTTS("## Section Title")).toBe("Section Title");
    });

    it("should remove h3-h6 heading markers", () => {
      expect(stripMarkdownForTTS("### Subsection")).toBe("Subsection");
      expect(stripMarkdownForTTS("#### Sub-subsection")).toBe("Sub-subsection");
      expect(stripMarkdownForTTS("##### Small heading")).toBe("Small heading");
      expect(stripMarkdownForTTS("###### Tiny heading")).toBe("Tiny heading");
    });

    it("should handle multiple headings", () => {
      const input = `# Title
## Section
### Subsection`;
      expect(stripMarkdownForTTS(input)).toBe("Title\nSection\nSubsection");
    });
  });

  describe("lists", () => {
    it("should remove unordered list markers with dashes", () => {
      const input = `- First item
- Second item
- Third item`;
      expect(stripMarkdownForTTS(input)).toBe("First item\nSecond item\nThird item");
    });

    it("should remove unordered list markers with asterisks", () => {
      const input = `* First item
* Second item`;
      expect(stripMarkdownForTTS(input)).toBe("First item\nSecond item");
    });

    it("should remove unordered list markers with plus signs", () => {
      const input = `+ First item
+ Second item`;
      expect(stripMarkdownForTTS(input)).toBe("First item\nSecond item");
    });

    it("should remove ordered list markers", () => {
      const input = `1. First item
2. Second item
3. Third item`;
      expect(stripMarkdownForTTS(input)).toBe("First item\nSecond item\nThird item");
    });

    it("should handle indented list items", () => {
      const input = `- First level
  - Second level
    - Third level`;
      expect(stripMarkdownForTTS(input)).toBe("First level\nSecond level\nThird level");
    });

    it("should handle mixed list types", () => {
      const input = `1. Ordered item
- Unordered item
2. Another ordered`;
      expect(stripMarkdownForTTS(input)).toBe("Ordered item\nUnordered item\nAnother ordered");
    });
  });

  describe("links and images", () => {
    it("should remove link markdown but keep text", () => {
      expect(stripMarkdownForTTS("[Click here](https://example.com)")).toBe("Click here");
      expect(stripMarkdownForTTS("Visit [our website](https://example.com) for more")).toBe(
        "Visit our website for more"
      );
    });

    it("should remove image markdown completely", () => {
      expect(stripMarkdownForTTS("![Alt text](image.jpg)")).toBe("image omitted");
      expect(stripMarkdownForTTS("See this image: ![Description](photo.png) above")).toBe(
        "See this image: image omitted above"
      );
    });

    it("should handle multiple links in text", () => {
      expect(
        stripMarkdownForTTS("[First link](url1) and [second link](url2) and [third](url3)")
      ).toBe("First link and second link and third");
    });

    it("should handle links with special characters in text", () => {
      expect(stripMarkdownForTTS("[Link with (parentheses)](url)")).toBe("Link with (parentheses)");
    });
  });

  describe("blockquotes", () => {
    it("should remove blockquote markers", () => {
      expect(stripMarkdownForTTS("> This is a quote")).toBe("This is a quote");
      expect(stripMarkdownForTTS(">Another quote")).toBe("Another quote");
    });

    it("should handle multi-line blockquotes", () => {
      const input = `> First line
> Second line
> Third line`;
      expect(stripMarkdownForTTS(input)).toBe("First line\nSecond line\nThird line");
    });

    it("should handle nested blockquotes", () => {
      const input = `> Level 1
>> Level 2
>>> Level 3`;
      expect(stripMarkdownForTTS(input)).toBe("Level 1\n> Level 2\n>> Level 3");
    });
  });

  describe("tables", () => {
    it("should remove simple tables", () => {
      const input = `| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |`;
      expect(stripMarkdownForTTS(input)).toBe("table omitted");
    });

    it("should remove tables with alignment", () => {
      const input = `| Left | Center | Right |
|:-----|:------:|------:|
| A    | B      | C     |`;
      expect(stripMarkdownForTTS(input)).toBe("table omitted");
    });

    it("should handle text around tables", () => {
      const input = `Before table
| Col1 | Col2 |
|------|------|
| Data | Data |
After table`;
      expect(stripMarkdownForTTS(input)).toBe("Before table\ntable omitted\nAfter table");
    });
  });

  describe("horizontal rules", () => {
    it("should remove horizontal rules with dashes", () => {
      expect(stripMarkdownForTTS("Text\n---\nMore text")).toBe("Text\n\nMore text");
      expect(stripMarkdownForTTS("Text\n------\nMore text")).toBe("Text\n\nMore text");
    });

    it("should remove horizontal rules with asterisks", () => {
      expect(stripMarkdownForTTS("Text\n***\nMore text")).toBe("Text\n\nMore text");
      expect(stripMarkdownForTTS("Text\n*****\nMore text")).toBe("Text\n\nMore text");
    });

    it("should remove horizontal rules with underscores", () => {
      expect(stripMarkdownForTTS("Text\n___\nMore text")).toBe("Text\n\nMore text");
    });
  });

  describe("HTML tags", () => {
    it("should remove HTML tags", () => {
      expect(stripMarkdownForTTS("Text with <strong>bold</strong> tag")).toBe("Text with bold tag");
      expect(stripMarkdownForTTS("Text with <em>italic</em> tag")).toBe("Text with italic tag");
    });

    it("should remove self-closing HTML tags", () => {
      expect(stripMarkdownForTTS("Text with<br/>break")).toBe("Text withbreak");
      expect(stripMarkdownForTTS("Text with <hr /> rule")).toBe("Text with  rule");
    });

    it("should remove HTML with attributes", () => {
      expect(stripMarkdownForTTS('<div class="test">Content</div>')).toBe("Content");
      expect(stripMarkdownForTTS('<a href="url">Link</a>')).toBe("Link");
    });
  });

  describe("whitespace handling", () => {
    it("should collapse multiple newlines", () => {
      expect(stripMarkdownForTTS("Line 1\n\n\n\nLine 2")).toBe("Line 1\n\nLine 2");
      expect(stripMarkdownForTTS("Text\n\n\n\n\n\nMore text")).toBe("Text\n\nMore text");
    });

    it("should trim whitespace from lines", () => {
      expect(stripMarkdownForTTS("  Text with spaces  ")).toBe("Text with spaces");
      expect(stripMarkdownForTTS("  Line 1  \n  Line 2  ")).toBe("Line 1\nLine 2");
    });

    it("should handle mixed whitespace", () => {
      expect(stripMarkdownForTTS("\n\n  Text  \n\n\n  More  \n\n")).toBe("Text\n\nMore");
    });
  });

  describe("complex markdown documents", () => {
    it("should handle a complete markdown document", () => {
      const input = `# Main Title

## Introduction

This is a **bold** statement with *italic* text and \`inline code\`.

### Features

- First feature with [link](url)
- Second feature with ~~strikethrough~~
- Third feature

\`\`\`javascript
// Code block
const x = 1;
\`\`\`

> A wise quote

| Table | Header |
|-------|--------|
| Data  | Value  |

---

#### Conclusion

Final thoughts with ![image](img.png) and more text.`;

      const expected = `Main Title

Introduction

This is a bold statement with italic text and .

Features

First feature with link
Second feature with strikethrough
Third feature

code omitted

A wise quote

table omitted

Conclusion

Final thoughts with image omitted and more text.`;

      expect(stripMarkdownForTTS(input)).toBe(expected);
    });

    it("should handle real-world markdown with asterisks", () => {
      const input = `*testing* and **more testing** and ***even more***`;
      expect(stripMarkdownForTTS(input)).toBe("testing and more testing and even more");
    });

    it("should handle edge case with multiple asterisks", () => {
      const input = `This is *italic*, **bold**, and ***bold italic*** text`;
      expect(stripMarkdownForTTS(input)).toBe("This is italic, bold, and bold italic text");
    });

    it("should handle markdown with thinking tags (integration test)", () => {
      const input = `<thinking>
This should be removed
</thinking>

# Title

Regular *markdown* content here.`;

      // Note: stripThinkingTags should be called first, then stripMarkdownForTTS
      expect(stripMarkdownForTTS(input)).toContain("Title");
      expect(stripMarkdownForTTS(input)).toContain("Regular markdown content here");
    });
  });

  describe("edge cases", () => {
    it("should handle empty string", () => {
      expect(stripMarkdownForTTS("")).toBe("");
    });

    it("should handle string with only whitespace", () => {
      expect(stripMarkdownForTTS("   \n\n   ")).toBe("");
    });

    it("should handle plain text without markdown", () => {
      expect(stripMarkdownForTTS("Plain text without any markdown")).toBe(
        "Plain text without any markdown"
      );
    });

    it("should handle malformed markdown", () => {
      expect(stripMarkdownForTTS("**Unclosed bold")).toBe("**Unclosed bold");
      expect(stripMarkdownForTTS("*Unclosed italic")).toBe("*Unclosed italic");
      expect(stripMarkdownForTTS("[Unclosed link")).toBe("[Unclosed link");
    });

    it("should handle special characters", () => {
      expect(stripMarkdownForTTS("Text with $pecial ch@rs & symbols!")).toBe(
        "Text with $pecial ch@rs & symbols!"
      );
    });

    it("should not remove math expressions (if not in code blocks)", () => {
      const input = "The equation $x = y + 2$ is simple";
      expect(stripMarkdownForTTS(input)).toBe("The equation $x = y + 2$ is simple");
    });

    it("should handle Unicode characters", () => {
      expect(stripMarkdownForTTS("Text with émojis 😀 and ñoñ-ASCII")).toBe(
        "Text with émojis 😀 and ñoñ-ASCII"
      );
    });
  });
});
