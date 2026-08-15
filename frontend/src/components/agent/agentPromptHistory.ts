import type { AgentTimelineItem } from "@/services/agentRuntimeService";

export type AgentPromptHistoryDirection = "older" | "newer";

export interface AgentPromptHistoryNavigation {
  entries: readonly string[];
  index: number;
}

export interface AgentPromptHistoryKeyState {
  key: string;
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  isComposing: boolean;
}

export interface AgentPromptHistoryStep {
  navigation: AgentPromptHistoryNavigation | null;
  value: string;
}

export function agentPromptHistory(items: readonly AgentTimelineItem[]): string[] {
  return items.flatMap((item) => {
    const text = item.text;
    return item.itemType === "message" &&
      item.role === "user" &&
      typeof text === "string" &&
      text.length > 0
      ? [text]
      : [];
  });
}

export function agentPromptHistoryDirection(
  keyState: AgentPromptHistoryKeyState,
  navigation: AgentPromptHistoryNavigation | null
): AgentPromptHistoryDirection | null {
  if (
    keyState.isComposing ||
    keyState.altKey ||
    keyState.ctrlKey ||
    keyState.metaKey ||
    keyState.shiftKey ||
    keyState.selectionStart === null ||
    keyState.selectionEnd === null ||
    keyState.selectionStart !== keyState.selectionEnd
  ) {
    return null;
  }

  if (keyState.key !== "ArrowUp" && keyState.key !== "ArrowDown") return null;

  if (navigation) {
    if (keyState.value !== navigation.entries[navigation.index]) return null;
    return keyState.key === "ArrowUp" ? "older" : "newer";
  }

  return keyState.key === "ArrowUp" && keyState.value === "" ? "older" : null;
}

export function navigateAgentPromptHistory(
  navigation: AgentPromptHistoryNavigation | null,
  history: readonly string[],
  direction: AgentPromptHistoryDirection
): AgentPromptHistoryStep | null {
  if (!navigation) {
    if (direction === "newer" || history.length === 0) return null;
    const entries = [...history];
    const index = entries.length - 1;
    return { navigation: { entries, index }, value: entries[index] };
  }

  if (direction === "older") {
    const index = Math.max(0, navigation.index - 1);
    return {
      navigation: { entries: navigation.entries, index },
      value: navigation.entries[index]
    };
  }

  if (navigation.index === navigation.entries.length - 1) {
    return { navigation: null, value: "" };
  }

  const index = navigation.index + 1;
  return {
    navigation: { entries: navigation.entries, index },
    value: navigation.entries[index]
  };
}
