import type { AgentTimelineItem } from "./agentRuntimeService";

export type AgentTimelineTurn =
  | { type: "user"; item: AgentTimelineItem; id: string }
  | { type: "assistant"; items: AgentTimelineItem[]; id: string };

export function hasRenderableThinkingText(text: string | null | undefined): boolean {
  return Boolean(text?.trim());
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
      assistantTurnId = `assistant-after-${item.id}`;
      continue;
    }
    assistantItems.push(item);
  }

  flushAssistantItems();
  return turns;
}
