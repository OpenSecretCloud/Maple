import { FEATURE_FLAGS, isForcedOn } from "@/services/flags";
import { isLinux, isMacOS, isTauriDesktop } from "@/utils/platform";

export interface AgentConnectionsAvailabilityChecks {
  isForcedOn: (key: string) => boolean;
  isTauriDesktop: () => boolean;
  isMacOS: () => boolean;
  isLinux: () => boolean;
}

const defaultChecks: AgentConnectionsAvailabilityChecks = {
  isForcedOn,
  isTauriDesktop,
  isMacOS,
  isLinux
};

/**
 * Agent connections is a local desktop preview, not a remotely enabled feature.
 * Keep this synchronous so navigation and direct-route admission share one gate.
 */
export function isAgentConnectionsAvailable(
  checks: AgentConnectionsAvailabilityChecks = defaultChecks
): boolean {
  try {
    return (
      checks.isForcedOn(FEATURE_FLAGS.AGENT_CONNECTIONS) &&
      checks.isTauriDesktop() &&
      (checks.isMacOS() || checks.isLinux())
    );
  } catch {
    return false;
  }
}
