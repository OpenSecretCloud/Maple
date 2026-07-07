import { invoke } from "@tauri-apps/api/core";

export interface AgentConfig {
  defaultProjectRoot?: string | null;
  defaultModel: string;
  runtimeKind: string;
  externalAcpUrl?: string | null;
}

export interface AgentStartRequest {
  projectRoot?: string | null;
  model?: string | null;
  gooseBinary?: string | null;
  mode?: string | null;
}

export interface AgentRuntimeStatus {
  running: boolean;
  acpUrl?: string | null;
  redactedAcpUrl?: string | null;
  httpBaseUrl?: string | null;
  statusUrl?: string | null;
  projectRoot?: string | null;
  gooseBinary?: string | null;
  pid?: number | null;
  model?: string | null;
  mode?: string | null;
  mapleProxyBaseUrl?: string | null;
  configDir: string;
  goosePathRoot?: string | null;
  logPath?: string | null;
  error?: string | null;
}

export interface RecentProjectRoot {
  path: string;
  name: string;
  lastUsedMs: number;
}

class AgentRuntimeService {
  async getRuntimeStatus(): Promise<AgentRuntimeStatus> {
    return await invoke<AgentRuntimeStatus>("agent_get_runtime_status");
  }

  async startRuntime(request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await invoke<AgentRuntimeStatus>("agent_start_runtime", { request: request ?? null });
  }

  async stopRuntime(): Promise<AgentRuntimeStatus> {
    return await invoke<AgentRuntimeStatus>("agent_stop_runtime");
  }

  async restartRuntime(request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await invoke<AgentRuntimeStatus>("agent_restart_runtime", { request: request ?? null });
  }

  async loadConfig(): Promise<AgentConfig> {
    return await invoke<AgentConfig>("agent_load_config");
  }

  async saveConfig(config: AgentConfig): Promise<void> {
    await invoke("agent_save_config", { config });
  }

  async listRecentProjectRoots(): Promise<RecentProjectRoot[]> {
    return await invoke<RecentProjectRoot[]>("agent_list_recent_project_roots");
  }

  async saveRecentProjectRoot(path: string): Promise<RecentProjectRoot[]> {
    return await invoke<RecentProjectRoot[]>("agent_save_recent_project_root", { path });
  }

  async appendSessionEvent(sessionId: string, event: unknown): Promise<void> {
    await invoke("agent_append_session_event", { sessionId, event });
  }
}

export const agentRuntimeService = new AgentRuntimeService();
