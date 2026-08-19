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
  /** Exact native keyset ordering timestamp; display code should use updatedMs. */
  pageSortMs: number;
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

/**
 * Closed presentation item admitted by the synchronized remote history/live
 * boundary. Unlike the embedded compatibility item, this type has no place
 * for provider JSON, tool input/output, credentials, or extension fields.
 */
export interface AgentPresentedTimelineItem {
  id: string;
  itemType: "message" | "thinking" | "tool" | "permission" | "system" | "error";
  role?: "user" | "assistant" | "thought" | "system";
  title?: string;
  text?: string;
  status?: string;
  createdMs: number;
  merge: "append" | "replace";
}

export interface AgentSessionDetail {
  session: AgentSessionSummary;
  timeline: AgentTimelineItem[];
  mcpErrors: AgentMcpConnectionError[];
}

export interface AgentPageRequest {
  cursor?: string | null;
  limit?: number;
}

export interface AgentPage<T> {
  items: T[];
  nextCursor?: string | null;
}

export interface AgentListSessionsPageRequest extends AgentPageRequest {
  projectRoot?: string | null;
}

export interface AgentListSessionRecordsPageRequest extends AgentPageRequest {
  sessionId: string;
}

/**
 * One native Goose message row and its safe Maple timeline projection. A
 * history page is counted in records, not projected items: one record may
 * legitimately contain text, thinking, and tool activity together.
 */
export interface AgentHistoryRecord {
  recordId: string;
  role: string;
  createdMs: number;
  items: AgentTimelineItem[];
}

export interface AgentSessionRecordsPage {
  /** Newest record first, matching the native keyset query. */
  records: AgentHistoryRecord[];
  nextCursor?: string | null;
  /** Opaque generation shared by every page in one history incarnation. */
  historyRevision: string;
}

export interface AgentLiveEventCursor {
  journalId: string;
  sequence: number;
}

export interface AgentLiveSessionSnapshot {
  sessionId: string;
  liveItems: AgentPresentedTimelineItem[];
}

export interface AgentPresentedHistoryRecord {
  recordId: string;
  role: string;
  createdMs: number;
  items: AgentPresentedTimelineItem[];
}

export interface AgentPresentedSessionRecordsPage {
  records: AgentPresentedHistoryRecord[];
  nextCursor?: string | null;
  historyRevision: string;
}

export interface AgentBeginSessionHistoryAttachResponse {
  attachId: string;
  page: AgentPresentedSessionRecordsPage;
  liveSessionsComplete: true;
  liveSessionCount: number;
  liveSessions: AgentLiveSessionSnapshot[];
  throughEventCursor: AgentLiveEventCursor;
}

export interface AgentLiveBarrierResponse {
  throughEventCursor: AgentLiveEventCursor;
  liveStreamId: string;
}

export type AgentLiveSnapshotReason =
  | "paused_overflow"
  | "slow_subscriber"
  | "journal_replaced"
  | "retention_gap"
  | "cursor_ahead"
  | "owner_changed"
  | "ordering_lost"
  | "journal_unavailable";

export interface AgentLiveSnapshotRequiredFrame {
  liveEventVersion: 1;
  eventType: "snapshotRequired";
  targetId: AgentExecutionTargetId;
  hostEpoch: string;
  connectionGeneration: number;
  reason: AgentLiveSnapshotReason;
  lastEventCursor: AgentLiveEventCursor;
}

interface AgentOrderedLiveEventCommon {
  liveEventVersion: 1;
  targetId: AgentExecutionTargetId;
  hostEpoch: string;
  connectionGeneration: number;
  eventEpoch: string;
  eventSequence: number;
  sessionId: string;
}

export type AgentOrderedLiveEvent = AgentOrderedLiveEventCommon &
  (
    | { eventType: "runStarted"; runId: string }
    | {
        eventType: "timelineUpsert";
        runId?: string;
        item: AgentPresentedTimelineItem;
      }
    | {
        eventType: "timelineCleared";
        runId: string;
        reason: "run_started" | "history_replaced";
      }
    | {
        eventType: "timelineCleared";
        runId?: never;
        reason: "explicit_reload";
      }
    | { eventType: "historyReplaced"; runId: string }
    | { eventType: "cursorAdvanced"; runId?: never }
    | {
        eventType: "sessionUpdated";
        runId?: string;
        session: AgentSessionSummary;
      }
    | {
        eventType: "runFinished";
        runId: string;
        terminal: "completed" | "cancelled" | "failed";
      }
    | { eventType: "sessionDeleted"; runId?: never }
    | {
        eventType: "userFacingError";
        runId: string;
        item: AgentPresentedTimelineItem;
      }
  );

export type AgentLiveChannelFrame = AgentOrderedLiveEvent | AgentLiveSnapshotRequiredFrame;
export type AgentLiveChannelHandler = (frame: AgentLiveChannelFrame) => void;

export interface AgentActiveLiveStream {
  readonly throughEventCursor: AgentLiveEventCursor;
  readonly liveStreamId: string;
  cancel(): Promise<void>;
}

export interface AgentPendingHistoryAttach {
  readonly response: AgentBeginSessionHistoryAttachResponse;
  activate(): Promise<AgentActiveLiveStream>;
  cancel(): Promise<void>;
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
}

export type AgentPermissionDecision = "allow_once" | "deny_once" | "cancel";

declare const agentExecutionTargetIdBrand: unique symbol;

/**
 * Stable, transport-opaque identity for the Maple installation executing an
 * Agent operation. The string remains serializable while callers must obtain a
 * target through one of the factories below instead of passing an arbitrary
 * session or device string to AgentRuntimeService.
 */
export type AgentExecutionTargetId = string & {
  readonly [agentExecutionTargetIdBrand]: "AgentExecutionTargetId";
};

export interface AgentExecutionTarget {
  readonly id: AgentExecutionTargetId;
  readonly kind: "local" | "remote";
  /** Human-facing label only. It is never used for routing or authorization. */
  readonly displayName?: string;
}

const LOCAL_AGENT_EXECUTION_TARGET_ID = "local" as AgentExecutionTargetId;
const MAX_AGENT_EXECUTION_TARGET_ID_BYTES = 128;
const MAX_AGENT_HOST_EPOCH_BYTES = 20;
const MAX_U64_DECIMAL = "18446744073709551615";
const AGENT_EXECUTION_TARGET_ID_PATTERN = /^[A-Za-z0-9._:-]+$/;
export const DEFAULT_AGENT_PAGE_SIZE = 25;
export const MAX_AGENT_PAGE_SIZE = 50;
const MAX_AGENT_CURSOR_BYTES = 512;
const MAX_AGENT_HISTORY_ITEMS_PER_RECORD = 200;
const MAX_AGENT_HISTORY_ROLE_BYTES = 128;
const MAX_AGENT_LIVE_ITEMS_PER_SESSION = 200;
const MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT = 64;
const MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT = 512;
const MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT = 8 * 1024 * 1024;
const AGENT_LIVE_JOURNAL_ID_PATTERN = /^[0-9a-f]{32}$/;
const AGENT_LIVE_PRESENTATION_VERSION = 1;
const MAX_AGENT_LIVE_ID_BYTES = 128;
const MAX_AGENT_LIVE_TITLE_BYTES = 1_024;
const MAX_AGENT_LIVE_TEXT_BYTES = 192 * 1_024;
const MAX_AGENT_LIVE_STATUS_BYTES = 256;
const MAX_AGENT_LIVE_PROJECT_ROOT_BYTES = 4_096;
const MAX_AGENT_LIVE_MODEL_BYTES = 256;
const MAX_AGENT_LIVE_MODE_BYTES = 64;
export const MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES = 1_048_576 - 8_192;
const AGENT_SAFE_HISTORY_TOKEN_PATTERN = /^[A-Za-z0-9._:-]+$/;
const SAFE_REMOTE_SETUP_WARNING =
  "Some Agent integrations could not connect. Review Agent settings on the host.";
const SAFE_REMOTE_AGENT_ERROR =
  "The Agent task failed. Open the host for additional diagnostic details.";
const SAFE_REMOTE_TOOL_TITLE = "Tool activity";
const SAFE_REMOTE_TOOL_FAILED = "The tool failed. Open the host for additional diagnostic details.";
const SAFE_REMOTE_TOOL_CANCELLED = "The tool was cancelled.";
const SAFE_REMOTE_PERMISSION_TITLE = "Tool permission";

export const LOCAL_AGENT_EXECUTION_TARGET: AgentExecutionTarget = Object.freeze({
  id: LOCAL_AGENT_EXECUTION_TARGET_ID,
  kind: "local"
});

export function createRemoteAgentExecutionTarget(
  id: unknown,
  displayName?: unknown
): AgentExecutionTarget {
  if (
    !isString(id) ||
    id.length === 0 ||
    id.length > MAX_AGENT_EXECUTION_TARGET_ID_BYTES ||
    !AGENT_EXECUTION_TARGET_ID_PATTERN.test(id)
  ) {
    throw new Error(
      "A remote Agent execution target ID must be 1-128 ASCII letters, digits, '.', '_', ':', or '-'"
    );
  }
  if (id === LOCAL_AGENT_EXECUTION_TARGET_ID) {
    throw new Error(`The Agent execution target ID "${id}" is reserved`);
  }
  if (displayName !== undefined && !isString(displayName)) {
    throw new Error("A remote Agent execution target display name must be a string");
  }
  return Object.freeze({
    id: id as AgentExecutionTargetId,
    kind: "remote",
    ...(displayName ? { displayName } : {})
  });
}

interface AgentEventCommonFields {
  sessionId?: string | null;
  runId?: string | null;
  eventEpoch?: string | null;
  eventSequence?: number | null;
  item?: AgentTimelineItem | null;
  status?: AgentRuntimeStatus | null;
  session?: AgentSessionSummary | null;
  message?: string | null;
}

export type AgentEventPayload = AgentEventCommonFields &
  (
    | { eventType: "runtimeStatus"; status: AgentRuntimeStatus }
    | { eventType: "sessionCreated"; sessionId: string; session: AgentSessionSummary }
    | {
        eventType: "sessionUpdated";
        sessionId: string;
        runId?: string | null;
        session: AgentSessionSummary;
      }
    | {
        eventType: "timelineItem";
        sessionId: string;
        runId?: string | null;
        item: AgentTimelineItem;
      }
    | { eventType: "runStarted"; sessionId: string; runId: string }
    | { eventType: "error"; runId: string; message: string }
    | {
        eventType: "error";
        sessionId: string;
        runId: string;
        item: AgentTimelineItem;
        message?: string | null;
      }
    | { eventType: "historyReplaced"; sessionId: string; runId: string }
    | {
        eventType: "runFinished";
        sessionId: string;
        runId: string;
        message: "completed" | "cancelled" | "failed";
      }
  );

export type AgentEventType = AgentEventPayload["eventType"];

/** Targeted events carry the full verified host incarnation + reconnect stamp. */
export type TargetedAgentEventEnvelope = AgentEventPayload & {
  eventEpoch: string;
  eventSequence: number;
  targetId: AgentExecutionTargetId;
  hostEpoch: string;
  connectionGeneration: number;
};

/** Embedded compatibility events are target-normalized but not yet sequenced. */
export type EmbeddedAgentEventEnvelope = AgentEventPayload & {
  targetId: AgentExecutionTargetId;
  connectionGeneration: 0;
};

/** Backward-compatible embedded shape. Target metadata is never partially present. */
export interface LegacyLocalAgentEventEnvelope extends AgentEventCommonFields {
  eventType: AgentEventType;
  targetId?: never;
  connectionGeneration?: never;
}

export type AgentEventEnvelope =
  | TargetedAgentEventEnvelope
  | EmbeddedAgentEventEnvelope
  | LegacyLocalAgentEventEnvelope;
export type AgentEventHandler = (event: AgentEventEnvelope) => void;
export type AgentBridgeEventHandler = (event: unknown) => void;
export type UnlistenAgentEvents = () => void;

declare const agentExecutionLeaseBrand: unique symbol;

/**
 * Native-issued authority for one account, verified host registration,
 * non-reusable host incarnation, and transport generation. It is constructed
 * only by runtime validation of a bridge result, then frozen before handoff.
 */
export type AgentExecutionLease = Readonly<{
  accountId: string;
  targetId: AgentExecutionTargetId;
  hostEpoch: string;
  connectionGeneration: number;
  [agentExecutionLeaseBrand]: "AgentExecutionLease";
}>;

const LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION = {
  getRuntimeStatus: "agent_get_runtime_status",
  startRuntime: "agent_start_runtime",
  restartRuntime: "agent_restart_runtime",
  stopRuntime: "agent_stop_runtime",
  clearUserData: "agent_clear_user_data",
  clearUserHistory: "agent_clear_user_history",
  loadConfig: "agent_load_config",
  saveConfig: "agent_save_config",
  listMcpServers: "agent_list_mcp_servers",
  saveMcpServers: "agent_save_mcp_servers",
  listSessionMcpServers: "agent_list_session_mcp_servers",
  setSessionMcpServerEnabled: "agent_set_session_mcp_server_enabled",
  listRecentProjectRoots: "agent_list_recent_project_roots",
  saveRecentProjectRoot: "agent_save_recent_project_root",
  removeProjectRoot: "agent_remove_project_root",
  getProjectSkillsTrust: "agent_get_project_skills_trust",
  setProjectSkillsTrust: "agent_set_project_skills_trust",
  saveProjectRootOrder: "agent_save_project_root_order",
  createSession: "agent_create_session",
  listSessions: "agent_list_sessions",
  loadSession: "agent_load_session",
  listSessionsPage: "agent_list_sessions_page",
  listSessionRecordsPage: "agent_list_session_records_page",
  renameSession: "agent_rename_session",
  deleteSession: "agent_delete_session",
  sendMessage: "agent_send_message",
  cancelRun: "agent_cancel_run",
  setPermissionMode: "agent_set_permission_mode",
  respondToPermission: "agent_permission_respond"
} as const;

export interface AgentRuntimeOperationRequestMap {
  getRuntimeStatus: undefined;
  startRuntime: { request: AgentStartRequest | null };
  restartRuntime: { request: AgentStartRequest | null };
  stopRuntime: undefined;
  clearUserData: undefined;
  clearUserHistory: undefined;
  loadConfig: undefined;
  saveConfig: { config: AgentConfig };
  listMcpServers: undefined;
  saveMcpServers: { servers: AgentMcpServer[] };
  listSessionMcpServers: { sessionId: string };
  setSessionMcpServerEnabled: {
    request: { sessionId: string; name: string; enabled: boolean };
  };
  listRecentProjectRoots: undefined;
  saveRecentProjectRoot: { path: string };
  removeProjectRoot: { path: string; fallbackPath: string | null };
  getProjectSkillsTrust: { path: string };
  setProjectSkillsTrust: { path: string; trusted: boolean };
  saveProjectRootOrder: { paths: string[] };
  createSession: { request: AgentCreateSessionRequest | null };
  listSessions: { projectRoot: string | null };
  loadSession: { sessionId: string };
  listSessionsPage: { request: AgentListSessionsPageRequest };
  listSessionRecordsPage: { request: AgentListSessionRecordsPageRequest };
  renameSession: { request: AgentRenameSessionRequest };
  deleteSession: { sessionId: string };
  sendMessage: { request: AgentSendMessageRequest };
  cancelRun: { runId: string };
  setPermissionMode: { request: { sessionId: string; mode: string } };
  respondToPermission: {
    response: { sessionId: string; requestId: string; decision: AgentPermissionDecision };
  };
}

export interface AgentRuntimeOperationResultMap {
  getRuntimeStatus: AgentRuntimeStatus;
  startRuntime: AgentRuntimeStatus;
  restartRuntime: AgentRuntimeLifecycleOutcome;
  stopRuntime: AgentRuntimeLifecycleOutcome;
  clearUserData: void;
  clearUserHistory: void;
  loadConfig: AgentConfig;
  saveConfig: void;
  listMcpServers: AgentMcpServer[];
  saveMcpServers: AgentMcpServer[];
  listSessionMcpServers: AgentSessionMcpServer[];
  setSessionMcpServerEnabled: AgentSessionMcpServer[];
  listRecentProjectRoots: RecentProjectRoot[];
  saveRecentProjectRoot: AgentProjectRootRegistration;
  removeProjectRoot: AgentConfig;
  getProjectSkillsTrust: AgentProjectSkillsTrustStatus;
  setProjectSkillsTrust: AgentProjectSkillsTrustStatus;
  saveProjectRootOrder: RecentProjectRoot[];
  createSession: AgentSessionDetail;
  listSessions: AgentSessionSummary[];
  loadSession: AgentSessionDetail;
  listSessionsPage: AgentPage<AgentSessionSummary>;
  listSessionRecordsPage: AgentSessionRecordsPage;
  renameSession: AgentSessionSummary;
  deleteSession: void;
  sendMessage: AgentRunResponse;
  cancelRun: void;
  setPermissionMode: void;
  respondToPermission: void;
}

/** Deliberate Maple operation vocabulary, independent of Tauri command names. */
export type AgentRuntimeOperation = keyof AgentRuntimeOperationRequestMap;
type AgentRemoteRuntimeOperation = Exclude<AgentRuntimeOperation, "listSessions" | "loadSession">;

