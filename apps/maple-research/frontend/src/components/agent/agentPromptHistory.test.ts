import { describe, expect, test } from "bun:test";
import type { AgentTimelineItem } from "@/services/agentRuntimeService";
import {
  agentPromptHistory,
  agentPromptHistoryDirection,
  AgentPromptHistoryReplacementTracker,
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

  test("matches the textarea's LF value to recalled CRLF and bare CR entries", () => {
    const crlfNavigation = navigateAgentPromptHistory(
      null,
      ["first", "line one\r\nline two"],
      "older"
    )!.navigation;
    expect(
      agentPromptHistoryDirection(
        keyState({ key: "ArrowUp", value: "line one\nline two" }),
        crlfNavigation
      )
    ).toBe("older");

    const crNavigation = navigateAgentPromptHistory(
      null,
      ["first", "line one\rline two"],
      "older"
    )!.navigation;
    expect(
      agentPromptHistoryDirection(
        keyState({ key: "ArrowDown", value: "line one\nline two" }),
        crNavigation
      )
    ).toBe("newer");
  });

  test("preserves canonical line endings in the navigation snapshot", () => {
    const history = ["older\rentry", "newer\r\nentry"];
    let step = navigateAgentPromptHistory(null, history, "older")!;

    expect(step.value).toBe("newer\r\nentry");
    expect(step.navigation?.entries).toEqual(history);

    step = navigateAgentPromptHistory(step.navigation, history, "older")!;
    expect(step.value).toBe("older\rentry");
    expect(step.navigation?.entries).toEqual(history);
  });

  test("does not mistake real edits for textarea newline normalization", () => {
    const navigation = navigateAgentPromptHistory(
      null,
      ["first", "line one\r\nline two"],
      "older"
    )!.navigation;

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

describe("AgentPromptHistoryReplacementTracker", () => {
  test("restores the current attempt fallback after a failed or raced replacement", () => {
    const tracker = new AgentPromptHistoryReplacementTracker();
    const attempt = tracker.begin("task-a", ["A", "B"]);

    expect(tracker.isReplacing("task-a")).toBe(true);
    expect(tracker.recover(attempt, "task-a")).toEqual(["A", "B"]);
    expect(tracker.isReplacing("task-a")).toBe(false);
  });

  test("an older same-task attempt cannot release or empty a newer replacement", () => {
    const tracker = new AgentPromptHistoryReplacementTracker();
    const older = tracker.begin("task-a", ["A", "B"]);
    const newer = tracker.begin("task-a", []);

    expect(tracker.recover(older, "task-a")).toBeNull();
    expect(tracker.isReplacing("task-a")).toBe(true);
    expect(tracker.recover(newer, "task-a")).toEqual(["A", "B"]);
    expect(tracker.isReplacing("task-a")).toBe(false);
  });

  test("does not restore an old task over the newly active task", () => {
    const tracker = new AgentPromptHistoryReplacementTracker();
    const taskA = tracker.begin("task-a", ["A"]);

    expect(tracker.recover(taskA, "task-b")).toBeNull();
    expect(tracker.isReplacing("task-a")).toBe(false);

    const taskB = tracker.begin("task-b", ["B"]);
    expect(tracker.recover(taskA, "task-b")).toBeNull();
    expect(tracker.isReplacing("task-b")).toBe(true);
    expect(tracker.recover(taskB, "task-b")).toEqual(["B"]);
  });

  test("abandons prompt-bearing fallback state when another task becomes active", () => {
    const tracker = new AgentPromptHistoryReplacementTracker();
    const taskA = tracker.begin("task-a", ["A"]);

    tracker.abandonInactive("task-a");
    expect(tracker.isReplacing("task-a")).toBe(true);

    tracker.abandonInactive("task-b");
    expect(tracker.isReplacing("task-a")).toBe(false);
    expect(tracker.recover(taskA, "task-a")).toBeNull();
  });

  test("only an authoritative replacement for the tracked task clears it", () => {
    const tracker = new AgentPromptHistoryReplacementTracker();
    const attempt = tracker.begin("task-a", ["A"]);

    tracker.authoritativeReplace("task-b");
    expect(tracker.isReplacing("task-a")).toBe(true);

    tracker.authoritativeReplace("task-a");
    expect(tracker.isReplacing("task-a")).toBe(false);
    expect(tracker.recover(attempt, "task-a")).toBeNull();
  });
});
