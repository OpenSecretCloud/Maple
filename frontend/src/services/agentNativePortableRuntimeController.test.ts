import { describe, expect, mock, test } from "bun:test";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import { isClosedAgentRemoteReadOnlyClient } from "@/services/agentRemoteProviderBridge";
import { AgentNativePortableRuntimeController } from "@/services/agentNativePortableRuntimeController";
import type {
  AgentNativePortableBridge,
  AgentNativePortableReadBinding,
  AgentNativePortableRefreshResult,
  AgentNativePortableWireLease
} from "@/services/agentNativePortableBridge";

const TARGET_A = `target_${"a".repeat(48)}`;
const TARGET_B = `target_${"b".repeat(48)}`;
const RUNTIME_A = `runtime_${"1".repeat(48)}`;
const LEASE_A = `lease_${"2".repeat(48)}`;
const ACCOUNT_A = "11111111-1111-1111-1111-111111111111";
const ACCOUNT_B = "22222222-2222-2222-2222-222222222222";

function refresh(
  runtimeId = RUNTIME_A,
  items: AgentNativePortableRefreshResult["items"] = [{ handle: TARGET_A, label: "Office Mac" }]
): AgentNativePortableRefreshResult {
  return {
    schemaVersion: 1,
    runtimeId,
    capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
    items
  };
}

function lease(targetHandle = TARGET_A): AgentNativePortableWireLease {
  return {
    leaseHandle: LEASE_A,
    targetHandle,
    hostEpoch: "9",
    connectionGeneration: 3
  };
}