type AgentRemoteRuntimeInvocation = {
  [Operation in AgentRemoteRuntimeOperation]: AgentRuntimeOperationRequestMap[Operation] extends undefined
    ? { operation: Operation }
    : { operation: Operation; request: AgentRuntimeOperationRequestMap[Operation] };
}[AgentRemoteRuntimeOperation];

/**
 * Controller-side request vocabulary only. The remote host adapter must bind
 * these operations to reviewed run capabilities, never to Tauri Desktop
 * command dispatch or another arbitrary string calling surface.
 */
export type AgentRuntimeInvocation = AgentRemoteRuntimeInvocation;

export interface AgentRuntimeBridge {
  /** Embedded-only Maple API credential sync. Never call this for a remote target. */
  syncLocalAuth?(userId: string): Promise<void>;
  /**
   * Ensure an already paired target is locally ready. Implementations may use
   * cached endpoint/key state, but must not forward account tokens or place an
   * enclave grant on the per-command/reconnect path. The returned native value
   * is decoded into a frozen AgentExecutionLease before any bridge call.
   */
  prepareTarget?(userId: string, target: AgentExecutionTarget): Promise<unknown>;
  runForUser<T>(
    userId: string,
    operation: () => Promise<T>,
    target?: AgentExecutionTarget
  ): Promise<T>;
  /**
   * Backward-compatible local Tauri invocation seam. Remote bridges implement
   * invokeTarget so native command strings never become their wire protocol.
   */
  invoke?<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  invokeTarget?(lease: AgentExecutionLease, invocation: AgentRuntimeInvocation): Promise<unknown>;
  listenToEvents?(
    /** Null exists only for the legacy embedded caller. */
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    handler: AgentBridgeEventHandler
  ): Promise<UnlistenAgentEvents>;
  beginSessionHistoryAttach?(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    request: AgentListSessionRecordsPageRequest,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult>;
  activateSessionHistoryAttach?(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    attachId: string
  ): Promise<unknown>;
  cancelSessionHistoryAttach?(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    attachId: string
  ): Promise<void>;
  resumeLiveEvents?(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    cursor: AgentLiveEventCursor,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult>;
  cancelLiveEvents?(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    liveStreamId: string
  ): Promise<void>;
}

export interface AgentBridgeLiveChannelResult {
  /** Raw result decoded by AgentRuntimeService before any cache mutation. */
  readonly result: unknown;
  /** Strong ownership for a Tauri Channel or remote stream callback. */
  readonly keepAlive: object;
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

export class AgentPageStaleError extends Error {
  constructor() {
    super("Agent history changed; reload its newest page");
    this.name = "AgentPageStaleError";
  }
}

export function isAgentPageStaleError(error: unknown): error is AgentPageStaleError {
  return error instanceof AgentPageStaleError;
}

export class AgentHistoryRecordTooLargeError extends Error {
  constructor() {
    super("An Agent history record is too large to present safely");
    this.name = "AgentHistoryRecordTooLargeError";
  }
}

const defaultAgentRuntimeBridge: AgentRuntimeBridge = {
  syncLocalAuth: async (userId) => await mapleApiAuthService.sync(userId),
  runForUser: async (userId, operation) => await agentOperationFence.run(userId, operation),
  invoke: invokeAgent,
  listenToEvents: listenToLocalAgentEvents,
  beginSessionHistoryAttach: beginLocalSessionHistoryAttach,
  activateSessionHistoryAttach: activateLocalSessionHistoryAttach,
  cancelSessionHistoryAttach: cancelLocalSessionHistoryAttach,
  resumeLiveEvents: resumeLocalLiveEvents,
  cancelLiveEvents: cancelLocalLiveEvents
};

const agentRuntimeStopCoordinator = new AgentRuntimeStopCoordinator({
  blockAndDrain: async (userId) => await agentOperationFence.blockAndDrain(userId),
  // The cleanup lease is already held. Native code owns the single composite
  // ACP-plus-runtime lifecycle gate; the manual ACP Stop command is reserved
  // for the settings page because it also changes saved configuration.
  stopHost: async (userId) => {
    return await invokeAgent<AgentRuntimeLifecycleOutcome>(
      LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION.stopRuntime,
      { userId }
    );
  }
});

interface RemoteAgentSubscriptionBinding {
  readonly unlisten: UnlistenAgentEvents;
  cancelled: boolean;
  cancellationPromise: Promise<void> | null;
}

interface RemoteAgentLogicalSubscription {
  readonly accountId: string;
  readonly handler: AgentEventHandler;
  lease: AgentExecutionLease;
  binding: RemoteAgentSubscriptionBinding | null;
  readonly cleanupBindings: Set<RemoteAgentSubscriptionBinding>;
  readonly replacementOpenings: Set<RemoteAgentResourceOpening>;
  bindingEpoch: number;
  closeRequested: boolean;
  cancellationPromise: Promise<void> | null;
  closed: boolean;
}

type RemoteAgentSubscriptionBindPhase = "initial" | "replacement";

interface RemoteAgentLiveStreamBinding {
  readonly lease: AgentExecutionLease;
  readonly liveStreamId: string;
  readonly keepAlive: unknown;
  cancelled: boolean;
  cancellationPromise: Promise<void> | null;
}

interface RemoteAgentLogicalLiveStream {
  readonly accountId: string;
  readonly handler: AgentLiveChannelHandler;
  retainedCursor: AgentLiveEventCursor | null;
  binding: RemoteAgentLiveStreamBinding | null;
  readonly cleanupBindings: Set<RemoteAgentLiveStreamBinding>;
  readonly replacementOpenings: Set<RemoteAgentResourceOpening>;
  bindingEpoch: number;
  registered: boolean;
  closeRequested: boolean;
  cancellationPromise: Promise<void> | null;
  closed: boolean;
}

interface RemoteAgentPendingHistoryAttachResource {
  readonly accountId: string;
  readonly lease: AgentExecutionLease;
  readonly attachId: string;
  readonly keepAlive: unknown;
  phase: "pending" | "active";
  cleanupRequired: boolean;
  cancelled: boolean;
  cancellationPromise: Promise<void> | null;
}

interface RemoteAgentResourceOpening {
  readonly accountId: string;
  retired: boolean;
  readonly settled: Promise<void>;
  settle(): void;
}

class AgentRuntimeAccountResourceRegistry {
  private readonly scopesByAccount = new Map<string, Set<AgentRuntimeServiceScope>>();
  private readonly blockedAccounts = new Set<string>();
  private readonly retirements = new Map<string, Promise<void>>();

  claim(scope: AgentRuntimeServiceScope, accountId: string): void {
    if (this.blockedAccounts.has(accountId)) {
      throw new Error("Remote Agent account resources are blocked during account retirement");
    }
    const scopes = this.scopesByAccount.get(accountId) ?? new Set<AgentRuntimeServiceScope>();
    scopes.add(scope);
    this.scopesByAccount.set(accountId, scopes);
  }

  release(scope: AgentRuntimeServiceScope, accountId: string): void {
    const scopes = this.scopesByAccount.get(accountId);
    if (!scopes) return;
    scopes.delete(scope);
    if (scopes.size === 0) this.scopesByAccount.delete(accountId);
  }

  activateAccount(accountId: string): void {
    this.blockedAccounts.delete(accountId);
  }

  async retryAccountCleanup(accountId: string): Promise<void> {
    const scopes = [...(this.scopesByAccount.get(accountId) ?? [])];
    const results = await Promise.allSettled(
      scopes.map((scope) => scope.retryAccountCleanup(accountId))
    );
    throwAgentCleanupFailures("Unable to retry retained remote Agent cleanup", results);
  }

  async retireAccount(accountId: string): Promise<void> {
    const existing = this.retirements.get(accountId);
    if (existing) return await existing;
    this.blockedAccounts.add(accountId);
    const retirement = Promise.resolve().then(async () => {
      const scopes = [...(this.scopesByAccount.get(accountId) ?? [])];
      const results = await Promise.allSettled(
        scopes.map((scope) => scope.retireAccount(accountId))
      );
      throwAgentCleanupFailures("Unable to retire remote Agent account resources", results);
    });
    this.retirements.set(accountId, retirement);
    try {
      await retirement;
    } finally {
      if (this.retirements.get(accountId) === retirement) this.retirements.delete(accountId);
    }
  }
}

const agentRuntimeAccountResourceRegistry = new AgentRuntimeAccountResourceRegistry();

class AgentRuntimeServiceScope {
  private readonly services = new Set<AgentRuntimeService>();
  private readonly retiringAccounts = new Map<string, Promise<void>>();

  register(service: AgentRuntimeService): void {
    this.services.add(service);
  }

  claimAccount(accountId: string): void {
    if (this.retiringAccounts.has(accountId)) {
      throw new Error("Remote Agent service scope is retiring this account");
    }
    agentRuntimeAccountResourceRegistry.claim(this, accountId);
  }

  releaseAccountIfIdle(accountId: string): void {
    for (const service of this.services) {
      if (service.ownsRemoteAccountResources(accountId)) return;
    }
    agentRuntimeAccountResourceRegistry.release(this, accountId);
  }

  async retireAccount(accountId: string): Promise<void> {
    const existing = this.retiringAccounts.get(accountId);
    if (existing) return await existing;
    const retirement = Promise.resolve().then(async () => {
      const results = await Promise.allSettled(
        [...this.services].map((service) => service.retireOwnAccount(accountId))
      );
      throwAgentCleanupFailures("Unable to retire remote Agent service resources", results);
      this.releaseAccountIfIdle(accountId);
    });
    this.retiringAccounts.set(accountId, retirement);
    try {
      await retirement;
    } finally {
      if (this.retiringAccounts.get(accountId) === retirement) {
        this.retiringAccounts.delete(accountId);
      }
    }
  }

  async retryAccountCleanup(accountId: string): Promise<void> {
    const results = await Promise.allSettled(
      [...this.services].map((service) => service.retryOwnRemoteCleanupForAccount(accountId))
    );
    throwAgentCleanupFailures("Unable to retry retained remote Agent service cleanup", results);
    this.releaseAccountIfIdle(accountId);
  }
}

export class AgentRuntimeService {
  private currentLease: AgentExecutionLease | null = null;
  private preparationEpoch = 0;
  private preparingAccountId: string | null = null;
  private preparationInFlight: {
    accountId: string;
    promise: Promise<AgentExecutionLease>;
  } | null = null;
  private readonly remoteSubscriptions = new Set<RemoteAgentLogicalSubscription>();
  private readonly remoteLiveStreams = new Set<RemoteAgentLogicalLiveStream>();
  private readonly remotePendingHistoryAttaches =
    new Set<RemoteAgentPendingHistoryAttachResource>();
  private readonly remoteResourceOpenings = new Set<RemoteAgentResourceOpening>();

  constructor(
    private readonly bridge: AgentRuntimeBridge = defaultAgentRuntimeBridge,
    readonly target: AgentExecutionTarget = LOCAL_AGENT_EXECUTION_TARGET,
    private readonly scope: AgentRuntimeServiceScope = new AgentRuntimeServiceScope()
  ) {
    this.scope.register(this);
  }

  forTarget(target: AgentExecutionTarget): AgentRuntimeService {
    return new AgentRuntimeService(this.bridge, target, this.scope);
  }

  async retireAccount(userId: string): Promise<void> {
    await this.scope.retireAccount(userId);
  }

  async retireOwnAccount(userId: string): Promise<void> {
    const openings = [...this.remoteResourceOpenings].filter(
      (opening) => opening.accountId === userId
    );
    for (const opening of openings) opening.retired = true;
    if (this.currentLease?.accountId === userId) this.currentLease = null;
    if (this.preparingAccountId === userId) {
      this.preparingAccountId = null;
      this.preparationEpoch += 1;
    }
    const firstCleanup = await Promise.allSettled([
      ...[...this.remoteSubscriptions]
        .filter((subscription) => subscription.accountId === userId)
        .map((subscription) => this.closeRemoteSubscription(subscription)),
      ...[...this.remotePendingHistoryAttaches]
        .filter((resource) => resource.accountId === userId)
        .map((resource) => this.cancelRemotePendingHistoryAttach(resource, false)),
      ...[...this.remoteLiveStreams]
        .filter((stream) => stream.accountId === userId)
        .map((stream) => this.closeRemoteLiveStream(stream, false)),
      ...openings.map((opening) => opening.settled)
    ]);
    const secondCleanup = await Promise.allSettled([
      ...[...this.remoteSubscriptions]
        .filter((subscription) => subscription.accountId === userId)
        .map((subscription) => this.closeRemoteSubscription(subscription)),
      ...[...this.remotePendingHistoryAttaches]
        .filter((resource) => resource.accountId === userId)
        .map((resource) => this.cancelRemotePendingHistoryAttach(resource, false)),
      ...[...this.remoteLiveStreams]
        .filter((stream) => stream.accountId === userId)
        .map((stream) => this.closeRemoteLiveStream(stream, false))
    ]);
    this.scope.releaseAccountIfIdle(userId);
    throwAgentCleanupFailures("Unable to retire remote Agent account resources", [
      ...firstCleanup,
      ...secondCleanup
    ]);
  }

  ownsRemoteAccountResources(userId: string): boolean {
    return (
      [...this.remoteResourceOpenings].some((opening) => opening.accountId === userId) ||
      [...this.remoteSubscriptions].some(
        (subscription) => !subscription.closed && subscription.accountId === userId
      ) ||
      [...this.remotePendingHistoryAttaches].some(
        (resource) => !resource.cancelled && resource.accountId === userId
      ) ||
      [...this.remoteLiveStreams].some((stream) => !stream.closed && stream.accountId === userId)
    );
  }

  /**
   * Structural capability check for route plumbing only. This does not confer
   * pairing authority: every operation still calls prepareTarget and accepts
   * only the exact native-issued execution lease before crossing the bridge.
   */
  supportsRemoteAgentMode(): boolean {
    return (
      this.target.kind === "remote" &&
      typeof this.bridge.prepareTarget === "function" &&
      typeof this.bridge.invokeTarget === "function" &&
      typeof this.bridge.listenToEvents === "function" &&
      typeof this.bridge.beginSessionHistoryAttach === "function" &&
      typeof this.bridge.activateSessionHistoryAttach === "function" &&
      typeof this.bridge.cancelSessionHistoryAttach === "function" &&
      typeof this.bridge.resumeLiveEvents === "function" &&
      typeof this.bridge.cancelLiveEvents === "function"
    );
  }

  async getRuntimeStatus(userId: string): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser(userId, "getRuntimeStatus", undefined);
  }

  async startRuntime(userId: string, request?: AgentStartRequest): Promise<AgentRuntimeStatus> {
    return await this.invokeForUser(userId, "startRuntime", {
      request: request ?? null
    });
  }

  async restartRuntime(
    userId: string,
    request?: AgentStartRequest
  ): Promise<AgentRuntimeLifecycleOutcome> {
    return await this.invokeForUser(userId, "restartRuntime", {
      request: request ?? null
    });
  }

  async stopRuntime(userId: string): Promise<AgentRuntimeLifecycleOutcome> {
    return await this.invokeControlForUser(userId, "stopRuntime", undefined);
  }

  async clearUserData(userId: string): Promise<void> {
    await this.invokeForUser(userId, "clearUserData", undefined);
  }

  async clearUserHistory(userId: string): Promise<void> {
    await this.invokeForUser(userId, "clearUserHistory", undefined);
  }

  async loadConfig(userId: string): Promise<AgentConfig> {
    return await this.invokeForUser(userId, "loadConfig", undefined);
  }

  async saveConfig(userId: string, config: AgentConfig): Promise<void> {
    await this.invokeForUser(userId, "saveConfig", { config });
  }

  async listMcpServers(userId: string): Promise<AgentMcpServer[]> {
    return await this.invokeForUser(userId, "listMcpServers", undefined);
  }

  async saveMcpServers(userId: string, servers: AgentMcpServer[]): Promise<AgentMcpServer[]> {
    return await this.invokeForUser(userId, "saveMcpServers", {
      servers
    });
  }

  async listSessionMcpServers(userId: string, sessionId: string): Promise<AgentSessionMcpServer[]> {
    return await this.invokeForUser(userId, "listSessionMcpServers", {
      sessionId
    });
  }

  async setSessionMcpServerEnabled(
    userId: string,
    sessionId: string,
    name: string,
    enabled: boolean
  ): Promise<AgentSessionMcpServer[]> {
    return await this.invokeForUser(userId, "setSessionMcpServerEnabled", {
      request: { sessionId, name, enabled }
    });
  }

  async listRecentProjectRoots(userId: string): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser(userId, "listRecentProjectRoots", undefined);
  }

  async saveRecentProjectRoot(userId: string, path: string): Promise<AgentProjectRootRegistration> {
    return await this.invokeForUser(userId, "saveRecentProjectRoot", {
      path
    });
  }

  async removeProjectRoot(
    userId: string,
    path: string,
    fallbackPath?: string | null
  ): Promise<AgentConfig> {
    return await this.invokeForUser(userId, "removeProjectRoot", {
      path,
      fallbackPath: fallbackPath ?? null
    });
  }

  async getProjectSkillsTrust(
    userId: string,
    path: string
  ): Promise<AgentProjectSkillsTrustStatus> {
    return await this.invokeForUser(userId, "getProjectSkillsTrust", { path });
  }

  async setProjectSkillsTrust(
    userId: string,
    path: string,
    trusted: boolean
  ): Promise<AgentProjectSkillsTrustStatus> {
    return await this.invokeForUser(userId, "setProjectSkillsTrust", { path, trusted });
  }

  async saveProjectRootOrder(userId: string, paths: string[]): Promise<RecentProjectRoot[]> {
    return await this.invokeForUser(userId, "saveProjectRootOrder", {
      paths
    });
  }

  async createSession(
    userId: string,
    request?: AgentCreateSessionRequest
  ): Promise<AgentSessionDetail> {
    return await this.invokeForUser(userId, "createSession", {
      request: request ?? null
    });
  }

  async listSessions(userId: string, projectRoot?: string | null): Promise<AgentSessionSummary[]> {
    this.assertLocalCompatibilityOperation("listSessions");
    return await this.invokeForUser(userId, "listSessions", {
      projectRoot: projectRoot ?? null
    });
  }

  async loadSession(userId: string, sessionId: string): Promise<AgentSessionDetail> {
    this.assertLocalCompatibilityOperation("loadSession");
    return await this.invokeForUser(userId, "loadSession", {
      sessionId
    });
  }

  async listSessionsPage(
    userId: string,
    request: AgentListSessionsPageRequest = {}
  ): Promise<AgentPage<AgentSessionSummary>> {
    validateAgentListSessionsPageRequest(request);
    let page: AgentPage<AgentSessionSummary>;
    try {
      page = await this.invokeForUser(userId, "listSessionsPage", { request });
    } catch (error) {
      throw normalizeAgentPageError(error);
    }
    validateReturnedAgentPage("session", request, page.items.length, page.nextCursor ?? null);
    return page;
  }

  async listSessionRecordsPage(
    userId: string,
    request: AgentListSessionRecordsPageRequest
  ): Promise<AgentSessionRecordsPage> {
    validateAgentListSessionRecordsPageRequest(request);
    let page: AgentSessionRecordsPage;
    try {
      page = await this.invokeForUser(userId, "listSessionRecordsPage", { request });
    } catch (error) {
      throw normalizeAgentPageError(error);
    }
    validateReturnedAgentPage(
      "history record",
      request,
      page.records.length,
      page.nextCursor ?? null
    );
    return page;
  }

  async beginSessionHistoryAttach(
    userId: string,
    request: AgentListSessionRecordsPageRequest,
    handler: AgentLiveChannelHandler
  ): Promise<AgentPendingHistoryAttach> {
    validateAgentListSessionRecordsPageRequest(request);
    if (request.cursor !== undefined && request.cursor !== null) {
      throw new Error("A synchronized Agent history attach must start at the newest page");
    }
    const opening = this.target.kind === "remote" ? this.beginRemoteResourceOpening(userId) : null;
    try {
      if (opening) await agentRuntimeAccountResourceRegistry.retryAccountCleanup(userId);
      return await this.bridge.runForUser(
        userId,
        async () => {
          const lease = await this.prepareInvocationTarget(userId, true);
          if (!lease) {
            throw new Error(
              "Synchronized Agent history requires a verified remote host connection stamp"
            );
          }
          if (opening?.retired) {
            throw new Error("Agent history attachment owner retired before opening");
          }
          const begin = this.bridge.beginSessionHistoryAttach;
          if (
            !begin ||
            !this.bridge.activateSessionHistoryAttach ||
            !this.bridge.cancelSessionHistoryAttach ||
            !this.bridge.cancelLiveEvents
          ) {
            throw new Error("Agent runtime bridge does not support synchronized history lifecycle");
          }
          const logicalStream = this.createRemoteLogicalLiveStream(userId, handler);
          let opened: AgentBridgeLiveChannelResult;
          try {
            opened = await begin.call(
              this.bridge,
              userId,
              lease,
              this.target,
              { ...request, cursor: null },
              this.liveChannelDecoder(lease, handler, logicalStream)
            );
          } catch (error) {
            throw normalizeAgentLiveError(error);
          }
          const provisionalAttachId = attachIdFromUnknown(opened.result);
          const pendingResource = provisionalAttachId
            ? this.createRemotePendingHistoryAttachResource(
                userId,
                lease,
                provisionalAttachId,
                opened.keepAlive
              )
            : null;
          let response: AgentBeginSessionHistoryAttachResponse;
          try {
            if (!isRecord(opened.keepAlive)) {
              throw new Error("Agent runtime bridge did not retain its live event channel");
            }
            response = decodeAgentBeginSessionHistoryAttachResponse(opened.result);
            validateReturnedAgentPage(
              "history record",
              request,
              response.page.records.length,
              response.page.nextCursor ?? null
            );
            if (!pendingResource || pendingResource.attachId !== response.attachId) {
              throw new Error("Agent history attachment lost its cleanup-owned native ID");
            }
            if (opening?.retired) {
              pendingResource.cleanupRequired = true;
              await this.cancelRemotePendingHistoryAttach(pendingResource, false);
              throw new Error("Agent history attachment owner retired while opening");
            }
          } catch (error) {
            if (pendingResource && !pendingResource.cancelled) {
              pendingResource.cleanupRequired = true;
              await this.cancelRemotePendingHistoryAttach(pendingResource, false);
            }
            throw error;
          }
          return this.createPendingHistoryAttach(
            userId,
            lease,
            response,
            logicalStream,
            pendingResource
          );
        },
        this.target
      );
    } finally {
      opening?.settle();
    }
  }

  async resumeLiveEvents(
    userId: string,
    cursor: AgentLiveEventCursor,
    handler: AgentLiveChannelHandler
  ): Promise<AgentActiveLiveStream> {
    validateAgentLiveEventCursor(cursor);
    const opening = this.target.kind === "remote" ? this.beginRemoteResourceOpening(userId) : null;
    try {
      if (opening) await agentRuntimeAccountResourceRegistry.retryAccountCleanup(userId);
      return await this.bridge.runForUser(
        userId,
        async () => {
          const lease = await this.prepareInvocationTarget(userId, true);
          if (!lease) {
            throw new Error("Agent live resume requires a verified remote host connection stamp");
          }
          if (opening?.retired) {
            throw new Error("Agent live resume owner retired before opening");
          }
          const resume = this.bridge.resumeLiveEvents;
          if (!resume || !this.bridge.cancelLiveEvents) {
            throw new Error("Agent runtime bridge does not support live event resume lifecycle");
          }
          const logicalStream = this.createRemoteLogicalLiveStream(userId, handler, cursor);
          let opened: AgentBridgeLiveChannelResult;
          try {
            opened = await resume.call(
              this.bridge,
              userId,
              lease,
              this.target,
              cursor,
              this.liveChannelDecoder(lease, handler, logicalStream)
            );
          } catch (error) {
            throw normalizeAgentLiveError(error);
          }
          const provisionalId = liveStreamIdFromUnknown(opened.result);
          const provisionalBinding =
            logicalStream && provisionalId
              ? this.createRemoteLiveStreamBinding(lease, provisionalId, opened.keepAlive)
              : null;
          if (logicalStream && provisionalBinding) {
            this.retainRemoteLiveStreamBinding(logicalStream, provisionalBinding);
          }
          let barrier: AgentLiveBarrierResponse;
          try {
            if (!isRecord(opened.keepAlive)) {
              throw new Error("Agent runtime bridge did not retain its live event channel");
            }
            barrier = decodeAgentLiveBarrierResponse(opened.result);
            if (!provisionalBinding || provisionalBinding.liveStreamId !== barrier.liveStreamId) {
              throw new Error("Agent live resume lost its cleanup-owned native ID");
            }
            if (
              barrier.throughEventCursor.journalId !== cursor.journalId ||
              barrier.throughEventCursor.sequence < cursor.sequence
            ) {
              throw new Error("Agent live resume returned a regressing event checkpoint");
            }
            if (opening?.retired) {
              throw new Error("Agent live resume owner retired while opening");
            }
          } catch (error) {
            if (logicalStream?.registered) {
              logicalStream.closeRequested = true;
              logicalStream.bindingEpoch += 1;
              await this.closeRemoteLiveStream(logicalStream, false);
            }
            throw error;
          }
          return this.createActiveLiveStream(
            userId,
            lease,
            opened.keepAlive,
            barrier,
            logicalStream,
            provisionalBinding
          );
        },
        this.target
      );
    } finally {
      opening?.settle();
    }
  }

  async renameSession(
    userId: string,
    request: AgentRenameSessionRequest
  ): Promise<AgentSessionSummary> {
    return await this.invokeForUser(userId, "renameSession", {
      request
    });
  }

  async deleteSession(userId: string, sessionId: string): Promise<void> {
    await this.invokeForUser(userId, "deleteSession", { sessionId });
  }

  async sendMessage(userId: string, request: AgentSendMessageRequest): Promise<AgentRunResponse> {
    return await this.invokeForUser(userId, "sendMessage", {
      request
    });
  }

  async cancelRun(userId: string, runId: string): Promise<void> {
    // Cancellation is a target control-plane operation. Keep it account- and
    // target-fenced, but never delay Stop on credential validation or refresh.
    await this.invokeControlForUser(userId, "cancelRun", { runId });
  }

  async setPermissionMode(userId: string, sessionId: string, mode: string): Promise<void> {
    await this.invokeForUser(userId, "setPermissionMode", {
      request: { sessionId, mode }
    });
  }

  async respondToPermission(
    userId: string,
    sessionId: string,
    requestId: string,
    decision: AgentPermissionDecision
  ): Promise<void> {
    await this.invokeForUser(userId, "respondToPermission", {
      response: { sessionId, requestId, decision }
    });
  }

  async listenToEvents(handler: AgentEventHandler): Promise<UnlistenAgentEvents>;
  async listenToEvents(userId: string, handler: AgentEventHandler): Promise<UnlistenAgentEvents>;
  async listenToEvents(
    userIdOrHandler: string | AgentEventHandler,
    accountHandler?: AgentEventHandler
  ): Promise<UnlistenAgentEvents> {
    const userId = typeof userIdOrHandler === "string" ? userIdOrHandler : null;
    const handler = typeof userIdOrHandler === "string" ? accountHandler : userIdOrHandler;
    if (!handler) throw new Error("Agent event subscription requires a handler");
    if (this.target.kind === "remote" && !userId) {
      throw new Error("Remote Agent event subscription requires an authenticated user");
    }
    if (!this.bridge.listenToEvents) {
      if (this.target.kind === "remote") {
        throw new Error(
          `Agent runtime bridge does not support events for remote target "${this.target.id as string}"`
        );
      }
      return () => {};
    }
    if (this.target.kind === "remote") {
      const accountId = userId as string;
      const opening = this.beginRemoteResourceOpening(accountId);
      try {
        await agentRuntimeAccountResourceRegistry.retryAccountCleanup(accountId);
        return await this.bridge.runForUser(
          accountId,
          async () => {
            const lease = await this.prepareRemoteLease(accountId);
            if (opening.retired) {
              throw new Error("Remote Agent event subscription owner retired before binding");
            }
            if (!sameExecutionLease(this.currentLease, lease)) {
              throw new Error(
                "Remote Agent execution lease changed before event subscription handoff"
              );
            }
            const subscription: RemoteAgentLogicalSubscription = {
              accountId,
              handler,
              lease,
              binding: null,
              cleanupBindings: new Set(),
              replacementOpenings: new Set(),
              bindingEpoch: 0,
              closeRequested: false,
              cancellationPromise: null,
              closed: false
            };
            this.remoteSubscriptions.add(subscription);
            try {
              await this.bindRemoteSubscription(subscription, lease, "initial");
              if (opening.retired) {
                await this.closeRemoteSubscription(subscription);
                throw retiredInitialRemoteSubscriptionError();
              }
            } catch (error) {
              if (subscription.closed) throw error;
              await this.closeRemoteSubscription(subscription);
              throw error;
            }
            return () => {
              void this.closeRemoteSubscription(subscription).catch(() => {
                // Cleanup ownership remains registered for account retirement
                // or the next same-account resource open to retry durably.
              });
            };
          },
          this.target
        );
      } finally {
        opening.settle();
      }
    }

    return await this.bridge.listenToEvents(null, this.target, (event) => {
      // The current embedded Tauri emitter has neither field. Normalize its
      // single local stream into generation zero until native events carry it.
      const decoded = decodeLegacyLocalAgentEvent(event);
      if (!decoded) return;
      handler({
        ...decoded,
        targetId: this.target.id,
        connectionGeneration: 0
      });
    });
  }

  private liveChannelDecoder(
    lease: AgentExecutionLease,
    handler: AgentLiveChannelHandler,
    logicalStream: RemoteAgentLogicalLiveStream | null = null,
    bindingEpoch = logicalStream?.bindingEpoch ?? 0
  ): AgentBridgeEventHandler {
    return (value) => {
      if (this.target.kind === "remote" && !sameExecutionLease(this.currentLease, lease)) return;
      if (
        logicalStream &&
        (logicalStream.closed ||
          logicalStream.closeRequested ||
          logicalStream.bindingEpoch !== bindingEpoch)
      ) {
        return;
      }
      const frame = decodeAgentLiveChannelFrame(
        value,
        lease.targetId,
        lease.hostEpoch,
        lease.connectionGeneration
      );
      if (!frame) return;
      if (logicalStream) {
        const cursor =
          frame.eventType === "snapshotRequired"
            ? frame.lastEventCursor
            : { journalId: frame.eventEpoch, sequence: frame.eventSequence };
        if (
          !logicalStream.retainedCursor ||
          (logicalStream.retainedCursor.journalId === cursor.journalId &&
            logicalStream.retainedCursor.sequence <= cursor.sequence)
        ) {
          logicalStream.retainedCursor = cursor;
        }
      }
      handler(frame);
    };
  }

  private createRemoteLogicalLiveStream(
    accountId: string,
    handler: AgentLiveChannelHandler,
    cursor: AgentLiveEventCursor | null = null
  ): RemoteAgentLogicalLiveStream | null {
    if (this.target.kind !== "remote") return null;
    return {
      accountId,
      handler,
      retainedCursor: cursor,
      binding: null,
      cleanupBindings: new Set(),
      replacementOpenings: new Set(),
      bindingEpoch: 0,
      registered: false,
      closeRequested: false,
      cancellationPromise: null,
      closed: false
    };
  }

  private beginRemoteResourceOpening(accountId: string): RemoteAgentResourceOpening {
    this.scope.claimAccount(accountId);
    let settled = false;
    let resolveSettled!: () => void;
    const opening: RemoteAgentResourceOpening = {
      accountId,
      retired: false,
      settled: new Promise<void>((resolve) => {
        resolveSettled = resolve;
      }),
      settle: () => {
        if (settled) return;
        settled = true;
        this.remoteResourceOpenings.delete(opening);
        resolveSettled();
        this.scope.releaseAccountIfIdle(accountId);
      }
    };
    this.remoteResourceOpenings.add(opening);
    return opening;
  }

  private beginRemoteLiveStreamReplacementOpening(
    logicalStream: RemoteAgentLogicalLiveStream
  ): RemoteAgentResourceOpening {
    const opening = this.beginRemoteResourceOpening(logicalStream.accountId);
    const settleOpening = opening.settle;
    opening.settle = () => {
      logicalStream.replacementOpenings.delete(opening);
      settleOpening();
    };
    logicalStream.replacementOpenings.add(opening);
    return opening;
  }

  async retryOwnRemoteCleanupForAccount(accountId: string): Promise<void> {
    const results = await Promise.allSettled([
      ...[...this.remoteSubscriptions]
        .filter(
          (subscription) => subscription.accountId === accountId && subscription.closeRequested
        )
        .map((subscription) => this.retryRemoteSubscriptionCleanup(subscription)),
      ...[...this.remotePendingHistoryAttaches]
        .filter((resource) => resource.accountId === accountId && resource.cleanupRequired)
        .map((resource) => this.retryRemotePendingHistoryAttachCleanup(resource)),
      ...[...this.remoteLiveStreams]
        .filter((stream) => stream.accountId === accountId && stream.closeRequested)
        .map((stream) => this.retryRemoteLiveStreamCleanup(stream))
    ]);
    throwAgentCleanupFailures("Unable to retry retained remote Agent live cleanup", results);
  }

  private async retryRemoteSubscriptionCleanup(
    subscription: RemoteAgentLogicalSubscription
  ): Promise<void> {
    const inheritedCancellation = subscription.cancellationPromise;
    if (inheritedCancellation) await inheritedCancellation.catch(() => {});
    if (!subscription.closed) await this.closeRemoteSubscription(subscription);
  }

  private async retryRemotePendingHistoryAttachCleanup(
    resource: RemoteAgentPendingHistoryAttachResource
  ): Promise<void> {
    const inheritedCancellation = resource.cancellationPromise;
    if (inheritedCancellation) await inheritedCancellation.catch(() => {});
    if (!resource.cancelled) await this.cancelRemotePendingHistoryAttach(resource, true);
  }

  private async retryRemoteLiveStreamCleanup(stream: RemoteAgentLogicalLiveStream): Promise<void> {
    const inheritedCancellation = stream.cancellationPromise;
    if (inheritedCancellation) await inheritedCancellation.catch(() => {});
    if (!stream.closed) await this.closeRemoteLiveStream(stream, true);
  }

  private createRemotePendingHistoryAttachResource(
    accountId: string,
    lease: AgentExecutionLease,
    attachId: string,
    keepAlive: unknown
  ): RemoteAgentPendingHistoryAttachResource {
    const resource: RemoteAgentPendingHistoryAttachResource = {
      accountId,
      lease,
      attachId,
      keepAlive,
      phase: "pending",
      cleanupRequired: false,
      cancelled: false,
      cancellationPromise: null
    };
    this.remotePendingHistoryAttaches.add(resource);
    return resource;
  }

  private releaseRemotePendingHistoryAttach(
    resource: RemoteAgentPendingHistoryAttachResource
  ): void {
    resource.cancelled = true;
    resource.cleanupRequired = false;
    this.remotePendingHistoryAttaches.delete(resource);
    this.scope.releaseAccountIfIdle(resource.accountId);
  }

  private async cancelRemotePendingHistoryAttach(
    resource: RemoteAgentPendingHistoryAttachResource,
    accountFenced: boolean
  ): Promise<void> {
    if (resource.cancelled) return;
    resource.cleanupRequired = true;
    const existing = resource.cancellationPromise;
    if (existing) return await existing;
    const cancellation = (async () => {
      await completeAgentLiveCleanup(async () => {
        const cleanup = async () => {
          if (resource.phase === "active") {
            const cancel = this.bridge.cancelLiveEvents;
            if (!cancel) throw new Error("Agent runtime bridge lost live stream cancellation");
            await cancel.call(
              this.bridge,
              resource.accountId,
              resource.lease,
              this.target,
              resource.attachId
            );
            return;
          }
          const cancel = this.bridge.cancelSessionHistoryAttach;
          if (!cancel) {
            throw new Error("Agent runtime bridge lost pending attachment cancellation");
          }
          await cancel.call(
            this.bridge,
            resource.accountId,
            resource.lease,
            this.target,
            resource.attachId
          );
        };
        if (accountFenced) {
          await this.runBoundLiveOperation(resource.accountId, resource.lease, false, cleanup);
        } else {
          await cleanup();
        }
      });
      this.releaseRemotePendingHistoryAttach(resource);
    })();
    resource.cancellationPromise = cancellation;
    try {
      await cancellation;
    } finally {
      if (resource.cancellationPromise === cancellation && !resource.cancelled) {
        resource.cancellationPromise = null;
      }
    }
  }

  private createPendingHistoryAttach(
    userId: string,
    lease: AgentExecutionLease,
    response: AgentBeginSessionHistoryAttachResponse,
    logicalStream: RemoteAgentLogicalLiveStream | null,
    pendingResource: RemoteAgentPendingHistoryAttachResource
  ): AgentPendingHistoryAttach {
    let lifecycle: "pending" | "activating" | "active" | "cancelled" = "pending";
    let activeStream: AgentActiveLiveStream | null = null;
    let activation: Promise<AgentActiveLiveStream> | null = null;
    let cleanupComplete = false;
    let cancellationPromise: Promise<void> | null = null;
    const isCancelled = () => lifecycle === "cancelled";

    const pending: AgentPendingHistoryAttach = {
      response,
      activate: async () => {
        if (lifecycle === "cancelled") {
          throw new Error("Agent history attachment was cancelled before activation");
        }
        if (lifecycle === "active" && activeStream) return activeStream;
        if (activation) return await activation;
        const activationOpening = this.beginRemoteResourceOpening(userId);
        lifecycle = "activating";
        activation = (async () => {
          const activate = this.bridge.activateSessionHistoryAttach;
          if (!activate) {
            throw new Error("Agent runtime bridge does not support history activation");
          }
          let barrier: AgentLiveBarrierResponse;
          let ownedBinding: RemoteAgentLiveStreamBinding | null = null;
          let activationReturned = false;
          try {
            const raw = await this.runBoundLiveOperation(userId, lease, true, async () => {
              return await activate.call(
                this.bridge,
                userId,
                lease,
                this.target,
                response.attachId
              );
            });
            activationReturned = true;
            // Native activation consumes the pending token before returning.
            // Transfer its exact ID into live cleanup ownership before parsing
            // or checking whether the component/account owner is still current.
            pendingResource.phase = "active";
            if (logicalStream) {
              ownedBinding = this.createRemoteLiveStreamBinding(
                lease,
                pendingResource.attachId,
                pendingResource.keepAlive
              );
              this.retainRemoteLiveStreamBinding(logicalStream, ownedBinding);
            }
            this.releaseRemotePendingHistoryAttach(pendingResource);
            barrier = decodeAgentLiveBarrierResponse(raw);
            if (barrier.liveStreamId !== response.attachId) {
              throw new Error("Agent history activation returned a mismatched live stream ID");
            }
            if (
              barrier.throughEventCursor.journalId !== response.throughEventCursor.journalId ||
              barrier.throughEventCursor.sequence < response.throughEventCursor.sequence
            ) {
              throw new Error("Agent history activation returned a regressing event checkpoint");
            }
          } catch (error) {
            if (activationReturned) pendingResource.phase = "active";
            if (logicalStream && !ownedBinding) {
              ownedBinding = this.createRemoteLiveStreamBinding(
                lease,
                pendingResource.attachId,
                pendingResource.keepAlive
              );
              this.retainRemoteLiveStreamBinding(logicalStream, ownedBinding);
            }
            if (activationReturned && logicalStream && !pendingResource.cancelled) {
              this.releaseRemotePendingHistoryAttach(pendingResource);
            }
            if (!pendingResource.cancelled) {
              pendingResource.cleanupRequired = true;
            }
            const cleanupResults = await Promise.allSettled([
              ...(logicalStream?.registered
                ? [this.closeRemoteLiveStream(logicalStream, true)]
                : []),
              ...(!pendingResource.cancelled
                ? [this.cancelRemotePendingHistoryAttach(pendingResource, true)]
                : [])
            ]);
            try {
              throwAgentCleanupFailures(
                "Unable to retire an ambiguously activated Agent attachment",
                cleanupResults
              );
            } catch (cleanupError) {
              cleanupComplete = false;
              throw cleanupError;
            }
            cleanupComplete = true;
            throw normalizeAgentLiveError(error);
          }
          const stream = this.createActiveLiveStream(
            userId,
            lease,
            pendingResource.keepAlive as object,
            barrier,
            logicalStream,
            ownedBinding
          );
          if (isCancelled()) {
            // Cancellation may race activation after the pending token was
            // already consumed. Retain the resulting active handle until its
            // non-benign cleanup succeeds so the pending owner can retry it.
            activeStream = stream;
            cleanupComplete = false;
            await stream.cancel();
            activeStream = null;
            cleanupComplete = true;
            throw new Error("Agent history attachment was cancelled during activation");
          }
          lifecycle = "active";
          activeStream = stream;
          return stream;
        })().finally(() => activationOpening.settle());
        return await activation;
      },
      cancel: async () => {
        if (cleanupComplete) return;
        if (cancellationPromise) return await cancellationPromise;
        lifecycle = "cancelled";
        const cancellation = (async () => {
          const streamToCancel = activeStream;
          if (streamToCancel) {
            await streamToCancel.cancel();
            activeStream = null;
            cleanupComplete = true;
            return;
          }
          const cleanupResults = await Promise.allSettled([
            ...(logicalStream?.registered ? [this.closeRemoteLiveStream(logicalStream, true)] : []),
            ...(!pendingResource.cancelled
              ? [this.cancelRemotePendingHistoryAttach(pendingResource, true)]
              : [])
          ]);
          throwAgentCleanupFailures(
            "Unable to retire the Agent history attachment",
            cleanupResults
          );
          cleanupComplete = true;
        })();
        cancellationPromise = cancellation;
        try {
          await cancellation;
        } finally {
          if (cancellationPromise === cancellation) cancellationPromise = null;
        }
      }
    };
    return Object.freeze(pending);
  }

  private createActiveLiveStream(
    userId: string,
    lease: AgentExecutionLease | null,
    keepAlive: object,
    barrier: AgentLiveBarrierResponse,
    logicalStream: RemoteAgentLogicalLiveStream | null,
    ownedBinding: RemoteAgentLiveStreamBinding | null = null
  ): AgentActiveLiveStream {
    if (logicalStream && lease) {
      const binding =
        ownedBinding ?? this.createRemoteLiveStreamBinding(lease, barrier.liveStreamId, keepAlive);
      if (binding.liveStreamId !== barrier.liveStreamId) {
        throw new Error("Agent live stream barrier mismatched its cleanup-owned native ID");
      }
      this.retainRemoteLiveStreamBinding(logicalStream, binding);
      logicalStream.retainedCursor = laterLiveCursor(
        logicalStream.retainedCursor,
        barrier.throughEventCursor
      );
      logicalStream.binding = binding;
      return this.publicRemoteLiveStream(logicalStream);
    }
    let cancelled = false;
    let cancellationPromise: Promise<void> | null = null;
    return Object.freeze({
      ...barrier,
      cancel: async () => {
        if (cancelled) return;
        if (cancellationPromise) return await cancellationPromise;
        const cancellation = (async () => {
          void keepAlive;
          await completeAgentLiveCleanup(() =>
            this.cancelBoundLiveStream(userId, lease, barrier.liveStreamId)
          );
          cancelled = true;
        })();
        cancellationPromise = cancellation;
        try {
          await cancellation;
        } finally {
          if (cancellationPromise === cancellation && !cancelled) cancellationPromise = null;
        }
      }
    });
  }

  private createRemoteLiveStreamBinding(
    lease: AgentExecutionLease,
    liveStreamId: string,
    keepAlive: unknown
  ): RemoteAgentLiveStreamBinding {
    return {
      lease,
      liveStreamId,
      keepAlive,
      cancelled: false,
      cancellationPromise: null
    };
  }

  private retainRemoteLiveStreamBinding(
    logicalStream: RemoteAgentLogicalLiveStream,
    binding: RemoteAgentLiveStreamBinding
  ): void {
    if (logicalStream.closed) {
      // A replacement open can resolve only after an earlier owner/lease
      // retirement finished. Resurrect only cleanup ownership, never delivery.
      logicalStream.closed = false;
      logicalStream.closeRequested = true;
    }
    logicalStream.cleanupBindings.add(binding);
    logicalStream.registered = true;
    this.remoteLiveStreams.add(logicalStream);
  }

  private publicRemoteLiveStream(
    logicalStream: RemoteAgentLogicalLiveStream
  ): AgentActiveLiveStream {
    const cancel = async () => {
      await this.closeRemoteLiveStream(logicalStream, true);
    };
    return Object.freeze({
      get throughEventCursor() {
        const cursor = logicalStream.retainedCursor;
        if (!cursor) throw new Error("Remote Agent live stream lost its retained cursor");
        return cursor;
      },
      get liveStreamId() {
        return (
          logicalStream.binding?.liveStreamId ??
          logicalStream.cleanupBindings.values().next().value?.liveStreamId ??
          "retired"
        );
      },
      cancel
    });
  }

  private async closeRemoteLiveStream(
    logicalStream: RemoteAgentLogicalLiveStream,
    accountFenced: boolean
  ): Promise<void> {
    if (logicalStream.closed) return;
    logicalStream.closeRequested = true;
    const existing = logicalStream.cancellationPromise;
    if (existing) return await existing;
    logicalStream.bindingEpoch += 1;
    const cancellation = (async () => {
      while (logicalStream.replacementOpenings.size > 0 || logicalStream.cleanupBindings.size > 0) {
        await Promise.all([...logicalStream.replacementOpenings].map((opening) => opening.settled));
        const results = await Promise.allSettled(
          [...logicalStream.cleanupBindings].map((binding) =>
            this.cancelRemoteLiveStreamBinding(logicalStream, binding, accountFenced)
          )
        );
        throwAgentCleanupFailures("Unable to retire the remote Agent live stream", results);
      }
      logicalStream.binding = null;
      logicalStream.closed = true;
      logicalStream.registered = false;
      this.remoteLiveStreams.delete(logicalStream);
      this.scope.releaseAccountIfIdle(logicalStream.accountId);
    })();
    logicalStream.cancellationPromise = cancellation;
    try {
      await cancellation;
    } finally {
      if (logicalStream.cancellationPromise === cancellation) {
        logicalStream.cancellationPromise = null;
      }
    }
  }

  private async cancelRemoteLiveStreamBinding(
    logicalStream: RemoteAgentLogicalLiveStream,
    binding: RemoteAgentLiveStreamBinding,
    accountFenced: boolean
  ): Promise<void> {
    if (binding.cancelled) return;
    const existing = binding.cancellationPromise;
    if (existing) return await existing;
    const cancellation = (async () => {
      await completeAgentLiveCleanup(async () => {
        const cancel = this.bridge.cancelLiveEvents;
        if (!cancel) throw new Error("Agent runtime bridge lost live stream cancellation");
        const cleanup = async () => {
          await cancel.call(
            this.bridge,
            logicalStream.accountId,
            binding.lease,
            this.target,
            binding.liveStreamId
          );
        };
        if (accountFenced) {
          await this.runBoundLiveOperation(logicalStream.accountId, binding.lease, false, cleanup);
        } else {
          await cleanup();
        }
      });
      binding.cancelled = true;
      logicalStream.cleanupBindings.delete(binding);
      if (logicalStream.binding === binding) logicalStream.binding = null;
    })();
    binding.cancellationPromise = cancellation;
    try {
      await cancellation;
    } finally {
      if (binding.cancellationPromise === cancellation && !binding.cancelled) {
        binding.cancellationPromise = null;
      }
    }
  }

  private async cancelBoundLiveStream(
    userId: string,
    lease: AgentExecutionLease | null,
    liveStreamId: string
  ): Promise<void> {
    const cancel = this.bridge.cancelLiveEvents;
    if (!cancel) throw new Error("Agent runtime bridge lost live stream cancellation");
    await this.runBoundLiveOperation(userId, lease, false, async () => {
      await cancel.call(this.bridge, userId, lease, this.target, liveStreamId);
    });
  }

  private async runBoundLiveOperation<T>(
    userId: string,
    lease: AgentExecutionLease | null,
    requireCurrentLease: boolean,
    operation: () => Promise<T>
  ): Promise<T> {
    return await this.bridge.runForUser(
      userId,
      async () => {
        if (
          requireCurrentLease &&
          this.target.kind === "remote" &&
          (!lease || !sameExecutionLease(this.currentLease, lease))
        ) {
          throw new Error("Remote Agent execution lease changed before live stream activation");
        }
        return await operation();
      },
      this.target
    );
  }

  private async invokeForUser<Operation extends AgentRuntimeOperation>(
    userId: string,
    operation: Operation,
    request: AgentRuntimeOperationRequestMap[Operation]
  ): Promise<AgentRuntimeOperationResultMap[Operation]> {
    const opening = this.target.kind === "remote" ? this.beginRemoteResourceOpening(userId) : null;
    try {
      return await this.bridge.runForUser(
        userId,
        async () => {
          const lease = await this.prepareInvocationTarget(userId, true);
          return await this.invokeTarget(operation, request, userId, lease);
        },
        this.target
      );
    } finally {
      opening?.settle();
    }
  }

  private async invokeControlForUser<Operation extends AgentRuntimeOperation>(
    userId: string,
    operation: Operation,
    request: AgentRuntimeOperationRequestMap[Operation]
  ): Promise<AgentRuntimeOperationResultMap[Operation]> {
    const opening = this.target.kind === "remote" ? this.beginRemoteResourceOpening(userId) : null;
    try {
      return await this.bridge.runForUser(
        userId,
        async () => {
          const lease = await this.prepareInvocationTarget(userId, false);
          return await this.invokeTarget(operation, request, userId, lease);
        },
        this.target
      );
    } finally {
      opening?.settle();
    }
  }

  private async prepareInvocationTarget(
    userId: string,
    syncLocalAuth: boolean
  ): Promise<AgentExecutionLease | null> {
    if (this.target.kind === "remote") {
      return await this.prepareRemoteLease(userId);
    }
    if (!syncLocalAuth) return null;
    if (!this.bridge.syncLocalAuth) {
      throw new Error("Agent runtime bridge cannot synchronize local authentication");
    }
    await this.bridge.syncLocalAuth(userId);
    return null;
  }

  private async prepareRemoteLease(userId: string): Promise<AgentExecutionLease> {
    if (!this.bridge.prepareTarget) {
      throw new Error(
        `Agent runtime bridge cannot prepare remote target "${this.target.id as string}"`
      );
    }
    const sharedPreparation = this.preparationInFlight;
    if (sharedPreparation?.accountId === userId) {
      return await sharedPreparation.promise;
    }

    // Reserve authority before entering native async work. A later preparation
    // always wins, and an account switch retires the prior account immediately
    // rather than waiting for the replacement connection to finish.
    const preparationEpoch = ++this.preparationEpoch;
    this.preparingAccountId = userId;
    const promise = (async () => {
      if (this.currentLease?.accountId !== userId) {
        this.currentLease = null;
        await this.retireRemoteSubscriptionsExcept(userId);
        await this.retireRemotePendingHistoryAttachesExcept(userId);
        await this.retireRemoteLiveStreamsExcept(userId);
      }
      if (preparationEpoch !== this.preparationEpoch || this.preparingAccountId !== userId) {
        throw new Error("Remote Agent target preparation was superseded");
      }
      return await this.completeRemotePreparation(userId, preparationEpoch);
    })();
    this.preparationInFlight = { accountId: userId, promise };
    try {
      return await promise;
    } finally {
      if (this.preparationInFlight?.promise === promise) this.preparationInFlight = null;
    }
  }

  private async completeRemotePreparation(
    userId: string,
    preparationEpoch: number
  ): Promise<AgentExecutionLease> {
    let rawLease: unknown;
    try {
      rawLease = await this.bridge.prepareTarget!(userId, this.target);
    } catch (error) {
      if (preparationEpoch !== this.preparationEpoch || this.preparingAccountId !== userId) {
        throw new Error("Remote Agent target preparation was superseded");
      }
      throw error;
    }
    if (preparationEpoch !== this.preparationEpoch || this.preparingAccountId !== userId) {
      throw new Error("Remote Agent target preparation was superseded");
    }
    const lease = decodeExecutionLease(rawLease, {
      accountId: userId,
      targetId: this.target.id
    });
    if (preparationEpoch !== this.preparationEpoch || this.preparingAccountId !== userId) {
      throw new Error("Remote Agent target preparation was superseded");
    }
    const previousLease = this.currentLease;
    this.currentLease = lease;
    if (!sameOptionalExecutionLease(previousLease, lease)) {
      try {
        await this.refreshRemoteSubscriptions(lease);
        await this.refreshRemoteLiveStreams(lease);
      } catch (error) {
        if (sameExecutionLease(this.currentLease, lease)) this.currentLease = null;
        throw error;
      }
      if (preparationEpoch !== this.preparationEpoch || this.preparingAccountId !== userId) {
        throw new Error("Remote Agent target preparation was superseded");
      }
    }
    return lease;
  }

  private async refreshRemoteSubscriptions(lease: AgentExecutionLease): Promise<void> {
    const subscriptions = [...this.remoteSubscriptions].filter(
      (subscription) =>
        !subscription.closed &&
        !subscription.closeRequested &&
        subscription.accountId === lease.accountId
    );
    for (const subscription of subscriptions) {
      await this.bindRemoteSubscription(subscription, lease, "replacement");
    }
  }

  private async refreshRemoteLiveStreams(lease: AgentExecutionLease): Promise<void> {
    const streams = [...this.remoteLiveStreams].filter(
      (stream) => !stream.closed && stream.accountId === lease.accountId
    );
    for (const stream of streams) await this.rebindRemoteLiveStream(stream, lease);
  }

  private async rebindRemoteLiveStream(
    stream: RemoteAgentLogicalLiveStream,
    lease: AgentExecutionLease
  ): Promise<void> {
    if (stream.closeRequested) {
      await this.closeRemoteLiveStream(stream, false);
      return;
    }
    const resume = this.bridge.resumeLiveEvents;
    const cancel = this.bridge.cancelLiveEvents;
    const cursor = stream.retainedCursor;
    if (!resume || !cancel || !cursor) {
      this.notifyRemoteLiveStreamInvalidated(stream, lease);
      throw new Error("Agent runtime bridge cannot replace its synchronized live stream");
    }
    const bindingEpoch = ++stream.bindingEpoch;
    const cleanupResults = await Promise.allSettled(
      [...stream.cleanupBindings].map((binding) =>
        this.cancelRemoteLiveStreamBinding(stream, binding, false)
      )
    );
    try {
      throwAgentCleanupFailures("Unable to retire the replaced Agent live stream", cleanupResults);
    } catch (error) {
      this.notifyRemoteLiveStreamInvalidated(stream, lease);
      throw error;
    }
    if (stream.closed || stream.closeRequested || !sameExecutionLease(this.currentLease, lease)) {
      return;
    }

    const replacementOpening = this.beginRemoteLiveStreamReplacementOpening(stream);
    try {
      let opened: AgentBridgeLiveChannelResult;
      try {
        opened = await resume.call(
          this.bridge,
          stream.accountId,
          lease,
          this.target,
          cursor,
          this.liveChannelDecoder(lease, stream.handler, stream, bindingEpoch)
        );
      } catch (error) {
        this.notifyRemoteLiveStreamInvalidated(stream, lease);
        throw normalizeAgentLiveError(error);
      }

      const replacementId = liveStreamIdFromUnknown(opened.result);
      const provisionalBinding = replacementId
        ? this.createRemoteLiveStreamBinding(lease, replacementId, opened.keepAlive)
        : null;
      if (provisionalBinding) this.retainRemoteLiveStreamBinding(stream, provisionalBinding);

      let barrier: AgentLiveBarrierResponse;
      try {
        if (!isRecord(opened.keepAlive)) {
          throw new Error("Agent runtime bridge did not retain its replacement live event channel");
        }
        barrier = decodeAgentLiveBarrierResponse(opened.result);
        if (!provisionalBinding || provisionalBinding.liveStreamId !== barrier.liveStreamId) {
          throw new Error("Agent replacement live stream lost its cleanup-owned native ID");
        }
        if (
          barrier.throughEventCursor.journalId !== cursor.journalId ||
          barrier.throughEventCursor.sequence < cursor.sequence
        ) {
          throw new Error("Agent replacement live stream returned a regressing event checkpoint");
        }
      } catch (error) {
        stream.bindingEpoch += 1;
        if (provisionalBinding) {
          try {
            await this.cancelRemoteLiveStreamBinding(stream, provisionalBinding, false);
          } catch (cleanupError) {
            this.notifyRemoteLiveStreamInvalidated(stream, lease);
            throw cleanupError;
          }
        }
        this.notifyRemoteLiveStreamInvalidated(stream, lease);
        throw error;
      }

      if (
        stream.closed ||
        stream.closeRequested ||
        stream.bindingEpoch !== bindingEpoch ||
        !sameExecutionLease(this.currentLease, lease)
      ) {
        if (provisionalBinding) {
          await this.cancelRemoteLiveStreamBinding(stream, provisionalBinding, false);
        }
        return;
      }
      stream.retainedCursor = laterLiveCursor(stream.retainedCursor, barrier.throughEventCursor);
      stream.binding = provisionalBinding;
    } finally {
      replacementOpening.settle();
    }
  }

  private notifyRemoteLiveStreamInvalidated(
    stream: RemoteAgentLogicalLiveStream,
    lease: AgentExecutionLease
  ): void {
    const cursor = stream.retainedCursor;
    if (!cursor || stream.closed) return;
    stream.handler({
      liveEventVersion: 1,
      eventType: "snapshotRequired",
      targetId: lease.targetId,
      hostEpoch: lease.hostEpoch,
      connectionGeneration: lease.connectionGeneration,
      reason: "ordering_lost",
      lastEventCursor: cursor
    });
  }

  private async bindRemoteSubscription(
    subscription: RemoteAgentLogicalSubscription,
    lease: AgentExecutionLease,
    phase: RemoteAgentSubscriptionBindPhase
  ): Promise<void> {
    if (!this.bridge.listenToEvents) {
      throw new Error(
        `Agent runtime bridge does not support events for remote target "${this.target.id as string}"`
      );
    }
    if (subscription.closeRequested) {
      await this.closeRemoteSubscription(subscription);
      if (phase === "initial") throw retiredInitialRemoteSubscriptionError();
      return;
    }
    const bindingEpoch = ++subscription.bindingEpoch;
    subscription.lease = lease;
    const replacementOpening = this.beginRemoteSubscriptionReplacementOpening(subscription);
    try {
      let replacementUnlisten: UnlistenAgentEvents;
      try {
        replacementUnlisten = await this.bridge.listenToEvents(lease, this.target, (event) => {
          if (
            subscription.closed ||
            subscription.closeRequested ||
            subscription.bindingEpoch !== bindingEpoch ||
            !sameExecutionLease(subscription.lease, lease) ||
            !sameExecutionLease(this.currentLease, lease)
          ) {
            return;
          }
          const decoded = decodeRemoteAgentEvent(event, lease);
          if (decoded) subscription.handler(decoded);
        });
      } catch (error) {
        if (subscription.closeRequested || subscription.closed) {
          if (phase === "initial") throw retiredInitialRemoteSubscriptionError();
          return;
        }
        if (
          subscription.bindingEpoch !== bindingEpoch ||
          !sameExecutionLease(subscription.lease, lease) ||
          !sameExecutionLease(this.currentLease, lease)
        ) {
          // The rejecting native bind belongs to retired authority. A newer
          // bind owns the logical subscription, so there is no native listener
          // from this failed attempt to retain.
          return;
        }
        throw error;
      }

      const replacementBinding: RemoteAgentSubscriptionBinding = {
        unlisten: replacementUnlisten,
        cancelled: false,
        cancellationPromise: null
      };
      subscription.cleanupBindings.add(replacementBinding);
      if (
        subscription.closed ||
        subscription.closeRequested ||
        subscription.bindingEpoch !== bindingEpoch ||
        !sameExecutionLease(this.currentLease, lease)
      ) {
        await this.cancelRemoteSubscriptionBinding(subscription, replacementBinding);
        if (phase === "initial" && subscription.closeRequested) {
          throw retiredInitialRemoteSubscriptionError();
        }
        return;
      }

      subscription.binding = replacementBinding;
      const cleanupResults = await Promise.allSettled(
        [...subscription.cleanupBindings]
          .filter((binding) => binding !== replacementBinding)
          .map((binding) => this.cancelRemoteSubscriptionBinding(subscription, binding))
      );
      throwAgentCleanupFailures("Unable to retire the replaced Agent subscription", cleanupResults);
    } finally {
      replacementOpening.settle();
    }
  }

  private beginRemoteSubscriptionReplacementOpening(
    subscription: RemoteAgentLogicalSubscription
  ): RemoteAgentResourceOpening {
    const opening = this.beginRemoteResourceOpening(subscription.accountId);
    const settleOpening = opening.settle;
    opening.settle = () => {
      subscription.replacementOpenings.delete(opening);
      settleOpening();
    };
    subscription.replacementOpenings.add(opening);
    return opening;
  }

  private async cancelRemoteSubscriptionBinding(
    subscription: RemoteAgentLogicalSubscription,
    binding: RemoteAgentSubscriptionBinding
  ): Promise<void> {
    if (binding.cancelled) return;
    const existing = binding.cancellationPromise;
    if (existing) return await existing;
    const cancellation = (async () => {
      binding.unlisten();
      binding.cancelled = true;
      subscription.cleanupBindings.delete(binding);
      if (subscription.binding === binding) subscription.binding = null;
    })();
    binding.cancellationPromise = cancellation;
    try {
      await cancellation;
    } finally {
      if (binding.cancellationPromise === cancellation && !binding.cancelled) {
        binding.cancellationPromise = null;
      }
    }
  }

  private async closeRemoteSubscription(
    subscription: RemoteAgentLogicalSubscription
  ): Promise<void> {
    if (subscription.closed) return;
    subscription.closeRequested = true;
    const existing = subscription.cancellationPromise;
    if (existing) return await existing;
    subscription.bindingEpoch += 1;
    const cancellation = (async () => {
      while (subscription.replacementOpenings.size > 0 || subscription.cleanupBindings.size > 0) {
        await Promise.all([...subscription.replacementOpenings].map((opening) => opening.settled));
        const results = await Promise.allSettled(
          [...subscription.cleanupBindings].map((binding) =>
            this.cancelRemoteSubscriptionBinding(subscription, binding)
          )
        );
        throwAgentCleanupFailures("Unable to retire the remote Agent subscription", results);
      }
      subscription.binding = null;
      subscription.closed = true;
      this.remoteSubscriptions.delete(subscription);
      this.scope.releaseAccountIfIdle(subscription.accountId);
    })();
    subscription.cancellationPromise = cancellation;
    try {
      await cancellation;
    } finally {
      if (subscription.cancellationPromise === cancellation) {
        subscription.cancellationPromise = null;
      }
    }
  }

  private async retireRemoteSubscriptionsExcept(accountId: string): Promise<void> {
    const results = await Promise.allSettled(
      [...this.remoteSubscriptions]
        .filter((subscription) => subscription.accountId !== accountId)
        .map((subscription) => this.closeRemoteSubscription(subscription))
    );
    throwAgentCleanupFailures("Unable to retire prior-account Agent subscriptions", results);
  }

  private async retireRemoteLiveStreamsExcept(accountId: string): Promise<void> {
    for (const stream of [...this.remoteLiveStreams]) {
      if (stream.accountId !== accountId) await this.closeRemoteLiveStream(stream, false);
    }
  }

  private async retireRemotePendingHistoryAttachesExcept(accountId: string): Promise<void> {
    for (const resource of [...this.remotePendingHistoryAttaches]) {
      if (resource.accountId !== accountId) {
        await this.cancelRemotePendingHistoryAttach(resource, false);
      }
    }
  }

  private async invokeTarget<Operation extends AgentRuntimeOperation>(
    operation: Operation,
    request: AgentRuntimeOperationRequestMap[Operation],
    userId: string,
    lease: AgentExecutionLease | null
  ): Promise<AgentRuntimeOperationResultMap[Operation]> {
    if (this.target.kind === "remote") {
      if (operation === "listSessions" || operation === "loadSession") {
        throw new Error(`Remote Agent invocation cannot use unpaged operation "${operation}"`);
      }
      if (!lease || !sameExecutionLease(this.currentLease, lease) || !this.bridge.invokeTarget) {
        throw new Error(
          `Agent runtime bridge does not hold a current lease for remote target "${this.target.id as string}"`
        );
      }
      const invocation = createRemoteInvocation(operation, request);
      const result = await this.bridge.invokeTarget(lease, invocation);
      if (!sameExecutionLease(this.currentLease, lease)) {
        throw new Error(`Remote Agent execution lease changed while "${operation}" was in flight`);
      }
      return decodeAgentOperationResult(operation, result, true);
    }
    if (this.target.id !== LOCAL_AGENT_EXECUTION_TARGET_ID) {
      throw new Error(`Unknown local Agent execution target "${this.target.id as string}"`);
    }
    if (!this.bridge.invoke) {
      throw new Error("Agent runtime bridge does not support the local execution target");
    }
    if (!(operation in LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION)) {
      throw new Error(`The embedded Agent runtime does not yet support operation "${operation}"`);
    }
    const command =
      LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION[
        operation as keyof typeof LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION
      ];
    const args = { userId, ...(request ?? {}) };
    if (operation === "listSessionsPage" || operation === "listSessionRecordsPage") {
      const result = await this.bridge.invoke<unknown>(command, args);
      return decodeAgentOperationResult(operation, result, false);
    }
    return await this.bridge.invoke<AgentRuntimeOperationResultMap[Operation]>(command, args);
  }

  private assertLocalCompatibilityOperation(operation: "listSessions" | "loadSession"): void {
    if (this.target.kind === "remote") {
      throw new Error(
        `Remote Agent targets must use paged history APIs; "${operation}" is embedded-only compatibility`
      );
    }
  }
}

function retiredInitialRemoteSubscriptionError(): Error {
  return new Error("Remote Agent event subscription was retired before its initial bind completed");
}

function isConnectionGeneration(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isVerifiedConnectionGeneration(value: unknown): value is number {
  return isConnectionGeneration(value) && value > 0;
}

function isCanonicalHostEpoch(value: unknown): value is string {
  return (
    isString(value) &&
    value.length > 0 &&
    value.length <= MAX_AGENT_HOST_EPOCH_BYTES &&
    /^[1-9][0-9]*$/.test(value) &&
    (value.length < MAX_U64_DECIMAL.length || value <= MAX_U64_DECIMAL)
  );
}

function sameExecutionLease(left: AgentExecutionLease | null, right: AgentExecutionLease): boolean {
  return (
    left !== null &&
    left.accountId === right.accountId &&
    left.targetId === right.targetId &&
    left.hostEpoch === right.hostEpoch &&
    left.connectionGeneration === right.connectionGeneration
  );
}

function sameOptionalExecutionLease(
  left: AgentExecutionLease | null,
  right: AgentExecutionLease
): boolean {
  return left !== null && sameExecutionLease(left, right);
}

function laterLiveCursor(
  current: AgentLiveEventCursor | null,
  candidate: AgentLiveEventCursor
): AgentLiveEventCursor {
  if (!current) return candidate;
  if (current.journalId !== candidate.journalId) return candidate;
  return candidate.sequence >= current.sequence ? candidate : current;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isNullableString(value: unknown): value is string | null | undefined {
  return value === null || value === undefined || isString(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function decodeExecutionLease(
  value: unknown,
  expected: { accountId: string; targetId: AgentExecutionTargetId }
): AgentExecutionLease {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["targetId", "hostEpoch", "connectionGeneration"]) ||
    value.targetId !== expected.targetId ||
    !isCanonicalHostEpoch(value.hostEpoch) ||
    !isVerifiedConnectionGeneration(value.connectionGeneration)
  ) {
    throw new Error("Remote Agent bridge returned an invalid or mismatched execution lease");
  }
  return Object.freeze({
    accountId: expected.accountId,
    targetId: value.targetId as AgentExecutionTargetId,
    hostEpoch: value.hostEpoch,
    connectionGeneration: value.connectionGeneration
  }) as AgentExecutionLease;
}

function validateAgentPageRequest(
  request: unknown
): asserts request is AgentPageRequest & Record<string, unknown> {
  if (!isRecord(request)) throw new Error("Agent page request must be an object");
  if (
    request.limit !== undefined &&
    (!isFiniteNumber(request.limit) ||
      !Number.isInteger(request.limit) ||
      request.limit < 1 ||
      request.limit > MAX_AGENT_PAGE_SIZE)
  ) {
    throw new Error(`Agent page limit must be between 1 and ${MAX_AGENT_PAGE_SIZE}`);
  }
  if (
    request.cursor !== undefined &&
    request.cursor !== null &&
    (!isString(request.cursor) ||
      request.cursor.length === 0 ||
      request.cursor.length > MAX_AGENT_CURSOR_BYTES ||
      !isAscii(request.cursor))
  ) {
    throw new Error("Agent page cursor must be non-empty bounded ASCII");
  }
}

function validateReturnedAgentPage(
  kind: string,
  request: AgentPageRequest,
  itemCount: number,
  nextCursor: string | null
): void {
  const requestedLimit = request.limit ?? DEFAULT_AGENT_PAGE_SIZE;
  if (itemCount > requestedLimit) {
    throw new Error(`Agent ${kind} page exceeded the requested record limit`);
  }
  if (nextCursor && itemCount === 0) {
    throw new Error(`Agent ${kind} page returned a cursor without records`);
  }
  if (nextCursor && request.cursor && nextCursor === request.cursor) {
    throw new Error(`Agent ${kind} page cursor did not progress`);
  }
}

function normalizeAgentPageError(error: unknown): unknown {
  if (isAgentPageStaleError(error)) return error;
  const code = isRecord(error) ? error.code : undefined;
  if (code === "history_record_too_large") return new AgentHistoryRecordTooLargeError();
  const message = error instanceof Error ? error.message : isString(error) ? error : "";
  if (
    code === "stale_history" ||
    code === "StaleHistory" ||
    message.includes("Agent task history changed; reload its newest page")
  ) {
    return new AgentPageStaleError();
  }
  return error;
}

export class AgentLiveSnapshotRequiredError extends Error {
  constructor(
    readonly reason: AgentLiveSnapshotReason,
    readonly lastEventCursor?: AgentLiveEventCursor
  ) {
    super("Agent live history requires a synchronized snapshot");
    this.name = "AgentLiveSnapshotRequiredError";
  }
}

export function isAgentLiveSnapshotRequiredError(
  error: unknown
): error is AgentLiveSnapshotRequiredError {
  return error instanceof AgentLiveSnapshotRequiredError;
}

function normalizeAgentLiveError(error: unknown): unknown {
  if (isAgentLiveSnapshotRequiredError(error)) return error;
  if (!isRecord(error)) return error;
  const code = error.code;
  if (code === "history_record_too_large") return new AgentHistoryRecordTooLargeError();
  if (code !== "snapshot_required") return error;
  const reason = decodeAgentLiveSnapshotReason(error.reason);
  if (!reason) return new Error("Agent live history returned an invalid snapshot reason");
  const cursor =
    error.lastEventCursor === undefined
      ? undefined
      : decodeAgentLiveEventCursor(error.lastEventCursor);
  if (cursor === null) return new Error("Agent live history returned an invalid event cursor");
  return new AgentLiveSnapshotRequiredError(reason, cursor);
}

function isBenignAgentLiveCleanupError(error: unknown): boolean {
  if (!isRecord(error)) return false;
  return (
    error.code === "attach_not_found" ||
    error.code === "stale_lease" ||
    error.code === "channel_closed"
  );
}

async function completeAgentLiveCleanup(cleanup: () => Promise<void>): Promise<void> {
  try {
    await cleanup();
  } catch (error) {
    if (!isBenignAgentLiveCleanupError(error)) throw error;
  }
}

function throwAgentCleanupFailures(
  message: string,
  results: readonly PromiseSettledResult<unknown>[]
): void {
  const failures = results.flatMap((result) =>
    result.status === "rejected" ? [result.reason] : []
  );
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new AgentRuntimeResourceCleanupError(message, failures);
}

class AgentRuntimeResourceCleanupError extends Error {
  constructor(
    message: string,
    readonly errors: readonly unknown[]
  ) {
    super(message);
    this.name = "AgentRuntimeResourceCleanupError";
  }
}

function validateAgentListSessionsPageRequest(
  request: unknown
): asserts request is AgentListSessionsPageRequest {
  validateAgentPageRequest(request);
  if (
    request.projectRoot !== undefined &&
    request.projectRoot !== null &&
    !isString(request.projectRoot)
  ) {
    throw new Error("Agent session page project root must be a string or null");
  }
}

function validateAgentListSessionRecordsPageRequest(
  request: unknown
): asserts request is AgentListSessionRecordsPageRequest {
  validateAgentPageRequest(request);
  if (!isString(request.sessionId) || request.sessionId.length === 0) {
    throw new Error("Agent history page session ID must be a non-empty string");
  }
}

function validateAgentLiveEventCursor(value: unknown): asserts value is AgentLiveEventCursor {
  if (!decodeAgentLiveEventCursor(value)) {
    throw new Error("Agent live event cursor is invalid");
  }
}

function isAscii(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    if (value.charCodeAt(index) > 0x7f) return false;
  }
  return true;
}

function isPrintableAscii(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x20 || code > 0x7e) return false;
  }
  return true;
}

