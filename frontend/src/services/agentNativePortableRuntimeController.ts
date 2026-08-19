import type { AgentPortableRuntimeController } from "@/contexts/AgentPortableRuntimeControllerProvider";
import {
  createAgentRemoteReadOnlyClient,
  type AgentRemoteReadOnlyClient
} from "@/services/agentRemoteProviderBridge";
import type { AgentPortableRuntimeState } from "@/services/agentRouteRuntime";
import {
  createAgentNativePortableReadOnlySource,
  isAgentNativePortableAccountId,
  tauriAgentNativePortableBridge,
  type AgentNativePortableBridge,
  type AgentNativePortableRefreshResult,
  type AgentNativePortableTarget,
  type AgentNativePortableWireLease
} from "@/services/agentNativePortableBridge";

const MAX_RUNTIME_KEY_LENGTH = 256;

interface PortableRuntimeLane {
  readonly identity: object;
  readonly accountId: string;
  readonly listeners: Set<() => void>;
  operationRevision: number;
  activeBindingKey: string | null;
  snapshot: AgentPortableRuntimeState | null;
}

/**
 * Account-scoped controller for the persisted-only native provider. Constructing
 * it is inert: native work begins only after an authenticated account subscribes.
 * Maple intentionally does not install this controller in App yet.
 */
export class AgentNativePortableRuntimeController implements AgentPortableRuntimeController {
  private lane: PortableRuntimeLane | null = null;

  constructor(
    private readonly bridge: AgentNativePortableBridge = tauriAgentNativePortableBridge
  ) {}

  getSnapshot(accountId: string): AgentPortableRuntimeState | null {
    const lane = this.lane;
    return lane && lane.accountId === accountId && lane.listeners.size > 0 ? lane.snapshot : null;
  }

  subscribe(accountId: string, listener: () => void): () => void {
    if (!isAgentNativePortableAccountId(accountId) || typeof listener !== "function") {
      return () => {};
    }

    let lane = this.lane;
    if (!lane || lane.accountId !== accountId || lane.listeners.size === 0) {
      const retiredListeners = lane ? [...lane.listeners] : [];
      if (lane) {
        lane.operationRevision += 1;
        lane.activeBindingKey = null;
        lane.snapshot = null;
      }
      lane = {
        identity: Object.freeze({}),
        accountId,
        listeners: new Set(),
        operationRevision: 1,
        activeBindingKey: null,
        snapshot: Object.freeze({ accountId, status: "loading" })
      };
      this.lane = lane;
      for (const retired of retiredListeners) safeNotify(retired);
      const identity = lane.identity;
      const revision = lane.operationRevision;
      queueMicrotask(() => void this.refreshTargets(identity, revision));
    }

    const subscribedLane = lane;
    subscribedLane.listeners.add(listener);
    let subscribed = true;
    return () => {
      if (!subscribed) return;
      subscribed = false;
      subscribedLane.listeners.delete(listener);
      if (this.lane === subscribedLane && subscribedLane.listeners.size === 0) {
        subscribedLane.operationRevision += 1;
        subscribedLane.activeBindingKey = null;
        subscribedLane.snapshot = null;
      }
    };
  }

  private async refreshTargets(identity: object, revision: number): Promise<void> {
    const lane = this.currentLane(identity, revision);
    if (!lane) return;
    try {
      const refreshed = await this.bridge.refreshTargets(lane.accountId);
      const current = this.currentLane(identity, revision);
      if (!current) return;
      if (refreshed.items.length === 0) {
        this.publish(current, {
          accountId: current.accountId,
          status: "unavailable",
          reason: "noPairedHost"
        });
        return;
      }
      const targets = refreshed.items.map((target) =>
        Object.freeze({
          key: target.handle,
          label: target.label,
          select: () => this.selectTarget(identity, revision, refreshed, target)
        })
      );
      this.publish(current, {
        accountId: current.accountId,
        status: "selectionRequired",
        targets: Object.freeze(targets)
      });
    } catch {
      const current = this.currentLane(identity, revision);
      if (current) {
        this.publish(current, {
          accountId: current.accountId,
          status: "unavailable",
          reason: "pairingUnavailable"
        });
      }
    }
  }

