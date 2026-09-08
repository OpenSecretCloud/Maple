export type ChatProjectionKeyResolver<TKey extends string> = (key: TKey) => string;

export type ChatProjectionScrollLease<TKey extends string> = Readonly<{
  ownerKey: TKey;
  sequence: number;
}>;

function chatProjectionKeysMatch<TKey extends string>(
  first: TKey,
  second: TKey,
  resolveKey: ChatProjectionKeyResolver<TKey>
): boolean {
  return resolveKey(first) === resolveKey(second);
}

/**
 * Owns the one-time positioning request for the runtime currently projected
 * into Unified Chat's shared scroll container.
 */
export class ChatProjectionScrollCoordinator<TKey extends string = string> {
  private ownerKey: TKey | null = null;
  private positionPending = false;
  private sequence = 0;

  activate(key: TKey, resolveKey: ChatProjectionKeyResolver<TKey>): boolean {
    if (this.ownerKey && chatProjectionKeysMatch(this.ownerKey, key, resolveKey)) {
      return false;
    }

    this.ownerKey = key;
    this.positionPending = true;
    this.sequence += 1;
    return true;
  }

  deactivate(): void {
    this.ownerKey = null;
    this.positionPending = false;
    this.sequence += 1;
  }

  owns(key: TKey, resolveKey: ChatProjectionKeyResolver<TKey>): boolean {
    return Boolean(this.ownerKey && chatProjectionKeysMatch(this.ownerKey, key, resolveKey));
  }

  captureLease(
    key: TKey,
    resolveKey: ChatProjectionKeyResolver<TKey>
  ): ChatProjectionScrollLease<TKey> | null {
    if (!this.owns(key, resolveKey)) return null;
    return Object.freeze({ ownerKey: key, sequence: this.sequence });
  }

  ownsLease(
    lease: ChatProjectionScrollLease<TKey>,
    resolveKey: ChatProjectionKeyResolver<TKey>
  ): boolean {
    return this.sequence === lease.sequence && this.owns(lease.ownerKey, resolveKey);
  }

  takePositionRequest(
    key: TKey,
    ready: boolean,
    resolveKey: ChatProjectionKeyResolver<TKey>
  ): boolean {
    if (!ready || !this.positionPending || !this.owns(key, resolveKey)) {
      return false;
    }

    this.positionPending = false;
    return true;
  }
}

export type ChatProjectionMessage = Readonly<{
  id: string;
  type: string;
  role?: string;
}>;

export type ChatProjectionScrollTarget =
  | Readonly<{ type: "latest-user"; messageId: string }>
  | Readonly<{ type: "bottom" }>;

/**
 * A growing assistant block is not a stable bottom anchor. When returning to
 * an active stream, keep the newest user turn visible; completed chats open at
 * the bottom as before.
 */
export function chatProjectionScrollTarget(
  messages: readonly ChatProjectionMessage[],
  isGenerating: boolean
): ChatProjectionScrollTarget {
  if (isGenerating) {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      const message = messages[index];
      if (message.type === "message" && message.role === "user") {
        return { type: "latest-user", messageId: message.id };
      }
    }
  }

  return { type: "bottom" };
}

export function projectedUserTurnScrollTop({
  currentScrollTop,
  containerTop,
  userTurnTop,
  topOffset = 16
}: Readonly<{
  currentScrollTop: number;
  containerTop: number;
  userTurnTop: number;
  topOffset?: number;
}>): number {
  return Math.max(0, currentScrollTop + userTurnTop - containerTop - topOffset);
}
