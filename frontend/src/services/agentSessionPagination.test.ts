import { describe, expect, test } from "bun:test";

import { AgentSessionPaginationCache } from "./agentSessionPagination";
import type { AgentSessionSummary } from "./agentRuntimeService";

function session(
  id: string,
  title = id,
  updatedMs = Number(id.replace(/\D/g, "")) || 1,
  pageSortMs = updatedMs
): AgentSessionSummary {
  return {
    id,
    title,
    projectRoot: "/project",
    createdMs: 1,
    updatedMs,
    pageSortMs,
    messageCount: 1,
    model: "model",
    mode: "smart_approve"
  };
}

describe("AgentSessionPaginationCache", () => {
  test("refreshes the head without discarding loaded older task pages", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), { items: [session("s4"), session("s3")], nextCursor: "s3" });
    cache.commit(cache.beginOlder()!, { items: [session("s2"), session("s1")], nextCursor: null });
    cache.commit(cache.beginHead(), { items: [session("s5"), session("s4")], nextCursor: "s4" });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s5", "s4", "s3", "s2", "s1"]);
    expect(cache.snapshot().hasMore).toBe(false);
  });

  test("treats a complete head refresh as authoritative for unchanged tasks", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s2"), session("s1")],
      nextCursor: null
    });

    cache.commit(cache.beginHead(), { items: [session("s2")], nextCursor: null });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s2"]);
    expect(cache.snapshot().hasMore).toBe(false);
  });

  test("keeps an absent task mutated while an authoritative head refresh was in flight", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s2", "Two", 20), session("s1", "One", 10)],
      nextCursor: null
    });
    const token = cache.beginHead();
    cache.upsert(session("s1", "Live title", 30));

    cache.commit(token, { items: [session("s2", "Two", 20)], nextCursor: null });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s1", "s2"]);
    expect(cache.snapshot().items[0].title).toBe("Live title");
  });

  test("drops previously loaded older tasks when a complete head supersedes them", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s4"), session("s3")],
      nextCursor: "after-s3"
    });
    cache.commit(cache.beginOlder()!, {
      items: [session("s2"), session("s1")],
      nextCursor: null
    });

    cache.commit(cache.beginHead(), { items: [session("s5")], nextCursor: null });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s5"]);
    expect(cache.snapshot().nextCursor).toBeNull();
  });

  test("restarts older paging from the refreshed head when no loaded task overlaps", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s4"), session("s3")],
      nextCursor: "after-s3"
    });
    cache.commit(cache.beginOlder()!, {
      items: [session("s2"), session("s1")],
      nextCursor: null
    });

    cache.commit(cache.beginHead(), {
      items: [session("s8"), session("s7")],
      nextCursor: "after-s7"
    });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual([
      "s8",
      "s7",
      "s4",
      "s3",
      "s2",
      "s1"
    ]);
    expect(cache.snapshot().nextCursor).toBe("after-s7");
    expect(cache.beginOlder()?.cursor).toBe("after-s7");
  });

  test("does not mistake an event-only task for overlap with the loaded page range", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s4"), session("s3")],
      nextCursor: "after-s3"
    });
    cache.commit(cache.beginOlder()!, {
      items: [session("s2"), session("s1")],
      nextCursor: null
    });
    cache.upsert(session("s8", "Eight", 80));

    cache.commit(cache.beginHead(), {
      items: [session("s9", "Nine", 90), session("s8", "Eight", 80)],
      nextCursor: "after-s8"
    });

    expect(cache.snapshot().nextCursor).toBe("after-s8");
    expect(cache.beginOlder()?.cursor).toBe("after-s8");
  });

  test("keeps a reactive summary that raced a head page", () => {
    const cache = new AgentSessionPaginationCache();
    const token = cache.beginHead();
    cache.upsert(session("s1", "Live title"));

    cache.commit(token, { items: [session("s1", "Stale title")], nextCursor: null });

    expect(cache.snapshot().items[0].title).toBe("Live title");
  });

  test("rejects a superseded head request", () => {
    const cache = new AgentSessionPaginationCache();
    const first = cache.beginHead();
    const second = cache.beginHead();

    expect(cache.commit(first, { items: [session("stale")], nextCursor: null })).toBe("stale");
    expect(cache.commit(second, { items: [session("fresh")], nextCursor: null })).toBe("applied");
    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["fresh"]);
  });

  test("does not revive a task deleted while a page was in flight", () => {
    const cache = new AgentSessionPaginationCache();
    cache.upsert(session("s1"));
    const token = cache.beginHead();
    cache.remove("s1");

    cache.commit(token, { items: [session("s1")], nextCursor: null });

    expect(cache.snapshot().items).toEqual([]);
  });

  test("moves an event-updated task to newest-first position", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s2", "Two", 20), session("s1", "One", 10)],
      nextCursor: null
    });

    cache.upsert(session("s1", "One updated", 11, 30));

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s1", "s2"]);
  });

  test("uses descending task id as a deterministic equal-page-key tie break", () => {
    const cache = new AgentSessionPaginationCache();

    cache.commit(cache.beginHead(), {
      items: [
        session("task-a", "A", 300, 20),
        session("task-c", "C", 100, 20),
        session("task-b", "B", 200, 20)
      ],
      nextCursor: null
    });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["task-c", "task-b", "task-a"]);
  });

  test("keeps an updated older task stable when it moves between page requests", () => {
    const cache = new AgentSessionPaginationCache();
    cache.commit(cache.beginHead(), {
      items: [session("s4", "Four", 40, 40), session("s3", "Three", 30, 30)],
      nextCursor: "after-s3"
    });
    cache.upsert(session("s1", "One moved", 11, 50));

    cache.commit(cache.beginOlder()!, {
      items: [session("s2", "Two", 20, 20), session("s1", "One stale", 10, 10)],
      nextCursor: null
    });

    expect(cache.snapshot().items.map((item) => item.id)).toEqual(["s1", "s4", "s3", "s2"]);
    expect(cache.snapshot().items[0]).toMatchObject({
      title: "One moved",
      updatedMs: 11,
      pageSortMs: 50
    });
  });
});
