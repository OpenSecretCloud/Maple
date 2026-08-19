import { describe, expect, test } from "bun:test";

import {
  AgentHistoryPaginationCache,
  AgentHistoryProjectionLimitError,
  MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES,
  MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES,
  MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT,
  MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES,
  MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT,
  MAX_AGENT_LIVE_ITEMS_PER_SESSION,
  MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT,
  MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM,
  MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION,
  MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT,
  MAX_AGENT_RETIRED_EVENT_EPOCHS
} from "./agentHistoryPagination";
import type {
  AgentHistoryRecord,
  AgentSessionRecordsPage,
  AgentTimelineItem
} from "./agentRuntimeService";
import { MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES } from "./agentRuntimeService";
import type { AgentSynchronizedLiveSnapshot } from "./agentHistoryPagination";

function item(
  id: string,
  text: string,
  overrides: Partial<AgentTimelineItem> = {}
): AgentTimelineItem {
  return {
    id,
    itemType: "message",
    role: "assistant",
    text,
    createdMs: Number(id.replace(/\D/g, "")) || 0,
    merge: "replace",
    ...overrides
  };
}

function record(
  recordId: string,
  items: AgentTimelineItem[] = [item(`item-${recordId}`, recordId)]
): AgentHistoryRecord {
  return {
    recordId,
    role: "assistant",
    createdMs: Number(recordId.replace(/\D/g, "")) || 0,
    items
  };
}

function page(
  records: AgentHistoryRecord[],
  nextCursor: string | null,
  historyRevision = "history-1"
): AgentSessionRecordsPage {
  return { records, nextCursor, historyRevision };
}

function ownedCache(accountId = "user", targetId = "local"): AgentHistoryPaginationCache {
  return new AgentHistoryPaginationCache({ accountId, targetId });
}

function synchronizedSnapshot(
  liveSessions: readonly {
    sessionId: string;
    liveItems: readonly AgentTimelineItem[];
  }[],
  journalId: string,
  sequence: number
): AgentSynchronizedLiveSnapshot {
  return {
    liveSessionsComplete: true,
    liveSessionCount: liveSessions.length,
    liveSessions,
    throughEventCursor: { journalId, sequence }
  };
}

function largeProjectionPage(
  prefix: string,
  recordCount: number,
  text: string,
  nextCursor: string | null
): AgentSessionRecordsPage {
  return page(
    Array.from({ length: recordCount }, (_, index) =>
      record(`${prefix}-record-${index}`, [item(`${prefix}-item-${index}`, text)])
    ),
    nextCursor
  );
}

function repeatedReplacementPage(
  prefix: string,
  recordCount: number,
  text: string,
  nextCursor: string | null
): AgentSessionRecordsPage {
  return page(
    Array.from({ length: recordCount }, (_, index) =>
      record(`${prefix}-record-${index}`, [item(`${prefix}-shared`, text)])
    ),
    nextCursor
  );
}

function boundedLiveItems(prefix: string, itemCount = 13): AgentTimelineItem[] {
  const text = "l".repeat(160 * 1024);
  return Array.from({ length: itemCount }, (_, index) => item(`${prefix}-live-${index}`, text));
}

