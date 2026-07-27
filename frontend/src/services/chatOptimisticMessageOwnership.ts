const optimisticMessageByRuntimeOwner = new WeakMap<object, Map<number, string>>();

/** Registers the exact optimistic user item owned by an account-scoped run. */
export function registerChatOptimisticMessage(
  runtimeOwner: object,
  runToken: number,
  messageId: string
): void {
  const registry = optimisticMessageByRuntimeOwner.get(runtimeOwner) ?? new Map<number, string>();
  registry.set(runToken, messageId);
  optimisticMessageByRuntimeOwner.set(runtimeOwner, registry);
}

/** A remounted chat view can recover ownership through the persistent store. */
export function getRegisteredChatOptimisticMessage(
  runtimeOwner: object,
  runToken: number
): string | undefined {
  return optimisticMessageByRuntimeOwner.get(runtimeOwner)?.get(runToken);
}

/** Identity-safe removal cannot clear a replacement registration. */
export function unregisterChatOptimisticMessage(
  runtimeOwner: object,
  runToken: number,
  messageId: string
): boolean {
  const registry = optimisticMessageByRuntimeOwner.get(runtimeOwner);
  if (registry?.get(runToken) !== messageId) return false;

  registry.delete(runToken);
  if (registry.size === 0) optimisticMessageByRuntimeOwner.delete(runtimeOwner);
  return true;
}

export function markOptimisticMessageIncomplete<TMessage extends Readonly<{ id: string }>>(
  messages: readonly TMessage[],
  messageId: string | undefined
): TMessage[] {
  if (!messageId) return [...messages];
  return messages.map((message) =>
    message.id === messageId ? ({ ...message, status: "incomplete" } as TMessage) : message
  );
}
