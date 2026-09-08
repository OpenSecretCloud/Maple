import { agentOperationFence } from "@/services/agentOperationFence";
import { mapleApiAuthService } from "@/services/mapleApiAuthService";
import { isTauriDesktop } from "@/utils/platform";

export type MapleAcpPermissionMode = "read_only" | "allow_all";

export interface MapleAcpConfig {
  enabled: boolean;
  permissionMode: MapleAcpPermissionMode;
  allowedProjectRoots: string[];
  maxConnections: number;
}

export interface MapleAcpHarness {
  command: string;
  args: string[];
}

export interface MapleAcpStatus {
  running: boolean;
  enabled: boolean;
  connectedClients: number;
  activeSessions: number;
  activeRuns: number;
  endpoint: string | null;
  endpointKind: string | null;
  protocolVersion: string | number | null;
  error: string | null;
  buzzCredentialsAvailable: boolean;
  harness: MapleAcpHarness | null;
}

export interface BuzzCustomHarnessDefinition {
  id: "maple";
  label: "Maple";
  command: string;
  args: string[];
  env: Record<string, never>;
}

export interface PaseoCustomProviderConfig {
  agents: {
    providers: {
      "maple-acp": {
        extends: "acp";
        label: "Maple Agent";
        description: string;
        command: string[];
        params: {
          supportsMcpServers: true;
        };
      };
    };
  };
}

export const BUZZ_MAPLE_HARNESS_ID = "maple" as const;
export const BUZZ_MAPLE_HARNESS_NAME = "Maple" as const;
export const PASEO_MAPLE_PROVIDER_ID = "maple-acp" as const;
export const PASEO_MAPLE_PROVIDER_NAME = "Maple Agent" as const;
export const MAX_MAPLE_ACP_CONNECTIONS = 8 as const;
export const BUZZ_MAPLE_AGENT_PARALLELISM = MAX_MAPLE_ACP_CONNECTIONS;
export const BUZZ_DEFAULT_AGENT_PARALLELISM = 10 as const;

export interface MapleAcpBridge {
  isDesktop(): boolean;
  syncAuth(userId: string): Promise<void>;
  runForUser<T>(userId: string, operation: () => Promise<T>): Promise<T>;
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

export const DEFAULT_MAPLE_ACP_CONFIG: MapleAcpConfig = Object.freeze({
  enabled: false,
  permissionMode: "read_only",
  allowedProjectRoots: [],
  maxConnections: MAX_MAPLE_ACP_CONNECTIONS
});

const defaultBridge: MapleAcpBridge = {
  isDesktop: isTauriDesktop,
  syncAuth: async (userId) => await mapleApiAuthService.sync(userId),
  runForUser: async (userId, operation) => await agentOperationFence.run(userId, operation),
  invoke: invokeMapleAcp
};

export class MapleAcpService {
  constructor(private readonly bridge: MapleAcpBridge = defaultBridge) {}

  async loadConfig(userId: string): Promise<MapleAcpConfig> {
    const raw = await this.invokeAuthenticated<unknown>(userId, "agent_acp_load_config");
    return normalizeMapleAcpConfig(raw);
  }

  async saveConfig(userId: string, config: MapleAcpConfig): Promise<MapleAcpConfig> {
    const normalized = normalizeMapleAcpConfig(config);
    const raw = await this.invokeAuthenticated<unknown>(userId, "agent_acp_save_config", {
      config: normalized
    });
    return normalizeMapleAcpConfig(raw);
  }

  async start(userId: string): Promise<MapleAcpStatus> {
    const raw = await this.invokeAuthenticated<unknown>(userId, "agent_acp_start");
    return normalizeMapleAcpStatus(raw);
  }

  async restoreEnabled(userId: string): Promise<MapleAcpStatus> {
    const raw = await this.invokeAuthenticated<unknown>(userId, "agent_acp_restore_enabled");
    return normalizeMapleAcpStatus(raw);
  }

  async stop(userId: string): Promise<MapleAcpStatus> {
    // Stopping is a local control-plane operation. It must remain available
    // when credentials are expired or are being cleared during logout.
    const raw = await this.invokeLocal<unknown>(userId, "agent_acp_stop");
    return normalizeMapleAcpStatus(raw);
  }

  async getStatus(userId: string): Promise<MapleAcpStatus> {
    // Polling status should neither refresh credentials nor turn a local
    // diagnostics page into recurring authenticated network work.
    const raw = await this.invokeLocal<unknown>(userId, "agent_acp_get_status");
    return normalizeMapleAcpStatus(raw);
  }

  private async invokeAuthenticated<T>(
    userId: string,
    command: string,
    args?: Record<string, unknown>
  ): Promise<T> {
    this.requireDesktop();
    return await this.bridge.runForUser(userId, async () => {
      await this.bridge.syncAuth(userId);
      return await this.bridge.invoke<T>(command, { userId, ...args });
    });
  }