  private selectTarget(
    identity: object,
    refreshRevision: number,
    refreshed: AgentNativePortableRefreshResult,
    target: AgentNativePortableTarget
  ): void {
    const lane = this.currentLane(identity, refreshRevision);
    if (!lane || lane.snapshot?.status !== "selectionRequired") return;
    if (!refreshed.items.some((candidate) => candidate.handle === target.handle)) return;

    lane.operationRevision += 1;
    const prepareRevision = lane.operationRevision;
    lane.activeBindingKey = null;
    this.publish(lane, { accountId: lane.accountId, status: "loading" });
    void this.prepareTarget(identity, prepareRevision, refreshed, target);
  }

  private async prepareTarget(
    identity: object,
    revision: number,
    refreshed: AgentNativePortableRefreshResult,
    target: AgentNativePortableTarget
  ): Promise<void> {
    const lane = this.currentLane(identity, revision);
    if (!lane) return;
    try {
      const lease = await this.bridge.prepareTarget(
        lane.accountId,
        refreshed.runtimeId,
        target.handle
      );
      const current = this.currentLane(identity, revision);
      if (!current) return;
      if (lease.targetHandle !== target.handle) {
        throw new Error("Paired-host Agent lease belongs to another target");
      }

      const bindingKey = portableBindingKey(refreshed.runtimeId, lease);
      current.activeBindingKey = bindingKey;
      const assertCurrent = () => {
        const active = this.currentLane(identity, revision);
        if (!active || active.activeBindingKey !== bindingKey) {
          throw new Error("Paired-host Agent binding is no longer current");
        }
      };
      const source = createAgentNativePortableReadOnlySource(
        this.bridge,
        {
          accountId: current.accountId,
          runtimeId: refreshed.runtimeId,
          lease
        },
        assertCurrent
      );
      const client: AgentRemoteReadOnlyClient = createAgentRemoteReadOnlyClient({
        accountId: current.accountId,
        targetId: lease.targetHandle,
        targetLabel: target.label,
        capabilities: refreshed.capabilities,
        source
      });
      assertCurrent();
      this.publish(current, {
        accountId: current.accountId,
        status: "readOnlyReady",
        client,
        capabilities: refreshed.capabilities,
        runtimeKey: bindingKey
      });
    } catch {
      const current = this.currentLane(identity, revision);
      if (current) {
        current.activeBindingKey = null;
        this.publish(current, {
          accountId: current.accountId,
          status: "unavailable",
          reason: "pairingUnavailable"
        });
      }
    }
  }

  private currentLane(identity: object, revision: number): PortableRuntimeLane | null {
    const lane = this.lane;
    return lane &&
      lane.identity === identity &&
      lane.operationRevision === revision &&
      lane.listeners.size > 0
      ? lane
      : null;
  }

  private publish(lane: PortableRuntimeLane, snapshot: AgentPortableRuntimeState): void {
    if (this.lane !== lane || snapshot.accountId !== lane.accountId || lane.listeners.size === 0) {
      return;
    }
    lane.snapshot = Object.freeze(snapshot);
    for (const listener of [...lane.listeners]) safeNotify(listener);
  }
}

function portableBindingKey(runtimeId: string, lease: AgentNativePortableWireLease): string {
  const key = JSON.stringify([
    runtimeId,
    lease.leaseHandle,
    lease.targetHandle,
    lease.hostEpoch,
    lease.connectionGeneration
  ]);
  if (key.length === 0 || key.length > MAX_RUNTIME_KEY_LENGTH) {
    throw new Error("Paired-host Agent lifecycle identity is invalid");
  }
  return key;
}

function safeNotify(listener: () => void): void {
  try {
    listener();
  } catch {
    // A consumer cannot prevent other account-scoped observers from fencing.
  }
}
