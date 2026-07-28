import { invoke } from "@tauri-apps/api/core";
import { isTauriDesktop } from "@/utils/platform";

export interface ProxyConfig {
  host: string;
  port: number;
  api_key: string;
  enabled: boolean;
  enable_cors?: boolean;
  backend_url?: string;
  auto_start?: boolean;
}

export interface ProxyStatus {
  running: boolean;
  config: ProxyConfig;
  error?: string;
}

export type DeleteProxyApiKey = (name: string) => Promise<void>;

export interface ManualProxyKeyProvisioner {
  name: string;
  createApiKey: (name: string) => Promise<string>;
  onApiKeyCreated?: (apiKey: string) => void;
}

export class ProxyAuthenticationChangedError extends Error {
  constructor() {
    super("The authenticated Maple account changed before the local proxy finished starting");
    this.name = "ProxyAuthenticationChangedError";
  }
}

export class ProxyApiKeyCreationError extends Error {
  constructor() {
    super("Failed to create API key. Please create an API key manually first");
    this.name = "ProxyApiKeyCreationError";
  }
}

export interface AgentProxyKeyRecord {
  userId: string;
  name: string;
}

export interface AgentProxyKeyRegistry {
  keys: AgentProxyKeyRecord[];
  activeName?: string;
}

const AGENT_PROXY_OWNER_KEY = "maple-agent-proxy-owner-v1";
const AGENT_PROXY_KEY_REGISTRY_KEY = "maple-agent-proxy-keys-v1";

export function removeAgentProxyKeyRecord(
  registry: AgentProxyKeyRegistry,
  name: string
): AgentProxyKeyRegistry {
  return {
    keys: registry.keys.filter((candidate) => candidate.name !== name),
    activeName: registry.activeName === name ? undefined : registry.activeName
  };
}

export function deactivateAgentProxyKeyRegistry(
  registry: AgentProxyKeyRegistry
): AgentProxyKeyRegistry {
  return { ...registry, activeName: undefined };
}

export function manualProxyConfigsMatch(active: ProxyConfig, desired: ProxyConfig): boolean {
  return (
    active.host.trim().toLowerCase() === desired.host.trim().toLowerCase() &&
    active.port === desired.port &&
    active.api_key.trim() === desired.api_key.trim() &&
    active.enabled === desired.enabled &&
    (active.enable_cors ?? false) === (desired.enable_cors ?? false) &&
    normalizeBackendUrl(active.backend_url) === normalizeBackendUrl(desired.backend_url) &&
    (active.auto_start ?? false) === (desired.auto_start ?? false)
  );
}

type ProxyCommandInvoker = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface ProxyAuthenticationSnapshot {
  userId: string;
  generation: number;
}

export class ProxyService {
  private operationTail: Promise<void> = Promise.resolve();
  private authenticatedUserId: string | null = null;
  private authGeneration = 0;

  constructor(
    private readonly invokeCommand: ProxyCommandInvoker = invoke,
    private readonly desktopCheck: () => boolean = isTauriDesktop
  ) {}

  private validatePort(port: number): void {
    if (!Number.isInteger(port) || port < 0 || port > 65535) {
      throw new Error(`Port must be a valid u16 integer (0-65535), got: ${port}`);
    }
  }

  async startProxy(config: ProxyConfig): Promise<ProxyStatus> {
    try {
      this.validatePort(config.port);
      return await this.invokeCommand<ProxyStatus>("start_proxy", { config });
    } catch (error) {
      console.error("Failed to start proxy:", error);
      throw error;
    }
  }

  async stopProxy(): Promise<ProxyStatus> {
    try {
      return await this.invokeCommand<ProxyStatus>("stop_proxy");
    } catch (error) {
      console.error("Failed to stop proxy:", error);
      throw error;
    }
  }

  async getProxyStatus(): Promise<ProxyStatus> {
    try {
      return await this.invokeCommand<ProxyStatus>("get_proxy_status");
    } catch (error) {
      console.error("Failed to get proxy status:", error);
      throw error;
    }
  }

  async loadProxyConfig(): Promise<ProxyConfig> {
    try {
      return await this.invokeCommand<ProxyConfig>("load_proxy_config");
    } catch (error) {
      console.error("Failed to load proxy config:", error);
      // Return default config if loading fails
      return {
        host: "127.0.0.1",
        port: 8080,
        api_key: "",
        enabled: false,
        enable_cors: false
      };
    }
  }

