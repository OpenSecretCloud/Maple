import { isTauriDesktop } from "@/utils/platform";
import { agentOperationFence, type AgentOperationBlock } from "@/services/agentOperationFence";
import { AgentAuthLifecycleCoordinator } from "@/services/agentAuthLifecycle";
import { mapleApiAuthService } from "@/services/mapleApiAuthService";

export interface AgentConfig {
  defaultProjectRoot?: string | null;
  defaultModel: string;
  projectSkillsTrust?: AgentProjectSkillsTrust[];
  removedProjectRoots?: string[];
}

export interface AgentProjectSkillsTrust {
  path: string;
  trusted: boolean;
}

export interface AgentProjectSkillsTrustStatus {
  path: string;
  decision?: boolean | null;
  available: boolean;
}

export interface AgentMcpKeyValue {
  key: string;
  value: string;
}

export type AgentMcpTransport =
  | {
      type: "stdio";
      command: string;
      environment: AgentMcpKeyValue[];
    }
  | {
      type: "streamable_http";
      url: string;
      environment: AgentMcpKeyValue[];
      headers: AgentMcpKeyValue[];
    };

export interface AgentMcpServer {
  name: string;
  description: string;
  enabled: boolean;
  timeoutSeconds: number;
  transport: AgentMcpTransport;
}

export interface AgentMcpConnectionError {
  name: string;
  error: string;
}

export interface AgentSessionMcpServer {
  name: string;
  description: string;
  transport: "stdio" | "streamable_http";
  enabled: boolean;
  available: boolean;
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

export interface AgentRuntimeLifecycleOutcome {
  status: AgentRuntimeStatus;
  acpShutdownError: string | null;
}

export interface RecentProjectRoot {
  path: string;
  name: string;
  lastUsedMs: number;
}

export interface AgentProjectRootRegistration {
  projectRoot: string;
  roots: RecentProjectRoot[];
  config: AgentConfig;
}

export interface AgentCreateSessionRequest {
  projectRoot?: string | null;
  title?: string | null;
  model?: string | null;
  contextLimit?: number | null;
  mode?: string | null;
  mcpServerNames?: string[] | null;
}

export interface AgentRenameSessionRequest {
  sessionId: string;
  title: string;
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

export interface AgentQueuedMessage {
  queueId: string;
  messageId: string;
  sessionId: string;
  text: string;
  createdMs: number;
}

export interface AgentDesktopQueueSnapshot {
  revision: number;
  items: AgentQueuedMessage[];
}

export interface AgentQueueControlRequest {
  sessionId: string;
  queueId: string;
}

export interface AgentSessionDetail {
  session: AgentSessionSummary;
  timeline: AgentTimelineItem[];
  mcpErrors: AgentMcpConnectionError[];
  queue: AgentDesktopQueueSnapshot;
}

export interface AgentSendMessageRequest {
  sessionId: string;
  text: string;
  model?: string | null;
  contextLimit?: number | null;
  mode?: string | null;
  visionCapable: boolean;
}

export interface AgentRunResponse {
  runId: string;
  queued?: AgentQueuedMessage | null;
  queue: AgentDesktopQueueSnapshot;
}

export type AgentPermissionDecision = "allow_once" | "deny_once" | "cancel";

export interface AgentEventEnvelope {
  eventType: string;
  sessionId?: string | null;
  runId?: string | null;
  item?: AgentTimelineItem | null;
  status?: AgentRuntimeStatus | null;
  session?: AgentSessionSummary | null;
  message?: string | null;
  queue?: AgentDesktopQueueSnapshot | null;
  promotedQueueId?: string | null;
}

export type AgentEventHandler = (event: AgentEventEnvelope) => void;
export type UnlistenAgentEvents = () => void;

export interface AgentRuntimeBridge {
  syncAuth(userId: string): Promise<void>;
  runForUser<T>(userId: string, operation: () => Promise<T>): Promise<T>;
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

export interface AgentRuntimeStopBridge {
  blockAndDrain(userId: string): Promise<AgentOperationBlock>;
  stopHost(userId: string): Promise<AgentRuntimeLifecycleOutcome>;
}

/**
 * Owns the security-sensitive host shutdown shared by logout and account
 * transitions. Native code attempts ACP cleanup first and always attempts the
 * core runtime stop; credential cleanup proceeds only when both succeeded.
 */
export class AgentRuntimeStopCoordinator {
  constructor(private readonly bridge: AgentRuntimeStopBridge) {}

