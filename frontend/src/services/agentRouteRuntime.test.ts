import { describe, expect, test } from "bun:test";
import {
  AgentRuntimeService,
  LOCAL_AGENT_EXECUTION_TARGET,
  createRemoteAgentExecutionTarget,
  type AgentRuntimeBridge
} from "@/services/agentRuntimeService";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import { createAgentRemoteReadOnlyClient } from "@/services/agentRemoteProviderBridge";
import {
  agentRemoteReadOnlyProjectionKey,
  agentRouteProjectionKey,
  resolveAgentRouteRuntime,
  type AgentPortableRuntimeState
} from "@/services/agentRouteRuntime";

const DESKTOP = {
  isTauri: true,
  isTauriDesktop: true,
  isTauriMobile: false
} as const;
const PORTABLE = {
  isTauri: true,
  isTauriDesktop: false,
  isTauriMobile: true
} as const;
const WEB = {
  isTauri: false,
  isTauriDesktop: false,
  isTauriMobile: false
} as const;

function localService(): AgentRuntimeService {
  return new AgentRuntimeService(undefined, LOCAL_AGENT_EXECUTION_TARGET);
}

function remoteService(id = "6ef8cbe0-57dd-4750-a51b-9dc900d51659"): AgentRuntimeService {
  const bridge: AgentRuntimeBridge = {
    runForUser: async (_userId, operation) => await operation(),
    prepareTarget: async (_userId, target) => ({
      targetId: target.id,
      hostEpoch: "1",
      connectionGeneration: 1
    }),
    invokeTarget: async () => {
      throw new Error("not invoked by route resolution");
    }
  };
  return new AgentRuntimeService(bridge, createRemoteAgentExecutionTarget(id, "MacBook"));
}

function readOnlyRuntime(
  accountId = "account-a",
  targetId = "6ef8cbe0-57dd-4750-a51b-9dc900d51659"
): Extract<AgentPortableRuntimeState, { status: "readOnlyReady" }> {
  const client = createAgentRemoteReadOnlyClient({
    accountId,
    targetId,
    targetLabel: "MacBook",
    source: {
      getRuntimeStatus: async () => ({ running: false, activeRunCount: 0 }),
      listSessionSummariesPage: async () => ({ items: [] }),
      listPersistedRecordsPage: async () => ({
        records: [],
        historyRevision: "revision-a"
      })
    },
    capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES
  });
  return {
    accountId,
    status: "readOnlyReady",
    client,
    capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
    runtimeKey: "binding-a"
  };
}

