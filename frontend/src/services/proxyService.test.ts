import { describe, expect, it } from "bun:test";

import {
  deactivateAgentProxyKeyRegistry,
  manualProxyConfigsMatch,
  removeAgentProxyKeyRecord,
  type AgentProxyKeyRegistry,
  type ProxyConfig
} from "./proxyService";

const desiredConfig: ProxyConfig = {
  host: "127.0.0.1",
  port: 37615,
  api_key: "workspace-key",
  enabled: true,
  enable_cors: true,
  backend_url: "http://127.0.0.1:31938",
  auto_start: false
};

describe("manualProxyConfigsMatch", () => {
  it("requires the native process to be running with the requested durable config", () => {
    expect(
      manualProxyConfigsMatch(
        {
          ...desiredConfig,
          host: "127.0.0.1",
          api_key: " workspace-key ",
          backend_url: "http://127.0.0.1:31938/"
        },
        desiredConfig
      )
    ).toBe(true);
    expect(manualProxyConfigsMatch({ ...desiredConfig, auto_start: true }, desiredConfig)).toBe(
      false
    );
    expect(manualProxyConfigsMatch({ ...desiredConfig, port: 8080 }, desiredConfig)).toBe(false);
    expect(
      manualProxyConfigsMatch({ ...desiredConfig, api_key: "another-key" }, desiredConfig)
    ).toBe(false);
    expect(
      manualProxyConfigsMatch(
        { ...desiredConfig, backend_url: "https://enclave.trymaple.ai" },
        desiredConfig
      )
    ).toBe(false);
  });
});

describe("Agent proxy key registry", () => {
  it("removes only the exact revoked key and preserves other devices/accounts", () => {
    const registry: AgentProxyKeyRegistry = {
      keys: [
        { userId: "user-a", name: "maple-agent-local" },
        { userId: "user-a", name: "maple-agent-other-device" },
        { userId: "user-b", name: "maple-agent-user-b" }
      ],
      activeName: "maple-agent-local"
    };

    expect(removeAgentProxyKeyRecord(registry, "maple-agent-local")).toEqual({
      keys: [
        { userId: "user-a", name: "maple-agent-other-device" },
        { userId: "user-b", name: "maple-agent-user-b" }
      ],
      activeName: undefined
    });
  });

  it("detaches a manual proxy config without forgetting the tracked key", () => {
    const registry: AgentProxyKeyRegistry = {
      keys: [{ userId: "user-a", name: "maple-agent-local" }],
      activeName: "maple-agent-local"
    };

    expect(deactivateAgentProxyKeyRegistry(registry)).toEqual({
      keys: [{ userId: "user-a", name: "maple-agent-local" }],
      activeName: undefined
    });
  });
});
