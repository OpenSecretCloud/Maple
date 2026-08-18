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

export interface AgentQueuedMessageEdit {
  sessionId: string;
  queueId: string;
  stashedDraft: string;
}

export function restoreQueuedMessageToComposer(currentInput: string, queuedText: string): string {
  const draft = currentInput.trim();
  return draft ? `${queuedText}\n${currentInput}` : queuedText;
}

export function beginQueuedMessageEdit({
  current,
  sessionId,
  item,
  composerText
}: {
  current: AgentQueuedMessageEdit | null;
  sessionId: string;
  item: AgentQueuedMessage;
  composerText: string;
}): { edit: AgentQueuedMessageEdit; composer: string } | null {
  if (current?.sessionId === sessionId && current.queueId === item.queueId) {
    return null;
  }
  return {
    edit: {
      sessionId,
      queueId: item.queueId,
      stashedDraft: current?.sessionId === sessionId ? current.stashedDraft : composerText
    },
    composer: item.text
  };
}

export function discardQueuedMessageEdit(edit: AgentQueuedMessageEdit): string {
  return edit.stashedDraft;
}

export function queuedMessageEditStillPresent(
  edit: AgentQueuedMessageEdit | null,
  items: AgentQueuedMessage[]
): boolean {
  return Boolean(edit && items.some((item) => item.queueId === edit.queueId));
}

export function shouldPrepareThoughtAfterAgentSend(queued?: AgentQueuedMessage | null): boolean {
  return !queued;
}
