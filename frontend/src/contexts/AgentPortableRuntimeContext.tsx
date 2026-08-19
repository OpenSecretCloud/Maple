import { createContext, useContext, type ReactNode } from "react";
import type { AgentPortableRuntimeState } from "@/services/agentRouteRuntime";

const AgentPortableRuntimeContext = createContext<AgentPortableRuntimeState | null>(null);

/**
 * Injection seam for the future authoritative paired-target registry. Keeping
 * it separate from the route prevents URL or preference state from becoming a
 * target-selection authority.
 */
export function AgentPortableRuntimeProvider({
  value,
  children
}: {
  value: AgentPortableRuntimeState | null;
  children: ReactNode;
}) {
  return (
    <AgentPortableRuntimeContext.Provider value={value}>
      {children}
    </AgentPortableRuntimeContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useAgentPortableRuntime(): AgentPortableRuntimeState | null {
  return useContext(AgentPortableRuntimeContext);
}
