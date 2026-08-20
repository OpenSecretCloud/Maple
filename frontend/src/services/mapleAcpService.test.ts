import { describe, expect, test } from "bun:test";
import {
  BUZZ_DEFAULT_AGENT_PARALLELISM,
  BUZZ_MAPLE_AGENT_PARALLELISM,
  BUZZ_MAPLE_HARNESS_ID,
  BUZZ_MAPLE_HARNESS_NAME,
  DEFAULT_MAPLE_ACP_CONFIG,
  MAX_MAPLE_ACP_CONNECTIONS,
  PASEO_MAPLE_PROVIDER_ID,
  PASEO_MAPLE_PROVIDER_NAME,
  MapleAcpService,
  buildBuzzCustomHarness,
  buildPaseoCustomProviderConfig,
  isMapleAcpConfigReady,
  normalizeMapleAcpConfig,
  normalizeMapleAcpStatus,
  serializeBuzzCustomHarness,
  serializePaseoCustomProviderConfig,
  type MapleAcpBridge,
  type MapleAcpHarness
} from "./mapleAcpService";

class RecordingBridge implements MapleAcpBridge {
  readonly events: string[] = [];
  lastArgs: Record<string, unknown> | undefined;
  result: unknown = undefined;
  desktop = true;

  isDesktop(): boolean {
    return this.desktop;
  }

  async syncAuth(userId: string): Promise<void> {
    this.events.push(`sync:${userId}`);
  }

  async runForUser<T>(userId: string, operation: () => Promise<T>): Promise<T> {
    this.events.push(`fence:${userId}`);
    return await operation();
  }

  async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    this.events.push(`invoke:${command}`);
    this.lastArgs = args;
    return this.result as T;
  }
}

const harness: MapleAcpHarness = {
  command: "/Applications/Maple Desktop.app/Contents/MacOS/maple",
  args: ["acp"]
};

