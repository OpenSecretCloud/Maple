import { isTauriDesktop } from "@/utils/platform";

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
  llmLogDir?: string | null;
  latestLlmLogPath?: string | null;
  error?: string | null;
}

export interface RecentProjectRoot {
  path: string;
  name: string;
  lastUsedMs: number;
}

class AgentRuntimeService {
  async getRuntimeStatus(): Promise<AgentRuntimeStatus> {
    return await invokeAgent<AgentRuntimeStatus>("agent_get_runtime_status");
  }

  async startRuntime(request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await invokeAgent<AgentRuntimeStatus>("agent_start_runtime", {
      request: request ?? null
    });
  }

  async stopRuntime(): Promise<AgentRuntimeStatus> {
    return await invokeAgent<AgentRuntimeStatus>("agent_stop_runtime");
  }

  async restartRuntime(request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await invokeAgent<AgentRuntimeStatus>("agent_restart_runtime", {
      request: request ?? null
    });
  }

  async loadConfig(): Promise<AgentConfig> {
    return await invokeAgent<AgentConfig>("agent_load_config");
  }

  async saveConfig(config: AgentConfig): Promise<void> {
    await invokeAgent("agent_save_config", { config });
  }

  async listRecentProjectRoots(): Promise<RecentProjectRoot[]> {
    return await invokeAgent<RecentProjectRoot[]>("agent_list_recent_project_roots");
  }

  async saveRecentProjectRoot(path: string): Promise<RecentProjectRoot[]> {
    return await invokeAgent<RecentProjectRoot[]>("agent_save_recent_project_root", { path });
  }

  async appendSessionEvent(sessionId: string, event: unknown): Promise<void> {
    await invokeAgent("agent_append_session_event", { sessionId, event });
  }

  async appendRuntimeLog(message: string): Promise<void> {
    await invokeAgent("agent_append_runtime_log", { message });
  }
}

async function invokeAgent<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriDesktop()) {
    throw new Error("Agent Mode is available in Maple Desktop.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

export const agentRuntimeService = new AgentRuntimeService();