const AGENT_UTF8_ENCODER = new TextEncoder();

function hasUnsafeLiveIdentifierCharacter(value: string): boolean {
  for (const character of value) {
    const codePoint = character.codePointAt(0)!;
    if (
      codePoint <= 0x1f ||
      (codePoint >= 0x7f && codePoint <= 0x9f) ||
      codePoint === 0x061c ||
      codePoint === 0x200e ||
      codePoint === 0x200f ||
      (codePoint >= 0x202a && codePoint <= 0x202e) ||
      (codePoint >= 0x2066 && codePoint <= 0x2069)
    ) {
      return true;
    }
  }
  return false;
}

function hasUnpairedSurrogate(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return true;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return true;
    }
  }
  return false;
}

function utf8ByteLength(value: string): number {
  return AGENT_UTF8_ENCODER.encode(value).byteLength;
}

function compareUtf8Bytes(left: string, right: string): number {
  const leftBytes = AGENT_UTF8_ENCODER.encode(left);
  const rightBytes = AGENT_UTF8_ENCODER.encode(right);
  const sharedLength = Math.min(leftBytes.length, rightBytes.length);
  for (let index = 0; index < sharedLength; index += 1) {
    if (leftBytes[index] !== rightBytes[index]) return leftBytes[index] - rightBytes[index];
  }
  return leftBytes.length - rightBytes.length;
}