  async saveProxySettings(config: ProxyConfig): Promise<void> {
    try {
      this.validatePort(config.port);
      await this.invokeCommand("save_proxy_settings", { config });
    } catch (error) {
      console.error("Failed to save proxy settings:", error);
      throw error;
    }
  }

  async testProxyPort(host: string, port: number): Promise<boolean> {
    try {
      this.validatePort(port);
      return await this.invokeCommand<boolean>("test_proxy_port", { host, port });
    } catch (error) {
      console.error("Failed to test proxy port:", error);
      throw error;
    }
  }

  observeAuthenticatedUser(nextUserId: string | null): void {
    const normalizedUserId = nextUserId?.trim() || null;
    if (this.authenticatedUserId === normalizedUserId) return;
    this.authenticatedUserId = normalizedUserId;
    this.authGeneration += 1;
  }

  async startManualProxy(
    userId: string,
    config: ProxyConfig,
    keyProvisioner?: ManualProxyKeyProvisioner
  ): Promise<ProxyStatus> {
    const auth = this.captureAuthentication(userId);
    let desiredConfig = config;

    if (!desiredConfig.api_key.trim()) {
      if (!keyProvisioner) throw new Error("Starting the local proxy requires an API key");

      let apiKey: string;
      try {
        apiKey = await keyProvisioner.createApiKey(keyProvisioner.name);
      } catch {
        throw new ProxyApiKeyCreationError();
      }
      desiredConfig = { ...desiredConfig, api_key: apiKey };
      keyProvisioner.onApiKeyCreated?.(apiKey);
      this.assertAuthenticationCurrent(auth);
    }

    return await this.enqueueProxyOperation(async () => {
      this.assertAuthenticationCurrent(auth);
      let proxyStarted = false;

      try {
        const status = await this.startProxy(desiredConfig);
        proxyStarted = status.running && manualProxyConfigsMatch(status.config, desiredConfig);
        this.assertAuthenticationCurrent(auth);

        if (!proxyStarted) {
          throw new Error(
            "The local proxy changed while the manual setup was starting. Review the current settings and try again."
          );
        }

        // Do not discard the previous Agent ownership association until the
        // native mutation has actually succeeded and this account still owns
        // the operation.
        this.markCurrentProxyConfigAsManual();
        return status;
      } catch (error) {
        if (proxyStarted && error instanceof ProxyAuthenticationChangedError) {
          await this.resetProxyLocalState();
        }
        throw error;
      }
    });
  }

  async saveManualProxySettings(config: ProxyConfig): Promise<void> {
    await this.enqueueProxyOperation(async () => {
      await this.saveProxySettings(config);
      this.markCurrentProxyConfigAsManual();
    });
  }

  async stopManualProxy(): Promise<ProxyStatus> {
    return await this.enqueueProxyOperation(async () => await this.stopProxy());
  }

  private markCurrentProxyConfigAsManual(): void {
    this.clearAgentProxyOwner();
    const registry = this.loadAgentProxyKeyRegistry();
    if (registry.activeName) {
      this.saveAgentProxyKeyRegistry(deactivateAgentProxyKeyRegistry(registry));
    }
  }

  // Stop and scrub local credentials first so an offline backend can never
  // prevent logout. Exact locally-created key records remain queued in local
  // metadata when remote revocation fails and are retried by a later
  // authenticated cleanup for that account.
  async stopAndResetProxy(userId?: string | null, deleteApiKey?: DeleteProxyApiKey): Promise<void> {
    if (!this.desktopCheck()) return;

    // Invalidate an in-flight manual start before waiting for the serialized
    // native reset. This is synchronous so delayed key creation cannot enqueue
    // a stale start ahead of logout cleanup.
    this.authGeneration += 1;

    await this.enqueueProxyOperation(async () => {
      await this.resetProxyLocalState();
      if (userId && deleteApiKey) {
        // Start the authenticated cleanup while the caller still owns its SDK
        // session, but do not await an unbounded encrypted fetch. Successful
        // deletions remove their exact records; failures/timeouts leave records
        // available for the account's next initialization retry.
        void this.revokeTrackedAgentProxyKeysBestEffort(userId, deleteApiKey).catch(() => {});
      }
    });
  }

  private async enqueueProxyOperation<T>(operation: () => Promise<T>): Promise<T> {
    const queued = this.operationTail.then(operation);
    this.operationTail = queued.then(
      () => undefined,
      () => undefined
    );
    return await queued;
  }

