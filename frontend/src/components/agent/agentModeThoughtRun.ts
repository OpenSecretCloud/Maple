import type { AgentEventEnvelope, AgentTimelineItem } from "@/services/agentRuntimeService";
import {
  settleAgentThoughtRun,
  type AgentThoughtRunTracker,
  type AgentThoughtRunTerminalStatus
} from "@/services/agentThoughtRunLifecycle";
import { agentThoughtPhasesForLatestTurn, type AgentThoughtPhase } from "@/services/agentTimeline";

function terminalStatus(message: string | null | undefined): AgentThoughtRunTerminalStatus | null {
  return message === "completed" || message === "failed" || message === "cancelled"
    ? message
    : null;
}

export function handleAgentModeThoughtRunFinished({
  event,
  timelineRevision,
  tracker,
  finalizePhase,
  releaseProvisional,
  cancelAndInvalidateLabels,
  loadTimeline,
  canApplyTimeline,
  replaceTimeline
}: {
  event: AgentEventEnvelope;
  timelineRevision?: number;
  tracker: AgentThoughtRunTracker;
  finalizePhase: (phase: AgentThoughtPhase) => void;
  releaseProvisional: (phase: AgentThoughtPhase) => void;
  cancelAndInvalidateLabels: (sessionId: string, assistantTurnId: string | null) => void;
  loadTimeline: (sessionId: string) => Promise<AgentTimelineItem[]>;
  canApplyTimeline: (sessionId: string) => boolean;
  replaceTimeline: (
    sessionId: string,
    timeline: AgentTimelineItem[],
    timelineRevision?: number
  ) => boolean;
}): Promise<void> | null {
  const status = terminalStatus(event.message);
  const sessionId = event.sessionId;
  if (event.eventType !== "runFinished" || !sessionId || !status) return null;

  settleAgentThoughtRun({
    sessionId,
    status,
    tracker,
    finalizePhase,
    releaseProvisional,
    cancelAndInvalidateLabels
  });

  return loadTimeline(sessionId).then((timeline) => {
    if (!canApplyTimeline(sessionId)) return;
    const replaced = replaceTimeline(sessionId, timeline, timelineRevision);
    if (!replaced || status === "cancelled") return;
    agentThoughtPhasesForLatestTurn(sessionId, timeline).forEach(finalizePhase);
  });
}
