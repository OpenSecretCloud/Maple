import { describe, expect, test } from "bun:test";
import type { AgentTimelineItem } from "@/services/agentRuntimeService";
import {
  agentPromptHistory,
  agentPromptHistoryDirection,
  navigateAgentPromptHistory,
  type AgentPromptHistoryKeyState,
  type AgentPromptHistoryNavigation
} from "./agentPromptHistory";

function item(
  id: string,
  text: string | null,
  itemType: AgentTimelineItem["itemType"] = "message",
  role: AgentTimelineItem["role"] = "user"
): AgentTimelineItem {
  return { id, itemType, role, text, createdMs: 0, merge: "replace" };
}

function keyState(overrides: Partial<AgentPromptHistoryKeyState> = {}): AgentPromptHistoryKeyState {
  return {
    key: "ArrowUp",
    value: "",
    selectionStart: 0,
    selectionEnd: 0,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    isComposing: false,
    ...overrides
  };
}

describe("agentPromptHistory", () => {
  test("derives only non-empty canonical user messages in chronological order", () => {
    expect(
      agentPromptHistory([
        item("assistant", "answer", "message", "assistant"),
        item("thinking", "reasoning", "thinking", "thought"),
        item("tool-user", "tool payload", "tool", "user"),
        item("permission-user", "permission payload", "permission", "user"),
        item("system-user", "system payload", "system", "user"),
        item("error-user", "error payload", "error", "user"),
        item("empty", ""),
        item("missing", null),
        item("first", "  first\nline  "),
        item("failed", "accepted before failure"),
        item("failure", "later failure", "error", "system"),
        item("cancelled", "accepted before cancellation"),
        item("cancellation", "later cancellation", "system", "system"),
        item("duplicate-a", "重複 👩‍💻"),
        item("duplicate-b", "重複 👩‍💻")
      ])
    ).toEqual([
      "  first\nline  ",
      "accepted before failure",
      "accepted before cancellation",
      "重複 👩‍💻",
      "重複 👩‍💻"
    ]);
  });

  test("navigates without wrapping and restores the empty composer", () => {
    let navigation: AgentPromptHistoryNavigation | null = null;

    let step = navigateAgentPromptHistory(navigation, ["A", "B"], "older")!;
    navigation = step.navigation;
    expect(step.value).toBe("B");

    step = navigateAgentPromptHistory(navigation, ["A", "B"], "older")!;
    navigation = step.navigation;
    expect(step.value).toBe("A");

    step = navigateAgentPromptHistory(navigation, ["A", "B"], "older")!;
    navigation = step.navigation;
    expect(step.value).toBe("A");

    step = navigateAgentPromptHistory(navigation, ["A", "B"], "newer")!;
    navigation = step.navigation;
    expect(step.value).toBe("B");

    step = navigateAgentPromptHistory(navigation, ["A", "B"], "newer")!;
    expect(step).toEqual({ navigation: null, value: "" });
  });

  test("snapshots duplicate entries so live timeline updates cannot shift navigation", () => {
    let step = navigateAgentPromptHistory(null, ["same", "same"], "older")!;
    expect(step.navigation?.index).toBe(1);

    step = navigateAgentPromptHistory(step.navigation, ["same", "same", "new"], "older")!;
    expect(step.navigation?.index).toBe(0);
    expect(step.value).toBe("same");

    step = navigateAgentPromptHistory(step.navigation, ["same", "same", "new"], "newer")!;
    expect(step.navigation?.index).toBe(1);

    step = navigateAgentPromptHistory(step.navigation, ["same", "same", "new"], "newer")!;
    expect(step).toEqual({ navigation: null, value: "" });

    step = navigateAgentPromptHistory(step.navigation, ["same", "same", "new"], "older")!;
    expect(step.value).toBe("new");
  });

  test("starts only with unmodified Up in an empty unselected composer", () => {
    expect(agentPromptHistoryDirection(keyState(), null)).toBe("older");
    expect(agentPromptHistoryDirection(keyState({ key: "ArrowDown" }), null)).toBeNull();
    expect(agentPromptHistoryDirection(keyState({ value: "draft" }), null)).toBeNull();
    expect(agentPromptHistoryDirection(keyState({ selectionEnd: 1 }), null)).toBeNull();

    for (const modifier of ["altKey", "ctrlKey", "metaKey", "shiftKey"] as const) {
      expect(agentPromptHistoryDirection(keyState({ [modifier]: true }), null)).toBeNull();
    }

    expect(agentPromptHistoryDirection(keyState({ isComposing: true }), null)).toBeNull();
  });

  test("navigates directly from an untouched recalled multiline prompt", () => {
    const navigation = navigateAgentPromptHistory(
      null,
      ["first", "line one\nline two"],
      "older"
    )!.navigation;

    expect(
      agentPromptHistoryDirection(
        keyState({
          key: "ArrowUp",
          value: "line one\nline two",
          selectionStart: 4,
          selectionEnd: 4
        }),
        navigation
      )
    ).toBe("older");
    expect(
      agentPromptHistoryDirection(
        keyState({
          key: "ArrowDown",
          value: "line one\nline two",
          selectionStart: 4,
          selectionEnd: 4
        }),
        navigation
      )
    ).toBe("newer");
    expect(
      agentPromptHistoryDirection(
        keyState({ value: "line one\nline two edited", selectionStart: 24, selectionEnd: 24 }),
        navigation
      )
    ).toBeNull();
  });

  test("does not start without history or move newer outside navigation", () => {
    expect(navigateAgentPromptHistory(null, [], "older")).toBeNull();
    expect(navigateAgentPromptHistory(null, ["A"], "newer")).toBeNull();
  });
});
