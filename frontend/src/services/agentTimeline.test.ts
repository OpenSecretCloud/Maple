import { describe, expect, test } from "bun:test";
import type { AgentTimelineItem } from "./agentRuntimeService";
import {
  AgentLiveThoughtPhaseTracker,
  activeAgentThinkingItemId,
  agentThinkingPhaseId,
  agentThoughtPhasesForLatestTurn,
  coalesceAdjacentThinkingItems,
  getAgentTurnCopyText,
  groupAgentTimelineItems,
  hasAgentUserMessage,
  hasRenderableThinkingText,
  shouldShowAgentAssistantLoader
} from "./agentTimeline";

function item(
  id: string,
  itemType: AgentTimelineItem["itemType"],
  role?: AgentTimelineItem["role"],
  text?: string
): AgentTimelineItem {
  return { id, itemType, role, text, createdMs: 0, merge: "replace" };
}

function thinking(id: string, text: string): AgentTimelineItem {
  return {
    ...item(id, "thinking", "thought", text),
    title: "Thinking"
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
    expect(hasAgentUserMessage([])).toBe(false);
    expect(
      hasAgentUserMessage([
        item("error", "error", "system"),
        item("assistant", "message", "assistant")
      ])
    ).toBe(false);
    expect(hasAgentUserMessage([item("user", "message", "user")])).toBe(true);
  });
});

