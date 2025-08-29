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

class ProxyService {
  async startProxy(config: ProxyConfig): Promise<ProxyStatus> {
    try {
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
      await invoke("save_proxy_settings", { config });
    } catch (error) {
      console.error("Failed to save proxy settings:", error);
      throw error;
    }
  }

  async testProxyPort(host: string, port: number): Promise<boolean> {
    try {
      return await invoke<boolean>("test_proxy_port", { host, port });
    } catch (error) {
      console.error("Failed to test proxy port:", error);
      throw error;
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

export const proxyService = new ProxyService();
