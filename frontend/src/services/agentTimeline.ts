import type { AgentTimelineItem } from "./agentRuntimeService";

export type AgentTimelineTurn =
  | { type: "user"; item: AgentTimelineItem; id: string }
  | { type: "assistant"; items: AgentTimelineItem[]; id: string };

export interface AgentThoughtPhase {
  sessionId: string;
  phaseId: string;
  userRequest: string;
  reasoningText: string;
}

interface ActiveAgentThoughtPhase {
  phaseId: string;
  parts: Map<string, string>;
}

interface LiveAgentThoughtSession {
  assistantTurnId: string | null;
  userItemId: string | null;
  userRequest: string;
  nextPhaseIndex: number;
  activePhase: ActiveAgentThoughtPhase | null;
  seenItemIds: Set<string>;
}

function agentAssistantTurnId(userItemId: string): string {
  return `assistant-after-${userItemId}`;
}

export function agentThinkingPhaseId(assistantTurnId: string, phaseIndex: number): string {
  return `${assistantTurnId}:thought-${phaseIndex}`;
}

export function agentThinkingPhaseTurnId(phaseId: string): string {
  return phaseId.replace(/:thought-\d+$/u, "");
}

export function agentThoughtPhasesForLatestTurn(
  sessionId: string,
  items: AgentTimelineItem[]
): AgentThoughtPhase[] {
  let latestUserIndex = -1;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.itemType === "message" && item.role === "user") {
      latestUserIndex = index;
      break;
    }
  }
  if (latestUserIndex < 0) return [];

  const tracker = new AgentLiveThoughtPhaseTracker();
  const phases: AgentThoughtPhase[] = [];
  for (const item of items.slice(latestUserIndex)) {
    const completedPhase = tracker.observeTimelineItem(sessionId, item);
    if (completedPhase) phases.push(completedPhase);
  }
  const trailingPhase = tracker.finishRun(sessionId);
  if (trailingPhase) phases.push(trailingPhase);
  return phases;
}

export class AgentLiveThoughtPhaseTracker {
  private readonly sessions = new Map<string, LiveAgentThoughtSession>();
  private readonly completedPhaseIds = new Map<string, Set<string>>();

  prepareUserRequest(sessionId: string, userRequest: string): void {
    const existing = this.sessions.get(sessionId);
    this.sessions.set(sessionId, {
      assistantTurnId: null,
      userItemId: null,
      userRequest,
      nextPhaseIndex: 0,
      activePhase: null,
      seenItemIds: existing?.seenItemIds ?? new Set()
    });
  }

  observeTimelineItem(sessionId: string, item: AgentTimelineItem): AgentThoughtPhase | null {
    const session = this.session(sessionId);
    if (item.itemType === "message" && item.role === "user") {
      if (session.seenItemIds.has(item.id)) {
        if (session.userItemId === item.id && item.text !== undefined && item.text !== null) {
          session.userRequest =
            item.merge === "append" ? `${session.userRequest}${item.text}` : item.text;
        }
        return null;
      }

      const completedPhase = this.completeActivePhase(sessionId);
      const preparedUserRequest = session.assistantTurnId === null ? session.userRequest : "";
      session.seenItemIds.add(item.id);
      this.sessions.set(sessionId, {
        assistantTurnId: agentAssistantTurnId(item.id),
        userItemId: item.id,
        userRequest: item.text ?? preparedUserRequest,
        nextPhaseIndex: 0,
        activePhase: null,
        seenItemIds: session.seenItemIds
      });
      return completedPhase;
    }

    if (item.itemType !== "thinking") {
      if (session.seenItemIds.has(item.id)) return null;
      session.seenItemIds.add(item.id);
      return this.completeActivePhase(sessionId);
    }

    const seenBefore = session.seenItemIds.has(item.id);
    if (seenBefore && !session.activePhase?.parts.has(item.id)) return null;
    session.seenItemIds.add(item.id);

    if (!session.activePhase) {
      const assistantTurnId = session.assistantTurnId ?? "assistant-leading";
      session.activePhase = {
        phaseId: agentThinkingPhaseId(assistantTurnId, session.nextPhaseIndex),
        parts: new Map()
      };
    }

    const previousText = session.activePhase.parts.get(item.id) ?? "";
    if (item.text !== undefined && item.text !== null) {
      session.activePhase.parts.set(
        item.id,
        item.merge === "append" ? `${previousText}${item.text}` : item.text
      );
    }
    return null;
  }

