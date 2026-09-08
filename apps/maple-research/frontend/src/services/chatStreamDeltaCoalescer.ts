export type ChatStreamTextDelta = {
  kind: "message" | "reasoning";
  itemId: string;
  contentIndex: number;
  delta: string;
};

export type ChatStreamDeltaScheduler = {
  schedule: (task: () => void, delayMs: number) => unknown;
  cancel: (handle: unknown) => void;
};

export type ChatStreamTerminalState = "completed" | "cancelled" | "error" | null;
export type ChatStreamEofDisposition = "completed" | "terminal" | "truncated" | "stale";

type ChatStreamDeltaCoalescerOptions = {
  delayMs?: number;
  isCurrent: () => boolean;
  onFlush: (deltas: ChatStreamTextDelta[]) => void;
  scheduler?: ChatStreamDeltaScheduler;
};

type FlushableChatStreamDeltaCoalescer = {
  flush: () => boolean;
};

const coalescersByRuntimeOwner = new WeakMap<
  object,
  Map<number, FlushableChatStreamDeltaCoalescer>
>();

const defaultScheduler: ChatStreamDeltaScheduler = {
  schedule: (task, delayMs) => setTimeout(task, delayMs),
  cancel: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>)
};

const DEFAULT_STREAM_DELTA_FLUSH_DELAY_MS = 40;

/**
 * OpenAI emits `error` or `response.failed`; OpenSecret's terminal stream-error
 * event is currently named `response.error`.
 */
export function isTerminalChatStreamErrorEvent(eventType: string): boolean {
  return eventType === "error" || eventType === "response.error" || eventType === "response.failed";
}

/**
 * A clean iterator EOF is not proof that the response completed: fetch streams
 * can close without a terminal SSE event. Intentional cancellation and account
 * teardown clear run ownership first, so their EOF remains silent.
 */
export function classifyChatStreamEof(
  terminalState: ChatStreamTerminalState,
  isRunCurrent: boolean
): ChatStreamEofDisposition {
  if (terminalState === "completed") return "completed";
  if (terminalState !== null) return "terminal";
  return isRunCurrent ? "truncated" : "stale";
}

/**
 * Removes only response items created by one failed stream attempt. The local
 * user message and items from earlier turns or concurrent runs are not owned by
 * this attempt and therefore remain untouched before a retry.
 */
export function removeOwnedChatStreamAttemptItems<TMessage extends Readonly<{ id: string }>>(
  messages: readonly TMessage[],
  ownedItemIds: ReadonlySet<string>
): TMessage[] {
  if (ownedItemIds.size === 0) return [...messages];
  return messages.filter((message) => !ownedItemIds.has(message.id));
}

function deltaTargetKey(delta: ChatStreamTextDelta): string {
  return `${delta.kind}\u0000${delta.itemId}\u0000${delta.contentIndex}`;
}

/**
 * Keeps cancellation lookup scoped to the account-owned runtime store rather
 * than a particular UnifiedChat mount. The WeakMap cannot retain a disposed
 * account store, and identity-safe removal cannot unregister a replacement.
 */
export function registerChatStreamDeltaCoalescer(
  runtimeOwner: object,
  runToken: number,
  coalescer: FlushableChatStreamDeltaCoalescer
): void {
  const registry = coalescersByRuntimeOwner.get(runtimeOwner) ?? new Map();
  registry.set(runToken, coalescer);
  coalescersByRuntimeOwner.set(runtimeOwner, registry);
}

export function flushRegisteredChatStreamDeltas(runtimeOwner: object, runToken: number): boolean {
  return coalescersByRuntimeOwner.get(runtimeOwner)?.get(runToken)?.flush() ?? false;
}

export function unregisterChatStreamDeltaCoalescer(
  runtimeOwner: object,
  runToken: number,
  coalescer: FlushableChatStreamDeltaCoalescer
): boolean {
  const registry = coalescersByRuntimeOwner.get(runtimeOwner);
  if (registry?.get(runToken) !== coalescer) return false;

  registry.delete(runToken);
  if (registry.size === 0) coalescersByRuntimeOwner.delete(runtimeOwner);
  return true;
}

/**
 * Coalesces high-frequency SSE text deltas without sharing state between runs.
 * A run-token check immediately before every flush prevents a cancelled run's
 * delayed task from writing into a replacement run for the same conversation.
 */
export function createChatStreamDeltaCoalescer({
  delayMs = DEFAULT_STREAM_DELTA_FLUSH_DELAY_MS,
  isCurrent,
  onFlush,
  scheduler = defaultScheduler
}: ChatStreamDeltaCoalescerOptions) {
  let scheduledHandle: unknown | null = null;
  let pendingOrder: string[] = [];
  let pendingByTarget = new Map<string, ChatStreamTextDelta>();
  let closed = false;

  const takePending = (): ChatStreamTextDelta[] => {
    const deltas = pendingOrder.map((key) => pendingByTarget.get(key)!);
    pendingOrder = [];
    pendingByTarget = new Map();
    return deltas;
  };

  const flushScheduledBatch = (): boolean => {
    scheduledHandle = null;
    if (pendingOrder.length === 0) return false;

    const deltas = takePending();
    if (!isCurrent()) {
      closed = true;
      return false;
    }
    onFlush(deltas);
    return true;
  };

  const flush = (): boolean => {
    if (scheduledHandle !== null) {
      scheduler.cancel(scheduledHandle);
      scheduledHandle = null;
    }
    if (pendingOrder.length === 0) return false;

    const deltas = takePending();
    if (!isCurrent()) {
      closed = true;
      return false;
    }
    onFlush(deltas);
    return true;
  };

  const discard = (): void => {
    if (scheduledHandle !== null) {
      scheduler.cancel(scheduledHandle);
      scheduledHandle = null;
    }
    pendingOrder = [];
    pendingByTarget = new Map();
    closed = true;
  };

  return {
    enqueue(delta: ChatStreamTextDelta): void {
      if (closed || !delta.delta) return;

      const key = deltaTargetKey(delta);
      const pending = pendingByTarget.get(key);
      if (pending) {
        pending.delta += delta.delta;
      } else {
        pendingOrder.push(key);
        pendingByTarget.set(key, { ...delta });
      }

      if (scheduledHandle === null) {
        scheduledHandle = scheduler.schedule(flushScheduledBatch, delayMs);
      }
    },
    flush,
    finish(): boolean {
      const didFlush = flush();
      closed = true;
      return didFlush;
    },
    discard,
    hasPending: () => pendingOrder.length > 0,
    isClosed: () => closed
  };
}
