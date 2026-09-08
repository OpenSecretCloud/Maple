import { describe, expect, test } from "bun:test";
import { AgentLiveThoughtPhaseTracker, type AgentThoughtPhase } from "./agentTimeline";
import {
  settleAgentThoughtRun,
  type AgentThoughtRunTracker,
  type AgentThoughtRunTerminalStatus
} from "./agentThoughtRunLifecycle";

function liveTracker(): AgentLiveThoughtPhaseTracker {
  const tracker = new AgentLiveThoughtPhaseTracker();
  tracker.observeTimelineItem("session", {
    id: "user",
    itemType: "message",
    role: "user",
    text: "Inspect login",
    createdMs: 0,
    merge: "replace"
  });
  tracker.observeTimelineItem("session", {
    id: "thought",
    itemType: "thinking",
    role: "thought",
    text: "Evaluating the login flow",
    createdMs: 0,
    merge: "replace"
  });
  return tracker;
}

function settle(
  status: AgentThoughtRunTerminalStatus,
  tracker: AgentThoughtRunTracker = liveTracker()
) {
  const sequence: string[] = [];
  const finalized: AgentThoughtPhase[] = [];
  const released: AgentThoughtPhase[] = [];
  const invalidated: Array<{ sessionId: string; assistantTurnId: string | null }> = [];

  settleAgentThoughtRun({
    sessionId: "session",
    status,
    tracker: {
      activePhase: (sessionId) => {
        sequence.push("activePhase");
        return tracker.activePhase(sessionId);
      },
      finishRun: (sessionId) => {
        sequence.push("finishRun");
        return tracker.finishRun(sessionId);
      },
      discardRun: (sessionId) => {
        sequence.push("discardRun");
        return tracker.discardRun(sessionId);
      }
    },
    finalizePhase: (phase) => {
      sequence.push("finalize: retain provisional and start final");
      finalized.push(phase);
    },
    releaseProvisional: (phase) => {
      sequence.push("releaseProvisional");
      released.push(phase);
    },
    cancelAndInvalidateLabels: (sessionId, assistantTurnId) => {
      sequence.push("cancelAndInvalidateLabels");
      invalidated.push({ sessionId, assistantTurnId });
    }
  });

  return { tracker, sequence, finalized, released, invalidated };
}

describe("settleAgentThoughtRun", () => {
  for (const status of ["completed", "failed"] as const) {
    test(`${status} follows the finish/finalize path and preserves its provisional`, () => {
      const result = settle(status);

      expect(result.sequence).toEqual([
        "activePhase",
        "finishRun",
        "finalize: retain provisional and start final"
      ]);
      expect(result.finalized).toEqual([
        {
          sessionId: "session",
          phaseId: "assistant-after-user:thought-0",
          userRequest: "Inspect login",
          reasoningText: "Evaluating the login flow"
        }
      ]);
      expect(result.released).toEqual([]);
      expect(result.invalidated).toEqual([]);
    });
  }

  test("discards, cancels, and invalidates only a cancelled run", () => {
    const result = settle("cancelled");

    expect(result.sequence).toEqual(["discardRun", "cancelAndInvalidateLabels"]);
    expect(result.finalized).toEqual([]);
    expect(result.released).toEqual([]);
    expect(result.invalidated).toEqual([
      { sessionId: "session", assistantTurnId: "assistant-after-user" }
    ]);
    expect(result.tracker.finishRun("session")).toBeNull();
  });

  test("releases a provisional when an active phase was already completed elsewhere", () => {
    const activePhase: AgentThoughtPhase = {
      sessionId: "session",
      phaseId: "assistant-after-user:thought-0",
      userRequest: "Inspect login",
      reasoningText: "Evaluating the login flow"
    };
    const result = settle("completed", {
      activePhase: () => activePhase,
      finishRun: () => null,
      discardRun: () => {
        throw new Error("completed runs must not be discarded");
      }
    });

    expect(result.finalized).toEqual([]);
    expect(result.sequence).toEqual(["activePhase", "finishRun", "releaseProvisional"]);
    expect(result.released).toEqual([activePhase]);
    expect(result.invalidated).toEqual([]);
  });
});