describe("resolveAgentRouteRuntime", () => {
  test("preserves the embedded local runtime on Tauri Desktop", () => {
    const local = localService();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: DESKTOP,
        portableRuntime: null,
        localService: local
      })
    ).toEqual({ status: "ready", service: local, runtimeKey: "embedded" });
  });

  test("does not replace Desktop local behavior with a portable read-only selection", () => {
    const local = localService();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: DESKTOP,
        portableRuntime: readOnlyRuntime(),
        localService: local
      })
    ).toEqual({ status: "ready", service: local, runtimeKey: "embedded" });
  });

  test("never falls back to the local service on a portable Tauri client", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: null,
        localService: localService()
      })
    ).toEqual({ status: "unavailable", reason: "remoteProviderUnavailable" });
  });

  test("keeps a portable route pending while the verified provider loads", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: { accountId: "account-a", status: "loading" }
      })
    ).toEqual({ status: "loading" });
  });

  test("mounts only the narrow read-only client supplied by the portable provider", () => {
    const portableRuntime = readOnlyRuntime();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime
      })
    ).toEqual({
      status: "readOnlyReady",
      client: portableRuntime.client,
      capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
      runtimeKey: "binding-a"
    });
  });

  test("rejects an incomplete or widened capability snapshot before any bridge call", () => {
    const portableRuntime = readOnlyRuntime();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          ...portableRuntime,
          capabilities: {
            ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
            persistedRecordsPage: false
          }
        }
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          ...portableRuntime,
          capabilities: {
            ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
            mutations: true
          }
        }
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });

    const hiddenExtension = { ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES };
    Object.defineProperty(hiddenExtension, "sendMessage", { value: true });
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: { ...portableRuntime, capabilities: hiddenExtension }
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
  });

  test("rejects a narrow client carrying a generic invoke extension", () => {
    const portableRuntime = readOnlyRuntime();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          ...portableRuntime,
          client: { ...portableRuntime.client, invoke: () => {} }
        } as AgentPortableRuntimeState
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
  });

  test("rejects a missing provider runtime lifecycle fence", () => {
    const portableRuntime = readOnlyRuntime();
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: { ...portableRuntime, runtimeKey: "" }
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
  });

  test("rejects paired-host state retained for another account", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-b",
        platform: PORTABLE,
        portableRuntime: readOnlyRuntime("account-a")
      })
    ).toEqual({ status: "unavailable", reason: "pairingUnavailable" });
  });

  test("rejects a client whose authenticated binding differs from provider state", () => {
    const accountA = readOnlyRuntime("account-a");
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: { ...accountA, client: readOnlyRuntime("account-b").client }
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
  });

  test("passes through a verified provider's target-selection state", () => {
    const target = {
      key: "choice-a",
      label: "Office Mac",
      select: () => {}
    };
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          accountId: "account-a",
          status: "selectionRequired",
          targets: [target]
        }
      })
    ).toEqual({ status: "selectionRequired", targets: [target] });
  });

  test("treats an empty selection as no paired host", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          accountId: "account-a",
          status: "selectionRequired",
          targets: []
        }
      })
    ).toEqual({ status: "unavailable", reason: "noPairedHost" });
  });

  test("keeps browser clients unavailable even if handed a read-only client", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: WEB,
        portableRuntime: readOnlyRuntime()
      })
    ).toEqual({ status: "unavailable", reason: "requiresTauri" });
  });

  test("rejects ambiguous Tauri platform classification", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: {
          isTauri: true,
          isTauriDesktop: true,
          isTauriMobile: true
        },
        portableRuntime: null
      })
    ).toEqual({ status: "unavailable", reason: "unsupportedTauriClient" });
  });

  test("rejects a forged portable full-ready state", () => {
    expect(
      resolveAgentRouteRuntime({
        accountId: "account-a",
        platform: PORTABLE,
        portableRuntime: {
          accountId: "account-a",
          status: "ready",
          service: remoteService(),
          runtimeKey: "binding-a"
        } as unknown as AgentPortableRuntimeState
      })
    ).toEqual({ status: "unavailable", reason: "invalidPortableRuntime" });
  });

  test("namespaces mounted projections by account, target, lifecycle, and mode", () => {
    const targetA = readOnlyRuntime("account-a", "6ef8cbe0-57dd-4750-a51b-9dc900d51659").client;
    const targetB = readOnlyRuntime("account-a", "0ad3f0e4-2aa3-4583-87e7-d400465738db").client;
    expect(agentRemoteReadOnlyProjectionKey("account-a", targetA, "binding-a")).not.toBe(
      agentRemoteReadOnlyProjectionKey("account-a", targetB, "binding-a")
    );
    expect(agentRemoteReadOnlyProjectionKey("account-a", targetA, "binding-a")).not.toBe(
      agentRemoteReadOnlyProjectionKey("account-b", targetA, "binding-a")
    );
    expect(agentRemoteReadOnlyProjectionKey("account-a", targetA, "binding-a")).not.toBe(
      agentRemoteReadOnlyProjectionKey("account-a", targetA, "binding-b")
    );
    expect(agentRemoteReadOnlyProjectionKey("account-a", targetA, "binding-a")).not.toBe(
      agentRouteProjectionKey("account-a", localService(), "embedded")
    );
  });
});
