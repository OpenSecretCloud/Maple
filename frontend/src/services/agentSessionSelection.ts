export class AgentSessionSelectionMemory {
  private readonly sessionIdsByOwner = new Map<string, string>();

  remember(ownerKey: string, sessionId: string): void {
    this.sessionIdsByOwner.set(ownerKey, sessionId);
  }

  forget(ownerKey: string, expectedSessionId?: string): void {
    if (
      expectedSessionId !== undefined &&
      this.sessionIdsByOwner.get(ownerKey) !== expectedSessionId
    ) {
      return;
    }

    this.sessionIdsByOwner.delete(ownerKey);
  }

  resolve(
    ownerKey: string,
    sessions: readonly { id: string }[],
    { historyComplete = true }: { historyComplete?: boolean } = {}
  ): string | null {
    const rememberedSessionId = this.sessionIdsByOwner.get(ownerKey);
    if (rememberedSessionId === undefined) return null;

    if (sessions.some((session) => session.id === rememberedSessionId)) {
      return rememberedSessionId;
    }

    // A paged sidebar cannot distinguish a deleted task from one beyond the
    // loaded head until its cursor is exhausted. Preserve the memory meanwhile.
    if (historyComplete) this.sessionIdsByOwner.delete(ownerKey);
    return null;
  }
}