function isBoundedLiveIdentifier(value: unknown, maxBytes: number): value is string {
  return (
    isString(value) &&
    value.length > 0 &&
    !hasUnpairedSurrogate(value) &&
    utf8ByteLength(value) <= maxBytes &&
    !hasUnsafeLiveIdentifierCharacter(value)
  );
}

function isBoundedLiveText(value: unknown, maxBytes: number): value is string {
  return (
    isString(value) &&
    !value.includes("\0") &&
    !hasUnpairedSurrogate(value) &&
    utf8ByteLength(value) <= maxBytes
  );
}

function isOptionalBoundedLiveText(value: unknown, maxBytes: number): value is string | undefined {
  return value === undefined || isBoundedLiveText(value, maxBytes);
}

function isBoundedAgentDisplayText(value: unknown, maxBytes: number): value is string {
  return (
    isBoundedLiveText(value, maxBytes) &&
    value.length > 0 &&
    !hasUnsafeLiveIdentifierCharacter(value)
  );
}

function isOptionalBoundedAgentDisplayText(
  value: unknown,
  maxBytes: number
): value is string | null | undefined {
  return value === undefined || value === null || isBoundedAgentDisplayText(value, maxBytes);
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function hasOnlyKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const allowed = new Set(keys);
  return Object.keys(value).every((key) => allowed.has(key));
}

