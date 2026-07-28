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
  owner_user_id?: string;
}

export interface ProxyStatus {
  running: boolean;
  config: ProxyConfig;
  error?: string;
}

export type DeleteProxyApiKey = (name: string) => Promise<void>;
export type CreateProxyApiKey = (name: string) => Promise<string>;

export interface ManualProxyKeyProvisioner {
  name: string;
  createApiKey: CreateProxyApiKey;
  refreshApiKeys: () => Promise<void>;
  onApiKeyCreated?: (apiKey: string) => void;
}

export class ProxyAuthenticationChangedError extends Error {
  constructor() {
    super("The authenticated Maple account changed before the local proxy finished starting");
    this.name = "ProxyAuthenticationChangedError";
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
    (active.auto_start ?? false) === (desired.auto_start ?? false) &&
    (active.owner_user_id?.trim() || "") === (desired.owner_user_id?.trim() || "")
  );
}

type ProxyCommandInvoker = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface ProxyAuthenticationSnapshot {
  userId: string;
  generation: number;
}

export class ProxyService {
  private operationTail: Promise<void> = Promise.resolve();
  private authenticationObserved = false;
  private authenticatedUserId: string | null = null;
  private authGeneration = 0;
  private authReadyGeneration = -1;
  private authTransitionGeneration = -1;
  private authTransitionPromise: Promise<void> = Promise.resolve();
  private authRetryMode: "initialize" | "reset" = "initialize";

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

