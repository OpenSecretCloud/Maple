import { afterEach, describe, expect, mock, test } from "bun:test";
import { useEffect } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import {
  AgentPortableRuntimeControllerProvider,
  type AgentPortableRuntimeController
} from "@/contexts/AgentPortableRuntimeControllerProvider";
import { useAgentPortableRuntime } from "@/contexts/AgentPortableRuntimeContext";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import type { AgentRemoteReadOnlyClient } from "@/services/agentRemoteProviderBridge";
import type { AgentPortableRuntimeState } from "@/services/agentRouteRuntime";

class TestPortableRuntimeController implements AgentPortableRuntimeController {
  private readonly snapshots = new Map<string, AgentPortableRuntimeState | null>();
  private readonly listeners = new Map<string, Set<() => void>>();

  getSnapshot(accountId: string): AgentPortableRuntimeState | null {
    return this.snapshots.get(accountId) ?? null;
  }

  subscribe(accountId: string, listener: () => void): () => void {
    const listeners = this.listeners.get(accountId) ?? new Set();
    listeners.add(listener);
    this.listeners.set(accountId, listeners);
    return () => listeners.delete(listener);
  }

  set(accountId: string, snapshot: AgentPortableRuntimeState | null): void {
    this.snapshots.set(accountId, snapshot);
    for (const listener of this.listeners.get(accountId) ?? []) listener();
  }

  subscriberCount(accountId: string): number {
    return this.listeners.get(accountId)?.size ?? 0;
  }
}

function RuntimeProbe({
  onRender,
  onReadOnlyReady
}: {
  readonly onRender: (value: AgentPortableRuntimeState | null) => void;
  readonly onReadOnlyReady?: (client: AgentRemoteReadOnlyClient) => void;
}) {
  const value = useAgentPortableRuntime();
  onRender(value);
  useEffect(() => {
    if (value?.status === "readOnlyReady") onReadOnlyReady?.(value.client);
  }, [onReadOnlyReady, value]);
  return <span>{value ? `${value.accountId}:${value.status}` : "unavailable"}</span>;
}

