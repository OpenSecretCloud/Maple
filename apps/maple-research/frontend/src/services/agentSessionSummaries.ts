import type { AgentSessionSummary } from "./agentRuntimeService";

/**
 * Apply an authoritative session-list snapshot without letting a request that
 * started before a reactive metadata event overwrite that newer event.
 */
export function reconcileAgentSessionSnapshot(
  snapshot: readonly AgentSessionSummary[],
  current: readonly AgentSessionSummary[],
  changedAfterRequest: ReadonlySet<string>,
  deletedSessionIds: ReadonlySet<string>
): AgentSessionSummary[] {
  const currentById = new Map(current.map((session) => [session.id, session]));
  const snapshotIds = new Set(snapshot.map((session) => session.id));
  const newlyObserved = current.filter(
    (session) =>
      changedAfterRequest.has(session.id) &&
      !snapshotIds.has(session.id) &&
      !deletedSessionIds.has(session.id)
  );

  return [
    ...newlyObserved,
    ...snapshot
      .filter((session) => !deletedSessionIds.has(session.id))
      .map((session) => {
        if (!changedAfterRequest.has(session.id)) return session;
        return currentById.get(session.id) ?? session;
      })
  ];
}