  async stop(userId: string): Promise<AgentOperationBlock> {
    const block = await this.bridge.blockAndDrain(userId);
    try {
      const outcome = await this.bridge.stopHost(userId);
      if (outcome.acpShutdownError) {
        throw new AgentRuntimePartialStopError(outcome);
      }
      return block;
    } catch (error) {
      block.release();
      throw error;
    }
  }
}

export class AgentRuntimePartialStopError extends Error {
  constructor(readonly outcome: AgentRuntimeLifecycleOutcome) {
    super(
      `Agent runtime stopped, but ACP cleanup failed: ${outcome.acpShutdownError || "unknown ACP error"}`
    );
    this.name = "AgentRuntimePartialStopError";
  }
}

const defaultAgentRuntimeBridge: AgentRuntimeBridge = {
  syncAuth: async (userId) => await mapleApiAuthService.sync(userId),
  runForUser: async (userId, operation) => await agentOperationFence.run(userId, operation),
  invoke: invokeAgent
};

const agentRuntimeStopCoordinator = new AgentRuntimeStopCoordinator({
  blockAndDrain: async (userId) => await agentOperationFence.blockAndDrain(userId),
  // The cleanup lease is already held. Native code owns the single composite
  // ACP-plus-runtime lifecycle gate; the manual ACP Stop command is reserved
  // for the settings page because it also changes saved configuration.
  stopHost: async (userId) => {
    return await invokeAgent<AgentRuntimeLifecycleOutcome>("agent_stop_runtime", { userId });
  }
});

export class AgentRuntimeService {
  constructor(private readonly bridge: AgentRuntimeBridge = defaultAgentRuntimeBridge) {}

  async getRuntimeStatus(userId: string): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser<AgentRuntimeStatus>(userId, "agent_get_runtime_status");
  }

  async startRuntime(userId: string, request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser<AgentRuntimeStatus>(userId, "agent_start_runtime", {
      userId,
      request: request ?? null
    });
  }