describe("MapleAcpService", () => {
  test("start synchronizes native authentication inside the account fence", async () => {
    const bridge = new RecordingBridge();
    bridge.result = { running: true, harness };
    const service = new MapleAcpService(bridge);

    await service.start("user-a");

    expect(bridge.events).toEqual(["fence:user-a", "sync:user-a", "invoke:agent_acp_start"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a" });
  });

  test("saved service restoration stays authenticated and account-fenced", async () => {
    const bridge = new RecordingBridge();
    bridge.result = { running: true, enabled: true, harness };
    const service = new MapleAcpService(bridge);

    expect(await service.restoreEnabled("user-a")).toMatchObject({
      running: true,
      enabled: true
    });

    expect(bridge.events).toEqual([
      "fence:user-a",
      "sync:user-a",
      "invoke:agent_acp_restore_enabled"
    ]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a" });
  });

  test("stop and polling remain account-fenced without refreshing credentials", async () => {
    const bridge = new RecordingBridge();
    bridge.result = { running: false };
    const service = new MapleAcpService(bridge);

    await service.getStatus("user-a");
    await service.stop("user-a");

    expect(bridge.events).toEqual([
      "fence:user-a",
      "invoke:agent_acp_get_status",
      "fence:user-a",
      "invoke:agent_acp_stop"
    ]);
  });

  test("save sends a normalized, account-scoped config", async () => {
    const bridge = new RecordingBridge();
    bridge.result = {
      enabled: false,
      permissionMode: "read_only",
      allowedProjectRoots: [],
      maxConnections: 1
    };
    const service = new MapleAcpService(bridge);

    await service.saveConfig("user-a", {
      enabled: false,
      permissionMode: "read_only",
      allowedProjectRoots: [" /tmp/project ", "/tmp/project"],
      maxConnections: 1
    });

    expect(bridge.lastArgs).toEqual({
      userId: "user-a",
      config: {
        enabled: false,
        permissionMode: "read_only",
        allowedProjectRoots: ["/tmp/project"],
        maxConnections: 1
      }
    });
  });

  test("rejects every operation outside Maple Desktop", async () => {
    const bridge = new RecordingBridge();
    bridge.desktop = false;
    const service = new MapleAcpService(bridge);

    await expect(service.loadConfig("user-a")).rejects.toThrow("available in Maple Desktop");
    await expect(service.saveConfig("user-a", DEFAULT_MAPLE_ACP_CONFIG)).rejects.toThrow(
      "available in Maple Desktop"
    );
    await expect(service.start("user-a")).rejects.toThrow("available in Maple Desktop");
    await expect(service.restoreEnabled("user-a")).rejects.toThrow("available in Maple Desktop");
    await expect(service.stop("user-a")).rejects.toThrow("available in Maple Desktop");
    await expect(service.getStatus("user-a")).rejects.toThrow("available in Maple Desktop");
    expect(bridge.events).toEqual([]);
  });
});

describe("Maple ACP response normalization", () => {
  test("fails closed on malformed config values", () => {
    expect(
      normalizeMapleAcpConfig({
        enabled: "yes",
        permissionMode: "unattended",
        allowedProjectRoots: ["/tmp/a", 42, " /tmp/a ", "/tmp/b"],
        maxConnections: -3
      })
    ).toEqual({
      enabled: false,
      permissionMode: "read_only",
      allowedProjectRoots: ["/tmp/a", "/tmp/b"],
      maxConnections: 1
    });
  });

  test("keeps explicit connection limits inside the native range", () => {
    expect(normalizeMapleAcpConfig({ maxConnections: 0 }).maxConnections).toBe(1);
    expect(normalizeMapleAcpConfig({ maxConnections: 1 }).maxConnections).toBe(1);
    expect(
      normalizeMapleAcpConfig({ maxConnections: MAX_MAPLE_ACP_CONNECTIONS }).maxConnections
    ).toBe(MAX_MAPLE_ACP_CONNECTIONS);
    expect(normalizeMapleAcpConfig({ maxConnections: 999 }).maxConnections).toBe(
      MAX_MAPLE_ACP_CONNECTIONS
    );
  });

  test("migrates the former Maple-owned allow-all bypass to caller-owned approvals", () => {
    expect(
      normalizeMapleAcpConfig({
        enabled: true,
        permissionMode: "allow_all",
        allowedProjectRoots: [],
        maxConnections: 1
      })
    ).toEqual({
      enabled: true,
      permissionMode: "read_only",
      allowedProjectRoots: [],
      maxConnections: 1
    });
  });

  test("accepts the native status and tolerates optional future fields", () => {
    expect(
      normalizeMapleAcpStatus({
        running: true,
        enabled: true,
        connectedClients: 2,
        activeSessions: 3,
        activeRuns: 1,
        endpointKind: "unix_socket",
        endpoint: { path: "/tmp/maple.sock" },
        protocolVersion: 1,
        lastError: "",
        buzzCredentialsAvailable: true,
        harness,
        ignoredFutureField: "okay"
      })
    ).toEqual({
      running: true,
      enabled: true,
      connectedClients: 2,
      activeSessions: 3,
      activeRuns: 1,
      endpoint: "/tmp/maple.sock",
      endpointKind: "unix_socket",
      protocolVersion: 1,
      error: null,
      buzzCredentialsAvailable: true,
      harness
    });
  });
});

describe("Buzz custom harness output", () => {
  test("matches the separate values expected by the Buzz custom harness form", () => {
    expect(buildBuzzCustomHarness(harness)).toEqual({
      id: BUZZ_MAPLE_HARNESS_ID,
      label: BUZZ_MAPLE_HARNESS_NAME,
      command: harness.command,
      args: ["acp"],
      env: {}
    });

    const serialized = serializeBuzzCustomHarness(harness);
    expect(JSON.parse(serialized)).toEqual(buildBuzzCustomHarness(harness));
    expect(serialized).toContain(harness.command);
    expect(serialized).not.toContain("BUZZ_PRIVATE_KEY");
    expect(serialized).not.toContain("accessToken");
    expect(serialized).not.toContain("endpoint");
  });

  test("keeps Buzz parallelism within Maple's default connection limit", () => {
    expect(BUZZ_MAPLE_AGENT_PARALLELISM).toBe(MAX_MAPLE_ACP_CONNECTIONS);
    expect(BUZZ_DEFAULT_AGENT_PARALLELISM).toBe(10);
    expect(DEFAULT_MAPLE_ACP_CONFIG.maxConnections).toBe(Number(BUZZ_MAPLE_AGENT_PARALLELISM));
  });
});

describe("Paseo custom provider output", () => {
  test("builds a dynamic ACP provider with Paseo tool injection enabled", () => {
    const config = buildPaseoCustomProviderConfig(harness);
    const provider = config.agents.providers[PASEO_MAPLE_PROVIDER_ID];

    expect(provider).toEqual({
      extends: "acp",
      label: PASEO_MAPLE_PROVIDER_NAME,
      description: "Maple's signed-in Agent Mode over local ACP",
      command: [harness.command, "acp"],
      params: {
        supportsMcpServers: true
      }
    });
    expect(provider).not.toHaveProperty("models");

    const serialized = serializePaseoCustomProviderConfig(harness);
    expect(JSON.parse(serialized)).toEqual(config);
    expect(serialized).toContain(harness.command);
    expect(serialized).not.toContain("accessToken");
    expect(serialized).not.toContain("apiKey");
  });
});

describe("Agent connections policy state", () => {
  const savedConfig = {
    enabled: true,
    permissionMode: "allow_all" as const,
    allowedProjectRoots: [],
    maxConnections: 1
  };

  test("keeps mutations locked until the saved config has loaded", () => {
    expect(isMapleAcpConfigReady(null, null)).toBe(false);
    expect(isMapleAcpConfigReady(savedConfig, null)).toBe(false);
    expect(isMapleAcpConfigReady(null, savedConfig)).toBe(false);
    expect(isMapleAcpConfigReady(savedConfig, savedConfig)).toBe(true);
  });
});
