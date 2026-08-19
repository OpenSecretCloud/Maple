import { describe, expect, mock, test } from "bun:test";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import {
  createAgentRemoteReadOnlyClient,
  decodeAgentRemoteRecordsPage,
  decodeAgentRemoteRuntimeSummary,
  decodeAgentRemoteSessionPage,
  isClosedAgentRemoteReadOnlyClient,
  type AgentRemoteHistoryRecord,
  type AgentRemotePageRequest,
  type AgentRemoteRecordsPage,
  type AgentRemoteReadOnlySource,
  type AgentRemoteRecordsPageRequest
} from "@/services/agentRemoteProviderBridge";

const ACCOUNT_ID = "account-a";
const TARGET_ID = `target_${"a".repeat(48)}`;
const MAX_REMOTE_HISTORY_RECORD_JSON_BYTES = 1_040_384;
const MAX_REMOTE_TIMELINE_TEXT_BYTES = 192 * 1_024;

function sessionSummary(id = "session-a") {
  return {
    id,
    title: "Paired task",
    createdMs: 1,
    updatedMs: 2,
    pageSortMs: 2,
    messageCount: 1
  };
}

function recordsPage(): AgentRemoteRecordsPage {
  return {
    records: [
      {
        recordId: "record-a",
        role: "assistant",
        createdMs: 3,
        items: [
          {
            id: "message-a",
            itemType: "message",
            role: "assistant",
            text: "Safe persisted text",
            createdMs: 3,
            merge: "replace"
          }
        ]
      }
    ],
    nextCursor: "history-cursor-b",
    historyRevision: "history-revision-a"
  };
}

function historyRecordWithExactJsonBytes(targetBytes: number): AgentRemoteHistoryRecord {
  const items = Array.from({ length: 6 }, (_, index) => ({
    id: `boundary-message-${index}`,
    itemType: "message" as const,
    role: "assistant" as const,
    text: "",
    createdMs: index,
    merge: "replace" as const
  }));
  const record = {
    recordId: "record-boundary",
    role: "assistant",
    createdMs: 0,
    items
  };
  let remaining = targetBytes - jsonByteLength(record);
  if (remaining < 0) throw new Error("history record boundary fixture is too small");
  for (const item of items) {
    const itemBytes = Math.min(remaining, MAX_REMOTE_TIMELINE_TEXT_BYTES);
    item.text = "x".repeat(itemBytes);
    remaining -= itemBytes;
  }
  if (remaining !== 0 || jsonByteLength(record) !== targetBytes) {
    throw new Error("history record boundary fixture cannot reach the requested byte count");
  }
  return record;
}

function jsonByteLength(value: unknown): number {
  return new TextEncoder().encode(JSON.stringify(value)).byteLength;
}

function source(overrides: Partial<AgentRemoteReadOnlySource> = {}): AgentRemoteReadOnlySource {
  return {
    getRuntimeStatus: async () => ({ running: true, activeRunCount: 1 }),
    listSessionSummariesPage: async () => ({
      items: [sessionSummary()],
      nextCursor: "session-cursor-b"
    }),
    listPersistedRecordsPage: async () => recordsPage(),
    ...overrides
  };
}

function client(remoteSource: AgentRemoteReadOnlySource = source()) {
  return createAgentRemoteReadOnlyClient({
    accountId: ACCOUNT_ID,
    targetId: TARGET_ID,
    targetLabel: "Office Mac",
    source: remoteSource,
    capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES
  });
}