describe("getAgentTurnCopyText", () => {
  test("copies raw user text and only the final assistant message", () => {
    const userText = "## Plan\n\nPreserve `raw` markdown 🍁";
    const turns = groupAgentTimelineItems([
      item("leading-tool", "tool", "assistant", "Ignore tool output"),
      item("user", "message", "user", userText),
      thinking("thought", "Ignore private reasoning"),
      item("preamble", "message", "assistant", "Ignore preamble"),
      item("tool", "tool", "assistant", "Ignore tool call"),
      item("tool-result", "tool", "assistant", "Ignore tool result"),
      item("final", "message", "assistant", "**Final** 🍁")
    ]);

    expect(turns.map(getAgentTurnCopyText)).toEqual(["", userText, "**Final** 🍁"]);
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

describe("AgentLiveThoughtPhaseTracker", () => {
  test("requests one stable label candidate when adjacent live reasoning finishes", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    expect(
      tracker.observeTimelineItem("session", item("user-turn", "message", "user", "Inspect login"))
    ).toBeNull();
    expect(
      tracker.observeTimelineItem("session", {
        ...thinking("live-thought-a", "Inspecting"),
        merge: "append"
      })
    ).toBeNull();
    expect(
      tracker.observeTimelineItem("session", {
        ...thinking("live-thought-a", " auth"),
        merge: "append"
      })
    ).toBeNull();
    expect(
      tracker.observeTimelineItem("session", {
        ...thinking("live-thought-b", "."),
        merge: "append"
      })
    ).toBeNull();

    const completed = tracker.observeTimelineItem("session", {
      ...item("tool", "tool", "assistant", "tool output must not be sent"),
      input: { secret: "tool input must not be sent" },
      output: { secret: "tool output must not be sent" }
    });

    expect(completed).toEqual({
      sessionId: "session",
      phaseId: agentThinkingPhaseId("assistant-after-user-turn", 0),
      userRequest: "Inspect login",
      reasoningText: "Inspecting auth."
    });
    expect(tracker.observeTimelineItem("session", item("tool-update", "tool"))).toBeNull();
    expect(tracker.finishRun("session")).toBeNull();
  });

  test("exposes the current reasoning snapshot with the same identity used at completion", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Inspect login"));
    tracker.observeTimelineItem("session", {
      ...thinking("thought", "Investigating"),
      merge: "append"
    });
    tracker.observeTimelineItem("session", {
      ...thinking("thought", " authentication"),
      merge: "append"
    });

    const active = tracker.activePhase("session");
    expect(active).toEqual({
      sessionId: "session",
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      userRequest: "Inspect login",
      reasoningText: "Investigating authentication"
    });

    const completed = tracker.observeTimelineItem("session", {
      ...item("tool", "tool"),
      input: { private: "not label context" },
      output: { private: "not label context" }
    });
    expect(completed).toEqual(active);
    expect(tracker.activePhase("session")).toBeNull();
  });

  test("tools split reasoning phases into stable ordinals", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Do the work"));
    tracker.observeTimelineItem("session", thinking("first-live-id", "Plan the change"));
    const first = tracker.observeTimelineItem("session", item("tool", "tool"));
    tracker.observeTimelineItem("session", thinking("second-live-id", "Verify the change"));
    const second = tracker.observeTimelineItem(
      "session",
      item("answer", "message", "assistant", "Done")
    );

    expect(first?.phaseId).toBe(agentThinkingPhaseId("assistant-after-user", 0));
    expect(second?.phaseId).toBe(agentThinkingPhaseId("assistant-after-user", 1));
  });

  test("does not split a thought when an earlier tool or permission row is updated in place", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Do the work"));
    tracker.observeTimelineItem("session", item("permission", "permission"));
    tracker.observeTimelineItem("session", {
      ...thinking("thought", "Inspecting"),
      merge: "append"
    });
    expect(
      tracker.observeTimelineItem("session", {
        ...item("permission", "permission"),
        status: "allow_once"
      })
    ).toBeNull();
    tracker.observeTimelineItem("session", {
      ...thinking("thought", " the code"),
      merge: "append"
    });

    expect(tracker.observeTimelineItem("session", item("tool", "tool"))).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      reasoningText: "Inspecting the code"
    });
    expect(tracker.finishRun("session")).toBeNull();
  });

  test("a trailing live phase finishes with the run without scanning old history", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Do the work"));
    tracker.observeTimelineItem("session", thinking("thought", "Finishing the work"));

    expect(tracker.finishRun("session")).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      reasoningText: "Finishing the work"
    });
    expect(tracker.finishRun("session")).toBeNull();
  });

  test("discards a trailing phase when its run is cancelled", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Do the work"));
    tracker.observeTimelineItem("session", thinking("thought", "Partial reasoning"));

    expect(tracker.discardRun("session")).toBe("assistant-after-user");
    expect(tracker.finishRun("session")).toBeNull();
  });

  test("drops stale reasoning immediately when live history is replaced", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Do the work"));
    tracker.observeTimelineItem("session", thinking("stale-thought", "Stale reasoning"));

    expect(tracker.resetForHistoryReplacement("session")).toBe("assistant-after-user");
    tracker.observeTimelineItem("session", thinking("fresh-thought", "Fresh reasoning"));

    expect(tracker.observeTimelineItem("session", item("tool", "tool"))).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      reasoningText: "Fresh reasoning"
    });
  });

  test("seeds an in-progress phase without generating from loaded history", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("persisted-reasoning-id", "Inspecting the login flow")
    ]);

    const completed = tracker.finishRun("session");
    expect(completed).toEqual({
      sessionId: "session",
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      userRequest: "Inspect login",
      reasoningText: "Inspecting the login flow"
    });
  });

  test("continues every raw part of an adjacent thought seeded from active history", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("reasoning-a", "Inspecting "),
      thinking("reasoning-b", "the login")
    ]);
    tracker.observeTimelineItem("session", {
      ...thinking("reasoning-b", " flow"),
      merge: "append"
    });

    expect(tracker.finishRun("session")).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      reasoningText: "Inspecting the login flow"
    });
  });

  test("arms a seeded whitespace-only thought that becomes visible in a later delta", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("reasoning", " \n ")
    ]);
    tracker.observeTimelineItem("session", {
      ...thinking("reasoning", "Inspecting"),
      merge: "append"
    });

    expect(tracker.observeTimelineItem("session", item("tool", "tool"))).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 0),
      reasoningText: " \n Inspecting"
    });
  });

  test("does not arm completed thoughts merely because old history was loaded", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("old-reasoning", "Inspected the login flow"),
      item("answer", "message", "assistant", "Done")
    ]);

    expect(tracker.finishRun("session")).toBeNull();
  });

  test("does not arm a thought followed by a hidden non-thinking boundary", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("old-reasoning", "Inspected the login flow"),
      item("hidden-boundary", "system", "system", " ")
    ]);

    expect(tracker.finishRun("session")).toBeNull();
  });

  test("keeps a hidden trailing phase after a hidden boundary at the next visible ordinal", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.seedActiveTimeline("session", [
      item("user", "message", "user", "Inspect login"),
      thinking("first-reasoning", "Inspected the login flow"),
      item("hidden-boundary", "system", "system", " "),
      thinking("next-reasoning", " ")
    ]);
    tracker.observeTimelineItem("session", {
      ...thinking("next-reasoning", "Checking tests"),
      merge: "append"
    });

    expect(tracker.finishRun("session")).toMatchObject({
      phaseId: agentThinkingPhaseId("assistant-after-user", 1),
      reasoningText: " Checking tests"
    });
  });

  test("keeps phase identity stable when live and persisted reasoning IDs differ", () => {
    const liveTracker = new AgentLiveThoughtPhaseTracker();
    liveTracker.observeTimelineItem(
      "session",
      item("stable-user", "message", "user", "Inspect login")
    );
    liveTracker.observeTimelineItem("session", thinking("live-reasoning-id", "Inspecting"));
    const live = liveTracker.observeTimelineItem("session", item("tool", "tool"));

    const loadedTracker = new AgentLiveThoughtPhaseTracker();
    loadedTracker.seedActiveTimeline("session", [
      item("stable-user", "message", "user", "Inspect login"),
      thinking("different-persisted-id", "Inspecting")
    ]);
    const loaded = loadedTracker.finishRun("session");

    expect(live?.phaseId).toBe(loaded?.phaseId);
    expect(live?.phaseId).toBe(agentThinkingPhaseId("assistant-after-stable-user", 0));
  });

  test("does not let a hidden whitespace-only thought shift visible phase ordinals", () => {
    const tracker = new AgentLiveThoughtPhaseTracker();
    tracker.observeTimelineItem("session", item("user", "message", "user", "Inspect login"));
    tracker.observeTimelineItem("session", thinking("hidden", " \n "));
    expect(tracker.observeTimelineItem("session", item("tool", "tool"))).toBeNull();
    tracker.observeTimelineItem("session", thinking("visible", "Inspecting"));

    expect(tracker.finishRun("session")?.phaseId).toBe(
      agentThinkingPhaseId("assistant-after-user", 0)
    );
  });
});