function decodeAgentLiveEventCursor(value: unknown): AgentLiveEventCursor | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["journalId", "sequence"]) ||
    !isString(value.journalId) ||
    !AGENT_LIVE_JOURNAL_ID_PATTERN.test(value.journalId) ||
    !isConnectionGeneration(value.sequence)
  ) {
    return null;
  }
  return { journalId: value.journalId, sequence: value.sequence };
}

function decodeAgentPresentedTimelineItem(value: unknown): AgentPresentedTimelineItem | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "id",
      "itemType",
      "role",
      "title",
      "text",
      "status",
      "createdMs",
      "merge"
    ]) ||
    !isBoundedLiveIdentifier(value.id, MAX_AGENT_LIVE_ID_BYTES) ||
    (value.itemType !== "message" &&
      value.itemType !== "thinking" &&
      value.itemType !== "tool" &&
      value.itemType !== "permission" &&
      value.itemType !== "system" &&
      value.itemType !== "error") ||
    (value.role !== undefined &&
      value.role !== "user" &&
      value.role !== "assistant" &&
      value.role !== "thought" &&
      value.role !== "system") ||
    !isOptionalBoundedLiveText(value.title, MAX_AGENT_LIVE_TITLE_BYTES) ||
    !isOptionalBoundedLiveText(value.text, MAX_AGENT_LIVE_TEXT_BYTES) ||
    !isOptionalBoundedLiveText(value.status, MAX_AGENT_LIVE_STATUS_BYTES) ||
    !isNonnegativeSafeInteger(value.createdMs) ||
    (value.merge !== "append" && value.merge !== "replace")
  ) {
    return null;
  }

  if (value.itemType === "tool") {
    let expectedText: string | undefined;
    switch (value.status) {
      case undefined:
      case "pending":
      case "running":
      case "completed":
        expectedText = undefined;
        break;
      case "failed":
      case "error":
        expectedText = SAFE_REMOTE_TOOL_FAILED;
        break;
      case "cancelled":
        expectedText = SAFE_REMOTE_TOOL_CANCELLED;
        break;
      default:
        return null;
    }
    if (
      value.role !== "assistant" ||
      value.title !== SAFE_REMOTE_TOOL_TITLE ||
      value.text !== expectedText
    ) {
      return null;
    }
  } else if (value.itemType === "permission") {
    if (
      value.role !== "system" ||
      value.title !== SAFE_REMOTE_PERMISSION_TITLE ||
      value.text !== undefined ||
      (value.status !== "allow_once" &&
        value.status !== "deny_once" &&
        value.status !== "completed" &&
        value.status !== "cancelled")
    ) {
      return null;
    }
  } else if (value.itemType === "error") {
    if (
      value.role !== "system" ||
      value.title !== "Agent error" ||
      value.text !== SAFE_REMOTE_AGENT_ERROR ||
      value.status !== "failed"
    ) {
      return null;
    }
  }

  return value as unknown as AgentPresentedTimelineItem;
}

