import { describe, expect, test } from "bun:test";
import {
  isAgentConnectionsAvailable,
  type AgentConnectionsAvailabilityChecks
} from "./agentConnectionsAvailability";
import { FEATURE_FLAGS } from "./flags";

function availabilityChecks({
  forcedOn = true,
  tauriDesktop = true,
  macOS = false,
  linux = false
}: {
  forcedOn?: boolean;
  tauriDesktop?: boolean;
  macOS?: boolean;
  linux?: boolean;
} = {}): AgentConnectionsAvailabilityChecks {
  return {
    isForcedOn: () => forcedOn,
    isTauriDesktop: () => tauriDesktop,
    isMacOS: () => macOS,
    isLinux: () => linux
  };
}

describe("isAgentConnectionsAvailable", () => {
  test("requires the local agent_connections force flag", () => {
    let requestedFlag = "";
    const checks = availabilityChecks({ macOS: true });
    checks.isForcedOn = (key) => {
      requestedFlag = key;
      return false;
    };

    expect(isAgentConnectionsAvailable(checks)).toBe(false);
    expect(requestedFlag).toBe(FEATURE_FLAGS.AGENT_CONNECTIONS);
  });

  test.each([
    ["macOS Tauri Desktop", true, true, false, true],
    ["Linux Tauri Desktop", true, false, true, true],
    ["Windows Tauri Desktop", true, false, false, false],
    ["macOS outside Tauri Desktop", false, true, false, false],
    ["Linux outside Tauri Desktop", false, false, true, false]
  ])("returns the supported state for %s", (_name, tauriDesktop, macOS, linux, expected) => {
    expect(isAgentConnectionsAvailable(availabilityChecks({ tauriDesktop, macOS, linux }))).toBe(
      expected
    );
  });

  test("fails closed if an availability check throws", () => {
    const checks = availabilityChecks({ macOS: true });
    checks.isTauriDesktop = () => {
      throw new Error("platform unavailable");
    };

    expect(isAgentConnectionsAvailable(checks)).toBe(false);
  });
});