describe("AgentPortableRuntimeControllerProvider", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("keeps production fail-closed when no controller is installed", () => {
    let current: AgentPortableRuntimeState | null | undefined;
    act(() => {
      renderer = create(
        <AgentPortableRuntimeControllerProvider accountId="account-a">
          <RuntimeProbe onRender={(value) => (current = value)} />
        </AgentPortableRuntimeControllerProvider>
      );
    });

    expect(current).toBeNull();
    expect(renderer?.toJSON()).toMatchObject({ children: ["unavailable"] });
  });

  test("publishes only a snapshot bound to the requested account", () => {
    const controller = new TestPortableRuntimeController();
    controller.set("account-a", { accountId: "account-a", status: "loading" });
    let current: AgentPortableRuntimeState | null | undefined;
    act(() => {
      renderer = create(
        <AgentPortableRuntimeControllerProvider accountId="account-a" controller={controller}>
          <RuntimeProbe onRender={(value) => (current = value)} />
        </AgentPortableRuntimeControllerProvider>
      );
    });

    expect(current).toEqual({ accountId: "account-a", status: "loading" });
    expect(controller.subscriberCount("account-a")).toBe(1);

    act(() => {
      controller.set("account-a", { accountId: "account-b", status: "loading" });
    });
    expect(current).toBeNull();
  });

  test("unsubscribes the prior account before publishing a replacement account", () => {
    const controller = new TestPortableRuntimeController();
    controller.set("account-a", { accountId: "account-a", status: "loading" });
    controller.set("account-b", {
      accountId: "account-b",
      status: "unavailable",
      reason: "noPairedHost"
    });
    const probe = (accountId: string) => (
      <AgentPortableRuntimeControllerProvider accountId={accountId} controller={controller}>
        <RuntimeProbe onRender={() => {}} />
      </AgentPortableRuntimeControllerProvider>
    );

    act(() => {
      renderer = create(probe("account-a"));
    });
    expect(controller.subscriberCount("account-a")).toBe(1);
    act(() => renderer?.update(probe("account-b")));
    expect(controller.subscriberCount("account-a")).toBe(0);
    expect(controller.subscriberCount("account-b")).toBe(1);
    expect(renderer?.toJSON()).toMatchObject({ children: ["account-b:unavailable"] });
  });

  test("removes controller state immediately for a signed-out account", () => {
    const controller = new TestPortableRuntimeController();
    controller.set("account-a", { accountId: "account-a", status: "loading" });
    const probe = (accountId: string | null) => (
      <AgentPortableRuntimeControllerProvider accountId={accountId} controller={controller}>
        <RuntimeProbe onRender={() => {}} />
      </AgentPortableRuntimeControllerProvider>
    );

    act(() => {
      renderer = create(probe("account-a"));
    });
    act(() => renderer?.update(probe(null)));
    expect(controller.subscriberCount("account-a")).toBe(0);
    expect(renderer?.toJSON()).toMatchObject({ children: ["unavailable"] });
  });

  test("does not revive stale readiness when the same account signs back in", () => {
    const statusCalls: string[] = [];
    const client: AgentRemoteReadOnlyClient = {
      binding: { accountId: "account-a", targetId: "target-a" },
      capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
      getRuntimeStatus: async () => {
        statusCalls.push("status");
        return { running: true, activeRunCount: 0 };
      },
      listSessionSummariesPage: async () => ({ items: [], nextCursor: null }),
      listPersistedRecordsPage: async () => ({
        records: [],
        nextCursor: null,
        historyRevision: "revision-a"
      })
    };
    const controller = new TestPortableRuntimeController();
    controller.set("account-a", {
      accountId: "account-a",
      status: "readOnlyReady",
      client,
      capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
      runtimeKey: "binding-a"
    });
    const renderedStatuses: string[] = [];
    const onRender = (value: AgentPortableRuntimeState | null) => {
      renderedStatuses.push(value?.status ?? "unavailable");
    };
    const onReadOnlyReady = (readyClient: AgentRemoteReadOnlyClient) => {
      void readyClient.getRuntimeStatus();
    };
    const probe = (accountId: string | null) => (
      <AgentPortableRuntimeControllerProvider accountId={accountId} controller={controller}>
        <RuntimeProbe onRender={onRender} onReadOnlyReady={onReadOnlyReady} />
      </AgentPortableRuntimeControllerProvider>
    );

    act(() => {
      renderer = create(probe("account-a"));
    });
    expect(statusCalls).toEqual(["status"]);

    act(() => renderer?.update(probe(null)));
    expect(controller.subscriberCount("account-a")).toBe(0);
    controller.set("account-a", { accountId: "account-a", status: "loading" });
    renderedStatuses.length = 0;
    statusCalls.length = 0;

    act(() => renderer?.update(probe("account-a")));
    expect(renderedStatuses).not.toContain("readOnlyReady");
    expect(statusCalls).toEqual([]);
    expect(controller.subscriberCount("account-a")).toBe(1);
    expect(renderer?.toJSON()).toMatchObject({ children: ["account-a:loading"] });
  });

  test("fails closed when readiness cannot subscribe to revocation updates", () => {
    const snapshot: AgentPortableRuntimeState = { accountId: "account-a", status: "loading" };
    const getSnapshot = mock(() => snapshot);
    const controller: AgentPortableRuntimeController = {
      getSnapshot,
      subscribe: () => {
        throw new Error("subscription unavailable");
      }
    };

    act(() => {
      renderer = create(
        <AgentPortableRuntimeControllerProvider accountId="account-a" controller={controller}>
          <RuntimeProbe onRender={() => {}} />
        </AgentPortableRuntimeControllerProvider>
      );
    });
    expect(renderer?.toJSON()).toMatchObject({ children: ["unavailable"] });
    expect(getSnapshot).not.toHaveBeenCalled();
  });

  test("ignores a synchronous notification when subscription then fails", () => {
    const getSnapshot = mock(
      (): AgentPortableRuntimeState => ({
        accountId: "account-a",
        status: "loading"
      })
    );
    const controller: AgentPortableRuntimeController = {
      getSnapshot,
      subscribe: (_accountId, listener) => {
        listener();
        throw new Error("subscription failed after notification");
      }
    };

    act(() => {
      renderer = create(
        <AgentPortableRuntimeControllerProvider accountId="account-a" controller={controller}>
          <RuntimeProbe onRender={() => {}} />
        </AgentPortableRuntimeControllerProvider>
      );
    });
    expect(renderer?.toJSON()).toMatchObject({ children: ["unavailable"] });
    expect(getSnapshot).not.toHaveBeenCalled();
  });
});