function isAgentPresentedUserFacingErrorItem(item: AgentPresentedTimelineItem): boolean {
  if (item.merge !== "replace" || item.role !== "system") return false;
  return (
    (item.itemType === "system" &&
      item.title === "Agent warning" &&
      item.text === SAFE_REMOTE_SETUP_WARNING &&
      item.status === "warning") ||
    (item.itemType === "error" &&
      item.title === "Agent error" &&
      item.text === SAFE_REMOTE_AGENT_ERROR &&
      item.status === "failed")
  );
}

function agentPresentedTimelineItemBudgetBytes(item: AgentPresentedTimelineItem): number {
  return (
    256 +
    utf8ByteLength(item.id) +
    utf8ByteLength(item.itemType) +
    (item.role ? utf8ByteLength(item.role) : 0) +
    (item.title ? utf8ByteLength(item.title) : 0) +
    (item.text ? utf8ByteLength(item.text) : 0) +
    (item.status ? utf8ByteLength(item.status) : 0) +
    utf8ByteLength(item.merge)
  );
}

function decodeAgentPresentedSessionSummary(value: unknown): AgentSessionSummary | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "id",
      "title",
      "projectRoot",
      "createdMs",
      "updatedMs",
      "pageSortMs",
      "messageCount",
      "model",
      "mode"
    ]) ||
    !isBoundedLiveIdentifier(value.id, MAX_AGENT_LIVE_ID_BYTES) ||
    !isBoundedAgentDisplayText(value.title, MAX_AGENT_LIVE_TITLE_BYTES) ||
    !isBoundedAgentDisplayText(value.projectRoot, MAX_AGENT_LIVE_PROJECT_ROOT_BYTES) ||
    !isNonnegativeSafeInteger(value.createdMs) ||
    !isNonnegativeSafeInteger(value.updatedMs) ||
    !isNonnegativeSafeInteger(value.pageSortMs) ||
    !isNonnegativeSafeInteger(value.messageCount) ||
    !isOptionalBoundedAgentDisplayText(value.model, MAX_AGENT_LIVE_MODEL_BYTES) ||
    !isBoundedAgentDisplayText(value.mode, MAX_AGENT_LIVE_MODE_BYTES)
  ) {
    return null;
  }
  return {
    id: value.id,
    title: value.title,
    projectRoot: value.projectRoot,
    createdMs: value.createdMs,
    updatedMs: value.updatedMs,
    pageSortMs: value.pageSortMs,
    messageCount: value.messageCount,
    ...(value.model === undefined ? {} : { model: value.model }),
    mode: value.mode
  };
}

function isAgentPresentedHistoryRecord(value: unknown): value is AgentPresentedHistoryRecord {
  if (
    !(
      isRecord(value) &&
      hasOnlyKeys(value, ["recordId", "role", "createdMs", "items"]) &&
      isString(value.recordId) &&
      value.recordId.length > 0 &&
      value.recordId.length <= MAX_AGENT_CURSOR_BYTES &&
      AGENT_SAFE_HISTORY_TOKEN_PATTERN.test(value.recordId) &&
      isString(value.role) &&
      value.role.length > 0 &&
      value.role.length <= MAX_AGENT_HISTORY_ROLE_BYTES &&
      isPrintableAscii(value.role) &&
      isNonnegativeSafeInteger(value.createdMs) &&
      Array.isArray(value.items) &&
      value.items.length <= MAX_AGENT_HISTORY_ITEMS_PER_RECORD &&
      value.items.every((item) => decodeAgentPresentedTimelineItem(item) !== null)
    )
  ) {
    return false;
  }
  // Conservative counterpart to native's exact CBOR frame cap. Fixed per-item
  // overhead exceeds the closed map's actual CBOR keys/integers without JSON's
  // escape expansion, keeping the bridge boundary bounded near the same 1 MiB.
  let estimatedBytes = 512 + utf8ByteLength(value.recordId) + utf8ByteLength(value.role);
  for (const item of value.items as AgentPresentedTimelineItem[]) {
    estimatedBytes += agentPresentedTimelineItemBudgetBytes(item);
    if (estimatedBytes > MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES) return false;
  }
  return true;
}

function isAgentPresentedSessionRecordsPage(
  value: unknown
): value is AgentPresentedSessionRecordsPage {
  if (
    !(
      isRecord(value) &&
      hasOnlyKeys(value, ["records", "nextCursor", "historyRevision"]) &&
      Array.isArray(value.records) &&
      value.records.length <= MAX_AGENT_PAGE_SIZE &&
      value.records.every(isAgentPresentedHistoryRecord) &&
      isNullableString(value.nextCursor) &&
      (value.nextCursor === undefined ||
        value.nextCursor === null ||
        (value.nextCursor.length > 0 &&
          value.nextCursor.length <= MAX_AGENT_CURSOR_BYTES &&
          AGENT_SAFE_HISTORY_TOKEN_PATTERN.test(value.nextCursor))) &&
      isString(value.historyRevision) &&
      value.historyRevision.length > 0 &&
      value.historyRevision.length <= MAX_AGENT_CURSOR_BYTES &&
      AGENT_SAFE_HISTORY_TOKEN_PATTERN.test(value.historyRevision)
    )
  ) {
    return false;
  }
  const recordIds = new Set<string>();
  for (const record of value.records as AgentPresentedHistoryRecord[]) {
    if (recordIds.has(record.recordId)) return false;
    recordIds.add(record.recordId);
  }
  return true;
}

