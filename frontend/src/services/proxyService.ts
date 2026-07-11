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

export type CreateProxyApiKey = (name: string) => Promise<string>;
export type DeleteProxyApiKey = (name: string) => Promise<void>;

export class AgentProxyManualConfigConflictError extends Error {
  constructor() {
    super(
      "A saved Local OpenAI Proxy credential must be explicitly replaced before Agent Mode can use this proxy"
    );
    this.name = "AgentProxyManualConfigConflictError";
  }
}

export class AgentProxyReplacementSetupError extends Error {
  constructor(cause: unknown) {
    super(
      `The saved local proxy setup was replaced, but Agent Mode could not finish configuring its proxy: ${errorMessage(cause)}`
    );
    this.name = "AgentProxyReplacementSetupError";
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
const MAX_PROXY_RECONCILE_ATTEMPTS = 3;

export function shouldResetAgentProxyOwner(
  storedOwner: string | null,
  userId: string,
  hasExistingProxyState: boolean
): boolean {
  return hasExistingProxyState && storedOwner !== null && storedOwner !== userId;
}

export function shouldBlockOnOwnerlessProxy(
  storedOwner: string | null,
  trackedOwner: string | null,
  hasExistingProxyState: boolean
): boolean {
  return hasExistingProxyState && storedOwner === null && trackedOwner === null;
}

export function addAgentProxyKeyRecord(
  registry: AgentProxyKeyRegistry,
  record: AgentProxyKeyRecord
): AgentProxyKeyRegistry {
  return {
    keys: [...registry.keys.filter((candidate) => candidate.name !== record.name), record],
    activeName: record.name
  };
}

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

export function agentProxyConfigsMatch(active: ProxyConfig, desired: ProxyConfig): boolean {
  return (
    active.host.trim().toLowerCase() === desired.host.trim().toLowerCase() &&
    active.port === desired.port &&
    active.api_key.trim() === desired.api_key.trim() &&
    active.enabled === desired.enabled &&
    (active.enable_cors ?? true) === (desired.enable_cors ?? true) &&
    normalizeBackendUrl(active.backend_url) === normalizeBackendUrl(desired.backend_url)
  );
}

export function manualProxyConfigsMatch(active: ProxyConfig, desired: ProxyConfig): boolean {
  return (
    agentProxyConfigsMatch(active, desired) &&
    (active.auto_start ?? false) === (desired.auto_start ?? false)
  );
}

export function enforceAgentProxySecurity(config: ProxyConfig): ProxyConfig {
  return {
    ...config,
    // Agent credentials are account-backed and the local proxy has no inbound
    // client authentication. Never inherit a manual LAN bind.
    host: "127.0.0.1",
    // Authentication/owner reconciliation happens after app startup. An Agent
    // credential must never start before that boundary runs.
    auto_start: false
  };
}

class ProxyService {
  private ensureReadyTail: Promise<void> = Promise.resolve();

  private validatePort(port: number): void {
    if (!Number.isInteger(port) || port < 0 || port > 65535) {
      throw new Error(`Port must be a valid u16 integer (0-65535), got: ${port}`);
    }
  }

  private validateAgentPort(port: number): void {
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error(`Port must be a valid TCP port (1-65535), got: ${port}`);
    }
  }

  async startProxy(config: ProxyConfig): Promise<ProxyStatus> {
    try {
      this.validatePort(config.port);
      return await invoke<ProxyStatus>("start_proxy", { config });
    } catch (error) {
      console.error("Failed to start proxy:", error);
      throw error;
    }
  }

  async stopProxy(): Promise<ProxyStatus> {
    try {
      return await invoke<ProxyStatus>("stop_proxy");
    } catch (error) {
      console.error("Failed to stop proxy:", error);
      throw error;
    }
  }

  async getProxyStatus(): Promise<ProxyStatus> {
    try {
      return await invoke<ProxyStatus>("get_proxy_status");
    } catch (error) {
      console.error("Failed to get proxy status:", error);
      throw error;
    }
  }

  async loadProxyConfig(): Promise<ProxyConfig> {
    try {
      return await invoke<ProxyConfig>("load_proxy_config");
    } catch (error) {
      console.error("Failed to load proxy config:", error);
      // Return default config if loading fails
      return {
        host: "127.0.0.1",
        port: 8080,
        api_key: "",
        enabled: false
      };
    }
  }

  async saveProxySettings(config: ProxyConfig): Promise<void> {
    try {
      this.validatePort(config.port);
      await invoke("save_proxy_settings", { config });
    } catch (error) {
      console.error("Failed to save proxy settings:", error);
      throw error;
    }
  }

  async ensureProxyReady(
    userId: string,
    createApiKey: CreateProxyApiKey,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<ProxyStatus> {
    if (!userId.trim()) throw new Error("Agent proxy setup requires an authenticated user");

    return await this.enqueueProxyOperation(async () => {
      return await this.ensureProxyReadyInner(userId, createApiKey, deleteApiKey);
    });
  }

  private async ensureProxyReadyInner(
    userId: string,
    createApiKey: CreateProxyApiKey,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<ProxyStatus> {
    let status = await this.getProxyStatus();
    // Saved settings are the source of truth even while a process is running:
    // they may come from a different managed workspace or have been changed
    // since that process started.
    let savedConfig = await this.loadAgentProxyConfig();
    const storedOwner = this.loadAgentProxyOwner();
    const activeTrackedKey = this.loadActiveTrackedKey();
    const trackedOwner = savedConfig.api_key.trim() ? activeTrackedKey?.userId || null : null;
    const hasExistingProxyState = status.running || Boolean(savedConfig.api_key.trim());
    if (shouldBlockOnOwnerlessProxy(storedOwner, trackedOwner, hasExistingProxyState)) {
      throw new AgentProxyManualConfigConflictError();
    }

    if (shouldResetAgentProxyOwner(storedOwner ?? trackedOwner, userId, hasExistingProxyState)) {
      await this.resetProxyLocalState();
      status = await this.getProxyStatus();
      savedConfig = await this.loadAgentProxyConfig();
    }

    const reusableTrackedKey =
      savedConfig.api_key.trim() && activeTrackedKey?.userId === userId
        ? activeTrackedKey.name
        : undefined;
    await this.revokeTrackedAgentProxyKeys(userId, deleteApiKey, reusableTrackedKey);

    let apiKey = savedConfig.api_key.trim();
    let newlyCreatedKeyName: string | null = null;

    if (!apiKey) {
      let created: { key: string; name: string };
      try {
        created = await this.createTrackedAgentProxyKey(userId, createApiKey, deleteApiKey);
      } catch (error) {
        await this.resetProxyLocalState();
        throw error;
      }
      apiKey = created.key;
      newlyCreatedKeyName = created.name;
    }

    let nextConfig: ProxyConfig;
    let readyStatus: ProxyStatus;
    try {
      nextConfig = this.buildAgentProxyConfig(savedConfig, apiKey);
      if (!status.running && (!savedConfig.api_key.trim() || savedConfig.auto_start !== false)) {
        await this.saveProxySettings({ ...nextConfig, enabled: false });
      } else if (savedConfig.auto_start !== false) {
        // A previously manual proxy may have auto-started with this credential.
        // Persist the Agent-safe preference even when the already-running
        // process otherwise matches and does not need a restart.
        await this.saveProxySettings(nextConfig);
      }
      readyStatus = await this.reconcileRunningProxy(status, nextConfig);
    } catch (error) {
      if (newlyCreatedKeyName) {
        await this.revokeTrackedAgentProxyKey(newlyCreatedKeyName, deleteApiKey);
        await this.resetProxyLocalState();
      }
      throw error;
    }

    if ((await this.checkProxyBackendAuth(readyStatus)) === "auth_error") {
      const trackedKey = this.loadActiveTrackedKey();
      if (trackedKey?.userId === userId) {
        await this.revokeTrackedAgentProxyKey(trackedKey.name, deleteApiKey);
      }

      let created: { key: string; name: string };
      try {
        created = await this.createTrackedAgentProxyKey(userId, createApiKey, deleteApiKey);
      } catch (error) {
        await this.resetProxyLocalState();
        throw error;
      }
      apiKey = created.key;
      newlyCreatedKeyName = created.name;
      nextConfig = this.buildAgentProxyConfig(readyStatus.config, apiKey);
      try {
        readyStatus = await this.reconcileRunningProxy(readyStatus, nextConfig);
      } catch (error) {
        await this.revokeTrackedAgentProxyKey(created.name, deleteApiKey);
        await this.resetProxyLocalState();
        throw error;
      }

      if ((await this.checkProxyBackendAuth(readyStatus)) === "auth_error") {
        await this.revokeTrackedAgentProxyKey(created.name, deleteApiKey);
        await this.resetProxyLocalState();
        throw new Error("Maple proxy API key was refreshed but the backend still returned 401");
      }
    }

    this.saveAgentProxyOwner(userId);
    return readyStatus;
  }

  private async loadAgentProxyConfig(): Promise<ProxyConfig> {
    try {
      return await invoke<ProxyConfig>("load_proxy_config");
    } catch (error) {
      console.error("Failed to load Agent proxy config:", error);
      throw error;
    }
  }

  private async reconcileRunningProxy(
    initialStatus: ProxyStatus,
    desiredConfig: ProxyConfig
  ): Promise<ProxyStatus> {
    let status = initialStatus;

    for (let attempt = 0; attempt < MAX_PROXY_RECONCILE_ATTEMPTS; attempt += 1) {
      if (status.running && agentProxyConfigsMatch(status.config, desiredConfig)) {
        return status;
      }
      if (status.running) {
        await this.stopProxy();
      }

      // start_proxy is idempotent. If a delayed auto-start wins this race, it
      // can return a different running config; the next bounded iteration
      // stops that winner and retries the desired configuration.
      status = await this.startProxy(desiredConfig);
    }

    if (status.running && agentProxyConfigsMatch(status.config, desiredConfig)) {
      return status;
    }

    throw new Error("Maple proxy could not be reconciled with the Agent Mode configuration");
  }

  private buildAgentProxyConfig(savedConfig: ProxyConfig, apiKey: string): ProxyConfig {
    const backendUrl =
      import.meta.env.VITE_OPEN_SECRET_API_URL ||
      savedConfig.backend_url ||
      "https://enclave.trymaple.ai";
    const port = Number(savedConfig.port || 8080);
    this.validateAgentPort(port);

    return enforceAgentProxySecurity({
      ...savedConfig,
      host: savedConfig.host || "127.0.0.1",
      port,
      api_key: apiKey,
      enabled: true,
      enable_cors: savedConfig.enable_cors ?? true,
      backend_url: backendUrl,
      auto_start: false
    });
  }

  private async checkProxyBackendAuth(
    status: ProxyStatus
  ): Promise<"ok" | "auth_error" | "unknown_error"> {
    if (!status.running || !status.config.api_key.trim()) {
      return "auth_error";
    }

    const host = status.config.host === "0.0.0.0" ? "127.0.0.1" : status.config.host;
    try {
      const response = await fetch(`http://${host}:${status.config.port}/v1/models`);
      if (response.ok) return "ok";

      const body = await response.text();
      if (
        response.status === 401 ||
        body.includes('"status":401') ||
        body.toLowerCase().includes("unauthorized")
      ) {
        return "auth_error";
      }

      return "unknown_error";
    } catch {
      return "unknown_error";
    }
  }

  async testProxyPort(host: string, port: number): Promise<boolean> {
    try {
      this.validatePort(port);
      return await invoke<boolean>("test_proxy_port", { host, port });
    } catch (error) {
      console.error("Failed to test proxy port:", error);
      throw error;
    }
  }

  async startManualProxy(config: ProxyConfig): Promise<ProxyStatus> {
    return await this.enqueueProxyOperation(async () => {
      const status = await this.startProxy(config);
      if (!status.running || !manualProxyConfigsMatch(status.config, config)) {
        throw new Error(
          "The local proxy changed while the manual setup was starting. Review the current settings and try again."
        );
      }
      // Do not discard the previous Agent ownership association until the
      // native mutation has actually succeeded.
      this.markCurrentProxyConfigAsManual();
      return status;
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

  async replaceOwnerlessProxyAndEnsureReady(
    userId: string,
    createApiKey: CreateProxyApiKey,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<ProxyStatus> {
    if (!userId.trim()) throw new Error("Agent proxy setup requires an authenticated user");

    return await this.enqueueProxyOperation(async () => {
      const [status, config] = await Promise.all([
        this.getProxyStatus(),
        this.loadAgentProxyConfig()
      ]);
      const storedOwner = this.loadAgentProxyOwner();
      const trackedOwner = config.api_key.trim()
        ? this.loadActiveTrackedKey()?.userId || null
        : null;
      const hasExistingProxyState = status.running || Boolean(config.api_key.trim());

      if (!shouldBlockOnOwnerlessProxy(storedOwner, trackedOwner, hasExistingProxyState)) {
        throw new Error("The saved proxy credential changed before it could be replaced");
      }

      // This destructive reset is reached only from the explicit replacement
      // action in Agent Mode. The unverified backend key is deliberately not
      // revoked because it may belong to another account.
      await this.resetProxyLocalState();
      try {
        return await this.ensureProxyReadyInner(userId, createApiKey, deleteApiKey);
      } catch (error) {
        // The destructive user-approved replacement has already happened.
        // Tell the UI to leave conflict mode so ordinary Agent setup can be
        // retried instead of presenting a dead replacement button.
        throw new AgentProxyReplacementSetupError(error);
      }
    });
  }

  // Stop and scrub local credentials first so an offline backend can never
  // prevent logout. Exact locally-created key records remain queued in local
  // metadata when remote revocation fails and are retried when that account
  // next initializes Agent Mode.
  async stopAndResetProxy(userId?: string | null, deleteApiKey?: DeleteProxyApiKey): Promise<void> {
    if (!isTauriDesktop()) return;

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
    const queued = this.ensureReadyTail.then(operation);
    this.ensureReadyTail = queued.then(
      () => undefined,
      () => undefined
    );
    return await queued;
  }

  private async resetProxyLocalState(): Promise<void> {
    if (!isTauriDesktop()) return;

    try {
      await invoke<ProxyStatus>("stop_and_reset_proxy");
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
      // A stale association is conservative: the next Agent initialization
      // will reconcile or explicitly reset it, while no credential remains.
    }
  }

  private loadAgentProxyOwner(): string | null {
    if (typeof localStorage === "undefined") return null;
    return localStorage.getItem(AGENT_PROXY_OWNER_KEY);
  }

  private saveAgentProxyOwner(userId: string): void {
    if (typeof localStorage === "undefined") {
      throw new Error("Local storage is unavailable for Agent proxy ownership");
    }
    localStorage.setItem(AGENT_PROXY_OWNER_KEY, userId);
  }

  private clearAgentProxyOwner(): void {
    if (typeof localStorage === "undefined") return;
    localStorage.removeItem(AGENT_PROXY_OWNER_KEY);
  }

  private async createTrackedAgentProxyKey(
    userId: string,
    createApiKey: CreateProxyApiKey,
    deleteApiKey: DeleteProxyApiKey
  ): Promise<{ key: string; name: string }> {
    const name = createAgentProxyKeyName();
    const key = await createApiKey(name);

    try {
      const registry = addAgentProxyKeyRecord(this.loadAgentProxyKeyRegistry(), { userId, name });
      this.saveAgentProxyKeyRegistry(registry);
    } catch (trackingError) {
      try {
        await deleteApiKey(name);
      } catch (revokeError) {
        throw new Error(
          `Created an Agent proxy key but could not track or revoke it. Tracking failed: ${errorMessage(trackingError)}. Revocation failed: ${errorMessage(revokeError)}`
        );
      }
      throw trackingError;
    }

    return { key, name };
  }

  private async revokeTrackedAgentProxyKeys(
    userId: string,
    deleteApiKey: DeleteProxyApiKey,
    keepName?: string
  ): Promise<void> {
    const records = this.loadAgentProxyKeyRegistry().keys.filter(
      (record) => record.userId === userId && record.name !== keepName
    );
    for (const record of records) {
      await this.revokeTrackedAgentProxyKey(record.name, deleteApiKey);
    }
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

  private loadActiveTrackedKey(): AgentProxyKeyRecord | null {
    const registry = this.loadAgentProxyKeyRegistry();
    if (!registry.activeName) return null;
    return registry.keys.find((record) => record.name === registry.activeName) || null;
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

function createAgentProxyKeyName(): string {
  const date = new Date().toISOString().slice(0, 10).replace(/-/g, "");
  const random =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID().slice(0, 8)
      : Math.random().toString(36).slice(2, 10);
  return `maple-agent-${date}-${random}`;
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export const proxyService = new ProxyService();
