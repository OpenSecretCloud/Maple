import { describe, expect, test } from "bun:test";
import {
  AgentRemoteSessionPaginationCache,
  AgentRemoteSessionWindowLimitError
} from "@/services/agentRemoteSessionPagination";
import type { AgentRemoteSessionSummary } from "@/services/agentRemoteProviderBridge";

function session(
  id: string,
  pageSortMs: number,
  overrides: Partial<AgentRemoteSessionSummary> = {}
): AgentRemoteSessionSummary {
  return {
    id,
    title: id,
    createdMs: pageSortMs,
    updatedMs: pageSortMs,
    pageSortMs,
    messageCount: 1,
    ...overrides
  };
}

describe("remote Agent session pagination", () => {
  test("loads one head and explicit older pages in stable newest-first order", () => {
    const cache = new AgentRemoteSessionPaginationCache(5);
    const head = cache.beginHead();
    expect(cache.commit(head, { items: [session("b", 20)], nextCursor: "cursor-b" })).toBe(
      "applied"
    );
    const older = cache.beginOlder()!;
    expect(older.cursor).toBe("cursor-b");
    expect(
      cache.commit(older, {
        items: [session("a", 10), session("c", 30)],
        nextCursor: null
      })
    ).toBe("applied");
    expect(cache.snapshot()).toEqual({
      items: [session("c", 30), session("b", 20), session("a", 10)],
      nextCursor: null,
      headLoaded: true,
      isLoading: false,
      hasMore: false
    });
  });

  test("deduplicates an overlapping older page without mutation semantics", () => {
    const cache = new AgentRemoteSessionPaginationCache(5);
    cache.commit(cache.beginHead(), {
      items: [session("b", 20)],
      nextCursor: "cursor-b"
    });
    cache.commit(cache.beginOlder()!, {
      items: [session("b", 999), session("a", 10)]
    });
    expect(cache.snapshot().items).toEqual([session("b", 20), session("a", 10)]);
  });

  test("rejects an over-window page atomically and settles loading", () => {
    const cache = new AgentRemoteSessionPaginationCache(2);
    cache.commit(cache.beginHead(), {
      items: [session("b", 20)],
      nextCursor: "cursor-b"
    });
    const before = cache.snapshot();
    const token = cache.beginOlder()!;
    expect(() =>
      cache.commit(token, {
        items: [session("a", 10), session("c", 5)],
        nextCursor: "cursor-c"
      })
    ).toThrow(AgentRemoteSessionWindowLimitError);
    expect(cache.snapshot()).toEqual({ ...before, isLoading: false });
  });

  test("clear makes an in-flight response stale and starts an empty lifecycle", () => {
    const cache = new AgentRemoteSessionPaginationCache();
    const token = cache.beginHead();
    cache.clear();
    expect(cache.commit(token, { items: [session("stale", 1)] })).toBe("stale");
    expect(cache.snapshot()).toEqual({
      items: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: false,
      hasMore: false
    });
  });

  test("does not begin an older page before a head or while another page is active", () => {
    const cache = new AgentRemoteSessionPaginationCache();
    expect(cache.beginOlder()).toBeNull();
    cache.commit(cache.beginHead(), { items: [], nextCursor: "cursor-a" });
    const older = cache.beginOlder();
    expect(older).not.toBeNull();
    expect(cache.beginOlder()).toBeNull();
    cache.fail(older!);
    expect(cache.snapshot().isLoading).toBe(false);
  });
});