describe("Agent remote read-only provider bridge", () => {
  test("exposes only the three authenticated read methods and sanitized binding", () => {
    const readOnlyClient = client();
    expect(Object.keys(readOnlyClient).sort()).toEqual(
      [
        "binding",
        "capabilities",
        "getRuntimeStatus",
        "listPersistedRecordsPage",
        "listSessionSummariesPage"
      ].sort()
    );
    expect(readOnlyClient.binding).toEqual({
      accountId: ACCOUNT_ID,
      targetId: TARGET_ID,
      targetLabel: "Office Mac"
    });
    expect("invoke" in readOnlyClient).toBe(false);
    expect("sendMessage" in readOnlyClient).toBe(false);
    expect("startRuntime" in readOnlyClient).toBe(false);
    expect("subscribeToEvents" in readOnlyClient).toBe(false);
    expect(isClosedAgentRemoteReadOnlyClient(readOnlyClient)).toBe(true);
  });

  test("forwards only bounded requests and preserves opaque cursors", async () => {
    const sessionCalls: AgentRemotePageRequest[] = [];
    const recordCalls: AgentRemoteRecordsPageRequest[] = [];
    const readOnlyClient = client(
      source({
        listSessionSummariesPage: async (request) => {
          sessionCalls.push(request);
          return { items: [sessionSummary()], nextCursor: "session-cursor-b" };
        },
        listPersistedRecordsPage: async (request) => {
          recordCalls.push(request);
          return recordsPage();
        }
      })
    );

    expect(await readOnlyClient.getRuntimeStatus()).toEqual({
      running: true,
      activeRunCount: 1
    });
    expect(
      await readOnlyClient.listSessionSummariesPage({ cursor: "session-cursor-a", limit: 7 })
    ).toEqual({ items: [sessionSummary()], nextCursor: "session-cursor-b" });
    expect(
      await readOnlyClient.listPersistedRecordsPage({
        sessionId: "session-a",
        cursor: "history-cursor-a",
        limit: 9
      })
    ).toEqual(recordsPage());
    expect(sessionCalls).toEqual([{ cursor: "session-cursor-a", limit: 7 }]);
    expect(recordCalls).toEqual([{ sessionId: "session-a", cursor: "history-cursor-a", limit: 9 }]);
  });

  test("rejects invalid bindings and incomplete or widened capability grants", () => {
    expect(() =>
      createAgentRemoteReadOnlyClient({
        accountId: ACCOUNT_ID,
        targetId: "local",
        source: source(),
        capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES
      })
    ).toThrow("remote target binding");
    expect(() =>
      createAgentRemoteReadOnlyClient({
        accountId: ACCOUNT_ID,
        targetId: TARGET_ID,
        targetLabel: "Trusted\u202ehost",
        source: source(),
        capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES
      })
    ).toThrow("target label is invalid");
    expect(() =>
      createAgentRemoteReadOnlyClient({
        accountId: ACCOUNT_ID,
        targetId: TARGET_ID,
        source: source(),
        capabilities: {
          ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
          persistedRecordsPage: false
        }
      })
    ).toThrow("unavailable or too broad");
    expect(() =>
      createAgentRemoteReadOnlyClient({
        accountId: ACCOUNT_ID,
        targetId: TARGET_ID,
        source: source(),
        capabilities: {
          ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
          mutations: true
        }
      })
    ).toThrow("unavailable or too broad");
  });

  test("rejects hidden request extensions before calling the source", async () => {
    const list = mock(async () => ({ items: [] }));
    const readOnlyClient = client(source({ listSessionSummariesPage: list }));
    await expect(
      readOnlyClient.listSessionSummariesPage({
        cursor: null,
        limit: 5,
        projectRoot: "/must-not-cross"
      } as AgentRemotePageRequest)
    ).rejects.toThrow("request is invalid");
    expect(list).not.toHaveBeenCalled();
  });

  test("enforces closed runtime status and its consistency bound", async () => {
    expect(decodeAgentRemoteRuntimeSummary({ running: false, activeRunCount: 0 })).toEqual({
      running: false,
      activeRunCount: 0
    });
    expect(decodeAgentRemoteRuntimeSummary({ running: false, activeRunCount: 1 })).toBeNull();
    expect(decodeAgentRemoteRuntimeSummary({ running: true, activeRunCount: 65 })).toBeNull();
    expect(
      decodeAgentRemoteRuntimeSummary({
        running: true,
        activeRunCount: 1,
        activeRuns: { secret: "run" }
      })
    ).toBeNull();
    await expect(
      client(
        source({ getRuntimeStatus: async () => ({ running: true, activeRunCount: 65 }) })
      ).getRuntimeStatus()
    ).rejects.toThrow("response is invalid");
  });

  test("admits only the six-field session summary and rejects duplicates", () => {
    const request = { cursor: null, limit: 5 };
    const decoded = decodeAgentRemoteSessionPage({ items: [sessionSummary()] }, request);
    expect(decoded).toEqual({ items: [sessionSummary()] });
    expect("projectRoot" in decoded!.items[0]).toBe(false);
    expect(
      decodeAgentRemoteSessionPage(
        { items: [{ ...sessionSummary(), projectRoot: "/remote/private" }] },
        request
      )
    ).toBeNull();
    expect(
      decodeAgentRemoteSessionPage({ items: [sessionSummary(), sessionSummary()] }, request)
    ).toBeNull();
  });

  test("rejects raw tool payloads and unknown record extensions", () => {
    const request = { sessionId: "session-a", cursor: null, limit: 5 };
    const unsafeInput = recordsPage();
    unsafeInput.records[0].items[0] = {
      ...unsafeInput.records[0].items[0],
      input: { token: "secret" }
    } as (typeof unsafeInput.records)[number]["items"][number];
    expect(decodeAgentRemoteRecordsPage(unsafeInput, request)).toBeNull();
    expect(
      decodeAgentRemoteRecordsPage({ ...recordsPage(), providerPrivateField: "secret" }, request)
    ).toBeNull();
  });

  test("rejects duplicate history record IDs directly", () => {
    const page = recordsPage();
    expect(
      decodeAgentRemoteRecordsPage(
        { ...page, records: [page.records[0], { ...page.records[0] }] },
        { sessionId: "session-a", cursor: null, limit: 5 }
      )
    ).toBeNull();
  });

  test("admits exactly N JSON bytes per record and rejects N plus one", () => {
    const request = { sessionId: "session-a", cursor: null, limit: 5 };
    const atLimit = historyRecordWithExactJsonBytes(MAX_REMOTE_HISTORY_RECORD_JSON_BYTES);
    const aboveLimit = historyRecordWithExactJsonBytes(MAX_REMOTE_HISTORY_RECORD_JSON_BYTES + 1);
    expect(jsonByteLength(atLimit)).toBe(MAX_REMOTE_HISTORY_RECORD_JSON_BYTES);
    expect(jsonByteLength(aboveLimit)).toBe(MAX_REMOTE_HISTORY_RECORD_JSON_BYTES + 1);
    expect(
      decodeAgentRemoteRecordsPage({ records: [atLimit], historyRevision: "revision-a" }, request)
    ).not.toBeNull();
    expect(
      decodeAgentRemoteRecordsPage(
        { records: [aboveLimit], historyRevision: "revision-a" },
        request
      )
    ).toBeNull();
  });

  test("enforces safe remote tool, permission, and error projections", () => {
    const request = { sessionId: "session-a", limit: 5 };
    const page: AgentRemoteRecordsPage = {
      ...recordsPage(),
      records: [
        {
          ...recordsPage().records[0],
          items: [
            {
              id: "tool-a",
              itemType: "tool",
              role: "assistant",
              title: "Tool activity",
              status: "failed",
              text: "The tool failed. Open the host for additional diagnostic details.",
              createdMs: 3,
              merge: "replace"
            }
          ]
        }
      ]
    };
    expect(decodeAgentRemoteRecordsPage(page, request)).not.toBeNull();
    expect(
      decodeAgentRemoteRecordsPage(
        {
          ...page,
          records: [
            {
              ...page.records[0],
              items: [{ ...page.records[0].items[0], text: "raw stderr and credentials" }]
            }
          ]
        },
        request
      )
    ).toBeNull();
  });

  test("rejects a valid-shape history page beyond the cumulative 8 MiB ledger", () => {
    const text = "x".repeat(180 * 1_024);
    const records = Array.from({ length: 47 }, (_, index) => ({
      recordId: `record-${index}`,
      role: "assistant",
      createdMs: index,
      items: [
        {
          id: `message-${index}`,
          itemType: "message",
          role: "assistant",
          text,
          createdMs: index,
          merge: "replace"
        }
      ]
    }));
    expect(
      decodeAgentRemoteRecordsPage(
        { records, historyRevision: "revision-a" },
        { sessionId: "session-a", limit: 50 }
      )
    ).toBeNull();
  });

  test("rejects clients that retain extension or generic invoke fields", () => {
    const readOnlyClient = client();
    expect(isClosedAgentRemoteReadOnlyClient({ ...readOnlyClient })).toBe(false);
    expect(isClosedAgentRemoteReadOnlyClient({ ...readOnlyClient, invoke: () => {} })).toBe(false);
  });
});