  private async resetProxyLocalState(): Promise<void> {
    if (!this.desktopCheck()) return;

    try {
      await this.invokeCommand<ProxyStatus>("stop_and_reset_proxy");
    } catch (error) {
      console.error("Failed to stop and reset proxy:", error);
      throw error;
    }

    // These values contain only ownership/key-name metadata. The native
    // config/keyring scrub above is the credential boundary, so a WebView
    // storage failure here must not report that logout itself failed.
    try {
      this.clearAgentProxyOwner();
      this.clearActiveTrackedKey();
    } catch {
      // A stale legacy association is harmless while no credential remains;
      // a later cleanup can remove the metadata.
    }
  }

  private captureAuthentication(userId: string): ProxyAuthenticationSnapshot {
    const normalizedUserId = userId.trim();
    if (!normalizedUserId || this.authenticatedUserId !== normalizedUserId) {
      throw new ProxyAuthenticationChangedError();
    }
    return { userId: normalizedUserId, generation: this.authGeneration };
  }

  private assertAuthenticationCurrent(auth: ProxyAuthenticationSnapshot): void {
    if (this.authenticatedUserId !== auth.userId || this.authGeneration !== auth.generation) {
      throw new ProxyAuthenticationChangedError();
    }
  }

  private clearAgentProxyOwner(): void {
    if (typeof localStorage === "undefined") return;
    localStorage.removeItem(AGENT_PROXY_OWNER_KEY);
  }

  private async revokeTrackedAgentProxyKeysBestEffort(
    userId: string,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<void> {
    const records = this.loadAgentProxyKeyRegistry().keys.filter(
      (record) => record.userId === userId
    );
    for (const record of records) {
      try {
        await this.revokeTrackedAgentProxyKey(record.name, deleteApiKey);
      } catch {
        // Keep this exact record for retry, but continue so one backend/network
        // failure does not prevent revocation of the account's other keys.
      }
    }
  }

  private async revokeTrackedAgentProxyKey(
    name: string,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<void> {
    try {
      await deleteApiKey(name);
    } catch (error) {
      if (!isMissingApiKeyError(error)) throw error;
    }

    const registry = removeAgentProxyKeyRecord(this.loadAgentProxyKeyRegistry(), name);
    this.saveAgentProxyKeyRegistry(registry);
  }

  private clearActiveTrackedKey(): void {
    const registry = this.loadAgentProxyKeyRegistry();
    if (!registry.activeName) return;
    this.saveAgentProxyKeyRegistry(deactivateAgentProxyKeyRegistry(registry));
  }

  private loadAgentProxyKeyRegistry(): AgentProxyKeyRegistry {
    if (typeof localStorage === "undefined") return { keys: [] };
    const stored = localStorage.getItem(AGENT_PROXY_KEY_REGISTRY_KEY);
    if (!stored) return { keys: [] };

    let parsed: Partial<AgentProxyKeyRegistry>;
    try {
      parsed = JSON.parse(stored) as Partial<AgentProxyKeyRegistry>;
    } catch {
      localStorage.removeItem(AGENT_PROXY_KEY_REGISTRY_KEY);
      return { keys: [] };
    }
    if (!Array.isArray(parsed.keys)) {
      localStorage.removeItem(AGENT_PROXY_KEY_REGISTRY_KEY);
      return { keys: [] };
    }
    const keys = parsed.keys.filter((record): record is AgentProxyKeyRecord =>
      Boolean(
        record &&
        typeof record === "object" &&
        typeof record.userId === "string" &&
        record.userId.trim() &&
        typeof record.name === "string" &&
        record.name.trim()
      )
    );
    const activeName =
      typeof parsed.activeName === "string" &&
      keys.some((record) => record.name === parsed.activeName)
        ? parsed.activeName
        : undefined;
    return { keys, activeName };
  }

  private saveAgentProxyKeyRegistry(registry: AgentProxyKeyRegistry): void {
    if (typeof localStorage === "undefined") {
      throw new Error("Local storage is unavailable for Agent proxy key tracking");
    }
    localStorage.setItem(AGENT_PROXY_KEY_REGISTRY_KEY, JSON.stringify(registry));
  }
}

function normalizeBackendUrl(value?: string): string {
  return (value || "").trim().replace(/\/+$/, "");
}

function isMissingApiKeyError(error: unknown): boolean {
  if (error && typeof error === "object" && "status" in error && error.status === 404) {
    return true;
  }
  const message = error instanceof Error ? error.message : String(error);
  return /\b404\b|not found/i.test(message);
}

export const proxyService = new ProxyService();
