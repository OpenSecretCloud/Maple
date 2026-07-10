import { describe, expect, it } from "bun:test";

import {
  addAgentProxyKeyRecord,
  agentProxyConfigsMatch,
  deactivateAgentProxyKeyRegistry,
  enforceAgentProxySecurity,
  manualProxyConfigsMatch,
  removeAgentProxyKeyRecord,
  shouldBlockOnOwnerlessProxy,
  shouldResetAgentProxyOwner,
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

describe("agentProxyConfigsMatch", () => {
  it("accepts the same effective running configuration", () => {
    expect(
      agentProxyConfigsMatch(
        {
          ...desiredConfig,
          host: "127.0.0.1",
          api_key: " workspace-key ",
          backend_url: "http://127.0.0.1:31938/"
        },
        desiredConfig
      )
    ).toBe(true);
  });

  it.each([
    ["host", { host: "0.0.0.0" }],
    ["port", { port: 8080 }],
    ["API key", { api_key: "another-key" }],
    ["backend", { backend_url: "https://enclave.trymaple.ai" }],
    ["enabled state", { enabled: false }],
    ["CORS behavior", { enable_cors: false }]
  ])("rejects a mismatched %s", (_label, override) => {
    expect(agentProxyConfigsMatch({ ...desiredConfig, ...override }, desiredConfig)).toBe(false);
  });

  it("does not restart solely for an auto-start preference change", () => {
    expect(agentProxyConfigsMatch({ ...desiredConfig, auto_start: true }, desiredConfig)).toBe(
      true
    );
  });
});

describe("enforceAgentProxySecurity", () => {
  it("forces loopback binding and disables pre-auth auto-start", () => {
    expect(
      enforceAgentProxySecurity({
        ...desiredConfig,
        host: "0.0.0.0",
        auto_start: true
      })
    ).toEqual({
      ...desiredConfig,
      host: "127.0.0.1",
      auto_start: false
    });
  });
});

describe("manualProxyConfigsMatch", () => {
  it("requires the native process to be running with the requested durable config", () => {
    expect(manualProxyConfigsMatch(desiredConfig, desiredConfig)).toBe(true);
    expect(manualProxyConfigsMatch({ ...desiredConfig, auto_start: true }, desiredConfig)).toBe(
      false
    );
    expect(manualProxyConfigsMatch({ ...desiredConfig, port: 8080 }, desiredConfig)).toBe(false);
  });
});

describe("shouldResetAgentProxyOwner", () => {
  it("keeps a proxy owned by the authenticated account", () => {
    expect(shouldResetAgentProxyOwner("user-a", "user-a", true)).toBe(false);
  });

  it("forces a reset when another account owned the proxy", () => {
    expect(shouldResetAgentProxyOwner("user-a", "user-b", true)).toBe(true);
  });

  it("does not silently destroy ownerless manual proxy state", () => {
    expect(shouldResetAgentProxyOwner(null, "user-a", true)).toBe(false);
  });

  it("allows a clean ownerless proxy to be initialized without another reset", () => {
    expect(shouldResetAgentProxyOwner(null, "user-a", false)).toBe(false);
  });
});

describe("shouldBlockOnOwnerlessProxy", () => {
  it("blocks an unverified saved manual credential", () => {
    expect(shouldBlockOnOwnerlessProxy(null, null, true)).toBe(true);
  });

  it("recognizes a locally tracked Agent credential after an interrupted setup", () => {
    expect(shouldBlockOnOwnerlessProxy(null, "user-a", true)).toBe(false);
  });

  it("does not block a clean proxy config", () => {
    expect(shouldBlockOnOwnerlessProxy(null, null, false)).toBe(false);
  });
});

describe("Agent proxy key registry", () => {
  it("tracks the exact locally created key as active", () => {
    const registry = addAgentProxyKeyRecord(
      { keys: [] },
      { userId: "user-a", name: "maple-agent-a" }
    );

    expect(registry).toEqual({
      keys: [{ userId: "user-a", name: "maple-agent-a" }],
      activeName: "maple-agent-a"
    });
  });

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
