import type { AgentTimelineItem } from "./agentRuntimeService";

export function hasRenderableThinkingText(text: string | null | undefined): boolean {
  return Boolean(text?.trim());
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
