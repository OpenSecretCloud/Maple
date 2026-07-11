export type AgentAccountCleanup = (userId: string) => Promise<void>;
export type AgentAccountActivation = (userId: string) => void;

/**
 * Serializes authenticated-user transitions around Agent Mode cleanup.
 *
 * Cleanup targets are retained until they succeed, so a rapid A -> B -> C
 * transition cannot activate C while A or B still owns a runtime/proxy. The
 * coordinator deliberately contains no React or Tauri dependencies so the
 * ordering contract can be tested directly.
 */
export class AgentAuthLifecycleCoordinator {
  private currentUserId: string | null = null;
  private readonly pendingCleanupUserIds = new Set<string>();
  private tail: Promise<void> = Promise.resolve();

  constructor(
    private readonly cleanupAccount: AgentAccountCleanup,
    private readonly activateAccount: AgentAccountActivation
  ) {}

  transitionTo(nextUserId: string | null): Promise<void> {
    const previousUserId = this.currentUserId;
    if (previousUserId === nextUserId && this.pendingCleanupUserIds.size === 0) {
      return this.tail;
    }

    if (previousUserId && previousUserId !== nextUserId) {
      this.pendingCleanupUserIds.add(previousUserId);
    }
    this.currentUserId = nextUserId;

    const transition = this.tail
      .catch(() => undefined)
      .then(async () => {
        while (this.pendingCleanupUserIds.size > 0) {
          const userId = this.pendingCleanupUserIds.values().next().value as string;
          await this.cleanupAccount(userId);
          this.pendingCleanupUserIds.delete(userId);
        }

        if (this.currentUserId) {
          this.activateAccount(this.currentUserId);
        }
      });
    this.tail = transition;
    return transition;
  }

  async waitForUser(userId: string): Promise<void> {
    await this.tail;
    if (this.currentUserId !== userId) {
      throw new Error("Agent Mode authentication changed before initialization completed");
    }
  }
}
