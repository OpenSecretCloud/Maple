export interface AgentQueuedMessage {
  queueId: string;
  messageId: string;
  sessionId: string;
  text: string;
  createdMs: number;
}

export interface AgentDesktopQueueSnapshot {
  revision: number;
  items: AgentQueuedMessage[];
}

export function emptyAgentDesktopQueueSnapshot(): AgentDesktopQueueSnapshot {
  return { revision: 0, items: [] };
}

export function applyAgentDesktopQueueSnapshot(
  current: AgentDesktopQueueSnapshot | undefined,
  incoming: AgentDesktopQueueSnapshot
): AgentDesktopQueueSnapshot {
  if (current && incoming.revision < current.revision) {
    return current;
  }
  return incoming;
}

export function queueSnapshotWithoutItem(
  current: AgentDesktopQueueSnapshot | undefined,
  queueId: string
): AgentDesktopQueueSnapshot {
  const base = current ?? emptyAgentDesktopQueueSnapshot();
  return {
    revision: base.revision,
    items: base.items.filter((item) => item.queueId !== queueId)
  };
}

export function restoreQueuedMessageToComposer(currentInput: string, queuedText: string): string {
  const draft = currentInput.trim();
  return draft ? `${queuedText}\n${currentInput}` : queuedText;
}

export function shouldPrepareThoughtAfterAgentSend(queued?: AgentQueuedMessage | null): boolean {
  return !queued;
}