  async restartRuntime(
    userId: string,
    request?: AgentStartRequest
  ): Promise<AgentRuntimeLifecycleOutcome> {
    return await this.invokeForUser<AgentRuntimeLifecycleOutcome>(userId, "agent_restart_runtime", {
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

  async listMcpServers(userId: string): Promise<AgentMcpServer[]> {
    return await this.invokeForUser<AgentMcpServer[]>(userId, "agent_list_mcp_servers");
  }

  async saveMcpServers(userId: string, servers: AgentMcpServer[]): Promise<AgentMcpServer[]> {
    return await this.invokeForUser<AgentMcpServer[]>(userId, "agent_save_mcp_servers", {
      userId,
      servers
    });
  }

  async listSessionMcpServers(userId: string, sessionId: string): Promise<AgentSessionMcpServer[]> {
    return await this.invokeForUser<AgentSessionMcpServer[]>(
      userId,
      "agent_list_session_mcp_servers",
      { userId, sessionId }
    );
  }

  async setSessionMcpServerEnabled(
    userId: string,
    sessionId: string,
    name: string,
    enabled: boolean
  ): Promise<AgentSessionMcpServer[]> {
    return await this.invokeForUser<AgentSessionMcpServer[]>(
      userId,
      "agent_set_session_mcp_server_enabled",
      { userId, request: { sessionId, name, enabled } }
    );
  }

  async listRecentProjectRoots(userId: string): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser<RecentProjectRoot[]>(userId, "agent_list_recent_project_roots");
  }

  async saveRecentProjectRoot(userId: string, path: string): Promise<AgentProjectRootRegistration> {
    return await this.invokeForUser<AgentProjectRootRegistration>(
      userId,
      "agent_save_recent_project_root",
      {
        userId,
        path
      }
    );
  }

  async removeProjectRoot(
    userId: string,
    path: string,
    fallbackPath?: string | null
  ): Promise<AgentConfig> {
    return await this.invokeForUser<AgentConfig>(userId, "agent_remove_project_root", {
      userId,
      path,
      fallbackPath: fallbackPath ?? null
    });
  }

  async getProjectSkillsTrust(
    userId: string,
    path: string
  ): Promise<AgentProjectSkillsTrustStatus> {
    return await this.invokeForUser<AgentProjectSkillsTrustStatus>(
      userId,
      "agent_get_project_skills_trust",
      { userId, path }
    );
  }

  async setProjectSkillsTrust(
    userId: string,
    path: string,
    trusted: boolean
  ): Promise<AgentProjectSkillsTrustStatus> {
    return await this.invokeForUser<AgentProjectSkillsTrustStatus>(
      userId,
      "agent_set_project_skills_trust",
      { userId, path, trusted }
    );
  }

  async saveProjectRootOrder(userId: string, paths: string[]): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser<RecentProjectRoot[]>(userId, "agent_save_project_root_order", {
      userId,
      paths
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

  async renameSession(
    userId: string,
    request: AgentRenameSessionRequest
  ): Promise<AgentSessionSummary> {
    return await this.invokeForUser<AgentSessionSummary>(userId, "agent_rename_session", {
      userId,
      request
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

  async cancelQueuedMessage(
    userId: string,
    request: AgentQueueControlRequest
  ): Promise<AgentDesktopQueueSnapshot> {
    return await this.invokeLocalForUser<AgentDesktopQueueSnapshot>(
      userId,
      "agent_cancel_queued_message",
      { userId, request }
    );
  }

  async unqueueMessageForEdit(
    userId: string,
    request: AgentQueueControlRequest
  ): Promise<AgentQueuedMessage> {
    return await this.invokeLocalForUser<AgentQueuedMessage>(
      userId,
      "agent_unqueue_message_for_edit",
      { userId, request }
    );
  }

  async cancelRun(userId: string, runId: string): Promise<void> {
    // Cancellation is a local control-plane operation. Keep it account-fenced,
    // but never delay Stop on remote credential validation or token refresh.
    await this.invokeLocalForUser(userId, "agent_cancel_run", { userId, runId });
  }

  async setPermissionMode(userId: string, sessionId: string, mode: string): Promise<void> {
    await this.invokeForUser(userId, "agent_set_permission_mode", {
      userId,
      request: { sessionId, mode }
    });
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
    return await this.bridge.runForUser(userId, async () => {
      await this.bridge.syncAuth(userId);
      return await this.bridge.invoke<T>(command, { userId, ...args });
    });
  }

  private async invokeLocalForUser<T>(
    userId: string,
    command: string,
    args?: Record<string, unknown>
  ): Promise<T> {
    return await this.bridge.runForUser(userId, async () => {
      return await this.bridge.invoke<T>(command, { userId, ...args });
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
      await mapleApiAuthService.clear(userId);
      // Auth may already be gone, so remote revocation is not reliable here.
      // Scrub the local credential immediately; the exact tracked backend-key
      // record remains available for retry if this account signs in again.
      const { proxyService } = await import("@/services/proxyService");
      await proxyService.stopAndResetProxy();
    } finally {
      block.retainUntilNextSession();
    }
  },
  async (userId) => {
    await mapleApiAuthService.activate(userId);
    agentOperationFence.activateUserSession(userId);
  }
);

export function transitionAgentAuthUser(userId?: string | null): Promise<void> {
  return agentAuthLifecycle.transitionTo(userId || null);
}

export async function awaitAgentAuthUser(userId: string): Promise<void> {
  await agentAuthLifecycle.ensureCurrentUser(userId);
}

export async function clearMapleApiAuthForUser(userId?: string | null): Promise<void> {
  if (!isTauriDesktop()) return;
  if (!userId) throw new Error("Cannot clear Maple API authentication without a signed-in user");
  await mapleApiAuthService.clear(userId);
}

export async function restoreMapleApiAuthForUser(userId?: string | null): Promise<void> {
  if (!isTauriDesktop()) return;
  if (!userId) throw new Error("Cannot restore Maple API authentication without a signed-in user");
  await mapleApiAuthService.activate(userId);
}

export async function stopAgentRuntimeForUser(
  userId?: string | null
): Promise<AgentOperationBlock> {
  if (!isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot stop Agent Mode without an authenticated user");
  return await agentRuntimeStopCoordinator.stop(userId);
}

export async function clearAgentDataForUser(userId?: string | null): Promise<AgentOperationBlock> {
  if (!isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot clear Agent Mode data without an authenticated user");
  const block = await agentRuntimeStopCoordinator.stop(userId);
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
  const block = await agentRuntimeStopCoordinator.stop(userId);
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