function bridge(overrides: Partial<AgentNativePortableBridge> = {}): AgentNativePortableBridge {
  return {
    refreshTargets: async () => refresh(),
    prepareTarget: async (_accountId, _runtimeId, targetHandle) => lease(targetHandle),
    getRuntimeStatus: async () => ({ running: true, activeRunCount: 1 }),
    listSessionsPage: async () => ({
      items: [
        {
          id: "session-a",
          title: "Task",
          createdMs: 1,
          updatedMs: 2,
          pageSortMs: 2,
          messageCount: 1
        }
      ],
      nextCursor: "session-cursor-b"
    }),
    listRecordsPage: async () => ({
      records: [],
      historyRevision: "revision-a",
      nextCursor: null
    }),
    ...overrides
  };
}

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("native portable Agent runtime controller", () => {
  test("is inert until an exact account subscribes", async () => {
    const refreshTargets = mock(async () => refresh());
    const controller = new AgentNativePortableRuntimeController(bridge({ refreshTargets }));
    expect(controller.getSnapshot(ACCOUNT_A)).toBeNull();
    await flush();
    expect(refreshTargets).not.toHaveBeenCalled();
  });

  test("rejects noncanonical and nil account subscriptions before bridge work", async () => {
    const refreshTargets = mock(async () => refresh());
    const controller = new AgentNativePortableRuntimeController(bridge({ refreshTargets }));
    const invalidAccounts = [
      "00000000-0000-0000-0000-000000000000",
      "11111111-1111-1111-1111-11111111111A",
      "account-a"
    ];
    for (const accountId of invalidAccounts) {
      const unsubscribe = controller.subscribe(accountId, () => {});
      expect(controller.getSnapshot(accountId)).toBeNull();
      unsubscribe();
    }
    await flush();
    expect(refreshTargets).not.toHaveBeenCalled();
  });

  test("requires explicit target selection before publishing a branded read-only client", async () => {
    const prepareCalls: Array<[string, string, string]> = [];
    const readBindings: AgentNativePortableReadBinding[] = [];
    const sessionPages: unknown[] = [];
    const recordPages: unknown[] = [];
    const provider = bridge({
      prepareTarget: async (accountId, runtimeId, targetHandle) => {
        prepareCalls.push([accountId, runtimeId, targetHandle]);
        return lease(targetHandle);
      },
      getRuntimeStatus: async (binding) => {
        readBindings.push(binding);
        return { running: true, activeRunCount: 0 };
      },
      listSessionsPage: async (binding, page) => {
        readBindings.push(binding);
        sessionPages.push(page);
        return { items: [], nextCursor: null };
      },
      listRecordsPage: async (binding, page) => {
        readBindings.push(binding);
        recordPages.push(page);
        return { records: [], historyRevision: "revision-a" };
      }
    });
    const controller = new AgentNativePortableRuntimeController(provider);
    const unsubscribe = controller.subscribe(ACCOUNT_A, () => {});
    expect(controller.getSnapshot(ACCOUNT_A)).toEqual({
      accountId: ACCOUNT_A,
      status: "loading"
    });
    await flush();

    const selection = controller.getSnapshot(ACCOUNT_A);
    expect(selection?.status).toBe("selectionRequired");
    if (selection?.status !== "selectionRequired") throw new Error("expected target selection");
    expect(prepareCalls).toEqual([]);
    selection.targets[0].select();
    expect(controller.getSnapshot(ACCOUNT_A)?.status).toBe("loading");
    await flush();

    const ready = controller.getSnapshot(ACCOUNT_A);
    expect(ready?.status).toBe("readOnlyReady");
    if (ready?.status !== "readOnlyReady") throw new Error("expected read-only readiness");
    expect(isClosedAgentRemoteReadOnlyClient(ready.client)).toBe(true);
    expect(ready.client.binding).toEqual({
      accountId: ACCOUNT_A,
      targetId: TARGET_A,
      targetLabel: "Office Mac"
    });
    expect(JSON.parse(ready.runtimeKey)).toEqual([RUNTIME_A, LEASE_A, TARGET_A, "9", 3]);
    expect(prepareCalls).toEqual([[ACCOUNT_A, RUNTIME_A, TARGET_A]]);

    await ready.client.getRuntimeStatus();
    await ready.client.listSessionSummariesPage({ cursor: "session-cursor-a", limit: 5 });
    await ready.client.listPersistedRecordsPage({
      sessionId: "session-a",
      cursor: "history-cursor-a",
      limit: 6
    });
    expect(readBindings).toEqual([
      {
        accountId: ACCOUNT_A,
        runtimeId: RUNTIME_A,
        lease: lease()
      },
      {
        accountId: ACCOUNT_A,
        runtimeId: RUNTIME_A,
        lease: lease()
      },
      {
        accountId: ACCOUNT_A,
        runtimeId: RUNTIME_A,
        lease: lease()
      }
    ]);
    expect(sessionPages).toEqual([{ cursor: "session-cursor-a", limit: 5 }]);
    expect(recordPages).toEqual([{ sessionId: "session-a", cursor: "history-cursor-a", limit: 6 }]);
    unsubscribe();
  });

  test("maps an empty verified roster and refresh failure to closed unavailable states", async () => {
    const noTargets = new AgentNativePortableRuntimeController(
      bridge({ refreshTargets: async () => refresh(RUNTIME_A, []) })
    );
    const stopNoTargets = noTargets.subscribe(ACCOUNT_A, () => {});
    await flush();
    expect(noTargets.getSnapshot(ACCOUNT_A)).toEqual({
      accountId: ACCOUNT_A,
      status: "unavailable",
      reason: "noPairedHost"
    });
    stopNoTargets();

    const failed = new AgentNativePortableRuntimeController(
      bridge({ refreshTargets: async () => Promise.reject(new Error("native secret")) })
    );
    const stopFailed = failed.subscribe(ACCOUNT_A, () => {});
    await flush();
    expect(failed.getSnapshot(ACCOUNT_A)).toEqual({
      accountId: ACCOUNT_A,
      status: "unavailable",
      reason: "pairingUnavailable"
    });
    stopFailed();
  });

  test("ignores a late account-A refresh after account B replaces its lane", async () => {
    let resolveA!: (value: AgentNativePortableRefreshResult) => void;
    const pendingA = new Promise<AgentNativePortableRefreshResult>((resolve) => {
      resolveA = resolve;
    });
    const refreshTargets = mock(async (accountId: string) => {
      if (accountId === ACCOUNT_A) return await pendingA;
      return refresh(`runtime_${"4".repeat(48)}`, [{ handle: TARGET_B, label: "Laptop" }]);
    });
    const controller = new AgentNativePortableRuntimeController(bridge({ refreshTargets }));
    const stopA = controller.subscribe(ACCOUNT_A, () => {});
    await flush();
    const stopB = controller.subscribe(ACCOUNT_B, () => {});
    await flush();
    expect(controller.getSnapshot(ACCOUNT_A)).toBeNull();
    expect(controller.getSnapshot(ACCOUNT_B)?.status).toBe("selectionRequired");
    resolveA(refresh());
    await flush();
    expect(controller.getSnapshot(ACCOUNT_B)?.status).toBe("selectionRequired");
    stopA();
    stopB();
  });

  test("A to signed-out to the same A requires a fresh refresh and fences the old client", async () => {
    const status = mock(async () => ({ running: true, activeRunCount: 0 }));
    const refreshTargets = mock(async () => refresh());
    const controller = new AgentNativePortableRuntimeController(
      bridge({ refreshTargets, getRuntimeStatus: status })
    );
    const stopFirst = controller.subscribe(ACCOUNT_A, () => {});
    await flush();
    const firstSelection = controller.getSnapshot(ACCOUNT_A);
    if (firstSelection?.status !== "selectionRequired") throw new Error("expected selection");
    firstSelection.targets[0].select();
    await flush();
    const firstReady = controller.getSnapshot(ACCOUNT_A);
    if (firstReady?.status !== "readOnlyReady") throw new Error("expected readiness");

    stopFirst();
    expect(controller.getSnapshot(ACCOUNT_A)).toBeNull();
    await expect(firstReady.client.getRuntimeStatus()).rejects.toThrow("no longer current");
    expect(status).not.toHaveBeenCalled();

    const stopSecond = controller.subscribe(ACCOUNT_A, () => {});
    expect(controller.getSnapshot(ACCOUNT_A)?.status).toBe("loading");
    expect(refreshTargets).toHaveBeenCalledTimes(1);
    await flush();
    expect(refreshTargets).toHaveBeenCalledTimes(2);
    expect(controller.getSnapshot(ACCOUNT_A)?.status).toBe("selectionRequired");
    stopSecond();
  });

  test("fails closed when prepare returns a lease for another target", async () => {
    const controller = new AgentNativePortableRuntimeController(
      bridge({ prepareTarget: async () => lease(TARGET_B) })
    );
    const stop = controller.subscribe(ACCOUNT_A, () => {});
    await flush();
    const selection = controller.getSnapshot(ACCOUNT_A);
    if (selection?.status !== "selectionRequired") throw new Error("expected selection");
    selection.targets[0].select();
    await flush();
    expect(controller.getSnapshot(ACCOUNT_A)).toEqual({
      accountId: ACCOUNT_A,
      status: "unavailable",
      reason: "pairingUnavailable"
    });
    stop();
  });
});
