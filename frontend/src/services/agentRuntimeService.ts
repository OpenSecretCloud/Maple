import { isTauriDesktop } from "@/utils/platform";

export interface AgentConfig {
  defaultProjectRoot?: string | null;
  defaultModel: string;
  runtimeKind: string;
}

export interface AgentStartRequest {
  projectRoot?: string | null;
  model?: string | null;
  mode?: string | null;
}

export interface AgentRuntimeStatus {
  running: boolean;
  projectRoot?: string | null;
  model?: string | null;
  mode?: string | null;
  mapleProxyBaseUrl?: string | null;
  configDir: string;
  goosePathRoot?: string | null;
  logPath?: string | null;
  llmLogDir?: string | null;
  latestLlmLogPath?: string | null;
  proxyLlmLogDir?: string | null;
  latestProxyLlmLogPath?: string | null;
  error?: string | null;
}

export interface RecentProjectRoot {
  path: string;
  name: string;
  lastUsedMs: number;
}

export interface AgentCreateSessionRequest {
  projectRoot?: string | null;
  title?: string | null;
  model?: string | null;
  mode?: string | null;
}

export interface AgentSessionSummary {
  id: string;
  title: string;
  projectRoot: string;
  createdMs: number;
  updatedMs: number;
  messageCount: number;
  model?: string | null;
  mode: string;
}

export interface AgentTimelineItem {
  id: string;
  itemType: "message" | "thinking" | "tool" | "permission" | "system" | "error" | "usage";
  role?: "user" | "assistant" | "thought" | "system" | string | null;
  title?: string | null;
  text?: string | null;
  status?: string | null;
  input?: unknown;
  output?: unknown;
  raw?: unknown;
  createdMs: number;
  merge: "append" | "replace" | string;
}

export interface AgentSessionDetail {
  session: AgentSessionSummary;
  timeline: AgentTimelineItem[];
}

export interface AgentSendMessageRequest {
  sessionId: string;
  text: string;
  model?: string | null;
  mode?: string | null;
}

export interface AgentRunResponse {
  runId: string;
}

export type AgentPermissionDecision =
  | "allow_once"
  | "always_allow"
  | "deny_once"
  | "always_deny"
  | "cancel";

export interface AgentEventEnvelope {
  eventType: string;
  sessionId?: string | null;
  runId?: string | null;
  item?: AgentTimelineItem | null;
  status?: AgentRuntimeStatus | null;
  session?: AgentSessionSummary | null;
  message?: string | null;
  details?: unknown;
}

export type AgentEventHandler = (event: AgentEventEnvelope) => void;
export type UnlistenAgentEvents = () => void;

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

  async createSession(request?: AgentCreateSessionRequest): Promise<AgentSessionDetail> {
    return await invokeAgent<AgentSessionDetail>("agent_create_session", {
      request: request ?? null
    });
  }

  async listSessions(projectRoot?: string | null): Promise<AgentSessionSummary[]> {
    return await invokeAgent<AgentSessionSummary[]>("agent_list_sessions", {
      projectRoot: projectRoot ?? null
    });
  }

  async loadSession(sessionId: string): Promise<AgentSessionDetail> {
    return await invokeAgent<AgentSessionDetail>("agent_load_session", { sessionId });
  }

  async sendMessage(request: AgentSendMessageRequest): Promise<AgentRunResponse> {
    return await invokeAgent<AgentRunResponse>("agent_send_message", { request });
  }

  async cancelRun(runId: string): Promise<void> {
    await invokeAgent("agent_cancel_run", { runId });
  }

  async respondToPermission(requestId: string, decision: AgentPermissionDecision): Promise<void> {
    await invokeAgent("agent_permission_respond", {
      response: { requestId, decision }
    });
  }

  async appendRuntimeLog(message: string): Promise<void> {
    await invokeAgent("agent_append_runtime_log", { message });
  }

  async listenToEvents(handler: AgentEventHandler): Promise<UnlistenAgentEvents> {
    if (!isTauriDesktop()) {
      return () => {};
    }
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await listen<AgentEventEnvelope>("agent-event", (event) => {
      handler(event.payload);
    });
    return unlisten;
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
