/**
 * Closed presentation grant for the first paired-host transcript browser.
 *
 * This is not a host feature inventory and is never authorization by itself.
 * An authoritative paired-target provider may publish this snapshot only after
 * it has authenticated the exact account and target binding. Keeping mutation
 * and live-tail grants explicit prevents partial support from being mistaken
 * for full remote Agent Mode.
 */
export interface AgentRemoteCapabilitySnapshot {
  readonly runtimeStatus: boolean;
  readonly sessionSummariesPage: boolean;
  readonly persistedRecordsPage: boolean;
  readonly synchronizedLiveTail: boolean;
  readonly mutations: boolean;
}

const AGENT_REMOTE_CAPABILITY_KEYS = [
  "runtimeStatus",
  "sessionSummariesPage",
  "persistedRecordsPage",
  "synchronizedLiveTail",
  "mutations"
] as const;

export const AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES: AgentRemoteCapabilitySnapshot =
  Object.freeze({
    runtimeStatus: true,
    sessionSummariesPage: true,
    persistedRecordsPage: true,
    synchronizedLiveTail: false,
    mutations: false
  });

/** Decode a closed snapshot without retaining provider-owned extension data. */
export function decodeAgentRemoteCapabilitySnapshot(
  value: unknown
): AgentRemoteCapabilitySnapshot | null {
  if (!isRecord(value)) return null;
  if (
    Reflect.ownKeys(value).length !== AGENT_REMOTE_CAPABILITY_KEYS.length ||
    !AGENT_REMOTE_CAPABILITY_KEYS.every(
      (key) => Object.prototype.hasOwnProperty.call(value, key) && typeof value[key] === "boolean"
    )
  ) {
    return null;
  }

  return Object.freeze({
    runtimeStatus: value.runtimeStatus as boolean,
    sessionSummariesPage: value.sessionSummariesPage as boolean,
    persistedRecordsPage: value.persistedRecordsPage as boolean,
    synchronizedLiveTail: value.synchronizedLiveTail as boolean,
    mutations: value.mutations as boolean
  });
}

/**
 * Phase 1 is deliberately persisted-history-only. A live or mutation grant is
 * rejected instead of being silently ignored or routed to full Agent Mode.
 */
export function isAgentRemotePersistedTranscriptReady(
  value: unknown
): value is AgentRemoteCapabilitySnapshot {
  const snapshot = decodeAgentRemoteCapabilitySnapshot(value);
  return (
    snapshot !== null &&
    snapshot.runtimeStatus &&
    snapshot.sessionSummariesPage &&
    snapshot.persistedRecordsPage &&
    !snapshot.synchronizedLiveTail &&
    !snapshot.mutations
  );
}

export function sameAgentRemoteCapabilitySnapshot(
  left: AgentRemoteCapabilitySnapshot,
  right: AgentRemoteCapabilitySnapshot
): boolean {
  return AGENT_REMOTE_CAPABILITY_KEYS.every((key) => left[key] === right[key]);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
