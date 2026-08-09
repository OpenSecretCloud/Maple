import { describe, expect, test } from "bun:test";
import type { AgentSessionSummary } from "./agentRuntimeService";
import { reconcileAgentSessionSnapshot } from "./agentSessionSummaries";

function session(id: string, title: string, updatedMs = 1): AgentSessionSummary {
  return {
    id,
    title,
    projectRoot: "/tmp/project",
    createdMs: 0,
    updatedMs,
    messageCount: 1,
    model: "glm-5-2",
    mode: "smart_approve"
  };
}

describe("reconcileAgentSessionSnapshot", () => {
  test("keeps a reactive semantic title that arrived after refresh began", () => {
    const semantic = session("task-1", "Friendly Check-In", 1);
    const staleSnapshot = session("task-1", "hey how are you?", 1);

    expect(
      reconcileAgentSessionSnapshot([staleSnapshot], [semantic], new Set(["task-1"]), new Set())
    ).toEqual([semantic]);
  });

  test("accepts server changes when no newer reactive event raced the request", () => {
    const current = session("task-1", "hey how are you?");
    const snapshot = session("task-1", "Friendly Check-In");

    expect(reconcileAgentSessionSnapshot([snapshot], [current], new Set(), new Set())).toEqual([
      snapshot
    ]);
  });

  test("retains sessions observed after refresh began and drops older absent sessions", () => {
    const retained = session("task-1", "Retained");
    const newlyObserved = session("task-2", "Newly observed");
    const olderAbsent = session("task-4", "Older absent");
    const deleted = session("task-3", "Deleted");

    expect(
      reconcileAgentSessionSnapshot(
        [retained, deleted],
        [retained, newlyObserved, olderAbsent, deleted],
        new Set(["task-2", "task-3"]),
        new Set(["task-3"])
      )
    ).toEqual([newlyObserved, retained]);
  });
});
