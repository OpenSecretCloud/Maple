import { isTauriDesktop } from "@/utils/platform";
import { agentOperationFence, type AgentOperationBlock } from "@/services/agentOperationFence";
import { AgentAuthLifecycleCoordinator } from "@/services/agentAuthLifecycle";

export interface AgentConfig {
  defaultProjectRoot?: string | null;
  defaultModel: string;
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
  activeRuns?: Record<string, string>;
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
  itemType: "message" | "thinking" | "tool" | "permission" | "system" | "error";
  role?: "user" | "assistant" | "thought" | "system" | string | null;
  title?: string | null;
  text?: string | null;
  status?: string | null;
  input?: unknown;
  output?: unknown;
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
}

export type AgentEventHandler = (event: AgentEventEnvelope) => void;
export type UnlistenAgentEvents = () => void;

class AgentRuntimeService {
  async getRuntimeStatus(userId: string): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser<AgentRuntimeStatus>(userId, "agent_get_runtime_status");
  }

  async startRuntime(userId: string, request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser<AgentRuntimeStatus>(userId, "agent_start_runtime", {
      userId,
      request: request ?? null
    });
  }

  async restartRuntime(userId: string, request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser<AgentRuntimeStatus>(userId, "agent_restart_runtime", {
      userId,
      request: request ?? null
    });
  }

  async loadConfig(userId: string): Promise<AgentConfig> {
    return await this.invokeForUser<AgentConfig>(userId, "agent_load_config");
  }

  async saveConfig(userId: string, config: AgentConfig): Promise<void> {
    await this.invokeForUser(userId, "agent_save_config", { userId, config });
  }

  async listRecentProjectRoots(userId: string): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser<RecentProjectRoot[]>(userId, "agent_list_recent_project_roots");
  }

  async saveRecentProjectRoot(userId: string, path: string): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser<RecentProjectRoot[]>(userId, "agent_save_recent_project_root", {
      userId,
      path
    });
  }

  async createSession(
    userId: string,
    request?: AgentCreateSessionRequest
  ): Promise<AgentSessionDetail> {
    return await this.invokeForUser<AgentSessionDetail>(userId, "agent_create_session", {
      userId,
      request: request ?? null
    });
  }

  async listSessions(userId: string, projectRoot?: string | null): Promise<AgentSessionSummary[]> {
    return await this.invokeForUser<AgentSessionSummary[]>(userId, "agent_list_sessions", {
      userId,
      projectRoot: projectRoot ?? null
    });
  }

  async loadSession(userId: string, sessionId: string): Promise<AgentSessionDetail> {
    return await this.invokeForUser<AgentSessionDetail>(userId, "agent_load_session", {
      userId,
      sessionId
    });
  }

  async deleteSession(userId: string, sessionId: string): Promise<void> {
    await this.invokeForUser(userId, "agent_delete_session", { userId, sessionId });
  }

  async sendMessage(userId: string, request: AgentSendMessageRequest): Promise<AgentRunResponse> {
    return await this.invokeForUser<AgentRunResponse>(userId, "agent_send_message", {
      userId,
      request
    });
  }

  async cancelRun(userId: string, runId: string): Promise<void> {
    await this.invokeForUser(userId, "agent_cancel_run", { userId, runId });
  }

  async respondToPermission(
    userId: string,
    sessionId: string,
    requestId: string,
    decision: AgentPermissionDecision
  ): Promise<void> {
    await this.invokeForUser(userId, "agent_permission_respond", {
      userId,
      response: { sessionId, requestId, decision }
    });
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

  private async invokeForUser<T>(
    userId: string,
    command: string,
    args?: Record<string, unknown>
  ): Promise<T> {
    return await agentOperationFence.run(userId, async () => {
      return await invokeAgent<T>(command, { userId, ...args });
    });
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

const agentAuthLifecycle = new AgentAuthLifecycleCoordinator(
  async (userId) => {
    if (!isTauriDesktop()) return;
    const block = await stopAgentRuntimeForUser(userId);
    try {
      // Auth may already be gone, so remote revocation is not reliable here.
      // Scrub the local credential immediately; the exact tracked backend-key
      // record remains available for retry if this account signs in again.
      const { proxyService } = await import("@/services/proxyService");
      await proxyService.stopAndResetProxy();
    } finally {
      block.retainUntilNextSession();
    }
  },
  (userId) => agentOperationFence.activateUserSession(userId)
);

export function transitionAgentAuthUser(userId?: string | null): Promise<void> {
  return agentAuthLifecycle.transitionTo(userId || null);
}

export async function awaitAgentAuthUser(userId: string): Promise<void> {
  await agentAuthLifecycle.waitForUser(userId);
}

export async function stopAgentRuntimeForUser(
  userId?: string | null
): Promise<AgentOperationBlock> {
  if (!isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot stop Agent Mode without an authenticated user");
  const block = await agentOperationFence.blockAndDrain(userId);
  try {
    await invokeAgent<AgentRuntimeStatus>("agent_stop_runtime", { userId });
    return block;
  } catch (error) {
    block.release();
    throw error;
  }
}

export async function clearAgentDataForUser(userId?: string | null): Promise<AgentOperationBlock> {
  if (!isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot clear Agent Mode data without an authenticated user");
  const block = await agentOperationFence.blockAndDrain(userId);
  try {
    await invokeAgent("agent_clear_user_data", { userId });
    return block;
  } catch (error) {
    block.release();
    throw error;
  }
}

export async function clearAgentHistoryForUser(
  userId?: string | null
): Promise<AgentOperationBlock> {
  if (!isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot clear Agent Mode history without an authenticated user");
  const block = await agentOperationFence.blockAndDrain(userId);
  try {
    await invokeAgent("agent_clear_user_history", { userId });
    return block;
  } catch (error) {
    block.release();
    throw error;
  }
}

function noOpOperationBlock(): AgentOperationBlock {
  return { release: () => {}, retainUntilNextSession: () => {} };
}
