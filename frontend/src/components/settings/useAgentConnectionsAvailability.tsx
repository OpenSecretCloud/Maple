import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { isAgentConnectionsPlatformSupported } from "@/services/agentConnectionsAvailability";
import { FEATURE_FLAGS, flagsClient } from "@/services/flags";

export type AgentConnectionsAvailability = "checking" | "available" | "unavailable";

export interface AgentConnectionsFlagClient {
  isEnabled: (userId: string, key: string) => Promise<boolean>;
  peekIsEnabled: (userId: string, key: string) => boolean | undefined;
}

export interface AgentConnectionsAvailabilityDependencies {
  flagClient: AgentConnectionsFlagClient;
  isPlatformSupported: () => boolean;
}

const defaultDependencies: AgentConnectionsAvailabilityDependencies = {
  flagClient: flagsClient,
  isPlatformSupported: isAgentConnectionsPlatformSupported
};

const AgentConnectionsAvailabilityContext =
  createContext<AgentConnectionsAvailability>("unavailable");

type ResolvedAvailability = {
  available: boolean;
  lookupToken: object;
};

function useResolvedAgentConnectionsAvailability(
  userId: string | null,
  dependencies: AgentConnectionsAvailabilityDependencies
): AgentConnectionsAvailability {
  const { flagClient, isPlatformSupported } = dependencies;
  const platformSupported = isPlatformSupported();
  const lookupToken = useMemo(
    () => ({ flagClient, platformSupported, userId }),
    [flagClient, platformSupported, userId]
  );
  const cachedAvailability =
    platformSupported && userId
      ? flagClient.peekIsEnabled(userId, FEATURE_FLAGS.AGENT_CONNECTIONS)
      : undefined;
  const [resolvedAvailability, setResolvedAvailability] = useState<ResolvedAvailability | null>(
    null
  );

  useEffect(() => {
    if (!platformSupported || !userId) return;

    let disposed = false;
    void flagClient.isEnabled(userId, FEATURE_FLAGS.AGENT_CONNECTIONS).then(
      (available) => {
        if (!disposed) setResolvedAvailability({ available, lookupToken });
      },
      (error: unknown) => {
        console.warn(
          "Unable to load the Agent connections feature flag; keeping it hidden.",
          error
        );
        if (!disposed) setResolvedAvailability({ available: false, lookupToken });
      }
    );

    return () => {
      disposed = true;
    };
  }, [flagClient, lookupToken, platformSupported, userId]);

  if (!platformSupported || !userId) return "unavailable";

  const available =
    resolvedAvailability?.lookupToken === lookupToken
      ? resolvedAvailability.available
      : cachedAvailability;

  if (available === undefined) return "checking";
  return available ? "available" : "unavailable";
}

/**
 * Resolves the optional Agent connections surface once for the settings tree.
 * FlagsClient applies the local override before its user-scoped remote lookup.
 */
export function AgentConnectionsAvailabilityProvider({
  children,
  dependencies = defaultDependencies,
  userId
}: {
  children: ReactNode;
  dependencies?: AgentConnectionsAvailabilityDependencies;
  userId: string | null;
}) {
  const availability = useResolvedAgentConnectionsAvailability(userId, dependencies);
  return (
    <AgentConnectionsAvailabilityContext.Provider value={availability}>
      {children}
    </AgentConnectionsAvailabilityContext.Provider>
  );
}

// The consumer hook and provider intentionally share this module-private context.
// eslint-disable-next-line react-refresh/only-export-components
export function useAgentConnectionsAvailability(): AgentConnectionsAvailability {
  return useContext(AgentConnectionsAvailabilityContext);
}