  private async loadProxyConfigForAuthentication(): Promise<ProxyConfig> {
    try {
      return await this.invokeCommand<ProxyConfig>("load_proxy_config");
    } catch (error) {
      // Authentication reconciliation must never turn a read failure into an
      // empty config and then make a destructive ownership decision from it.
      console.error("Failed to load proxy config for authentication:", error);
      throw error;
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

  transitionAuthenticatedUser(nextUserId: string | null): Promise<void> {
    const normalizedUserId = nextUserId?.trim() || null;
    const previousUserId = this.authenticatedUserId;
    const previousGenerationWasReady = this.authReadyGeneration === this.authGeneration;

    if (this.authenticationObserved && previousUserId === normalizedUserId) {
      if (previousGenerationWasReady) return Promise.resolve();
      if (this.authTransitionGeneration === this.authGeneration) {
        return this.authTransitionPromise;
      }
      return this.authRetryMode === "reset"
        ? this.queueAuthenticationReset(this.authGeneration)
        : this.queueInitialAuthentication(normalizedUserId, this.authGeneration);
    }

    // Change the generation synchronously. An operation currently awaiting an
    // encrypted API request observes the transition before it can mutate the
    // native proxy. Cleanup remains serialized with native proxy mutations.
    const isFirstObservation = !this.authenticationObserved;
    this.authenticationObserved = true;
    this.authenticatedUserId = normalizedUserId;
    this.authGeneration += 1;
    const generation = this.authGeneration;

    if (!this.desktopCheck()) {
      this.authReadyGeneration = generation;
      return Promise.resolve();
    }

    if (isFirstObservation) {
      return this.queueInitialAuthentication(normalizedUserId, generation);
    }

    // A signed-out cold launch intentionally leaves released ownerless configs
    // intact. Reconcile them after login without assigning them to whichever
    // account happens to authenticate first.
    if (!previousUserId && normalizedUserId) {
      return this.queueInitialAuthentication(normalizedUserId, generation);
    }

    const requiresReset = Boolean(previousUserId) || !previousGenerationWasReady;

    if (!requiresReset) {
      this.authReadyGeneration = generation;
      return Promise.resolve();
    }
    return this.queueAuthenticationReset(generation);
  }

  async awaitAuthenticatedUser(userId: string): Promise<void> {
    const normalizedUserId = userId.trim();
    if (!normalizedUserId || this.authenticatedUserId !== normalizedUserId) {
      throw new ProxyAuthenticationChangedError();
    }
    await this.transitionAuthenticatedUser(normalizedUserId);
    this.assertAuthenticationCurrent({
      userId: normalizedUserId,
      generation: this.authGeneration
    });
  }

  async loadManualProxyState(
    userId: string
  ): Promise<{ config: ProxyConfig; status: ProxyStatus }> {
    await this.awaitAuthenticatedUser(userId);
    const auth = this.captureAuthentication(userId);
    return await this.enqueueProxyOperation(async () => {
      this.assertAuthenticationCurrent(auth);
      const [config, status] = await Promise.all([this.loadProxyConfig(), this.getProxyStatus()]);
      this.assertAuthenticationCurrent(auth);
      return { config, status };
    });
  }

  async startManualProxy(
    userId: string,
    config: ProxyConfig,
    keyProvisioner?: ManualProxyKeyProvisioner
  ): Promise<ProxyStatus> {
    await this.awaitAuthenticatedUser(userId);
    const auth = this.captureAuthentication(userId);
    let desiredConfig = config;

    if (!desiredConfig.api_key.trim()) {
      if (!keyProvisioner) {
        throw new Error("Starting the local proxy requires an API key");
      }

      const apiKey = await keyProvisioner.createApiKey(keyProvisioner.name);
      desiredConfig = { ...desiredConfig, api_key: apiKey };
      // Released Maple retained a newly-created key in component state when a
      // later refresh or native start failed. Preserve that retry path, and do
      // not attempt rollback through an SDK session that may now be another
      // account's.
      keyProvisioner.onApiKeyCreated?.(apiKey);
      this.assertAuthenticationCurrent(auth);
      await keyProvisioner.refreshApiKeys();
      this.assertAuthenticationCurrent(auth);
    }

    desiredConfig = { ...desiredConfig, owner_user_id: auth.userId };

    return await this.enqueueProxyOperation(async () => {
      let proxyStarted = false;
      try {
        this.assertAuthenticationCurrent(auth);
        const startedStatus = await this.startProxy(desiredConfig);
        proxyStarted = true;
        this.assertAuthenticationCurrent(auth);

        if (
          !startedStatus.running ||
          !manualProxyConfigsMatch(startedStatus.config, desiredConfig)
        ) {
          throw new Error(
            "The local proxy changed while the manual setup was starting. Review the current settings and try again."
          );
        }

        // Do not discard the previous Agent ownership association until the
        // native mutation has actually succeeded and the account fence still
        // owns the result.
        this.markCurrentProxyConfigAsManual();
        return startedStatus;
      } catch (error) {
        if (proxyStarted) await this.resetProxyLocalState();
        throw error;
      }
    });
  }

  async saveManualProxySettings(userId: string, config: ProxyConfig): Promise<void> {
    await this.awaitAuthenticatedUser(userId);
    const auth = this.captureAuthentication(userId);
    await this.enqueueProxyOperation(async () => {
      this.assertAuthenticationCurrent(auth);
      await this.saveProxySettings({ ...config, owner_user_id: auth.userId });
      this.assertAuthenticationCurrent(auth);
      this.markCurrentProxyConfigAsManual();
    });
  }

  async stopManualProxy(): Promise<ProxyStatus> {
    return await this.enqueueProxyOperation(async () => await this.stopProxy());
  }

  private captureAuthentication(userId: string): ProxyAuthenticationSnapshot {
    const normalizedUserId = userId.trim();
    if (
      !normalizedUserId ||
      this.authenticatedUserId !== normalizedUserId ||
      this.authReadyGeneration !== this.authGeneration
    ) {
      throw new ProxyAuthenticationChangedError();
    }
    return { userId: normalizedUserId, generation: this.authGeneration };
  }

  private assertAuthenticationCurrent(auth: ProxyAuthenticationSnapshot): void {
    if (
      this.authenticatedUserId !== auth.userId ||
      this.authGeneration !== auth.generation ||
      this.authReadyGeneration !== auth.generation
    ) {
      throw new ProxyAuthenticationChangedError();
    }
  }

  private markCurrentProxyConfigAsManual(): void {
    this.clearAgentProxyOwner();
    const registry = this.loadAgentProxyKeyRegistry();
    if (registry.activeName) {
      this.saveAgentProxyKeyRegistry(deactivateAgentProxyKeyRegistry(registry));
    }
  }

  // Stop and scrub local credentials first so an offline backend can never
  // prevent logout. Preserve the released Agent-key cleanup behavior; manual
  // startup does not add any new remote rollback work to this transition.
  async stopAndResetProxy(userId?: string | null, deleteApiKey?: DeleteProxyApiKey): Promise<void> {
    if (!this.desktopCheck()) return;

    if (userId && this.authenticatedUserId && this.authenticatedUserId !== userId) {
      throw new ProxyAuthenticationChangedError();
    }
    // Invalidate immediately instead of waiting for the serialized reset.
    // Keeping the current user allows a failed logout attempt to retry without
    // requiring a synthetic auth transition.
    this.authGeneration += 1;
    const generation = this.authGeneration;

    const reset = this.enqueueProxyOperation(async () => {
      await this.resetProxyLocalState();
      if (userId && deleteApiKey) {
        void this.revokeTrackedAgentProxyKeysBestEffort(userId, deleteApiKey).catch(() => {});
      }
    });
    await this.trackAuthenticationTransition(reset, generation, "reset");
  }

  private queueAuthenticationReset(generation: number): Promise<void> {
    const reset = this.enqueueProxyOperation(async () => await this.resetProxyLocalState());
    return this.trackAuthenticationTransition(reset, generation, "reset");
  }

  private queueInitialAuthentication(userId: string | null, generation: number): Promise<void> {
    const initialization = this.enqueueProxyOperation(async () => {
      if (!userId) {
        // Native startup handles released ownerless auto-start configs. Do not
        // read, rewrite, or scrub any config while the frontend is signed out.
        this.assertAuthenticationTransitionCurrent(null, generation);
        return;
      }

      this.assertAuthenticationTransitionCurrent(userId, generation);
      let config: ProxyConfig;
      try {
        config = await this.loadProxyConfigForAuthentication();
      } catch {
        // A local read/keyring failure must not brick the entire authenticated
        // UI or turn into a destructive default-config decision.
        return;
      }
      this.assertAuthenticationTransitionCurrent(userId, generation);

      const savedOwner = config.owner_user_id?.trim() || null;
      if (savedOwner && savedOwner !== userId) {
        // If reset fails, the same account observation must retry the reset,
        // never fall back to initializing the foreign config.
        this.authRetryMode = "reset";
        await this.resetProxyLocalState();
        return;
      }

      if (config.auto_start && config.api_key.trim()) {
        try {
          const status = await this.getProxyStatus();
          this.assertAuthenticationTransitionCurrent(userId, generation);
          if (!status.running) {
            await this.startProxy(config);
            this.assertAuthenticationTransitionCurrent(userId, generation);
          }
        } catch (error) {
          // Native startup already reports released-config failures, and an
          // authenticated auto-start failure must not block unrelated UI.
          console.error("Failed to auto-start the authenticated proxy:", error);
        }
      }
    });
    return this.trackAuthenticationTransition(initialization, generation, "initialize");
  }

  private assertAuthenticationTransitionCurrent(userId: string | null, generation: number): void {
    if (this.authenticatedUserId !== userId || this.authGeneration !== generation) {
      throw new ProxyAuthenticationChangedError();
    }
  }

  private trackAuthenticationTransition(
    transition: Promise<void>,
    generation: number,
    retryMode: "initialize" | "reset"
  ): Promise<void> {
    this.authRetryMode = retryMode;
    const tracked = transition.then(
      () => {
        if (this.authGeneration === generation) this.authReadyGeneration = generation;
        if (this.authTransitionGeneration === generation) this.authTransitionGeneration = -1;
      },
      (error) => {
        if (this.authTransitionGeneration === generation) this.authTransitionGeneration = -1;
        throw error;
      }
    );
    this.authTransitionGeneration = generation;
    this.authTransitionPromise = tracked;
    return tracked;
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
