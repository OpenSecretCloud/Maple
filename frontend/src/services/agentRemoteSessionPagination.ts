import type {
  AgentRemoteSessionPage,
  AgentRemoteSessionSummary
} from "@/services/agentRemoteProviderBridge";

export interface AgentRemoteSessionPageToken {
  readonly kind: "head" | "older";
  readonly cursor: string | null;
  readonly requestId: number;
  readonly cacheEpoch: number;
}

export interface AgentRemoteSessionPageSnapshot {
  readonly items: readonly AgentRemoteSessionSummary[];
  readonly nextCursor: string | null;
  readonly headLoaded: boolean;
  readonly isLoading: boolean;
  readonly hasMore: boolean;
}

export type AgentRemoteSessionPageCommitResult = "applied" | "stale";

export class AgentRemoteSessionWindowLimitError extends Error {
  constructor() {
    super("Remote Agent task window reached its presentation bound");
    this.name = "AgentRemoteSessionWindowLimitError";
  }
}

/** Mutation-free, bounded pager for the paired-host task browser. */
export class AgentRemoteSessionPaginationCache {
  private items: AgentRemoteSessionSummary[] = [];
  private nextCursor: string | null = null;
  private headLoaded = false;
  private nextRequestId = 0;
  private activeRequestId: number | null = null;
  private cacheEpoch = 0;

  constructor(private readonly maxItems = 200) {
    if (!Number.isSafeInteger(maxItems) || maxItems < 1) {
      throw new Error("Remote Agent task window must be a positive safe integer");
    }
  }

  beginHead(): AgentRemoteSessionPageToken {
    return this.begin("head", null);
  }

  beginOlder(): AgentRemoteSessionPageToken | null {
    if (!this.headLoaded || !this.nextCursor || this.activeRequestId !== null) return null;
    return this.begin("older", this.nextCursor);
  }

  commit(
    token: AgentRemoteSessionPageToken,
    page: AgentRemoteSessionPage
  ): AgentRemoteSessionPageCommitResult {
    if (token.cacheEpoch !== this.cacheEpoch || this.activeRequestId !== token.requestId) {
      return "stale";
    }
    this.activeRequestId = null;

    if (token.kind === "head") {
      if (page.items.length > this.maxItems) throw new AgentRemoteSessionWindowLimitError();
      this.items = [...page.items].sort(newestSessionFirst);
      this.nextCursor = page.nextCursor ?? null;
      this.headLoaded = true;
      return "applied";
    }

    const existingIds = new Set(this.items.map((item) => item.id));
    const incoming = page.items.filter((item) => !existingIds.has(item.id));
    if (this.items.length + incoming.length > this.maxItems) {
      throw new AgentRemoteSessionWindowLimitError();
    }
    this.items = [...this.items, ...incoming].sort(newestSessionFirst);
    this.nextCursor = page.nextCursor ?? null;
    return "applied";
  }

  fail(token: AgentRemoteSessionPageToken): void {
    if (token.cacheEpoch === this.cacheEpoch && this.activeRequestId === token.requestId) {
      this.activeRequestId = null;
    }
  }

  snapshot(): AgentRemoteSessionPageSnapshot {
    return {
      items: this.items,
      nextCursor: this.nextCursor,
      headLoaded: this.headLoaded,
      isLoading: this.activeRequestId !== null,
      hasMore: Boolean(this.nextCursor)
    };
  }

  clear(): void {
    this.items = [];
    this.nextCursor = null;
    this.headLoaded = false;
    this.nextRequestId += 1;
    this.activeRequestId = null;
    this.cacheEpoch += 1;
  }

  private begin(kind: "head" | "older", cursor: string | null): AgentRemoteSessionPageToken {
    this.nextRequestId += 1;
    this.activeRequestId = this.nextRequestId;
    return Object.freeze({
      kind,
      cursor,
      requestId: this.nextRequestId,
      cacheEpoch: this.cacheEpoch
    });
  }
}

function newestSessionFirst(
  left: AgentRemoteSessionSummary,
  right: AgentRemoteSessionSummary
): number {
  const pageOrder = right.pageSortMs - left.pageSortMs;
  if (pageOrder !== 0) return pageOrder;
  if (left.id === right.id) return 0;
  return left.id < right.id ? 1 : -1;
}
