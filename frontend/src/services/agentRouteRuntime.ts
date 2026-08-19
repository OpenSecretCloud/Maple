import {
  LOCAL_AGENT_EXECUTION_TARGET,
  agentRuntimeService,
  type AgentRuntimeService
} from "@/services/agentRuntimeService";
import {
  isClosedAgentRemoteReadOnlyClient,
  type AgentRemoteReadOnlyClient
} from "@/services/agentRemoteProviderBridge";
import {
  decodeAgentRemoteCapabilitySnapshot,
  isAgentRemotePersistedTranscriptReady,
  sameAgentRemoteCapabilitySnapshot,
  type AgentRemoteCapabilitySnapshot
} from "@/services/agentRemoteCapabilities";

/**
 * Presentation-only choice supplied by the account-scoped paired-target
 * provider. The route never turns the key or label into execution authority;
 * the provider owns selection and AgentRuntimeService still obtains a verified
 * native execution lease before every remote operation.
 */
export interface AgentPortableTargetChoice {
  readonly key: string;
  readonly label: string;
  readonly description?: string;
  select(): void;
}

export type AgentPortableRuntimeUnavailableReason = "noPairedHost" | "pairingUnavailable";

/**
 * Account-scoped portable-client state. A future pairing provider may publish
 * this only after consulting the authoritative native paired-target registry.
 * Raw route params, local storage, and display labels must never construct it.
 */
export type AgentPortableRuntimeState =
  | {
      readonly accountId: string;
      readonly status: "loading";
    }
  | {
      readonly accountId: string;
      readonly status: "selectionRequired";
      readonly targets: readonly AgentPortableTargetChoice[];
    }
  | {
      readonly accountId: string;
      readonly status: "readOnlyReady";
      readonly client: AgentRemoteReadOnlyClient;
      readonly capabilities: AgentRemoteCapabilitySnapshot;
      /**
       * Opaque provider lifecycle identity. It is a UI/request fence, never an
       * authorization token; native still issues and validates the lease.
       */
      readonly runtimeKey: string;
    }
  | {
      readonly accountId: string;
      readonly status: "unavailable";
      readonly reason: AgentPortableRuntimeUnavailableReason;
    };

export type AgentRouteUnavailableReason =
  | AgentPortableRuntimeUnavailableReason
  | "requiresTauri"
  | "unsupportedTauriClient"
  | "remoteProviderUnavailable"
  | "invalidPortableRuntime";

export type AgentRouteRuntimeState =
  | {
      readonly status: "ready";
      readonly service: AgentRuntimeService;
      readonly runtimeKey: string;
    }
  | {
      readonly status: "readOnlyReady";
      readonly client: AgentRemoteReadOnlyClient;
      readonly capabilities: AgentRemoteCapabilitySnapshot;
      readonly runtimeKey: string;
    }
  | {
      readonly status: "loading";
    }
  | {
      readonly status: "selectionRequired";
      readonly targets: readonly AgentPortableTargetChoice[];
    }
  | {
      readonly status: "unavailable";
      readonly reason: AgentRouteUnavailableReason;
    };

export interface AgentRoutePlatform {
  readonly isTauri: boolean;
  readonly isTauriDesktop: boolean;
  readonly isTauriMobile: boolean;
}

interface ResolveAgentRouteRuntimeOptions {
  readonly accountId: string;
  readonly platform: AgentRoutePlatform;
  readonly portableRuntime: AgentPortableRuntimeState | null;
  readonly localService?: AgentRuntimeService;
}

/** JSON tuple encoding avoids account/target delimiter collisions. */
export function agentRouteProjectionKey(
  accountId: string,
  service: AgentRuntimeService,
  runtimeKey = "embedded"
): string {
  return JSON.stringify([accountId, String(service.target.id), runtimeKey]);
}

export function agentRemoteReadOnlyProjectionKey(
  accountId: string,
  client: AgentRemoteReadOnlyClient,
  runtimeKey: string
): string {
  return JSON.stringify([accountId, client.binding.targetId, runtimeKey, "persisted-transcript"]);
}

/**
 * Keep Desktop on its embedded runtime. A portable Tauri client can use only a
 * remote service supplied by the verified paired-target provider; all missing,
 * stale-account, or local-service states fail closed without a local fallback.
 */
export function resolveAgentRouteRuntime({
  accountId,
  platform,
  portableRuntime,
  localService = agentRuntimeService
}: ResolveAgentRouteRuntimeOptions): AgentRouteRuntimeState {
  if (platform.isTauri && platform.isTauriDesktop && !platform.isTauriMobile) {
    if (
      localService.target.kind !== "local" ||
      localService.target.id !== LOCAL_AGENT_EXECUTION_TARGET.id
    ) {
      return { status: "unavailable", reason: "invalidPortableRuntime" };
    }
    return { status: "ready", service: localService, runtimeKey: "embedded" };
  }

  if (!platform.isTauri) {
    return { status: "unavailable", reason: "requiresTauri" };
  }

  if (!platform.isTauriMobile || platform.isTauriDesktop) {
    return { status: "unavailable", reason: "unsupportedTauriClient" };
  }

  if (!portableRuntime) {
    return { status: "unavailable", reason: "remoteProviderUnavailable" };
  }

  // Do not reveal or reuse another account's paired-host state during an auth
  // transition. The native lease check remains the final authority boundary.
  if (portableRuntime.accountId !== accountId) {
    return { status: "unavailable", reason: "pairingUnavailable" };
  }

  switch (portableRuntime.status) {
    case "loading":
      return { status: "loading" };
    case "selectionRequired":
      if (portableRuntime.targets.length === 0) {
        return { status: "unavailable", reason: "noPairedHost" };
      }
      return { status: "selectionRequired", targets: portableRuntime.targets };
    case "unavailable":
      return { status: "unavailable", reason: portableRuntime.reason };
    case "readOnlyReady": {
      const capabilities = decodeAgentRemoteCapabilitySnapshot(portableRuntime.capabilities);
      if (
        !isClosedAgentRemoteReadOnlyClient(portableRuntime.client) ||
        portableRuntime.client.binding.accountId !== accountId ||
        !isAgentRemotePersistedTranscriptReady(capabilities) ||
        !sameAgentRemoteCapabilitySnapshot(capabilities, portableRuntime.client.capabilities)
      ) {
        return { status: "unavailable", reason: "invalidPortableRuntime" };
      }
      if (!isBoundedRuntimeKey(portableRuntime.runtimeKey)) {
        return { status: "unavailable", reason: "invalidPortableRuntime" };
      }
      return {
        status: "readOnlyReady",
        client: portableRuntime.client,
        capabilities,
        runtimeKey: portableRuntime.runtimeKey
      };
    }
    default:
      return { status: "unavailable", reason: "invalidPortableRuntime" };
  }
}

function isBoundedRuntimeKey(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 256;
}