function decodeAgentBeginSessionHistoryAttachResponse(
  value: unknown
): AgentBeginSessionHistoryAttachResponse {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "attachId",
      "page",
      "liveSessionsComplete",
      "liveSessionCount",
      "liveSessions",
      "throughEventCursor"
    ]) ||
    !isBoundedAgentCursor(value.attachId) ||
    !isAgentPresentedSessionRecordsPage(value.page) ||
    value.page.nextCursor === null ||
    value.liveSessionsComplete !== true ||
    !Number.isSafeInteger(value.liveSessionCount) ||
    (value.liveSessionCount as number) < 0 ||
    (value.liveSessionCount as number) > MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT ||
    !Array.isArray(value.liveSessions) ||
    value.liveSessions.length !== value.liveSessionCount
  ) {
    throw new Error("Agent runtime bridge returned an invalid synchronized history attachment");
  }
  let previousSessionId: string | null = null;
  let totalItems = 0;
  let projectedBytes = 4 * 1024;
  for (const liveSession of value.liveSessions) {
    if (
      !isRecord(liveSession) ||
      !hasOnlyKeys(liveSession, ["sessionId", "liveItems"]) ||
      !isBoundedLiveIdentifier(liveSession.sessionId, MAX_AGENT_LIVE_ID_BYTES) ||
      (previousSessionId !== null &&
        compareUtf8Bytes(previousSessionId, liveSession.sessionId) >= 0) ||
      !Array.isArray(liveSession.liveItems) ||
      liveSession.liveItems.length === 0 ||
      liveSession.liveItems.length > MAX_AGENT_LIVE_ITEMS_PER_SESSION
    ) {
      throw new Error("Agent runtime bridge returned invalid synchronized live sessions");
    }
    previousSessionId = liveSession.sessionId;
    projectedBytes += 256 + utf8ByteLength(liveSession.sessionId);
    const itemIds = new Set<string>();
    for (const item of liveSession.liveItems) {
      const decoded = decodeAgentPresentedTimelineItem(item);
      if (!decoded || decoded.merge !== "replace" || itemIds.has(decoded.id)) {
        throw new Error("Agent runtime bridge returned an invalid synchronized live suffix");
      }
      itemIds.add(decoded.id);
      projectedBytes += agentPresentedTimelineItemBudgetBytes(decoded);
      if (projectedBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT) {
        throw new Error("Agent runtime bridge returned an oversized synchronized live snapshot");
      }
    }
    totalItems += liveSession.liveItems.length;
    if (totalItems > MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT) {
      throw new Error("Agent runtime bridge returned too many synchronized live items");
    }
  }
  const throughEventCursor = decodeAgentLiveEventCursor(value.throughEventCursor);
  if (!throughEventCursor) {
    throw new Error("Agent runtime bridge returned an invalid synchronized event checkpoint");
  }
  return value as unknown as AgentBeginSessionHistoryAttachResponse;
}

function decodeAgentLiveBarrierResponse(value: unknown): AgentLiveBarrierResponse {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["throughEventCursor", "liveStreamId"]) ||
    !isBoundedAgentCursor(value.liveStreamId)
  ) {
    throw new Error("Agent runtime bridge returned an invalid live stream barrier");
  }
  const throughEventCursor = decodeAgentLiveEventCursor(value.throughEventCursor);
  if (!throughEventCursor) {
    throw new Error("Agent runtime bridge returned an invalid live stream checkpoint");
  }
  return { throughEventCursor, liveStreamId: value.liveStreamId };
}

function liveStreamIdFromUnknown(value: unknown): string | null {
  return isRecord(value) && isBoundedAgentCursor(value.liveStreamId) ? value.liveStreamId : null;
}

function attachIdFromUnknown(value: unknown): string | null {
  return isRecord(value) && isBoundedAgentCursor(value.attachId) ? value.attachId : null;
}

const AGENT_LIVE_SNAPSHOT_REASONS = new Set<AgentLiveSnapshotReason>([
  "paused_overflow",
  "slow_subscriber",
  "journal_replaced",
  "retention_gap",
  "cursor_ahead",
  "owner_changed",
  "ordering_lost",
  "journal_unavailable"
]);

function decodeAgentLiveSnapshotReason(value: unknown): AgentLiveSnapshotReason | null {
  return isString(value) && AGENT_LIVE_SNAPSHOT_REASONS.has(value as AgentLiveSnapshotReason)
    ? (value as AgentLiveSnapshotReason)
    : null;
}

function createRemoteInvocation<Operation extends AgentRuntimeOperation>(
  operation: Operation,
  request: AgentRuntimeOperationRequestMap[Operation]
): AgentRuntimeInvocation {
  if (operation === "listSessions" || operation === "loadSession") {
    throw new Error(`Remote Agent invocation cannot use unpaged operation "${operation}"`);
  }
  return (request === undefined ? { operation } : { operation, request }) as AgentRuntimeInvocation;
}

function decodeRemoteAgentEvent(
  value: unknown,
  lease: AgentExecutionLease
): TargetedAgentEventEnvelope | null {
  return decodeTargetedAgentEvent(
    value,
    lease.targetId,
    lease.hostEpoch,
    lease.connectionGeneration
  );
}

function decodeTargetedAgentEvent(
  value: unknown,
  targetId: AgentExecutionTargetId,
  hostEpoch: string,
  connectionGeneration: number
): TargetedAgentEventEnvelope | null {
  if (
    !isRecord(value) ||
    value.targetId !== targetId ||
    value.hostEpoch !== hostEpoch ||
    value.connectionGeneration !== connectionGeneration
  ) {
    return null;
  }
  const payload = decodeAgentEventPayload(value);
  const ordering = decodeAgentEventOrdering(value);
  if (
    !payload ||
    !ordering ||
    ordering.eventEpoch === undefined ||
    ordering.eventSequence === undefined
  ) {
    return null;
  }
  return Object.freeze({
    ...payload,
    ...ordering,
    targetId,
    hostEpoch,
    connectionGeneration
  }) as TargetedAgentEventEnvelope;
}

function decodeAgentLiveChannelFrame(
  value: unknown,
  targetId: AgentExecutionTargetId,
  hostEpoch: string,
  connectionGeneration: number
): AgentLiveChannelFrame | null {
  if (
    isRecord(value) &&
    value.eventType === "snapshotRequired" &&
    value.liveEventVersion === AGENT_LIVE_PRESENTATION_VERSION &&
    value.targetId === targetId &&
    value.hostEpoch === hostEpoch &&
    value.connectionGeneration === connectionGeneration &&
    hasOnlyKeys(value, [
      "liveEventVersion",
      "eventType",
      "targetId",
      "hostEpoch",
      "connectionGeneration",
      "reason",
      "lastEventCursor"
    ])
  ) {
    const reason = decodeAgentLiveSnapshotReason(value.reason);
    const lastEventCursor = decodeAgentLiveEventCursor(value.lastEventCursor);
    return reason && lastEventCursor
      ? {
          liveEventVersion: AGENT_LIVE_PRESENTATION_VERSION,
          eventType: "snapshotRequired",
          targetId,
          hostEpoch,
          connectionGeneration,
          reason,
          lastEventCursor
        }
      : null;
  }
  if (
    !isRecord(value) ||
    value.liveEventVersion !== AGENT_LIVE_PRESENTATION_VERSION ||
    value.targetId !== targetId ||
    value.hostEpoch !== hostEpoch ||
    value.connectionGeneration !== connectionGeneration ||
    !isString(value.eventEpoch) ||
    !AGENT_LIVE_JOURNAL_ID_PATTERN.test(value.eventEpoch) ||
    !isNonnegativeSafeInteger(value.eventSequence) ||
    value.eventSequence === 0 ||
    !isBoundedLiveIdentifier(value.sessionId, MAX_AGENT_LIVE_ID_BYTES)
  ) {
    return null;
  }

  const common = {
    liveEventVersion: AGENT_LIVE_PRESENTATION_VERSION,
    targetId,
    hostEpoch,
    connectionGeneration,
    eventEpoch: value.eventEpoch,
    eventSequence: value.eventSequence,
    sessionId: value.sessionId
  } as const;
  const orderedKeys = [
    "liveEventVersion",
    "targetId",
    "hostEpoch",
    "connectionGeneration",
    "eventEpoch",
    "eventSequence",
    "sessionId",
    "eventType"
  ] as const;
  const requiredRunId = () =>
    isBoundedLiveIdentifier(value.runId, MAX_AGENT_LIVE_ID_BYTES) ? value.runId : null;
  const optionalRunId = () =>
    value.runId === undefined
      ? undefined
      : isBoundedLiveIdentifier(value.runId, MAX_AGENT_LIVE_ID_BYTES)
        ? value.runId
        : null;

  switch (value.eventType) {
    case "runStarted": {
      const runId = requiredRunId();
      return runId && hasOnlyKeys(value, [...orderedKeys, "runId"])
        ? { ...common, eventType: "runStarted", runId }
        : null;
    }
    case "timelineUpsert": {
      const runId = optionalRunId();
      const item = decodeAgentPresentedTimelineItem(value.item);
      return runId !== null && item && hasOnlyKeys(value, [...orderedKeys, "runId", "item"])
        ? {
            ...common,
            eventType: "timelineUpsert",
            ...(runId === undefined ? {} : { runId }),
            item
          }
        : null;
    }
    case "timelineCleared": {
      if (value.reason === "explicit_reload") {
        return hasOnlyKeys(value, [...orderedKeys, "reason"])
          ? { ...common, eventType: "timelineCleared", reason: "explicit_reload" }
          : null;
      }
      if (value.reason !== "run_started" && value.reason !== "history_replaced") return null;
      const runId = requiredRunId();
      return runId && hasOnlyKeys(value, [...orderedKeys, "runId", "reason"])
        ? { ...common, eventType: "timelineCleared", runId, reason: value.reason }
        : null;
    }
    case "historyReplaced": {
      const runId = requiredRunId();
      return runId && hasOnlyKeys(value, [...orderedKeys, "runId"])
        ? { ...common, eventType: "historyReplaced", runId }
        : null;
    }
    case "cursorAdvanced":
      return hasOnlyKeys(value, orderedKeys) ? { ...common, eventType: "cursorAdvanced" } : null;
    case "sessionUpdated": {
      const runId = optionalRunId();
      const session = decodeAgentPresentedSessionSummary(value.session);
      return runId !== null &&
        session?.id === value.sessionId &&
        hasOnlyKeys(value, [...orderedKeys, "runId", "session"])
        ? {
            ...common,
            eventType: "sessionUpdated",
            ...(runId === undefined ? {} : { runId }),
            session
          }
        : null;
    }
    case "runFinished": {
      const runId = requiredRunId();
      return runId &&
        (value.terminal === "completed" ||
          value.terminal === "cancelled" ||
          value.terminal === "failed") &&
        hasOnlyKeys(value, [...orderedKeys, "runId", "terminal"])
        ? { ...common, eventType: "runFinished", runId, terminal: value.terminal }
        : null;
    }
    case "sessionDeleted":
      return hasOnlyKeys(value, orderedKeys) ? { ...common, eventType: "sessionDeleted" } : null;
    case "userFacingError": {
      const runId = requiredRunId();
      const item = decodeAgentPresentedTimelineItem(value.item);
      return runId &&
        item &&
        isAgentPresentedUserFacingErrorItem(item) &&
        hasOnlyKeys(value, [...orderedKeys, "runId", "item"])
        ? { ...common, eventType: "userFacingError", runId, item }
        : null;
    }
    default:
      return null;
  }
}

function decodeLegacyLocalAgentEvent(value: unknown): AgentEventPayload | null {
  if (!isRecord(value) || "targetId" in value || "connectionGeneration" in value) return null;
  const payload = decodeAgentEventPayload(value);
  const ordering = decodeAgentEventOrdering(value);
  return payload && ordering ? ({ ...payload, ...ordering } as AgentEventPayload) : null;
}

function decodeAgentEventOrdering(
  value: Record<string, unknown>
): Pick<AgentEventCommonFields, "eventEpoch" | "eventSequence"> | null {
  const hasEpoch = value.eventEpoch !== undefined;
  const hasSequence = value.eventSequence !== undefined;
  if (!hasEpoch && !hasSequence) return {};
  if (
    !hasEpoch ||
    !hasSequence ||
    !isBoundedAgentCursor(value.eventEpoch) ||
    !isConnectionGeneration(value.eventSequence)
  ) {
    return null;
  }
  return {
    eventEpoch: value.eventEpoch,
    eventSequence: value.eventSequence
  };
}

function decodeAgentEventPayload(value: Record<string, unknown>): AgentEventPayload | null {
  switch (value.eventType) {
    case "runtimeStatus":
      return isAgentRuntimeStatus(value.status)
        ? { eventType: "runtimeStatus", status: value.status }
        : null;
    case "sessionCreated":
      return isString(value.sessionId) && isAgentSessionSummary(value.session)
        ? { eventType: "sessionCreated", sessionId: value.sessionId, session: value.session }
        : null;
    case "sessionUpdated":
      return isString(value.sessionId) &&
        isNullableString(value.runId) &&
        isAgentSessionSummary(value.session)
        ? {
            eventType: "sessionUpdated",
            sessionId: value.sessionId,
            ...(value.runId !== undefined ? { runId: value.runId } : {}),
            session: value.session
          }
        : null;
    case "timelineItem":
      return isString(value.sessionId) &&
        isNullableString(value.runId) &&
        isAgentTimelineItem(value.item)
        ? {
            eventType: "timelineItem",
            sessionId: value.sessionId,
            ...(value.runId !== undefined ? { runId: value.runId } : {}),
            item: value.item
          }
        : null;
    case "runStarted":
      return isString(value.sessionId) && isString(value.runId)
        ? { eventType: "runStarted", sessionId: value.sessionId, runId: value.runId }
        : null;
    case "error":
      if (!isString(value.runId)) return null;
      if (isString(value.message) && value.sessionId === undefined && value.item === undefined) {
        return { eventType: "error", runId: value.runId, message: value.message };
      }
      return isString(value.sessionId) &&
        isAgentTimelineItem(value.item) &&
        isNullableString(value.message)
        ? {
            eventType: "error",
            sessionId: value.sessionId,
            runId: value.runId,
            item: value.item,
            ...(value.message !== undefined ? { message: value.message } : {})
          }
        : null;
    case "historyReplaced":
      return isString(value.sessionId) && isString(value.runId)
        ? { eventType: "historyReplaced", sessionId: value.sessionId, runId: value.runId }
        : null;
    case "runFinished":
      return isString(value.sessionId) &&
        isString(value.runId) &&
        (value.message === "completed" ||
          value.message === "cancelled" ||
          value.message === "failed")
        ? {
            eventType: "runFinished",
            sessionId: value.sessionId,
            runId: value.runId,
            message: value.message
          }
        : null;
    default:
      return null;
  }
}

function decodeAgentOperationResult<Operation extends AgentRuntimeOperation>(
  operation: Operation,
  value: unknown,
  requireClosedRemotePresentation: boolean
): AgentRuntimeOperationResultMap[Operation] {
  if (requireClosedRemotePresentation) {
    if (operation === "createSession") {
      const detail = decodeAgentPresentedCreatedSession(value);
      if (!detail) {
        throw new Error(`Agent runtime bridge returned an invalid result for "${operation}"`);
      }
      return detail as AgentRuntimeOperationResultMap[Operation];
    }
    if (operation === "listSessionsPage") {
      const page = decodeAgentPresentedSessionPage(value);
      if (!page) {
        throw new Error(`Agent runtime bridge returned an invalid result for "${operation}"`);
      }
      return page as AgentRuntimeOperationResultMap[Operation];
    }
  }
  const valid = isRemoteOperationResult(operation, value, requireClosedRemotePresentation);
  if (!valid) throw new Error(`Agent runtime bridge returned an invalid result for "${operation}"`);
  return value as AgentRuntimeOperationResultMap[Operation];
}

function decodeAgentPresentedCreatedSession(value: unknown): AgentSessionDetail | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["session", "timeline", "mcpErrors"]) ||
    !Array.isArray(value.timeline) ||
    value.timeline.length !== 0 ||
    !Array.isArray(value.mcpErrors) ||
    value.mcpErrors.length !== 0
  ) {
    return null;
  }
  const session = decodeAgentPresentedSessionSummary(value.session);
  return session ? { session, timeline: [], mcpErrors: [] } : null;
}

function decodeAgentPresentedSessionPage(value: unknown): AgentPage<AgentSessionSummary> | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["items", "nextCursor"]) ||
    !Array.isArray(value.items) ||
    value.items.length > MAX_AGENT_PAGE_SIZE ||
    !isNullableString(value.nextCursor) ||
    (value.nextCursor !== undefined &&
      value.nextCursor !== null &&
      (value.nextCursor.length === 0 ||
        value.nextCursor.length > MAX_AGENT_CURSOR_BYTES ||
        !isAscii(value.nextCursor)))
  ) {
    return null;
  }
  const items: AgentSessionSummary[] = [];
  for (const item of value.items) {
    const summary = decodeAgentPresentedSessionSummary(item);
    if (!summary) return null;
    items.push(summary);
  }
  return {
    items,
    ...(value.nextCursor === undefined ? {} : { nextCursor: value.nextCursor })
  };
}

