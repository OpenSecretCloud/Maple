import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

import { getChatHistoryEllipsisButtonClass } from "./chatHistoryMenuPresentation";

describe("chat-history overflow menu presentation", () => {
  test("uses only genuine open and keyboard-focus color states in the compact menu", () => {
    const tokens = getChatHistoryEllipsisButtonClass(true).split(/\s+/);

    expect(tokens).toContain("data-[state=open]:text-foreground");
    expect(tokens).toContain("focus-visible:text-foreground");
    expect(tokens).not.toContain("hover:text-foreground");
    expect(tokens).not.toContain("group-hover:text-foreground");
    expect(tokens).toContain("h-11");
    expect(tokens).toContain("w-11");
    expect(tokens).toContain("bg-muted");
    expect(tokens).toContain("dark:bg-[hsl(var(--sidebar))]");
  });

  test("preserves pointer hover behavior in the desktop sidebar", () => {
    const tokens = getChatHistoryEllipsisButtonClass(false).split(/\s+/);

    expect(tokens).toContain("hover:text-foreground");
    expect(tokens).toContain("group-hover:text-foreground");
    expect(tokens).toContain("focus-visible:text-foreground");
  });

  test("applies the shared touch-safe presentation to chat and project triggers", () => {
    const source = readFileSync(new URL("./ChatHistoryList.tsx", import.meta.url), "utf8");

    expect(source.match(/getChatHistoryEllipsisButtonClass\(pagePresentation\)/g)).toHaveLength(2);
  });
});
