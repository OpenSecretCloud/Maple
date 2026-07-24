export type AgentAccountCleanup = (userId: string) => Promise<void>;
export type AgentAccountActivation = (userId: string) => Promise<void>;

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
  private activatedUserId: string | null = null;
  private readonly pendingCleanupUserIds = new Set<string>();
  private tail: Promise<void> = Promise.resolve();

  constructor(
    private readonly cleanupAccount: AgentAccountCleanup,
    private readonly activateAccount: AgentAccountActivation
  ) {}

  transitionTo(nextUserId: string | null): Promise<void> {
    const previousUserId = this.currentUserId;
    if (
      previousUserId === nextUserId &&
      this.activatedUserId === nextUserId &&
      this.pendingCleanupUserIds.size === 0
    ) {
      return this.tail;
    }

    if (previousUserId && previousUserId !== nextUserId) {
      this.pendingCleanupUserIds.add(previousUserId);
    }
    if (this.activatedUserId !== nextUserId) this.activatedUserId = null;
    this.currentUserId = nextUserId;

    const transition = this.tail
      .catch(() => undefined)
      .then(async () => {
        while (this.pendingCleanupUserIds.size > 0) {
          const userId = this.pendingCleanupUserIds.values().next().value as string;
          await this.cleanupAccount(userId);
          this.pendingCleanupUserIds.delete(userId);
        }

        const userId = this.currentUserId;
        if (userId && this.activatedUserId !== userId) {
          await this.activateAccount(userId);
          if (this.currentUserId === userId) this.activatedUserId = userId;
        }
      });
    this.tail = transition;
    return transition;
  }

  /**
   * Ensure the requested account is still current and fully activated.
   *
   * Agent initialization can race the root-level transition and may be the
   * first production caller to observe a transient activation failure. Queueing
   * another same-user transition retries through the existing serialized lane;
   * a concurrent account change is detected before and after that queue entry
   * so this operation can never reactivate a stale user.
   */
  async ensureCurrentUser(userId: string): Promise<void> {
    if (this.currentUserId !== userId) {
      throw new Error("Agent Mode authentication changed before initialization completed");
    }
    await this.transitionTo(userId);
    await this.waitForUser(userId);
  }

  async waitForUser(userId: string): Promise<void> {
    await this.tail;
    if (this.currentUserId !== userId || this.activatedUserId !== userId) {
      throw new Error("Agent Mode authentication changed before initialization completed");
    }
  }
}
