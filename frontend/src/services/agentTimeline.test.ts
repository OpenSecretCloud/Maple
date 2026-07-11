import { describe, expect, test } from "bun:test";
import type { AgentTimelineItem } from "./agentRuntimeService";
import { coalesceAdjacentThinkingItems, hasRenderableThinkingText } from "./agentTimeline";

function thinking(id: string, text: string): AgentTimelineItem {
  return {
    id,
    itemType: "thinking",
    role: "thought",
    title: "Thinking",
    text,
    createdMs: 0,
    merge: "replace"
  };
}

describe("hasRenderableThinkingText", () => {
  test("hides only empty and whitespace-only merged thoughts", () => {
    expect(hasRenderableThinkingText(undefined)).toBe(false);
    expect(hasRenderableThinkingText(" \n\t ")).toBe(false);
  });

  test("preserves all model content, including punctuation-only chunks", () => {
    expect(hasRenderableThinkingText("Inspecting.")).toBe(true);
    expect(hasRenderableThinkingText("🤔")).toBe(true);
    expect(hasRenderableThinkingText("=>")).toBe(true);
    expect(hasRenderableThinkingText(".")).toBe(true);
    expect(hasRenderableThinkingText("…")).toBe(true);
    expect(hasRenderableThinkingText(". Inspecting")).toBe(true);
  });
});

describe("coalesceAdjacentThinkingItems", () => {
  test("joins consecutive Goose reasoning messages without classifying their content", () => {
    const projected = coalesceAdjacentThinkingItems([
      thinking("reasoning", "Inspecting"),
      thinking("punctuation", ".")
    ]);

    expect(projected).toHaveLength(1);
    expect(projected[0].text).toBe("Inspecting.");
  });

  test("keeps reasoning phases separated by a tool row distinct", () => {
    const tool: AgentTimelineItem = {
      id: "tool",
      itemType: "tool",
      createdMs: 0,
      merge: "replace"
    };
    const projected = coalesceAdjacentThinkingItems([
      thinking("before", "Before"),
      tool,
      thinking("after", "After")
    ]);

    expect(projected.map((item) => item.text)).toEqual(["Before", undefined, "After"]);
  });
});