describe("agentThoughtPhasesForLatestTurn", () => {
  test("reconstructs only the latest turn without including tool data", () => {
    const phases = agentThoughtPhasesForLatestTurn("session", [
      item("old-user", "message", "user", "Old request"),
      thinking("old-thought", "Old reasoning"),
      item("old-answer", "message", "assistant", "Old answer"),
      item("latest-user", "message", "user", "Fix login"),
      thinking("first-thought", "Investigating login failures"),
      {
        ...item("tool", "tool"),
        input: { private: "not label context" },
        output: { private: "not label context" }
      },
      thinking("second-thought", "Formulating authentication findings"),
      item("answer", "message", "assistant", "Done")
    ]);

    expect(phases).toEqual([
      {
        sessionId: "session",
        phaseId: agentThinkingPhaseId("assistant-after-latest-user", 0),
        userRequest: "Fix login",
        reasoningText: "Investigating login failures"
      },
      {
        sessionId: "session",
        phaseId: agentThinkingPhaseId("assistant-after-latest-user", 1),
        userRequest: "Fix login",
        reasoningText: "Formulating authentication findings"
      }
    ]);
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

describe("shouldShowAgentAssistantLoader", () => {
  const userTurn = {
    type: "user" as const,
    id: "user",
    item: {
      id: "user",
      itemType: "message" as const,
      role: "user" as const,
      createdMs: 0,
      merge: "replace" as const
    }
  };
  const assistantTurn = {
    type: "assistant" as const,
    id: "assistant-after-user",
    items: [thinking("thought", "Working")]
  };

  test("shows while a sent user turn is waiting for its first assistant activity", () => {
    expect(shouldShowAgentAssistantLoader([userTurn], true)).toBe(true);
  });

  test("hides when the response is no longer pending or assistant activity has arrived", () => {
    expect(shouldShowAgentAssistantLoader([userTurn], false)).toBe(false);
    expect(shouldShowAgentAssistantLoader([userTurn, assistantTurn], true)).toBe(false);
  });

  test("does not invent an assistant turn without a user message", () => {
    expect(shouldShowAgentAssistantLoader([], true)).toBe(false);
    expect(shouldShowAgentAssistantLoader([assistantTurn], true)).toBe(false);
  });
});
