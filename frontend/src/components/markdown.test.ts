import { describe, expect, it } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { MarkdownContent } from "./markdown";

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
