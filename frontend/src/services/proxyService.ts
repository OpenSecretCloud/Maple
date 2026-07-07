import { invoke } from "@tauri-apps/api/core";

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

class ProxyService {
  private ensureReadyPromise: Promise<ProxyStatus> | null = null;

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

  async ensureProxyReady(createApiKey: CreateProxyApiKey): Promise<ProxyStatus> {
    if (!this.ensureReadyPromise) {
      this.ensureReadyPromise = this.ensureProxyReadyInner(createApiKey).finally(() => {
        this.ensureReadyPromise = null;
      });
    }

    return await this.ensureReadyPromise;
  }

  private async ensureProxyReadyInner(createApiKey: CreateProxyApiKey): Promise<ProxyStatus> {
    const status = await this.getProxyStatus();
    if (status.running && status.config.api_key.trim()) {
      return status;
    }

    const savedConfig = status.running ? status.config : await this.loadProxyConfig();
    let apiKey = savedConfig.api_key.trim();
    let createdApiKey = false;

    if (!apiKey) {
      apiKey = await createApiKey(createAgentProxyKeyName());
      createdApiKey = true;
    }

    const backendUrl =
      savedConfig.backend_url ||
      import.meta.env.VITE_OPEN_SECRET_API_URL ||
      "https://enclave.trymaple.ai";
    const port = Number(savedConfig.port || 8080);
    this.validateAgentPort(port);

    const nextConfig: ProxyConfig = {
      ...savedConfig,
      host: savedConfig.host || "127.0.0.1",
      port,
      api_key: apiKey,
      enabled: true,
      enable_cors: savedConfig.enable_cors ?? true,
      backend_url: backendUrl,
      auto_start: savedConfig.auto_start ?? false
    };

    if (status.running) {
      await this.stopProxy();
    } else if (createdApiKey) {
      await this.saveProxySettings({ ...nextConfig, enabled: false });
    }

    return await this.startProxy(nextConfig);
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

  // Stop proxy if running and reset saved config (used on logout)
  async stopAndResetProxy(): Promise<void> {
    try {
      // Check if proxy is running and stop it
      const status = await this.getProxyStatus();
      if (status.running) {
        await this.stopProxy();
      }
    } catch {
      // Proxy may not be running, that's fine
    }

    try {
      // Save default config to clear auto_start and API key
      await this.saveProxySettings({
        host: "127.0.0.1",
        port: 8080,
        api_key: "",
        enabled: false,
        enable_cors: true,
        auto_start: false
      });
    } catch (error) {
      console.error("Failed to reset proxy config:", error);
    }
  }

  // Helper to check if we're in Tauri desktop environment
  async isTauriDesktop(): Promise<boolean> {
    try {
      const { isTauri } = await import("@tauri-apps/api/core");
      const inTauri = await isTauri();

      if (!inTauri) return false;

      // Check if it's desktop (not mobile)
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();

      // Desktop platforms
      return platform === "macos" || platform === "windows" || platform === "linux";
    } catch {
      return false;
    }
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

export const proxyService = new ProxyService();
