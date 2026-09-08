import type { AgentLiveThoughtPhaseTracker, AgentThoughtPhase } from "./agentTimeline";

export type AgentThoughtRunTerminalStatus = "completed" | "failed" | "cancelled";

export type AgentThoughtRunTracker = Pick<
  AgentLiveThoughtPhaseTracker,
  "activePhase" | "finishRun" | "discardRun"
>;

export interface SettleAgentThoughtRunOptions {
  sessionId: string;
  status: AgentThoughtRunTerminalStatus;
  tracker: AgentThoughtRunTracker;
  finalizePhase: (phase: AgentThoughtPhase) => void;
  releaseProvisional: (phase: AgentThoughtPhase) => void;
  cancelAndInvalidateLabels: (sessionId: string, assistantTurnId: string | null) => void;
}

export function settleAgentThoughtRun({
  sessionId,
  status,
  tracker,
  finalizePhase,
  releaseProvisional,
  cancelAndInvalidateLabels
}: SettleAgentThoughtRunOptions): void {
  if (status === "cancelled") {
    const assistantTurnId = tracker.discardRun(sessionId);
    cancelAndInvalidateLabels(sessionId, assistantTurnId);
    return;
  }

  const activePhase = tracker.activePhase(sessionId);
  const completedPhase = tracker.finishRun(sessionId);
  if (completedPhase) {
    finalizePhase(completedPhase);
  } else if (activePhase) {
    // Finishing can consume a duplicate or non-renderable active phase without
    // returning it. Release any provisional scheduler entry that remains.
    releaseProvisional(activePhase);
  }
}
