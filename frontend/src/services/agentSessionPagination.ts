import type { AgentPage, AgentSessionSummary } from "./agentRuntimeService";

export interface AgentSessionPageToken {
  readonly kind: "head" | "older";
  readonly cursor: string | null;
  readonly requestId: number;
  readonly mutationRevision: number;
}

export interface AgentSessionPageSnapshot {
  readonly items: readonly AgentSessionSummary[];
  readonly nextCursor: string | null;
  readonly headLoaded: boolean;
  readonly isLoading: boolean;
  readonly hasMore: boolean;
}

export type AgentSessionPageCommitResult = "applied" | "stale";

function newestSessionFirst(left: AgentSessionSummary, right: AgentSessionSummary): number {
  const pageOrder = right.pageSortMs - left.pageSortMs;
  if (pageOrder !== 0) return pageOrder;
  if (left.id === right.id) return 0;
  return left.id < right.id ? 1 : -1;
}

/** Account-scoped count pager for the Agent task sidebar. */
export class AgentSessionPaginationCache {
  private items: AgentSessionSummary[] = [];
  private nextCursor: string | null = null;
  private headLoaded = false;
  private loadedOlder = false;
  private nextRequestId = 0;
  private activeRequestId: number | null = null;
  private mutationRevision = 0;
  private readonly revisionsById = new Map<string, number>();
  private readonly deletedIds = new Set<string>();
  private readonly pagedIds = new Set<string>();

  beginHead(): AgentSessionPageToken {
    return this.begin("head", null);
  }

  beginOlder(): AgentSessionPageToken | null {
    if (!this.headLoaded || !this.nextCursor || this.activeRequestId !== null) return null;
    return this.begin("older", this.nextCursor);
  }

  commit(
    token: AgentSessionPageToken,
    page: AgentPage<AgentSessionSummary>
  ): AgentSessionPageCommitResult {
    if (this.activeRequestId !== token.requestId) return "stale";
    this.activeRequestId = null;

    const currentById = new Map(this.items.map((item) => [item.id, item]));
    const resolveRacingMutation = (item: AgentSessionSummary) =>
      (this.revisionsById.get(item.id) ?? 0) > token.mutationRevision
        ? (currentById.get(item.id) ?? item)
        : item;
    const incoming = page.items
      .filter((item) => !this.deletedIds.has(item.id))
      .map(resolveRacingMutation);
    const incomingIds = new Set(incoming.map((item) => item.id));

    if (token.kind === "older") {
      const currentIds = new Set(this.items.map((item) => item.id));
      this.items = [...this.items, ...incoming.filter((item) => !currentIds.has(item.id))].sort(
        newestSessionFirst
      );
      incoming.forEach((item) => this.pagedIds.add(item.id));
      this.nextCursor = page.nextCursor ?? null;
      this.loadedOlder = true;
      return "applied";
    }

    const changedAfterRequest = this.items.filter(
      (item) =>
        (this.revisionsById.get(item.id) ?? 0) > token.mutationRevision &&
        !incomingIds.has(item.id) &&
        !this.deletedIds.has(item.id)
    );
    const changedIds = new Set(changedAfterRequest.map((item) => item.id));
    const isCompleteSnapshot = page.nextCursor == null;
    const retained = isCompleteSnapshot
      ? []
      : this.items.filter(
          (item) =>
            !incomingIds.has(item.id) && !changedIds.has(item.id) && !this.deletedIds.has(item.id)
        );
    const overlapsLoadedRange = incoming.some((item) => this.pagedIds.has(item.id));
    this.items = [...changedAfterRequest, ...incoming, ...retained].sort(newestSessionFirst);
    if (isCompleteSnapshot) {
      this.pagedIds.clear();
      this.items.forEach((item) => this.pagedIds.add(item.id));
    } else {
      incoming.forEach((item) => this.pagedIds.add(item.id));
    }
    if (isCompleteSnapshot || !this.headLoaded || !this.loadedOlder || !overlapsLoadedRange) {
      this.nextCursor = page.nextCursor ?? null;
    }
    this.headLoaded = true;
    return "applied";
  }

  fail(token: AgentSessionPageToken): void {
    if (this.activeRequestId === token.requestId) this.activeRequestId = null;
  }

  upsert(summary: AgentSessionSummary): void {
    this.mutationRevision += 1;
    this.revisionsById.set(summary.id, this.mutationRevision);
    this.deletedIds.delete(summary.id);
    const index = this.items.findIndex((item) => item.id === summary.id);
    if (index < 0) {
      this.items = [summary, ...this.items].sort(newestSessionFirst);
      return;
    }
    const next = [...this.items];
    next[index] = summary;
    this.items = next.sort(newestSessionFirst);
  }

  remove(sessionId: string): void {
    this.mutationRevision += 1;
    this.revisionsById.set(sessionId, this.mutationRevision);
    this.deletedIds.add(sessionId);
    this.pagedIds.delete(sessionId);
    this.items = this.items.filter((item) => item.id !== sessionId);
  }

  snapshot(): AgentSessionPageSnapshot {
    return {
      items: this.items,
      nextCursor: this.nextCursor,
      headLoaded: this.headLoaded,
      isLoading: this.activeRequestId !== null,
      hasMore: Boolean(this.nextCursor)
    };
  }

  summaryRevision(sessionId: string): number {
    return this.revisionsById.get(sessionId) ?? 0;
  }

  clear(): void {
    this.items = [];
    this.nextCursor = null;
    this.headLoaded = false;
    this.loadedOlder = false;
    this.nextRequestId += 1;
    this.activeRequestId = null;
    this.mutationRevision = 0;
    this.revisionsById.clear();
    this.deletedIds.clear();
    this.pagedIds.clear();
  }

  private begin(kind: "head" | "older", cursor: string | null): AgentSessionPageToken {
    this.nextRequestId += 1;
    this.activeRequestId = this.nextRequestId;
    return Object.freeze({
      kind,
      cursor,
      requestId: this.nextRequestId,
      mutationRevision: this.mutationRevision
    });
  }
}
