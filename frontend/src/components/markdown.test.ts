import { describe, expect, it } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { MarkdownContent, ThinkingBlock } from "./markdown";

function renderMarkdown(content: string): string {
  return renderToStaticMarkup(React.createElement(MarkdownContent, { content }));
}

describe("MarkdownContent images", () => {
  it("renders image alt text without loading remote or local URL schemes", () => {
    const rendered = renderMarkdown(
      [
        "![remote](https://example.com/tracker.png)",
        "![relative](/local-image.png)",
        "![data](data:image/png;base64,abc)",
        "![blob](blob:https://trymaple.ai/image-id)"
      ].join("\n\n")
    );

    expect(rendered).not.toContain("<img");
    expect(rendered).not.toContain("tracker.png");
    expect(rendered).not.toContain("local-image.png");
    expect(rendered).not.toContain("data:image");
    expect(rendered).not.toContain("blob:");
    expect(rendered).toContain("remote");
    expect(rendered).toContain("relative");
    expect(rendered).toContain("data");
    expect(rendered).toContain("blob");
  });

  it("continues to render ordinary links", () => {
    const rendered = renderMarkdown("[Maple](https://trymaple.ai)");

    expect(rendered).toContain('href="https://trymaple.ai"');
    expect(rendered).toContain(">Maple</a>");
  });

  it("omits images without alt text", () => {
    const rendered = renderMarkdown("![](https://example.com/hidden.png)");

    expect(rendered).not.toContain("<img");
    expect(rendered).not.toContain("hidden.png");
    expect(rendered).not.toContain("<span");
  });
});

describe("ThinkingBlock labels", () => {
  function renderThinkingBlock(isThinking: boolean, label?: string): string {
    return renderToStaticMarkup(
      React.createElement(ThinkingBlock, {
        content: "Inspected the authentication flow.",
        isThinking,
        showDuration: false,
        label
      })
    );
  }

  it("renders a completed description with fade transition styles", () => {
    const completed = renderThinkingBlock(false, "Inspecting authentication flow");
    const streaming = renderThinkingBlock(true, "Inspecting authentication flow");

    expect(completed).toContain("Inspecting authentication flow");
    expect(completed).not.toContain(">Thought<");
    expect(completed).toContain("transition-opacity");
    expect(completed).toContain("motion-reduce:transition-none");
    expect(completed).toContain('aria-live="polite"');
    expect(completed).toContain('aria-atomic="true"');
    expect(completed).not.toContain("animate-bounce");
    expect(streaming).toContain("Inspecting authentication flow");
    expect(streaming).not.toContain(">Thinking<");
    expect(streaming).toContain("transition-opacity");
    expect(streaming).toContain("animate-bounce");
  });

  it("shows Thinking with animated dots before a provisional label arrives", () => {
    const streaming = renderThinkingBlock(true);

    expect(streaming).toContain(">Thinking<");
    expect(streaming).toContain("animate-bounce");
    expect(streaming).toContain("transition-opacity");
  });

  it("keeps Thought as the completed fallback", () => {
    const completed = renderThinkingBlock(false);

    expect(completed).toContain("Thought");
    expect(completed).not.toContain("animate-bounce");
  });

  it("keeps Thinking and its dots visible while a completed phase awaits its generated label", () => {
    const pending = renderThinkingBlock(false, "Thinking");

    expect(pending).toContain(">Thinking<");
    expect(pending).not.toContain(">Thought<");
    expect(pending).toContain("transition-opacity");
    expect(pending).toContain("animate-bounce");
  });
});