  finishRun(sessionId: string): AgentThoughtPhase | null {
    return this.completeActivePhase(sessionId);
  }

  activePhase(sessionId: string): AgentThoughtPhase | null {
    const session = this.sessions.get(sessionId);
    const activePhase = session?.activePhase;
    if (!session || !activePhase || !session.userRequest.trim()) return null;
    return {
      sessionId,
      phaseId: activePhase.phaseId,
      userRequest: session.userRequest,
      reasoningText: Array.from(activePhase.parts.values()).join("")
    };
  }

  discardRun(sessionId: string): string | null {
    const assistantTurnId = this.sessions.get(sessionId)?.assistantTurnId ?? null;
    this.sessions.delete(sessionId);
    this.completedPhaseIds.delete(sessionId);
    return assistantTurnId;
  }

  resetForHistoryReplacement(sessionId: string): string | null {
    const session = this.sessions.get(sessionId);
    if (!session) return null;
    const assistantTurnId = session.assistantTurnId;
    session.nextPhaseIndex = 0;
    session.activePhase = null;
    session.seenItemIds = new Set(session.userItemId ? [session.userItemId] : []);

    if (assistantTurnId) {
      const phasePrefix = `${assistantTurnId}:thought-`;
      const completedForSession = this.completedPhaseIds.get(sessionId);
      completedForSession?.forEach((phaseId) => {
        if (phaseId.startsWith(phasePrefix)) completedForSession.delete(phaseId);
      });
    }
    return assistantTurnId;
  }

  seedActiveTimeline(sessionId: string, items: AgentTimelineItem[]): void {
    const visibleItems = coalesceAdjacentThinkingItems(items).filter(isRenderableAgentTimelineItem);
    const turns = groupAgentTimelineItems(visibleItems);
    let latestUserTurnIndex = -1;
    for (let index = turns.length - 1; index >= 0; index -= 1) {
      if (turns[index].type === "user") {
        latestUserTurnIndex = index;
        break;
      }
    }

    const previous = this.sessions.get(sessionId);
    const userTurn = latestUserTurnIndex >= 0 ? turns[latestUserTurnIndex] : undefined;
    const assistantTurn =
      latestUserTurnIndex >= 0 && turns[latestUserTurnIndex + 1]?.type === "assistant"
        ? turns[latestUserTurnIndex + 1]
        : undefined;
    const assistantItems = assistantTurn?.type === "assistant" ? assistantTurn.items : [];
    const thinkingItems = assistantItems.filter((item) => item.itemType === "thinking");
    const assistantTurnId =
      assistantTurn?.type === "assistant"
        ? assistantTurn.id
        : userTurn?.type === "user"
          ? agentAssistantTurnId(userTurn.id)
          : null;
    const rawTrailingThinkingItems: AgentTimelineItem[] = [];
    for (let index = items.length - 1; index >= 0; index -= 1) {
      const item = items[index];
      if (item.itemType !== "thinking") break;
      rawTrailingThinkingItems.unshift(item);
    }
    const trailingPhaseParts = new Map(
      rawTrailingThinkingItems.map((item) => [item.id, item.text ?? ""])
    );
    const trailingThinkingItem = rawTrailingThinkingItems[0] ?? null;
    const trailingPhaseIsVisible = hasRenderableThinkingText(
      rawTrailingThinkingItems.map((item) => item.text ?? "").join("")
    );
    const activePhaseIndex = trailingPhaseIsVisible
      ? thinkingItems.length - 1
      : thinkingItems.length;
    this.sessions.set(sessionId, {
      assistantTurnId,
      userItemId: userTurn?.type === "user" ? userTurn.id : null,
      userRequest:
        userTurn?.type === "user" ? (userTurn.item.text ?? "") : (previous?.userRequest ?? ""),
      nextPhaseIndex:
        trailingThinkingItem && assistantTurnId
          ? Math.max(0, activePhaseIndex)
          : thinkingItems.length,
      activePhase:
        trailingThinkingItem && assistantTurnId
          ? {
              phaseId: agentThinkingPhaseId(assistantTurnId, Math.max(0, activePhaseIndex)),
              parts: trailingPhaseParts
            }
          : null,
      seenItemIds: new Set(items.map((item) => item.id))
    });
  }

