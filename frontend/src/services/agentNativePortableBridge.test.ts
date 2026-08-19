import { describe, expect, mock, test } from "bun:test";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import type { AgentRemoteRecordsPage } from "@/services/agentRemoteProviderBridge";
import {
  AGENT_NATIVE_PORTABLE_COMMANDS,
  createAgentNativePortableReadOnlySource,
  decodeAgentNativePortableError,
  decodeAgentNativePortableRecordsPage,
  decodeAgentNativePortableRefreshResult,
  decodeAgentNativePortableWireLease,
  isAgentNativePortableAccountId,
  tauriAgentNativePortableBridge,
  type AgentNativePortableBridge,
  type AgentNativePortableReadBinding,
  type AgentNativePortableWireLease
} from "@/services/agentNativePortableBridge";

const ACCOUNT_ID = "11111111-1111-1111-1111-111111111111";
const RUNTIME_ID = `runtime_${"1".repeat(48)}`;
const TARGET_HANDLE = `target_${"2".repeat(48)}`;
const LEASE_HANDLE = `lease_${"3".repeat(48)}`;

function lease(): AgentNativePortableWireLease {
  return {
    leaseHandle: LEASE_HANDLE,
    targetHandle: TARGET_HANDLE,
    hostEpoch: "18446744073709551615",
    connectionGeneration: 7
  };
}

function binding(): AgentNativePortableReadBinding {
  return { accountId: ACCOUNT_ID, runtimeId: RUNTIME_ID, lease: lease() };
}

function recordItemsPage() {
  return {
    items: [
      {
        recordId: "record-a",
        role: "assistant",
        createdMs: 4,
        items: [
          {
            id: "message-a",
            itemType: "message",
            role: "assistant",
            text: "safe text",
            createdMs: 4,
            merge: "replace"
          }
        ]
      }
    ],
    historyRevision: "revision-a",
    nextCursor: "history-cursor-b"
  };
}