  private async invokeLocal<T>(
    userId: string,
    command: string,
    args?: Record<string, unknown>
  ): Promise<T> {
    this.requireDesktop();
    return await this.bridge.runForUser(userId, async () => {
      return await this.bridge.invoke<T>(command, { userId, ...args });
    });
  }

  private requireDesktop(): void {
    if (!this.bridge.isDesktop()) {
      throw new Error("Local ACP connections are available in Maple Desktop.");
    }
  }
}

export function normalizeMapleAcpConfig(value: unknown): MapleAcpConfig {
  const record = asRecord(value);
  const roots = Array.isArray(record?.allowedProjectRoots)
    ? record.allowedProjectRoots.filter(
        (root): root is string => typeof root === "string" && root.trim().length > 0
      )
    : DEFAULT_MAPLE_ACP_CONFIG.allowedProjectRoots;
  const maxConnections =
    typeof record?.maxConnections === "number" && Number.isSafeInteger(record.maxConnections)
      ? record.maxConnections
      : DEFAULT_MAPLE_ACP_CONFIG.maxConnections;

  return {
    enabled:
      typeof record?.enabled === "boolean" ? record.enabled : DEFAULT_MAPLE_ACP_CONFIG.enabled,
    permissionMode: normalizePermissionMode(record?.permissionMode),
    allowedProjectRoots: [...new Set(roots.map((root) => root.trim()))],
    maxConnections: Math.min(MAX_MAPLE_ACP_CONNECTIONS, Math.max(1, maxConnections))
  };
}

export function normalizeMapleAcpStatus(value: unknown): MapleAcpStatus {
  const record = asRecord(value);
  const harnessRecord = asRecord(record?.harness);
  const command = firstString(harnessRecord?.command, record?.executable, record?.executablePath);
  const rawArgs = Array.isArray(harnessRecord?.args)
    ? harnessRecord.args
    : Array.isArray(record?.args)
      ? record.args
      : [];
  const args = rawArgs.filter((argument): argument is string => typeof argument === "string");
  const error = firstString(record?.error, record?.lastError);

  return {
    running: record?.running === true,
    enabled: record?.enabled === true,
    connectedClients: safeCount(record?.connectedClients),
    activeSessions: safeCount(record?.activeSessions),
    activeRuns: safeCount(record?.activeRuns),
    endpoint: normalizeEndpoint(record?.endpoint),
    endpointKind: firstString(record?.endpointKind),
    protocolVersion: normalizeProtocolVersion(record?.protocolVersion),
    error: error || null,
    buzzCredentialsAvailable: record?.buzzCredentialsAvailable === true,
    harness: command ? { command, args } : null
  };
}

export function buildBuzzCustomHarness(harness: MapleAcpHarness): BuzzCustomHarnessDefinition {
  return {
    id: BUZZ_MAPLE_HARNESS_ID,
    label: BUZZ_MAPLE_HARNESS_NAME,
    command: harness.command,
    args: [...harness.args],
    env: {}
  };
}

export function serializeBuzzCustomHarness(harness: MapleAcpHarness): string {
  return JSON.stringify(buildBuzzCustomHarness(harness), null, 2);
}

export function buildPaseoCustomProviderConfig(
  harness: MapleAcpHarness
): PaseoCustomProviderConfig {
  return {
    agents: {
      providers: {
        [PASEO_MAPLE_PROVIDER_ID]: {
          extends: "acp",
          label: PASEO_MAPLE_PROVIDER_NAME,
          description: "Maple's signed-in Agent Mode over local ACP",
          command: [harness.command, ...harness.args],
          params: {
            supportsMcpServers: true
          }
        }
      }
    }
  };
}

export function serializePaseoCustomProviderConfig(harness: MapleAcpHarness): string {
  return JSON.stringify(buildPaseoCustomProviderConfig(harness), null, 2);
}

export function isMapleAcpConfigReady(
  config: MapleAcpConfig | null,
  savedConfig: MapleAcpConfig | null
): boolean {
  return config !== null && savedConfig !== null;
}

async function invokeMapleAcp<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriDesktop()) {
    throw new Error("Local ACP connections are available in Maple Desktop.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function normalizePermissionMode(value: unknown): MapleAcpPermissionMode {
  // `allow_all` was an exploratory Maple-owned bypass. Preserve the wire type
  // while older native builds exist, but migrate every UI policy to caller-owned
  // ACP approval routing.
  void value;
  return "read_only";
}

function safeCount(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : fallback;
}

function firstString(...values: unknown[]): string | null {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return null;
}

function normalizeEndpoint(value: unknown): string | null {
  if (typeof value === "string" && value.trim()) return value.trim();
  const record = asRecord(value);
  if (!record) return null;
  return firstString(record.display, record.path, record.address);
}

function normalizeProtocolVersion(value: unknown): string | number | null {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (typeof value === "number" && Number.isFinite(value)) return value;
  return null;
}

export const mapleAcpService = new MapleAcpService();