describe("AgentHistoryPaginationCache", () => {
  test("loads four record-count pages without splitting multi-item records", () => {
    const cache = ownedCache();
    const sessionId = "session";

    const head = cache.beginHead(sessionId);
    expect(
      cache.commit(
        head,
        page(
          [record("r8", [item("i8a", "thinking"), item("i8b", "answer")]), record("r7")],
          "after-r7"
        )
      )
    ).toBe("applied");

    for (const [newer, older, cursor] of [
      ["r6", "r5", "after-r5"],
      ["r4", "r3", "after-r3"],
      ["r2", "r1", null]
    ] as const) {
      const token = cache.beginOlder(sessionId);
      expect(token?.cursor).toBe(token ? token.cursor : null);
      expect(token).not.toBeNull();
      expect(cache.commit(token!, page([record(newer), record(older)], cursor))).toBe("applied");
    }

    const snapshot = cache.snapshot(sessionId);
    expect(snapshot.records.map((entry) => entry.recordId)).toEqual([
      "r1",
      "r2",
      "r3",
      "r4",
      "r5",
      "r6",
      "r7",
      "r8"
    ]);
    expect(snapshot.timeline.map((entry) => entry.id)).toEqual([
      "item-r1",
      "item-r2",
      "item-r3",
      "item-r4",
      "item-r5",
      "item-r6",
      "item-r7",
      "i8a",
      "i8b"
    ]);
    expect(snapshot.hasMore).toBe(false);
  });

  test("rejects repeated valid record appends before building an oversized projected string", () => {
    const cache = ownedCache();
    const sessionId = "append-window";
    const token = cache.beginHead(sessionId);
    const chunk = "x".repeat(Math.floor(MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES / 2) + 1024);
    const appendPage = page(
      [
        record("append-2", [item("shared", chunk, { merge: "append" })]),
        record("append-1", [item("shared", chunk, { merge: "append" })])
      ],
      "older"
    );

    expect(() => cache.commit(token, appendPage)).toThrow(AgentHistoryProjectionLimitError);
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: true
    });
    cache.fail(token);
  });

  test("rejects an aggregate page window atomically without advancing its cursor", () => {
    const cache = ownedCache();
    const sessionId = "page-window";
    const token = cache.beginHead(sessionId);
    const chunk = "p".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 9) + 1024);

    expect(() => cache.commit(token, largeProjectionPage("page", 9, chunk, "older"))).toThrow(
      AgentHistoryProjectionLimitError
    );
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: true
    });
    cache.fail(token);
  });

  test("charges every retained row even when replacement items coalesce", () => {
    const cache = ownedCache();
    const sessionId = "replacement-row-window";
    const token = cache.beginHead(sessionId);
    const chunk = "r".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 9) + 1024);
    const repeatedReplacementRows = page(
      Array.from({ length: 9 }, (_, index) =>
        record(`replacement-${index}`, [item("shared-replacement", chunk)])
      ),
      "older"
    );

    expect(() => cache.commit(token, repeatedReplacementRows)).toThrow(
      AgentHistoryProjectionLimitError
    );
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: true
    });
    cache.fail(token);
  });

  test("bounds the retained session projection across otherwise valid pages", () => {
    const cache = ownedCache();
    const sessionId = "session-window";
    const chunk = "s".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 10));
    cache.commit(cache.beginHead(sessionId), largeProjectionPage("head", 9, chunk, "older-1"));
    cache.commit(cache.beginOlder(sessionId)!, largeProjectionPage("older-1", 9, chunk, "older-2"));
    const before = cache.snapshot(sessionId);
    expect(before.records).toHaveLength(18);
    const overflow = cache.beginOlder(sessionId)!;

    expect(() => cache.commit(overflow, largeProjectionPage("older-2", 2, chunk, null))).toThrow(
      AgentHistoryProjectionLimitError
    );
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: before.records,
      nextCursor: "older-2",
      isLoading: true
    });
    cache.fail(overflow);
    expect(MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES).toBe(
      2 * MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES
    );
  });

  test("atomically rejects retained rows plus an existing live overlay over the session window", () => {
    const cache = ownedCache();
    const sessionId = "combined-session-window";
    for (const liveItem of boundedLiveItems("combined")) {
      expect(cache.mergeLiveItem(sessionId, liveItem)).toBe("applied");
    }
    const retainedText = "h".repeat(900 * 1024);
    cache.commit(
      cache.beginHead(sessionId),
      repeatedReplacementPage("combined-head", 8, retainedText, "older")
    );
    const before = cache.snapshot(sessionId);
    const overflow = cache.beginOlder(sessionId)!;

    expect(() =>
      cache.commit(overflow, repeatedReplacementPage("combined-older", 8, retainedText, null))
    ).toThrow(AgentHistoryProjectionLimitError);
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: before.records,
      timeline: before.timeline,
      nextCursor: "older",
      headLoaded: true,
      isLoading: true
    });
    cache.fail(overflow);
  });

  test("evicts only the oldest inactive session before enforcing the account window", () => {
    const cache = ownedCache();
    const chunk = "a".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 10));
    for (const sessionId of ["a", "b", "c", "d", "e"]) {
      cache.commit(cache.beginHead(sessionId), largeProjectionPage(sessionId, 9, chunk, null));
    }

    expect(cache.snapshot("a").records).toEqual([]);
    for (const sessionId of ["b", "c", "d", "e"]) {
      expect(cache.snapshot(sessionId).records).toHaveLength(9);
    }
    expect(MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES).toBe(
      4 * MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES
    );
  });

  test("rejects the account window when every retained session is protected", () => {
    const cache = ownedCache();
    const chunk = "z".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 10));
    const protectedIds = new Set<string>();
    for (const sessionId of ["protected-a", "protected-b", "protected-c", "protected-d"]) {
      cache.commit(cache.beginHead(sessionId), largeProjectionPage(sessionId, 9, chunk, null));
      protectedIds.add(sessionId);
      cache.reconcileRetention(protectedIds);
    }
    protectedIds.add("protected-e");
    cache.reconcileRetention(protectedIds);
    const overflow = cache.beginHead("protected-e");

    expect(() =>
      cache.commit(overflow, largeProjectionPage("protected-e", 9, chunk, null))
    ).toThrow(AgentHistoryProjectionLimitError);
    for (const sessionId of ["protected-a", "protected-b", "protected-c", "protected-d"]) {
      expect(cache.snapshot(sessionId).records).toHaveLength(9);
    }
    expect(cache.snapshot("protected-e").records).toEqual([]);
    cache.fail(overflow);
  });

  test("does not partially evict inactive sessions when account admission still fails", () => {
    const cache = ownedCache();
    const largeChunk = "q".repeat(Math.floor(MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES / 10));
    const protectedIds = new Set<string>();
    for (const sessionId of ["atomic-a", "atomic-b", "atomic-c", "atomic-d"]) {
      cache.commit(cache.beginHead(sessionId), largeProjectionPage(sessionId, 9, largeChunk, null));
      protectedIds.add(sessionId);
      cache.reconcileRetention(protectedIds);
    }
    cache.commit(
      cache.beginHead("atomic-evictable"),
      largeProjectionPage("atomic-evictable", 1, "e".repeat(512 * 1024), null)
    );
    const evictableBefore = cache.snapshot("atomic-evictable").records;
    const overflow = cache.beginHead("atomic-target");

    expect(() =>
      cache.commit(overflow, largeProjectionPage("atomic-target", 9, largeChunk, null))
    ).toThrow(AgentHistoryProjectionLimitError);
    expect(cache.snapshot("atomic-evictable").records).toEqual(evictableBefore);
    expect(cache.snapshot("atomic-target").records).toEqual([]);
    cache.fail(overflow);
  });

  test("bounds empty retained session states without evicting active requests", () => {
    const cache = ownedCache();
    const first = cache.beginHead("retained-0");
    cache.fail(first);
    for (let index = 1; index <= MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT; index += 1) {
      const token = cache.beginHead(`retained-${index}`);
      cache.fail(token);
    }

    expect(cache.commit(first, page([], null))).toBe("stale");
  });

  test("coalesces a tool request and response across record and page boundaries", () => {
    const cache = ownedCache();
    const sessionId = "session";
    const toolRequest = item("tool-1", "", {
      itemType: "tool",
      title: "Read file",
      status: "running",
      input: { path: "README.md" }
    });
    const toolResponse = item("tool-1", "done", {
      itemType: "tool",
      title: "Read file",
      status: "completed",
      output: "marker"
    });

    cache.commit(cache.beginHead(sessionId), page([record("response", [toolResponse])], "older"));
    cache.commit(cache.beginOlder(sessionId)!, page([record("request", [toolRequest])], null));

    expect(cache.snapshot(sessionId).timeline).toEqual([
      expect.objectContaining({
        id: "tool-1",
        status: "completed",
        input: { path: "README.md" },
        output: "marker",
        text: "done"
      })
    ]);
  });

  test("keeps a resolved live item authoritative over a persisted head refresh", () => {
    const cache = ownedCache();
    const sessionId = "session";

    cache.mergeLiveItem(
      sessionId,
      item("tool-1", "live result", {
        itemType: "tool",
        status: "completed",
        output: "live"
      })
    );
    cache.commit(
      cache.beginHead(sessionId),
      page(
        [
          record("r1", [
            item("tool-1", "persisted pending", {
              itemType: "tool",
              status: "running"
            })
          ])
        ],
        null
      )
    );

    expect(cache.snapshot(sessionId).timeline).toEqual([
      expect.objectContaining({
        id: "tool-1",
        status: "completed",
        output: "live",
        text: "live result"
      })
    ]);
  });

  test("rejects the first A load after A-B-A starts a newer A request", () => {
    const cache = ownedCache();
    const firstA = cache.beginHead("A");
    const onlyB = cache.beginHead("B");
    const secondA = cache.beginHead("A");

    expect(cache.commit(firstA, page([record("stale-a")], null))).toBe("stale");
    expect(cache.commit(onlyB, page([record("b")], null))).toBe("applied");
    expect(cache.commit(secondA, page([record("fresh-a")], null))).toBe("applied");
    expect(cache.snapshot("A").records.map((entry) => entry.recordId)).toEqual(["fresh-a"]);
  });

  test("settles only the matching remote-head token when an early return is superseded", () => {
    const cache = ownedCache();
    const obsolete = cache.beginHead("session");
    const replacement = cache.beginHead("session");

    cache.fail(obsolete);
    expect(cache.snapshot("session").isLoading).toBe(true);
    cache.fail(replacement);
    expect(cache.snapshot("session").isLoading).toBe(false);
  });

  test("invalidates an older page from a replaced history generation", () => {
    const cache = ownedCache();
    const sessionId = "session";
    cache.commit(cache.beginHead(sessionId), page([record("r2")], "older", "history-1"));
    const older = cache.beginOlder(sessionId)!;

    expect(cache.commit(older, page([record("r1")], null, "history-2"))).toBe("history-replaced");
    expect(cache.snapshot(sessionId)).toMatchObject({
      records: [],
      timeline: [],
      historyRevision: null,
      headLoaded: false,
      hasMore: false
    });
  });

  test("an explicit history replacement fences a late head and preserves the live suffix", () => {
    const cache = ownedCache();
    const sessionId = "session";
    cache.mergeLiveItem(sessionId, item("live", "obsolete"));
    const obsolete = cache.beginHead(sessionId);

    cache.invalidate(sessionId);

    expect(cache.commit(obsolete, page([record("obsolete")], null))).toBe("stale");
    expect(cache.snapshot(sessionId).timeline.map((entry) => entry.id)).toEqual(["live"]);
  });

  test("accumulates append deltas once when overlaying a persisted item", () => {
    const cache = ownedCache();
    const sessionId = "session";
    cache.commit(cache.beginHead(sessionId), page([record("r1", [item("answer", "base")])], null));
    cache.mergeLiveItem(sessionId, item("answer", " one", { merge: "append" }));
    cache.mergeLiveItem(sessionId, item("answer", " two", { merge: "append" }));

    expect(cache.snapshot(sessionId).timeline[0].text).toBe("base one two");
  });

  test("applies a delta received before a compatibility head exactly once", () => {
    const cache = ownedCache();
    const sessionId = "session";
    cache.mergeLiveItem(sessionId, item("answer", " delta", { merge: "append" }));

    cache.commit(cache.beginHead(sessionId), page([record("r1", [item("answer", "base")])], null));

    expect(cache.snapshot(sessionId).timeline[0].text).toBe("base delta");
  });

  test("fails closed when a later persisted base makes a retained delta exceed the item budget", () => {
    const cache = ownedCache();
    const halfBudget = "x".repeat(MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM / 2 + 1);
    expect(cache.mergeLiveItem("session", item("answer", halfBudget, { merge: "append" }))).toBe(
      "applied"
    );

    cache.commit(
      cache.beginHead("session"),
      page([record("r1", [item("answer", halfBudget)])], null)
    );

    expect(cache.snapshot("session")).toMatchObject({
      requiresSynchronizedReload: true,
      timeline: [{ id: "answer", text: halfBudget }]
    });
  });

  test("preserves legitimate repeated append deltas", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("answer", "ha", { merge: "append" }));
    cache.mergeLiveItem("session", item("answer", "ha", { merge: "append" }));

    expect(cache.snapshot("session").timeline[0].text).toBe("haha");
  });

  test("caches an ID-indexed long-history projection across a token stream", () => {
    const cache = ownedCache();
    const persisted = Array.from({ length: 1_500 }, (_, index) =>
      record(`history-${index}`, [item(`history-item-${index}`, `${index}`)])
    );
    cache.commit(cache.beginHead("session"), page(persisted, null));
    const initial = cache.snapshot("session").timeline;
    expect(cache.snapshot("session").timeline).toBe(initial);

    for (let index = 0; index < 256; index += 1) {
      expect(
        cache.mergeLiveItem("session", item("streaming-answer", "x", { merge: "append" }))
      ).toBe("applied");
      const projected = cache.snapshot("session").timeline;
      expect(cache.snapshot("session").timeline).toBe(projected);
    }

    const projected = cache.snapshot("session").timeline;
    expect(projected).toHaveLength(persisted.length + 1);
    expect(projected.at(-1)?.text).toBe("x".repeat(256));
  });

  test("fails closed before a cumulative multibyte append exceeds its item byte budget", () => {
    const cache = ownedCache();
    const chunk = "🪿".repeat(16_000);
    for (let index = 0; index < 3; index += 1) {
      expect(cache.mergeLiveItem("session", item("answer", chunk, { merge: "append" }))).toBe(
        "applied"
      );
    }
    const retainedText = cache.snapshot("session").timeline[0].text ?? "";

    expect(cache.mergeLiveItem("session", item("answer", chunk, { merge: "append" }))).toBe(
      "synchronized-reload-required"
    );
    expect(cache.snapshot("session").timeline[0].text).toBe(retainedText);
    expect(new TextEncoder().encode(retainedText).byteLength).toBeLessThanOrEqual(
      MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM
    );
    expect(cache.snapshot("session").requiresSynchronizedReload).toBe(true);
  });

  test("enforces cumulative live projection budgets per session and account", () => {
    const nearItemLimitText = "x".repeat(MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM - 1_024);
    const sessionCache = ownedCache();
    let sessionApplied = 0;
    while (
      sessionCache.mergeLiveItem(
        "session",
        item(`session-byte-${sessionApplied}`, nearItemLimitText)
      ) === "applied"
    ) {
      sessionApplied += 1;
    }
    expect(sessionApplied).toBeGreaterThan(40);
    expect(sessionCache.snapshot("session").requiresSynchronizedReload).toBe(true);
    expect(MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION).toBe(
      MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT
    );

    const accountCache = ownedCache();
    let accountApplied = 0;
    while (
      accountCache.mergeLiveItem(
        `account-session-${accountApplied % 2}`,
        item(`account-byte-${accountApplied}`, nearItemLimitText)
      ) === "applied"
    ) {
      accountApplied += 1;
    }
    expect(accountApplied).toBe(sessionApplied);
    expect(accountCache.snapshot("account-session-0").requiresSynchronizedReload).toBe(true);
  });

  test("rejects duplicate and out-of-order sequenced events without deduping compatibility events", () => {
    const cache = ownedCache();
    expect(cache.acceptEvent({})).toBe("accepted");
    expect(cache.acceptEvent({})).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 1 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 1 })).toBe("duplicate");
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 0 })).toBe("duplicate");
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 3 })).toBe("gap");
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 2 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "epoch-2", eventSequence: 1 })).toBe("gap");
  });

  test("tracks the account-wide journal watermark across interleaved sessions", () => {
    const cache = ownedCache();
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 1 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 2 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 3 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 2 })).toBe("duplicate");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 5 })).toBe("gap");
  });

  test("requires sequence one or a trusted checkpoint for a new journal epoch", () => {
    const cache = ownedCache();
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 4 })).toBe("gap");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 1 })).toBe("accepted");
    cache.installEventCheckpoint({ journalId: "replacement", sequence: 40 });
    expect(cache.acceptEvent({ eventEpoch: "replacement", eventSequence: 40 })).toBe("duplicate");
    expect(cache.acceptEvent({ eventEpoch: "replacement", eventSequence: 41 })).toBe("accepted");
  });

  test("history revision rebases committed records without dropping current live state", () => {
    const cache = ownedCache();
    cache.commit(cache.beginHead("session"), page([record("old")], null, "history-1"));
    cache.mergeLiveItem("session", item("live", "current"));

    cache.commit(cache.beginHead("session"), page([record("new")], null, "history-2"));

    expect(cache.snapshot("session").records.map((entry) => entry.recordId)).toEqual(["new"]);
    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "item-new",
      "live"
    ]);
    expect(cache.acceptEvent({ eventEpoch: "epoch-1", eventSequence: 1 })).toBe("accepted");
  });

  test("installs an authoritative live snapshot and atomic journal checkpoint", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("current-live", "current"));
    const head = cache.beginHead("session");
    cache.installSynchronizedAccountHead(
      head,
      page([record("persisted")], null),
      synchronizedSnapshot(
        [{ sessionId: "session", liveItems: [item("current-live", "authoritative")] }],
        "journal",
        12
      )
    );

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "item-persisted",
      "current-live"
    ]);
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 12 })).toBe("duplicate");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 13 })).toBe("accepted");
  });

  test("an authoritative terminal head clears stale live overlays", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("answer", "delta", { merge: "append" }));
    cache.installSynchronizedAccountHead(
      cache.beginHead("session"),
      page([record("terminal", [item("answer", "complete")])], null),
      synchronizedSnapshot([], "journal", 2)
    );

    expect(cache.snapshot("session").timeline).toEqual([
      expect.objectContaining({ id: "answer", text: "complete" })
    ]);
  });

  test("splices an authoritative live suffix at its shared user boundary", () => {
    const cache = ownedCache();
    cache.installSynchronizedAccountHead(
      cache.beginHead("session"),
      page(
        [
          record("current", [
            item("current-user", "Current turn", { role: "user" }),
            item("persisted-thought", "provider-history thought", { itemType: "thinking" })
          ]),
          record("prior", [
            item("prior-user", "Earlier turn", { role: "user" }),
            item("prior-answer", "Earlier answer")
          ])
        ],
        null
      ),
      synchronizedSnapshot(
        [
          {
            sessionId: "session",
            liveItems: [
              item("current-user", "Current turn", { role: "user" }),
              item("live-thought", "authoritative thought", { itemType: "thinking" })
            ]
          }
        ],
        "journal",
        4
      )
    );

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "prior-user",
      "prior-answer",
      "current-user",
      "live-thought"
    ]);
  });

  test("scans past unmatched live users to the first shared persisted boundary", () => {
    const cache = ownedCache();
    cache.installSynchronizedAccountHead(
      cache.beginHead("session"),
      page(
        [
          record("current", [
            item("shared-user", "Shared turn", { role: "user" }),
            item("stale-provider-thought", "stale", { itemType: "thinking" })
          ]),
          record("prior", [item("prior-answer", "Earlier answer")])
        ],
        null
      ),
      synchronizedSnapshot(
        [
          {
            sessionId: "session",
            liveItems: [
              item("unmatched-user", "Live-only turn", { role: "user" }),
              item("shared-user", "Shared turn", { role: "user" }),
              item("live-thought", "authoritative", { itemType: "thinking" })
            ]
          }
        ],
        "journal",
        5
      )
    );

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "prior-answer",
      "unmatched-user",
      "shared-user",
      "live-thought"
    ]);
  });

  test("a synchronized restart removes stale live-only deltas without guessing a boundary", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("stale-delta", "stale", { merge: "append" }));

    cache.installSynchronizedAccountHead(
      cache.beginHead("session"),
      page([record("persisted")], null),
      synchronizedSnapshot(
        [{ sessionId: "session", liveItems: [item("new-user", "New turn", { role: "user" })] }],
        "journal",
        6
      )
    );

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "item-persisted",
      "new-user"
    ]);
  });

  test("does not infer a journal checkpoint from an ordinary page commit", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("live", "compatibility live"));
    cache.commit(cache.beginHead("session"), page([record("persisted")], null));

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual([
      "item-persisted",
      "live"
    ]);
    expect(cache.acceptEvent({ eventEpoch: "untrusted", eventSequence: 21 })).toBe("gap");
  });

  test("resets all paged and sequenced state only when the account-target owner changes", () => {
    const cache = ownedCache("account-a", "target-a");
    cache.commit(cache.beginHead("session"), page([record("persisted")], null));
    expect(cache.acceptEvent({ eventEpoch: "journal-a", eventSequence: 1 })).toBe("accepted");

    expect(cache.bindOwner({ accountId: "account-a", targetId: "target-a" })).toBe("unchanged");
    expect(cache.snapshot("session").records).toHaveLength(1);
    expect(cache.acceptEvent({ eventEpoch: "journal-a", eventSequence: 2 })).toBe("accepted");

    expect(cache.bindOwner({ accountId: "account-a", targetId: "target-b" })).toBe("reset");
    expect(cache.snapshot("session").records).toEqual([]);
    expect(cache.acceptEvent({ eventEpoch: "journal-b", eventSequence: 1 })).toBe("accepted");
  });

  test("never reactivates a retired journal epoch", () => {
    const cache = ownedCache();
    expect(cache.acceptEvent({ eventEpoch: "journal-a", eventSequence: 1 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "journal-b", eventSequence: 1 })).toBe("gap");
    cache.installEventCheckpoint({ journalId: "journal-b", sequence: 0 });
    expect(cache.acceptEvent({ eventEpoch: "journal-b", eventSequence: 1 })).toBe("accepted");
    expect(cache.acceptEvent({ eventEpoch: "journal-a", eventSequence: 1 })).toBe("duplicate");
  });

  test("only a trusted checkpoint can rotate an established journal", () => {
    const cache = ownedCache();
    cache.installEventCheckpoint({ journalId: "journal-a", sequence: 10 });
    expect(cache.acceptEvent({ eventEpoch: "journal-b", eventSequence: 1 })).toBe("gap");
    cache.installEventCheckpoint({ journalId: "journal-b", sequence: 0 });
    expect(cache.acceptEvent({ eventEpoch: "journal-b", eventSequence: 1 })).toBe("accepted");

    expect(cache.bindOwner({ accountId: "other", targetId: "local" })).toBe("reset");
    expect(cache.acceptEvent({ eventEpoch: "journal-c", eventSequence: 1 })).toBe("accepted");
  });

  test("fences a synchronized head when an event arrives after the request begins", () => {
    const cache = ownedCache();
    const token = cache.beginHead("session");
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 1 })).toBe("accepted");

    expect(
      cache.installSynchronizedAccountHead(
        token,
        page([record("stale")], null),
        synchronizedSnapshot([], "journal", 0)
      )
    ).toBe("stale");
    expect(cache.snapshot("session").records).toEqual([]);
  });

  test("preflights a regressing checkpoint before changing records or live state", () => {
    const cache = ownedCache();
    cache.installEventCheckpoint({ journalId: "journal", sequence: 8 });
    cache.mergeLiveItem("session", item("live", "keep"));
    const token = cache.beginHead("session");

    expect(() =>
      cache.installSynchronizedAccountHead(
        token,
        page([record("must-not-commit")], null),
        synchronizedSnapshot([], "journal", 7)
      )
    ).toThrow("regress");
    expect(cache.snapshot("session").records).toEqual([]);
    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual(["live"]);
  });

  test("fences an old response across deletion and same-id reuse", () => {
    const cache = ownedCache();
    const deletedRequest = cache.beginHead("same-id");

    cache.remove("same-id");
    const replacementRequest = cache.beginHead("same-id");

    expect(cache.commit(deletedRequest, page([record("deleted-owner")], null))).toBe("stale");
    expect(cache.commit(replacementRequest, page([record("replacement")], null))).toBe("applied");
    expect(cache.snapshot("same-id").records.map((entry) => entry.recordId)).toEqual([
      "replacement"
    ]);
  });

  test("fences an old response across owner change and same-id reuse", () => {
    const cache = ownedCache("account-a", "target-a");
    const previousOwnerRequest = cache.beginHead("same-id");

    expect(cache.bindOwner({ accountId: "account-b", targetId: "target-b" })).toBe("reset");
    const nextOwnerRequest = cache.beginHead("same-id");

    expect(cache.commit(previousOwnerRequest, page([record("account-a")], null))).toBe("stale");
    expect(cache.commit(nextOwnerRequest, page([record("account-b")], null))).toBe("applied");
    expect(cache.snapshot("same-id").records.map((entry) => entry.recordId)).toEqual(["account-b"]);
  });

  test("a late fail or snapshot cannot recreate a removed session", () => {
    const cache = ownedCache();
    const request = cache.beginHead("removed");
    cache.remove("removed");

    cache.fail(request);
    expect(cache.snapshot("removed")).toMatchObject({
      records: [],
      timeline: [],
      headLoaded: false
    });

    const replacement = cache.beginHead("removed");
    expect(cache.commit(request, page([record("stale")], null))).toBe("stale");
    expect(cache.commit(replacement, page([record("fresh")], null))).toBe("applied");
  });

  test("bounds each live suffix and fails closed until a synchronized reload", () => {
    const cache = ownedCache();
    for (let index = 0; index < MAX_AGENT_LIVE_ITEMS_PER_SESSION; index += 1) {
      expect(cache.mergeLiveItem("session", item(`live-${index}`, `${index}`))).toBe("applied");
    }

    expect(cache.mergeLiveItem("session", item("overflow", "overflow"))).toBe(
      "synchronized-reload-required"
    );
    expect(cache.mergeLiveItem("session", item("live-0", "replacement"))).toBe(
      "synchronized-reload-required"
    );
    expect(cache.snapshot("session")).toMatchObject({
      requiresSynchronizedReload: true
    });
    expect(cache.snapshot("session").timeline).toHaveLength(MAX_AGENT_LIVE_ITEMS_PER_SESSION);

    cache.installSynchronizedAccountHead(
      cache.beginHead("session"),
      page([], null),
      synchronizedSnapshot([], "journal", 0)
    );
    expect(cache.snapshot("session")).toMatchObject({
      timeline: [],
      requiresSynchronizedReload: false
    });
  });

  test("bounds live sessions and account-wide live items", () => {
    const sessionBoundCache = ownedCache();
    for (let index = 0; index < MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT; index += 1) {
      expect(sessionBoundCache.mergeLiveItem(`session-${index}`, item(`item-${index}`, "x"))).toBe(
        "applied"
      );
    }
    expect(sessionBoundCache.mergeLiveItem("one-too-many", item("overflow", "x"))).toBe(
      "synchronized-reload-required"
    );

    const itemBoundCache = ownedCache();
    let remaining = MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT;
    let sessionIndex = 0;
    while (remaining > 0) {
      const itemCount = Math.min(remaining, MAX_AGENT_LIVE_ITEMS_PER_SESSION);
      for (let itemIndex = 0; itemIndex < itemCount; itemIndex += 1) {
        expect(
          itemBoundCache.mergeLiveItem(
            `account-session-${sessionIndex}`,
            item(`account-${sessionIndex}-${itemIndex}`, "x")
          )
        ).toBe("applied");
      }
      remaining -= itemCount;
      sessionIndex += 1;
    }
    expect(
      itemBoundCache.mergeLiveItem(
        `account-session-${sessionIndex - 1}`,
        item("account-overflow", "x")
      )
    ).toBe("synchronized-reload-required");
  });

  test("counts a pre-created empty state when it becomes the sixty-fifth live session", () => {
    const cache = ownedCache();
    const precreated = cache.beginHead("precreated-empty");
    cache.fail(precreated);
    for (let index = 0; index < MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT; index += 1) {
      expect(cache.mergeLiveItem(`live-${index}`, item(`live-item-${index}`, "x"))).toBe("applied");
    }

    expect(cache.mergeLiveItem("precreated-empty", item("late-live", "x"))).toBe(
      "synchronized-reload-required"
    );
    expect(cache.snapshot("precreated-empty")).toMatchObject({
      timeline: [],
      requiresSynchronizedReload: true
    });
  });

  test("bulk live seeding preflights capacity without applying a prefix", () => {
    const cache = ownedCache();
    const oversized = Array.from({ length: MAX_AGENT_LIVE_ITEMS_PER_SESSION + 1 }, (_, index) =>
      item(`bulk-${index}`, "x")
    );

    expect(cache.seedLiveTimeline("session", oversized)).toBe("synchronized-reload-required");
    expect(cache.snapshot("session").timeline).toEqual([]);
  });

  test("a synchronized head preflights live bounds without mutating records or checkpoint", () => {
    const cache = ownedCache();
    cache.commit(cache.beginHead("session"), page([record("existing")], null));
    const token = cache.beginHead("session");
    const oversized = Array.from({ length: MAX_AGENT_LIVE_ITEMS_PER_SESSION + 1 }, (_, index) =>
      item(`sync-${index}`, "x")
    );

    expect(() =>
      cache.installSynchronizedAccountHead(
        token,
        page([record("must-not-commit")], null),
        synchronizedSnapshot([{ sessionId: "session", liveItems: oversized }], "journal", 9)
      )
    ).toThrow("session limit");
    expect(cache.snapshot("session").records.map((entry) => entry.recordId)).toEqual(["existing"]);
    expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 10 })).toBe("gap");
  });

  test("a synchronized head rejects retained rows plus prospective live bytes before mutation", () => {
    const cache = ownedCache();
    const retainedText = "s".repeat(900 * 1024);
    cache.commit(
      cache.beginHead("retained-live"),
      repeatedReplacementPage("sync-retained-head", 8, retainedText, "older")
    );
    cache.commit(
      cache.beginOlder("retained-live")!,
      repeatedReplacementPage("sync-retained-older", 8, retainedText, null)
    );
    const retainedBefore = cache.snapshot("retained-live");
    const target = cache.beginHead("sync-target");

    expect(() =>
      cache.installSynchronizedAccountHead(
        target,
        page([record("must-not-commit")], "must-not-advance"),
        synchronizedSnapshot(
          [{ sessionId: "retained-live", liveItems: boundedLiveItems("prospective") }],
          "journal",
          9
        )
      )
    ).toThrow(AgentHistoryProjectionLimitError);
    expect(cache.snapshot("retained-live")).toMatchObject({
      records: retainedBefore.records,
      timeline: retainedBefore.timeline
    });
    expect(cache.snapshot("sync-target")).toMatchObject({
      records: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: true
    });
    expect(cache.eventCursor()).toBeNull();
    cache.fail(target);
  });

  test("a synchronized state-cap failure leaves target, live overlay, and checkpoint untouched", () => {
    const cache = ownedCache();
    expect(cache.mergeLiveItem("preserved-live", item("preserved", "keep"))).toBe("applied");
    cache.commit(
      cache.beginHead("eligible-but-insufficient"),
      page([record("preserved-row")], null)
    );
    let target: ReturnType<AgentHistoryPaginationCache["beginHead"]> | null = null;
    for (let index = 0; index < MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT - 2; index += 1) {
      const token = cache.beginHead(`busy-${index}`);
      if (index === 0) target = token;
    }
    expect(target).not.toBeNull();

    expect(() =>
      cache.installSynchronizedAccountHead(
        target!,
        page([record("must-not-commit")], "must-not-advance"),
        synchronizedSnapshot(
          [
            { sessionId: "missing-live-a", liveItems: [item("missing-item-a", "new-a")] },
            { sessionId: "missing-live-b", liveItems: [item("missing-item-b", "new-b")] }
          ],
          "journal",
          4
        )
      )
    ).toThrow(AgentHistoryProjectionLimitError);
    expect(cache.snapshot("busy-0")).toMatchObject({
      records: [],
      nextCursor: null,
      headLoaded: false,
      isLoading: true
    });
    expect(cache.snapshot("preserved-live").timeline).toMatchObject([
      { id: "preserved", text: "keep" }
    ]);
    expect(cache.snapshot("eligible-but-insufficient").records).toMatchObject([
      { recordId: "preserved-row" }
    ]);
    expect(cache.snapshot("missing-live-a").timeline).toEqual([]);
    expect(cache.snapshot("missing-live-b").timeline).toEqual([]);
    expect(cache.eventCursor()).toBeNull();
  });

  test("a synchronized state reservation evicts an eligible state before installing live data", () => {
    const cache = ownedCache();
    cache.commit(cache.beginHead("evictable"), page([record("old")], null));
    let target: ReturnType<AgentHistoryPaginationCache["beginHead"]> | null = null;
    for (let index = 0; index < MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT - 1; index += 1) {
      const token = cache.beginHead(`reserved-${index}`);
      if (index === 0) target = token;
    }

    expect(
      cache.installSynchronizedAccountHead(
        target!,
        page([record("installed")], null),
        synchronizedSnapshot(
          [{ sessionId: "reserved-live", liveItems: [item("reserved-item", "live")] }],
          "journal",
          5
        )
      )
    ).toBe("applied");
    expect(cache.snapshot("evictable")).toMatchObject({ records: [], headLoaded: false });
    expect(cache.snapshot("reserved-0").records.map((entry) => entry.recordId)).toEqual([
      "installed"
    ]);
    expect(cache.snapshot("reserved-live").timeline).toMatchObject([
      { id: "reserved-item", text: "live" }
    ]);
    expect(cache.eventCursor()).toEqual({ journalId: "journal", sequence: 5 });
  });

  test("releases all inactive persisted scrollback while preserving protected and live state", () => {
    const cache = ownedCache();
    cache.commit(cache.beginHead("inactive"), page([record("head")], "older"));
    cache.commit(cache.beginOlder("inactive")!, page([record("old")], null));
    cache.commit(cache.beginHead("protected"), page([record("protected")], null));
    cache.mergeLiveItem("live-inactive", item("live", "keep"));
    cache.commit(cache.beginHead("live-inactive"), page([record("live-persisted")], null));

    const released = cache.reconcileRetention(new Set(["protected"]));

    expect([...released].sort()).toEqual(["inactive", "live-inactive"]);
    expect(cache.snapshot("protected").records).toHaveLength(1);
    expect(cache.snapshot("live-inactive").timeline.map((entry) => entry.id)).toEqual(["live"]);
    expect(cache.snapshot("inactive").records).toEqual([]);
    expect(cache.snapshot("inactive").headLoaded).toBe(false);
  });

  test("deleting one session does not cancel another session page", () => {
    const cache = ownedCache();
    const deleted = cache.beginHead("deleted");
    const retained = cache.beginHead("retained");

    cache.remove("deleted");

    expect(cache.commit(deleted, page([record("deleted")], null))).toBe("stale");
    expect(cache.commit(retained, page([record("retained")], null))).toBe("applied");
  });

  test("bounds retired journal epochs and treats ancient journals as a gap", () => {
    const cache = ownedCache();
    cache.installEventCheckpoint({ journalId: "journal-0", sequence: 0 });
    for (let index = 1; index <= MAX_AGENT_RETIRED_EVENT_EPOCHS + 2; index += 1) {
      cache.installEventCheckpoint({ journalId: `journal-${index}`, sequence: 0 });
    }

    expect(cache.acceptEvent({ eventEpoch: "journal-0", eventSequence: 1 })).toBe("gap");
    expect(
      cache.acceptEvent({
        eventEpoch: `journal-${MAX_AGENT_RETIRED_EVENT_EPOCHS + 1}`,
        eventSequence: 1
      })
    ).toBe("duplicate");
  });

  test("a new run explicitly replaces the bounded compatibility live suffix", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("session", item("old-run", "old"));

    cache.startLiveSuffix("session");
    cache.mergeLiveItem("session", item("new-run", "new"));

    expect(cache.snapshot("session").timeline.map((entry) => entry.id)).toEqual(["new-run"]);
  });

  test("a run start cannot clear a synchronized-reload requirement", () => {
    const cache = ownedCache();
    for (let index = 0; index < MAX_AGENT_LIVE_ITEMS_PER_SESSION; index += 1) {
      cache.mergeLiveItem("session", item(`old-${index}`, "x"));
    }
    expect(cache.mergeLiveItem("session", item("overflow", "x"))).toBe(
      "synchronized-reload-required"
    );

    cache.startLiveSuffix("session");

    expect(cache.mergeLiveItem("session", item("new-run", "x"))).toBe(
      "synchronized-reload-required"
    );
    expect(cache.snapshot("session").requiresSynchronizedReload).toBe(true);
  });

  test("an explicit closed-stream clear replaces the live overlay without clearing account poison", () => {
    const cache = ownedCache();
    expect(cache.mergeLiveItem("session", item("live", "transient"))).toBe("applied");
    cache.requireSynchronizedReload();

    cache.clearLiveTimeline("session");

    const snapshot = cache.snapshot("session");
    expect(snapshot.timeline).toEqual([]);
    expect(snapshot.requiresSynchronizedReload).toBe(true);
    expect(cache.mergeLiveItem("session", item("later", "must-recover"))).toBe(
      "synchronized-reload-required"
    );
  });

  test("an untracked live-session overflow stays fail-closed after capacity frees", () => {
    const cache = ownedCache();
    for (let index = 0; index < MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT; index += 1) {
      cache.mergeLiveItem(`session-${index}`, item(`live-${index}`, "x"));
    }
    expect(cache.mergeLiveItem("overflow-session", item("lost", "x"))).toBe(
      "synchronized-reload-required"
    );

    cache.remove("session-0");

    expect(cache.mergeLiveItem("overflow-session", item("later", "x"))).toBe(
      "synchronized-reload-required"
    );
  });

  test("an incomplete B snapshot cannot clear A overflow but one complete C0 snapshot can", () => {
    const cache = ownedCache();
    cache.mergeLiveItem("B", item("b-live", "B"));
    for (let index = 0; index < MAX_AGENT_LIVE_ITEMS_PER_SESSION; index += 1) {
      cache.mergeLiveItem("A", item(`a-${index}`, "A"));
    }
    expect(cache.mergeLiveItem("A", item("a-overflow", "lost"))).toBe(
      "synchronized-reload-required"
    );
    cache.commit(cache.beginHead("B"), page([record("old-b")], null));
    const token = cache.beginHead("B");

    expect(() =>
      cache.installSynchronizedAccountHead(token, page([record("new-b")], null), {
        ...synchronizedSnapshot(
          [{ sessionId: "B", liveItems: [item("b-live", "B authoritative")] }],
          "journal",
          20
        ),
        liveSessionsComplete: false
      } as unknown as AgentSynchronizedLiveSnapshot)
    ).toThrow("complete");
    expect(cache.snapshot("A").requiresSynchronizedReload).toBe(true);
    expect(cache.snapshot("B").records.map((entry) => entry.recordId)).toEqual(["old-b"]);

    expect(
      cache.installSynchronizedAccountHead(
        token,
        page([record("new-b")], null),
        synchronizedSnapshot(
          [
            { sessionId: "A", liveItems: [item("a-authoritative", "A authoritative")] },
            { sessionId: "B", liveItems: [item("b-live", "B authoritative")] }
          ],
          "journal",
          20
        )
      )
    ).toBe("applied");
    expect(cache.snapshot("A")).toMatchObject({ requiresSynchronizedReload: false });
    expect(cache.snapshot("A").timeline.map((entry) => entry.id)).toEqual(["a-authoritative"]);
    expect(cache.snapshot("B").records.map((entry) => entry.recordId)).toEqual(["old-b", "new-b"]);
  });

  test("keeps identical timeline item IDs isolated by session", () => {
    const cache = ownedCache();
    const token = cache.beginHead("A");

    expect(
      cache.installSynchronizedAccountHead(
        token,
        page([], null),
        synchronizedSnapshot(
          [
            { sessionId: "A", liveItems: [item("shared", "A")] },
            { sessionId: "B", liveItems: [item("shared", "B")] }
          ],
          "journal",
          20
        )
      )
    ).toBe("applied");
    expect(cache.snapshot("A").timeline).toMatchObject([{ id: "shared", text: "A" }]);
    expect(cache.snapshot("B").timeline).toMatchObject([{ id: "shared", text: "B" }]);

    expect(cache.mergeLiveItem("B", item("shared", "!", { merge: "append" }))).toBe("applied");
    expect(cache.snapshot("A").timeline).toMatchObject([{ id: "shared", text: "A" }]);
    expect(cache.snapshot("B").timeline).toMatchObject([{ id: "shared", text: "B!" }]);
  });

  test("validates synchronized session order by canonical UTF-8 bytes", () => {
    const utf8Earlier = "\ue000";
    const utf8Later = "😀";
    const accepted = ownedCache();
    expect(
      accepted.installSynchronizedAccountHead(
        accepted.beginHead("head"),
        page([], null),
        synchronizedSnapshot(
          [
            { sessionId: utf8Earlier, liveItems: [] },
            { sessionId: utf8Later, liveItems: [] }
          ],
          "journal",
          1
        )
      )
    ).toBe("applied");

    const rejected = ownedCache();
    expect(() =>
      rejected.installSynchronizedAccountHead(
        rejected.beginHead("head"),
        page([], null),
        synchronizedSnapshot(
          [
            { sessionId: utf8Later, liveItems: [] },
            { sessionId: utf8Earlier, liveItems: [] }
          ],
          "journal",
          1
        )
      )
    ).toThrow("unique sorted IDs");
  });

  test("rejects count, ordering, duplicate, and delta snapshot violations before mutation", () => {
    const invalidSnapshots: AgentSynchronizedLiveSnapshot[] = [
      {
        ...synchronizedSnapshot([], "journal", 3),
        liveSessionCount: 1
      },
      synchronizedSnapshot(
        [
          { sessionId: "B", liveItems: [] },
          { sessionId: "A", liveItems: [] }
        ],
        "journal",
        3
      ),
      synchronizedSnapshot(
        [
          {
            sessionId: "A",
            liveItems: [item("duplicate", "one"), item("duplicate", "two")]
          }
        ],
        "journal",
        3
      ),
      synchronizedSnapshot(
        [
          {
            sessionId: "A",
            liveItems: [item("delta", "chunk", { merge: "append" })]
          }
        ],
        "journal",
        3
      )
    ];

    for (const snapshot of invalidSnapshots) {
      const cache = ownedCache();
      cache.mergeLiveItem("A", item("keep-live", "keep"));
      cache.commit(cache.beginHead("A"), page([record("keep-record")], null));
      const token = cache.beginHead("A");

      expect(() =>
        cache.installSynchronizedAccountHead(
          token,
          page([record("must-not-commit")], null),
          snapshot
        )
      ).toThrow();
      expect(cache.snapshot("A").records.map((entry) => entry.recordId)).toEqual(["keep-record"]);
      expect(cache.snapshot("A").timeline.map((entry) => entry.id)).toEqual([
        "item-keep-record",
        "keep-live"
      ]);
      expect(cache.acceptEvent({ eventEpoch: "journal", eventSequence: 4 })).toBe("gap");
    }
  });
});