describe("native portable Agent bridge", () => {
  test("pins the exact five commands and exposes no generic invoke", () => {
    expect(AGENT_NATIVE_PORTABLE_COMMANDS).toEqual({
      refreshTargets: "agent_portable_refresh_targets",
      prepareTarget: "agent_portable_prepare_target",
      getRuntimeStatus: "agent_portable_get_runtime_status",
      listSessionsPage: "agent_portable_list_sessions_page",
      listRecordsPage: "agent_portable_list_records_page"
    });
    expect(Object.keys(tauriAgentNativePortableBridge).sort()).toEqual(
      [
        "getRuntimeStatus",
        "listRecordsPage",
        "listSessionsPage",
        "prepareTarget",
        "refreshTargets"
      ].sort()
    );
    expect("invoke" in tauriAgentNativePortableBridge).toBe(false);
  });

  test("decodes only the exact authenticated refresh grant and target roster", () => {
    const result = decodeAgentNativePortableRefreshResult({
      schemaVersion: 1,
      runtimeId: RUNTIME_ID,
      capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
      items: [{ handle: TARGET_HANDLE, label: "Office Mac" }]
    });
    expect(result).toEqual({
      schemaVersion: 1,
      runtimeId: RUNTIME_ID,
      capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
      items: [{ handle: TARGET_HANDLE, label: "Office Mac" }]
    });
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        capabilities: { ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES, mutations: true }
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({ ...result, providerPrivateField: "secret" })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [
          { handle: TARGET_HANDLE, label: "Office Mac" },
          { handle: TARGET_HANDLE, label: "Duplicate" }
        ]
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "Trusted\u202ehost" }]
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: " Office Mac" }]
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "Office Mac " }]
      })
    ).toBeNull();
    for (const whitespace of [
      "\u00a0",
      "\u1680",
      "\u2000",
      "\u2028",
      "\u202f",
      "\u205f",
      "\u3000"
    ]) {
      expect(
        decodeAgentNativePortableRefreshResult({
          ...result,
          items: [{ handle: TARGET_HANDLE, label: `${whitespace}Office Mac` }]
        })
      ).toBeNull();
      expect(
        decodeAgentNativePortableRefreshResult({
          ...result,
          items: [{ handle: TARGET_HANDLE, label: `Office Mac${whitespace}` }]
        })
      ).toBeNull();
    }
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "\ufeffOffice Mac" }]
      })
    ).not.toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "Office Mac\ufeff" }]
      })
    ).not.toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "x".repeat(80) }]
      })
    ).not.toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "x".repeat(81) }]
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "🦀".repeat(64) }]
      })
    ).not.toBeNull();
    expect(
      decodeAgentNativePortableRefreshResult({
        ...result,
        items: [{ handle: TARGET_HANDLE, label: "🦀".repeat(65) }]
      })
    ).toBeNull();
  });

  test("accepts only canonical lowercase non-nil UUID account IDs", () => {
    expect(isAgentNativePortableAccountId(ACCOUNT_ID)).toBe(true);
    expect(isAgentNativePortableAccountId("ffffffff-ffff-ffff-ffff-ffffffffffff")).toBe(true);
    expect(isAgentNativePortableAccountId("00000000-0000-0000-0000-000000000000")).toBe(false);
    expect(isAgentNativePortableAccountId("11111111-1111-1111-1111-11111111111A")).toBe(false);
    expect(isAgentNativePortableAccountId("111111111111-1111-1111-111111111111")).toBe(false);
    expect(isAgentNativePortableAccountId(` ${ACCOUNT_ID}`)).toBe(false);
    expect(isAgentNativePortableAccountId(`${ACCOUNT_ID} `)).toBe(false);
  });

  test("keeps a fresh lease handle distinct from its selected target handle", () => {
    expect(decodeAgentNativePortableWireLease(lease())).toEqual(lease());
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        leaseHandle: TARGET_HANDLE
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        hostEpoch: "01"
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        hostEpoch: "9007199254740992"
      })
    ).not.toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        hostEpoch: "18446744073709551616"
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        hostEpoch: "0"
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        connectionGeneration: Number.MAX_SAFE_INTEGER + 1
      })
    ).toBeNull();
    expect(
      decodeAgentNativePortableWireLease({
        ...lease(),
        nativeLease: "must-not-cross"
      })
    ).toBeNull();
  });

  test("explicitly maps native history items to Phase 1 records", () => {
    const decoded = decodeAgentNativePortableRecordsPage(recordItemsPage(), {
      sessionId: "session-a",
      cursor: "history-cursor-a",
      limit: 5
    });
    expect(decoded).toEqual({
      records: recordItemsPage().items,
      historyRevision: "revision-a",
      nextCursor: "history-cursor-b"
    } as AgentRemoteRecordsPage);
    expect("items" in decoded!).toBe(false);
    expect(
      decodeAgentNativePortableRecordsPage(
        {
          ...recordItemsPage(),
          records: recordItemsPage().items
        },
        { sessionId: "session-a", limit: 5 }
      )
    ).toBeNull();
  });

  test("accepts only the closed native error code envelope", () => {
    expect(decodeAgentNativePortableError({ code: "stale_lease" })).toBe("stale_lease");
    expect(decodeAgentNativePortableError({ code: "stale_lease", message: "secret" })).toBe(
      "unavailable"
    );
    expect(decodeAgentNativePortableError("raw native error with secrets")).toBe("unavailable");
  });

  test("fails closed without leaking platform or invoke errors off mobile", async () => {
    await expect(tauriAgentNativePortableBridge.refreshTargets(ACCOUNT_ID)).rejects.toMatchObject({
      name: "AgentNativePortableError",
      code: "unavailable"
    });
  });

  test("the read source binds account, runtime, and every lease scalar before and after await", async () => {
    let current = true;
    let releaseStatus!: (value: { running: boolean; activeRunCount: number }) => void;
    const status = new Promise<{ running: boolean; activeRunCount: number }>((resolve) => {
      releaseStatus = resolve;
    });
    const statusCalls: AgentNativePortableReadBinding[] = [];
    const bridge: AgentNativePortableBridge = {
      refreshTargets: async () => {
        throw new Error("unexpected refresh");
      },
      prepareTarget: async () => {
        throw new Error("unexpected prepare");
      },
      getRuntimeStatus: async (captured) => {
        statusCalls.push(captured);
        return await status;
      },
      listSessionsPage: mock(async () => ({ items: [] })),
      listRecordsPage: mock(async () => ({ records: [], historyRevision: "revision-a" }))
    };
    const source = createAgentNativePortableReadOnlySource(bridge, binding(), () => {
      if (!current) throw new Error("stale binding");
    });

    const pending = source.getRuntimeStatus();
    expect(statusCalls).toEqual([binding()]);
    current = false;
    releaseStatus({ running: true, activeRunCount: 0 });
    await expect(pending).rejects.toThrow("stale binding");
  });
});
