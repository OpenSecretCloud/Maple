import { isLinux, isMacOS, isTauriDesktop } from "@/utils/platform";

export interface AgentConnectionsPlatformChecks {
  isTauriDesktop: () => boolean;
  isMacOS: () => boolean;
  isLinux: () => boolean;
}

const defaultChecks: AgentConnectionsPlatformChecks = {
  isTauriDesktop,
  isMacOS,
  isLinux
};

/**
 * ACP hosting is currently supported only by the macOS and Linux desktop app.
 * Use Maple's initialized platform helpers before consulting feature flags.
 */
export function isAgentConnectionsPlatformSupported(
  checks: AgentConnectionsPlatformChecks = defaultChecks
): boolean {
  try {
    return checks.isTauriDesktop() && (checks.isMacOS() || checks.isLinux());
  } catch {
    return false;
  }
}
