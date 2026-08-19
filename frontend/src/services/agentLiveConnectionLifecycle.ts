import type { AgentActiveLiveStream, AgentPendingHistoryAttach } from "./agentRuntimeService";

export class AgentLiveConnectionError extends Error {
  constructor(
    message: string,
    readonly errors: readonly unknown[]
  ) {
    super(message);
    this.name = "AgentLiveConnectionError";
  }
}

async function throwCancellationFailures(results: readonly PromiseSettledResult<void>[]) {
  const failures = results.flatMap((result) =>
    result.status === "rejected" ? [result.reason] : []
  );
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) {
    throw new AgentLiveConnectionError("Unable to retire the Agent live connection", failures);
  }
}

function invokeCancellation(cancel: () => Promise<void>): Promise<void> {
  try {
    return Promise.resolve(cancel());
  } catch (error) {
    return Promise.reject(error);
  }
}

/**
 * Owns every frontend live-channel handle until its cancellation succeeds.
 * The handles themselves already normalize benign not-found, stale-lease, and
 * channel-closed cleanup outcomes, so a rejection here is always fail-loud.
 */
export class AgentLiveConnectionRegistry {
  private readonly pendingHandles = new Set<AgentPendingHistoryAttach>();
  private readonly activeHandles = new Set<AgentActiveLiveStream>();

  get hasPending(): boolean {
    return this.pendingHandles.size > 0;
  }

  get pendingCount(): number {
    return this.pendingHandles.size;
  }

  get activeCount(): number {
    return this.activeHandles.size;
  }

  trackPending(pending: AgentPendingHistoryAttach): void {
    this.pendingHandles.add(pending);
  }

  promote(pending: AgentPendingHistoryAttach, active: AgentActiveLiveStream): void {
    this.pendingHandles.delete(pending);
    this.activeHandles.add(active);
  }

  trackActive(active: AgentActiveLiveStream): void {
    this.activeHandles.add(active);
  }

  async cancelPending(pending: AgentPendingHistoryAttach): Promise<void> {
    this.pendingHandles.add(pending);
    const [result] = await Promise.allSettled([invokeCancellation(() => pending.cancel())]);
    if (result.status === "fulfilled") this.pendingHandles.delete(pending);
    await throwCancellationFailures([result]);
  }

  async cancelActive(active: AgentActiveLiveStream): Promise<void> {
    this.activeHandles.add(active);
    const [result] = await Promise.allSettled([invokeCancellation(() => active.cancel())]);
    if (result.status === "fulfilled") this.activeHandles.delete(active);
    await throwCancellationFailures([result]);
  }

  async retire(): Promise<void> {
    const pending = [...this.pendingHandles];
    const active = [...this.activeHandles];
    const results = await Promise.allSettled([
      ...pending.map((handle) => invokeCancellation(() => handle.cancel())),
      ...active.map((handle) => invokeCancellation(() => handle.cancel()))
    ]);

    pending.forEach((handle, index) => {
      if (results[index]?.status === "fulfilled") this.pendingHandles.delete(handle);
    });
    active.forEach((handle, index) => {
      if (results[pending.length + index]?.status === "fulfilled") {
        this.activeHandles.delete(handle);
      }
    });
    await throwCancellationFailures(results);
  }
}

export async function recoverAgentLiveConnectionAfterReplacementFailure<TCursor>({
  replacementError,
  cursor,
  retire,
  resume
}: {
  replacementError: unknown;
  cursor: TCursor | null;
  retire: () => Promise<void>;
  resume: (cursor: TCursor) => Promise<void>;
}): Promise<never> {
  try {
    await retire();
  } catch (retirementError) {
    throw new AgentLiveConnectionError("Agent history replacement and live cleanup both failed", [
      replacementError,
      retirementError
    ]);
  }

  if (cursor !== null) {
    try {
      await resume(cursor);
    } catch (resumeError) {
      throw new AgentLiveConnectionError(
        "Agent history replacement failed and cursor recovery could not resume",
        [replacementError, resumeError]
      );
    }
  }
  throw replacementError;
}