  forgetSession(sessionId: string): void {
    this.sessions.delete(sessionId);
    this.completedPhaseIds.delete(sessionId);
  }

  private session(sessionId: string): LiveAgentThoughtSession {
    const existing = this.sessions.get(sessionId);
    if (existing) return existing;
    const session: LiveAgentThoughtSession = {
      assistantTurnId: null,
      userItemId: null,
      userRequest: "",
      nextPhaseIndex: 0,
      activePhase: null,
      seenItemIds: new Set()
    };
    this.sessions.set(sessionId, session);
    return session;
  }

  private completeActivePhase(sessionId: string): AgentThoughtPhase | null {
    const session = this.sessions.get(sessionId);
    const activePhase = session?.activePhase;
    if (!session || !activePhase) return null;
    session.activePhase = null;

    const reasoningText = Array.from(activePhase.parts.values()).join("");
    if (!reasoningText.trim()) return null;
    session.nextPhaseIndex += 1;

    const completedForSession = this.completedPhaseIds.get(sessionId) ?? new Set<string>();
    this.completedPhaseIds.set(sessionId, completedForSession);
    if (completedForSession.has(activePhase.phaseId)) return null;
    completedForSession.add(activePhase.phaseId);

    if (!session.userRequest.trim()) return null;
    return {
      sessionId,
      phaseId: activePhase.phaseId,
      userRequest: session.userRequest,
      reasoningText
    };
  }
}

export function getAgentTurnCopyText(turn: AgentTimelineTurn): string {
  if (turn.type === "user") return turn.item.text ?? "";

  for (let index = turn.items.length - 1; index >= 0; index -= 1) {
    const item = turn.items[index];
    if (item.itemType === "message" && item.role === "assistant") return item.text ?? "";
  }

  return "";
}

export function hasRenderableThinkingText(text: string | null | undefined): boolean {
  return Boolean(text?.trim());
}

export function isRenderableAgentTimelineItem(item: AgentTimelineItem): boolean {
  if (item.itemType === "message") return Boolean(item.text?.trim());
  if (item.itemType === "thinking") return hasRenderableThinkingText(item.text);
  if (item.itemType === "system" || item.itemType === "error") {
    return Boolean(item.title?.trim() || item.text?.trim());
  }
  return true;
}

export function hasAgentUserMessage(items: AgentTimelineItem[]): boolean {
  return items.some((item) => item.itemType === "message" && item.role === "user");
}

export function activeAgentThinkingItemId(
  items: AgentTimelineItem[],
  isRunActive: boolean
): string | null {
  if (!isRunActive || items.length === 0) return null;
  const trailingItem = items[items.length - 1];
  return trailingItem.itemType === "thinking" ? trailingItem.id : null;
}

export function coalesceAdjacentThinkingItems(items: AgentTimelineItem[]): AgentTimelineItem[] {
  return items.reduce<AgentTimelineItem[]>((projected, item) => {
    const previous = projected[projected.length - 1];
    if (item.itemType === "thinking" && previous?.itemType === "thinking") {
      projected[projected.length - 1] = {
        ...previous,
        text: `${previous.text ?? ""}${item.text ?? ""}`
      };
      return projected;
    }
    projected.push(item);
    return projected;
  }, []);
}

export function groupAgentTimelineItems(items: AgentTimelineItem[]): AgentTimelineTurn[] {
  const turns: AgentTimelineTurn[] = [];
  let assistantItems: AgentTimelineItem[] = [];
  let assistantTurnId = "assistant-leading";

  const flushAssistantItems = () => {
    if (assistantItems.length === 0) return;
    turns.push({
      type: "assistant",
      items: assistantItems,
      id: assistantTurnId
    });
    assistantItems = [];
  };

  for (const item of items) {
    if (item.itemType === "message" && item.role === "user") {
      flushAssistantItems();
      turns.push({ type: "user", item, id: item.id });
      assistantTurnId = agentAssistantTurnId(item.id);
      continue;
    }
    assistantItems.push(item);
  }

  flushAssistantItems();
  return turns;
}

export function shouldShowAgentAssistantLoader(
  turns: AgentTimelineTurn[],
  isResponsePending: boolean
): boolean {
  return isResponsePending && turns[turns.length - 1]?.type === "user";
}
