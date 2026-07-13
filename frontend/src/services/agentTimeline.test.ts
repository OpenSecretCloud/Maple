import { describe, expect, test } from "bun:test";
import type { AgentTimelineItem } from "./agentRuntimeService";
import {
  activeAgentThinkingItemId,
  coalesceAdjacentThinkingItems,
  groupAgentTimelineItems,
  hasAgentUserMessage,
  hasRenderableThinkingText
} from "./agentTimeline";

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

describe("hasAgentUserMessage", () => {
  test("locks only after a real user message appears", () => {
    const item = (
      itemType: AgentTimelineItem["itemType"],
      role: AgentTimelineItem["role"]
    ): AgentTimelineItem => ({
      id: `${itemType}-${role}`,
      itemType,
      role,
      createdMs: 0,
      merge: "replace"
    });

    expect(hasAgentUserMessage([])).toBe(false);
    expect(hasAgentUserMessage([item("error", "system"), item("message", "assistant")])).toBe(
      false
    );
    expect(hasAgentUserMessage([item("message", "user")])).toBe(true);
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

describe("activeAgentThinkingItemId", () => {
  test("marks only a trailing thought in an active run", () => {
    const items = [thinking("thought", "Working")];

    expect(activeAgentThinkingItemId(items, true)).toBe("thought");
    expect(activeAgentThinkingItemId(items, false)).toBeNull();
  });

  test("stops marking a thought active once later activity arrives", () => {
    const items = [
      thinking("thought", "Working"),
      {
        id: "tool",
        itemType: "tool",
        status: "running",
        createdMs: 0,
        merge: "replace"
      } satisfies AgentTimelineItem
    ];

    expect(activeAgentThinkingItemId(items, true)).toBeNull();
  });

  test("does not reactivate a previous thought while the next prompt is submitting", () => {
    const previousItems = [thinking("previous-thought", "Finished")];
    const hasActiveRun = true;
    const isSubmitting = true;

    expect(activeAgentThinkingItemId(previousItems, hasActiveRun && !isSubmitting)).toBeNull();
  });
});

describe("groupAgentTimelineItems", () => {
  function item(
    id: string,
    itemType: AgentTimelineItem["itemType"],
    role?: AgentTimelineItem["role"]
  ): AgentTimelineItem {
    return { id, itemType, role, createdMs: 0, merge: "replace" };
  }

  test("groups a complete agent response under one assistant turn", () => {
    const turns = groupAgentTimelineItems([
      item("user", "message", "user"),
      item("thinking", "thinking", "thought"),
      item("tool", "tool"),
      item("permission", "permission"),
      item("answer", "message", "assistant")
    ]);

    expect(turns).toHaveLength(2);
    expect(turns[0]).toMatchObject({ type: "user", id: "user" });
    expect(turns[1]).toMatchObject({
      type: "assistant",
      id: "assistant-after-user",
      items: [{ id: "thinking" }, { id: "tool" }, { id: "permission" }, { id: "answer" }]
    });
  });

  test("starts a new turn for each user message", () => {
    const turns = groupAgentTimelineItems([
      item("user-1", "message", "user"),
      item("answer-1", "message", "assistant"),
      item("user-2", "message", "user"),
      item("answer-2", "message", "assistant")
    ]);

    expect(turns.map((turn) => turn.type)).toEqual(["user", "assistant", "user", "assistant"]);
  });

  test("keeps leading system activity in an assistant turn", () => {
    const turns = groupAgentTimelineItems([
      item("system", "system", "system"),
      item("error", "error", "system"),
      item("user", "message", "user")
    ]);

    expect(turns[0]).toMatchObject({
      type: "assistant",
      id: "assistant-leading",
      items: [{ id: "system" }, { id: "error" }]
    });
    expect(turns[1]).toMatchObject({ type: "user", id: "user" });
  });

  test("keeps assistant keys stable as streamed items become renderable", () => {
    const before = groupAgentTimelineItems([item("user", "message", "user"), item("tool", "tool")]);
    const after = groupAgentTimelineItems([
      item("user", "message", "user"),
      item("thinking", "thinking", "thought"),
      item("tool", "tool")
    ]);

    expect(before[1].id).toBe("assistant-after-user");
    expect(after[1].id).toBe(before[1].id);
  });
});