function isRemoteOperationResult(
  operation: AgentRuntimeOperation,
  value: unknown,
  requireClosedRemotePresentation: boolean
): boolean {
  switch (operation) {
    case "getRuntimeStatus":
    case "startRuntime":
      return isAgentRuntimeStatus(value);
    case "restartRuntime":
    case "stopRuntime":
      return (
        isRecord(value) &&
        isAgentRuntimeStatus(value.status) &&
        (value.acpShutdownError === null || isString(value.acpShutdownError))
      );
    case "loadConfig":
    case "removeProjectRoot":
      return isAgentConfig(value);
    case "saveConfig":
    case "clearUserData":
    case "clearUserHistory":
    case "deleteSession":
    case "cancelRun":
    case "setPermissionMode":
    case "respondToPermission":
      return value === undefined || value === null;
    case "listMcpServers":
    case "saveMcpServers":
      return Array.isArray(value) && value.every(isAgentMcpServer);
    case "listSessionMcpServers":
    case "setSessionMcpServerEnabled":
      return Array.isArray(value) && value.every(isAgentSessionMcpServer);
    case "listRecentProjectRoots":
    case "saveProjectRootOrder":
      return Array.isArray(value) && value.every(isRecentProjectRoot);
    case "saveRecentProjectRoot":
      return (
        isRecord(value) &&
        isString(value.projectRoot) &&
        Array.isArray(value.roots) &&
        value.roots.every(isRecentProjectRoot) &&
        isAgentConfig(value.config)
      );
    case "getProjectSkillsTrust":
    case "setProjectSkillsTrust":
      return isAgentProjectSkillsTrustStatus(value);
    case "createSession":
      return isAgentSessionDetail(value);
    case "listSessions":
      return Array.isArray(value) && value.every(isAgentSessionSummary);
    case "loadSession":
      return isAgentSessionDetail(value);
    case "listSessionsPage":
      return isAgentPage(value, isAgentSessionSummary);
    case "listSessionRecordsPage":
      return requireClosedRemotePresentation
        ? isAgentPresentedSessionRecordsPage(value)
        : isAgentSessionRecordsPage(value);
    case "renameSession":
      return isAgentSessionSummary(value);
    case "sendMessage":
      return isRecord(value) && isString(value.runId);
  }
}

function isAgentRuntimeStatus(value: unknown): value is AgentRuntimeStatus {
  if (!isRecord(value) || typeof value.running !== "boolean") return false;
  if (
    !isNullableString(value.projectRoot) ||
    !isNullableString(value.model) ||
    !isNullableString(value.mode)
  ) {
    return false;
  }
  return (
    value.activeRuns === undefined ||
    (isRecord(value.activeRuns) && Object.values(value.activeRuns).every(isString))
  );
}

function isAgentConfig(value: unknown): value is AgentConfig {
  if (!isRecord(value) || !isString(value.defaultModel)) return false;
  if (!isNullableString(value.defaultProjectRoot)) return false;
  if (
    value.projectSkillsTrust !== undefined &&
    (!Array.isArray(value.projectSkillsTrust) ||
      !value.projectSkillsTrust.every(
        (item) => isRecord(item) && isString(item.path) && typeof item.trusted === "boolean"
      ))
  ) {
    return false;
  }
  return (
    value.removedProjectRoots === undefined ||
    (Array.isArray(value.removedProjectRoots) && value.removedProjectRoots.every(isString))
  );
}

function isAgentSessionSummary(value: unknown): value is AgentSessionSummary {
  return (
    isRecord(value) &&
    isString(value.id) &&
    isString(value.title) &&
    isString(value.projectRoot) &&
    isFiniteNumber(value.createdMs) &&
    isFiniteNumber(value.updatedMs) &&
    isFiniteNumber(value.pageSortMs) &&
    isFiniteNumber(value.messageCount) &&
    isNullableString(value.model) &&
    isString(value.mode)
  );
}

function isAgentTimelineItem(value: unknown): value is AgentTimelineItem {
  return (
    isRecord(value) &&
    isBoundedAgentCursor(value.id) &&
    (value.itemType === "message" ||
      value.itemType === "thinking" ||
      value.itemType === "tool" ||
      value.itemType === "permission" ||
      value.itemType === "system" ||
      value.itemType === "error") &&
    isFiniteNumber(value.createdMs) &&
    isString(value.merge) &&
    isNullableString(value.role) &&
    isNullableString(value.title) &&
    isNullableString(value.text) &&
    isNullableString(value.status)
  );
}

function isAgentSessionDetail(value: unknown): value is AgentSessionDetail {
  return (
    isRecord(value) &&
    isAgentSessionSummary(value.session) &&
    Array.isArray(value.timeline) &&
    value.timeline.every(isAgentTimelineItem) &&
    Array.isArray(value.mcpErrors) &&
    value.mcpErrors.every(
      (error) => isRecord(error) && isString(error.name) && isString(error.error)
    )
  );
}

function isRecentProjectRoot(value: unknown): value is RecentProjectRoot {
  return (
    isRecord(value) &&
    isString(value.path) &&
    isString(value.name) &&
    isFiniteNumber(value.lastUsedMs)
  );
}

function isAgentProjectSkillsTrustStatus(value: unknown): value is AgentProjectSkillsTrustStatus {
  return (
    isRecord(value) &&
    isString(value.path) &&
    (value.decision === undefined ||
      value.decision === null ||
      typeof value.decision === "boolean") &&
    typeof value.available === "boolean"
  );
}

function isAgentMcpServer(value: unknown): value is AgentMcpServer {
  return (
    isRecord(value) &&
    isString(value.name) &&
    isString(value.description) &&
    typeof value.enabled === "boolean" &&
    isFiniteNumber(value.timeoutSeconds) &&
    isRecord(value.transport) &&
    ((value.transport.type === "stdio" &&
      isString(value.transport.command) &&
      isKeyValueList(value.transport.environment)) ||
      (value.transport.type === "streamable_http" &&
        isString(value.transport.url) &&
        isKeyValueList(value.transport.environment) &&
        isKeyValueList(value.transport.headers)))
  );
}

function isKeyValueList(value: unknown): value is AgentMcpKeyValue[] {
  return (
    Array.isArray(value) &&
    value.every((item) => isRecord(item) && isString(item.key) && isString(item.value))
  );
}

function isAgentSessionMcpServer(value: unknown): value is AgentSessionMcpServer {
  return (
    isRecord(value) &&
    isString(value.name) &&
    isString(value.description) &&
    (value.transport === "stdio" || value.transport === "streamable_http") &&
    typeof value.enabled === "boolean" &&
    typeof value.available === "boolean"
  );
}

function isAgentPage<T>(
  value: unknown,
  isItem: (item: unknown) => item is T
): value is AgentPage<T> {
  return (
    isRecord(value) &&
    Array.isArray(value.items) &&
    value.items.length <= MAX_AGENT_PAGE_SIZE &&
    value.items.every(isItem) &&
    isNullableString(value.nextCursor) &&
    (value.nextCursor === undefined ||
      value.nextCursor === null ||
      (value.nextCursor.length > 0 &&
        value.nextCursor.length <= MAX_AGENT_CURSOR_BYTES &&
        isAscii(value.nextCursor)))
  );
}

function isBoundedAgentCursor(value: unknown): value is string {
  return (
    isString(value) && value.length > 0 && value.length <= MAX_AGENT_CURSOR_BYTES && isAscii(value)
  );
}

function isAgentHistoryRecord(value: unknown): value is AgentHistoryRecord {
  return (
    isRecord(value) &&
    hasOnlyKeys(value, ["recordId", "role", "createdMs", "items"]) &&
    isBoundedAgentCursor(value.recordId) &&
    isString(value.role) &&
    value.role.length > 0 &&
    value.role.length <= MAX_AGENT_HISTORY_ROLE_BYTES &&
    // Record roles are opaque paging metadata, never presentation authority.
    // Limit them to printable ASCII so controls and bidi markers cannot cross
    // the native/remote boundary; rendered semantics come only from `items`.
    isPrintableAscii(value.role) &&
    isFiniteNumber(value.createdMs) &&
    Array.isArray(value.items) &&
    value.items.length <= MAX_AGENT_HISTORY_ITEMS_PER_RECORD &&
    value.items.every(isAgentTimelineItem)
  );
}

function isAgentSessionRecordsPage(value: unknown): value is AgentSessionRecordsPage {
  return (
    isRecord(value) &&
    hasOnlyKeys(value, ["records", "nextCursor", "historyRevision"]) &&
    Array.isArray(value.records) &&
    value.records.length <= MAX_AGENT_PAGE_SIZE &&
    value.records.every(isAgentHistoryRecord) &&
    // Plain pages are intentionally unsynchronized. Absolute live state and
    // event checkpoints belong only to the paused attach-coordinator result.
    value.liveItems === undefined &&
    value.throughEventCursor === undefined &&
    isBoundedAgentCursor(value.historyRevision) &&
    isNullableString(value.nextCursor) &&
    (value.nextCursor === undefined ||
      value.nextCursor === null ||
      isBoundedAgentCursor(value.nextCursor))
  );
}

async function invokeAgent<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriDesktop()) {
    throw new Error("Agent Mode is available in Maple Desktop.");
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

async function listenToLocalAgentEvents(
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  handler: AgentBridgeEventHandler
): Promise<UnlistenAgentEvents> {
  if (lease) throw new Error("The embedded Agent event bridge does not accept a remote lease");
  if (target.kind !== "local" || target.id !== LOCAL_AGENT_EXECUTION_TARGET_ID) {
    throw new Error(`The local Agent bridge cannot subscribe to target "${target.id as string}"`);
  }
  if (!isTauriDesktop()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<unknown>("agent-event", (event) => {
    handler(event.payload);
  });
}

function expectedLiveLease(lease: AgentExecutionLease): {
  targetId: AgentExecutionTargetId;
  hostEpoch: string;
  connectionGeneration: number;
} {
  return {
    targetId: lease.targetId,
    hostEpoch: lease.hostEpoch,
    connectionGeneration: lease.connectionGeneration
  };
}

function assertTauriRemoteLiveBridge(
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget
): asserts lease is AgentExecutionLease {
  if (!lease || target.kind !== "remote" || target.id !== lease.targetId) {
    throw new Error("Synchronized Agent history requires a verified remote host lease");
  }
  if (!isTauriDesktop()) throw new Error("Agent Mode is available in Maple Desktop.");
}

async function beginLocalSessionHistoryAttach(
  userId: string,
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  request: AgentListSessionRecordsPageRequest,
  handler: AgentBridgeEventHandler
): Promise<AgentBridgeLiveChannelResult> {
  assertTauriRemoteLiveBridge(lease, target);
  const { Channel, invoke } = await import("@tauri-apps/api/core");
  const events = new Channel<unknown>(handler);
  const result = await invoke<unknown>("agent_begin_session_history_attach", {
    userId,
    request,
    expectedLease: expectedLiveLease(lease),
    events
  });
  return { result, keepAlive: events };
}

async function activateLocalSessionHistoryAttach(
  userId: string,
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  attachId: string
): Promise<unknown> {
  assertTauriRemoteLiveBridge(lease, target);
  return await invokeAgent<unknown>("agent_activate_session_history_attach", {
    userId,
    attachId,
    expectedLease: expectedLiveLease(lease)
  });
}

async function cancelLocalSessionHistoryAttach(
  userId: string,
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  attachId: string
): Promise<void> {
  assertTauriRemoteLiveBridge(lease, target);
  await invokeAgent<void>("agent_cancel_session_history_attach", {
    userId,
    attachId,
    expectedLease: expectedLiveLease(lease)
  });
}

async function resumeLocalLiveEvents(
  userId: string,
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  cursor: AgentLiveEventCursor,
  handler: AgentBridgeEventHandler
): Promise<AgentBridgeLiveChannelResult> {
  assertTauriRemoteLiveBridge(lease, target);
  const { Channel, invoke } = await import("@tauri-apps/api/core");
  const events = new Channel<unknown>(handler);
  const result = await invoke<unknown>("agent_resume_live_events", {
    userId,
    cursor,
    expectedLease: expectedLiveLease(lease),
    events
  });
  return { result, keepAlive: events };
}

async function cancelLocalLiveEvents(
  userId: string,
  lease: AgentExecutionLease | null,
  target: AgentExecutionTarget,
  liveStreamId: string
): Promise<void> {
  assertTauriRemoteLiveBridge(lease, target);
  await invokeAgent<void>("agent_cancel_live_events", {
    userId,
    liveStreamId,
    expectedLease: expectedLiveLease(lease)
  });
}

export const agentRuntimeService = new AgentRuntimeService();

export async function retireAgentRuntimeAccountResources(userId: string): Promise<void> {
  await agentRuntimeAccountResourceRegistry.retireAccount(userId);
}

export function activateAgentRuntimeAccountResources(userId: string): void {
  agentRuntimeAccountResourceRegistry.activateAccount(userId);
}

export interface AgentAuthAccountRetirementBridge {
  blockAndDrain(userId: string): Promise<AgentOperationBlock>;
  retireRemoteAccount(userId: string): Promise<void>;
  isDesktop(): boolean;
  stopLocalHost(userId: string): Promise<AgentRuntimeLifecycleOutcome>;
  clearLocalAuth(userId: string): Promise<void>;
  stopLocalProxy(): Promise<void>;
}

export async function retireAgentAuthAccount(
  userId: string,
  bridge: AgentAuthAccountRetirementBridge
): Promise<void> {
  const block = await bridge.blockAndDrain(userId);
  try {
    await bridge.retireRemoteAccount(userId);
    if (bridge.isDesktop()) {
      const outcome = await bridge.stopLocalHost(userId);
      if (outcome.acpShutdownError) throw new AgentRuntimePartialStopError(outcome);
      await bridge.clearLocalAuth(userId);
      await bridge.stopLocalProxy();
    }
    block.retainUntilNextSession();
  } catch (error) {
    block.release();
    throw error;
  }
}

const defaultAgentAuthAccountRetirementBridge: AgentAuthAccountRetirementBridge = {
  blockAndDrain: async (userId) => await agentOperationFence.blockAndDrain(userId),
  retireRemoteAccount: retireAgentRuntimeAccountResources,
  isDesktop: isTauriDesktop,
  stopLocalHost: async (userId) =>
    await invokeAgent<AgentRuntimeLifecycleOutcome>(
      LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION.stopRuntime,
      { userId }
    ),
  clearLocalAuth: async (userId) => await mapleApiAuthService.clear(userId),
  stopLocalProxy: async () => {
    const { proxyService } = await import("@/services/proxyService");
    await proxyService.stopAndResetProxy();
  }
};

const agentAuthLifecycle = new AgentAuthLifecycleCoordinator(
  async (userId) => await retireAgentAuthAccount(userId, defaultAgentAuthAccountRetirementBridge),
  async (userId) => {
    await mapleApiAuthService.activate(userId);
    activateAgentRuntimeAccountResources(userId);
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
  userId?: string | null,
  runtimeService: AgentRuntimeService = agentRuntimeService
): Promise<AgentOperationBlock> {
  if (runtimeService === agentRuntimeService) {
    if (!isTauriDesktop()) return noOpOperationBlock();
    if (!userId) throw new Error("Cannot stop Agent Mode without an authenticated user");
    return await agentRuntimeStopCoordinator.stop(userId);
  }
  if (!userId) throw new Error("Cannot stop Agent Mode without an authenticated user");
  const outcome = await runtimeService.stopRuntime(userId);
  if (outcome.acpShutdownError) throw new AgentRuntimePartialStopError(outcome);
  return noOpOperationBlock();
}

export async function clearAgentDataForUser(
  userId?: string | null,
  runtimeService: AgentRuntimeService = agentRuntimeService
): Promise<AgentOperationBlock> {
  if (runtimeService === agentRuntimeService && !isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot clear Agent Mode data without an authenticated user");
  const block = await stopAgentRuntimeForUser(userId, runtimeService);
  try {
    if (runtimeService === agentRuntimeService && isTauriDesktop()) {
      // The local cleanup fence is intentionally held, so bypass its ordinary
      // run gate while still deriving the command from the semantic operation.
      await invokeAgent(LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION.clearUserData, { userId });
    } else {
      await runtimeService.clearUserData(userId);
    }
    return block;
  } catch (error) {
    block.release();
    throw error;
  }
}

export async function clearAgentHistoryForUser(
  userId?: string | null,
  runtimeService: AgentRuntimeService = agentRuntimeService
): Promise<AgentOperationBlock> {
  if (runtimeService === agentRuntimeService && !isTauriDesktop()) return noOpOperationBlock();
  if (!userId) throw new Error("Cannot clear Agent Mode history without an authenticated user");
  const block = await stopAgentRuntimeForUser(userId, runtimeService);
  try {
    if (runtimeService === agentRuntimeService && isTauriDesktop()) {
      await invokeAgent(LOCAL_COMMAND_BY_AGENT_RUNTIME_OPERATION.clearUserHistory, { userId });
    } else {
      await runtimeService.clearUserHistory(userId);
    }
    return block;
  } catch (error) {
    block.release();
    throw error;
  }
}

function noOpOperationBlock(): AgentOperationBlock {
  return { release: () => {}, retainUntilNextSession: () => {} };
}
