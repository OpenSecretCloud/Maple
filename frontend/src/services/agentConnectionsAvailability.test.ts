import { describe, expect, test } from "bun:test";
import {
  isAgentConnectionsPlatformSupported,
  type AgentConnectionsPlatformChecks
} from "./agentConnectionsAvailability";

function platformChecks({
  tauriDesktop = true,
  macOS = false,
  linux = false
}: {
  tauriDesktop?: boolean;
  macOS?: boolean;
  linux?: boolean;
} = {}): AgentConnectionsPlatformChecks {
  return {
    isTauriDesktop: () => tauriDesktop,
    isMacOS: () => macOS,
    isLinux: () => linux
  };
}

describe("isAgentConnectionsPlatformSupported", () => {
  test.each([
    ["macOS Tauri Desktop", true, true, false, true],
    ["Linux Tauri Desktop", true, false, true, true],
    ["Windows Tauri Desktop", true, false, false, false],
    ["macOS outside Tauri Desktop", false, true, false, false],
    ["Linux outside Tauri Desktop", false, false, true, false]
  ])("returns the supported state for %s", (_name, tauriDesktop, macOS, linux, expected) => {
    expect(
      isAgentConnectionsPlatformSupported(platformChecks({ tauriDesktop, macOS, linux }))
    ).toBe(expected);
  });

  test("fails closed if an availability check throws", () => {
    const checks = platformChecks({ macOS: true });
    checks.isTauriDesktop = () => {
      throw new Error("platform unavailable");
    };

    expect(isAgentConnectionsPlatformSupported(checks)).toBe(false);
  });
});
