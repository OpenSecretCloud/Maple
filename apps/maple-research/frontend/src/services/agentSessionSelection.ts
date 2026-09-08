export class AgentSessionSelectionMemory {
  private readonly sessionIdsByUser = new Map<string, string>();

  remember(userId: string, sessionId: string): void {
    this.sessionIdsByUser.set(userId, sessionId);
  }

  forget(userId: string, expectedSessionId?: string): void {
    if (
      expectedSessionId !== undefined &&
      this.sessionIdsByUser.get(userId) !== expectedSessionId
    ) {
      return;
    }

    this.sessionIdsByUser.delete(userId);
  }

  resolve(userId: string, sessions: readonly { id: string }[]): string | null {
    const rememberedSessionId = this.sessionIdsByUser.get(userId);
    if (rememberedSessionId === undefined) return null;

    if (sessions.some((session) => session.id === rememberedSessionId)) {
      return rememberedSessionId;
    }

    this.sessionIdsByUser.delete(userId);
    return null;
  }
}
