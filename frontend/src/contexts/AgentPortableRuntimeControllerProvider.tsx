import { useEffect, useMemo, useState, type ReactNode } from "react";
import { AgentPortableRuntimeProvider } from "@/contexts/AgentPortableRuntimeContext";
import type { AgentPortableRuntimeState } from "@/services/agentRouteRuntime";

export interface AgentPortableRuntimeController {
  /** Return the controller's cached immutable snapshot for this exact account. */
  getSnapshot(accountId: string): AgentPortableRuntimeState | null;
  /** Subscribe only to state owned by this exact account. */
  subscribe(accountId: string, listener: () => void): () => void;
}

interface AgentPortableRuntimeControllerProviderProps {
  readonly accountId: string | null;
  readonly controller?: AgentPortableRuntimeController | null;
  readonly children: ReactNode;
}

interface PublishedPortableRuntime {
  readonly accountId: string;
  readonly controller: AgentPortableRuntimeController;
  readonly dependencyIdentity: object;
  readonly snapshot: AgentPortableRuntimeState | null;
}

/**
 * Account-fenced injection point for a future authoritative paired-target
 * controller. Maple does not install one in production yet, so the default is
 * deliberately null and the portable route remains unavailable.
 */
export function AgentPortableRuntimeControllerProvider({
  accountId,
  controller = null,
  children
}: AgentPortableRuntimeControllerProviderProps) {
  const [published, setPublished] = useState<PublishedPortableRuntime | null>(null);
  // A render-stable identity changes for every account/controller transition,
  // including A -> signed out -> the same A/controller. This synchronously
  // fences retained state before the replacement subscription effect runs.
  const dependencyIdentity = useMemo(
    () => Object.freeze({ accountId, controller }),
    [accountId, controller]
  );

  useEffect(() => {
    if (!controller || !accountId) return;
    let active = true;
    const publishSnapshot = () => {
      if (!active) return;
      try {
        const snapshot = controller.getSnapshot(accountId);
        setPublished({
          accountId,
          controller,
          dependencyIdentity,
          snapshot: snapshot?.accountId === accountId ? snapshot : null
        });
      } catch {
        setPublished({ accountId, controller, dependencyIdentity, snapshot: null });
      }
    };

    let unsubscribe: (() => void) | null = null;
    let subscriptionReady = false;
    const onControllerChange = () => {
      // A malformed controller may invoke its listener and then throw instead
      // of returning a revocation handle. Read only after subscribe succeeds.
      if (subscriptionReady) publishSnapshot();
    };
    try {
      unsubscribe = controller.subscribe(accountId, onControllerChange);
      if (typeof unsubscribe !== "function") unsubscribe = null;
    } catch {
      unsubscribe = null;
    }
    // Never publish readiness without a revocation subscription already held.
    if (unsubscribe) {
      subscriptionReady = true;
      publishSnapshot();
    } else setPublished({ accountId, controller, dependencyIdentity, snapshot: null });

    return () => {
      active = false;
      try {
        unsubscribe?.();
      } catch {
        // The account/controller identity check below removes the old snapshot
        // synchronously even when a provider cleanup reports failure.
      }
    };
  }, [accountId, controller, dependencyIdentity]);

  const portableRuntime =
    published?.dependencyIdentity === dependencyIdentity &&
    published.accountId === accountId &&
    published.controller === controller
      ? published.snapshot
      : null;

  return (
    <AgentPortableRuntimeProvider value={portableRuntime}>{children}</AgentPortableRuntimeProvider>
  );
}
