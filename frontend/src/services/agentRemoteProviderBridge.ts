import {
  decodeAgentRemoteCapabilitySnapshot,
  isAgentRemotePersistedTranscriptReady,
  type AgentRemoteCapabilitySnapshot
} from "@/services/agentRemoteCapabilities";

const MAX_REMOTE_AGENT_ACTIVE_RUNS = 64;
const MAX_REMOTE_AGENT_PAGE_SIZE = 50;
const MAX_REMOTE_AGENT_CURSOR_BYTES = 512;
const MAX_REMOTE_AGENT_ID_BYTES = 128;
const MAX_REMOTE_AGENT_TITLE_BYTES = 1_024;
const MAX_REMOTE_AGENT_STATUS_BYTES = 64;
const MAX_REMOTE_AGENT_TEXT_BYTES = 192 * 1_024;
const MAX_REMOTE_AGENT_HISTORY_ITEMS_PER_RECORD = 200;
const MAX_REMOTE_AGENT_HISTORY_ROLE_BYTES = 128;
const MAX_REMOTE_AGENT_HISTORY_RECORD_BYTES = 1_040_384;
const MAX_REMOTE_AGENT_HISTORY_PAGE_BYTES = 8 * 1_024 * 1_024;
const SAFE_REMOTE_TOOL_TITLE = "Tool activity";
const SAFE_REMOTE_TOOL_FAILED = "The tool failed. Open the host for additional diagnostic details.";
const SAFE_REMOTE_TOOL_CANCELLED = "The tool was cancelled.";
const SAFE_REMOTE_PERMISSION_TITLE = "Tool permission";
const SAFE_REMOTE_AGENT_ERROR =
  "The Agent task failed. Open the host for additional diagnostic details.";
const SAFE_REMOTE_TOKEN_PATTERN = /^[A-Za-z0-9._:-]+$/;
const agentRemoteReadOnlyClients = new WeakSet<object>();

export interface AgentRemoteAuthenticatedBinding {
  readonly accountId: string;
  readonly targetId: string;
  readonly targetLabel?: string;
}

/** Closed runtime status projection; host run IDs and provider fields stay out. */
export interface AgentRemoteRuntimeSummary {
  readonly running: boolean;
  readonly activeRunCount: number;
}

/** Persisted task metadata admitted to the paired-host presentation. */
export interface AgentRemoteSessionSummary {
  readonly id: string;
  readonly title: string;
  readonly createdMs: number;
  readonly updatedMs: number;
  readonly pageSortMs: number;
  readonly messageCount: number;
}

export interface AgentRemoteTimelineItem {
  readonly id: string;
  readonly itemType: "message" | "thinking" | "tool" | "permission" | "system" | "error";
  readonly role?: "user" | "assistant" | "thought" | "system";
  readonly title?: string;
  readonly text?: string;
  readonly status?: string;
  readonly createdMs: number;
  readonly merge: "append" | "replace";
}

export interface AgentRemoteHistoryRecord {
  readonly recordId: string;
  readonly role: string;
  readonly createdMs: number;
  readonly items: AgentRemoteTimelineItem[];
}

export interface AgentRemotePageRequest {
  readonly cursor?: string | null;
  readonly limit: number;
}

export interface AgentRemoteRecordsPageRequest extends AgentRemotePageRequest {
  readonly sessionId: string;
}

export interface AgentRemoteSessionPage {
  readonly items: AgentRemoteSessionSummary[];
  readonly nextCursor?: string | null;
}

export interface AgentRemoteRecordsPage {
  readonly records: AgentRemoteHistoryRecord[];
  readonly historyRevision: string;
  readonly nextCursor?: string | null;
}

/**
 * Named data source captured by the branded presentation client. This is not a
 * generic command surface and accepts neither an account nor an execution
 * operation from component input.
 */
export interface AgentRemoteReadOnlySource {
  getRuntimeStatus(): Promise<unknown>;
  listSessionSummariesPage(request: AgentRemotePageRequest): Promise<unknown>;
  listPersistedRecordsPage(request: AgentRemoteRecordsPageRequest): Promise<unknown>;
}

/**
 * The only remote operations exposed to the Phase 1 presentation. There is no
 * generic invoke, composer, mutation, permission, administration, or live API.
 */
export interface AgentRemoteReadOnlyClient {
  readonly binding: AgentRemoteAuthenticatedBinding;
  readonly capabilities: AgentRemoteCapabilitySnapshot;
  getRuntimeStatus(): Promise<AgentRemoteRuntimeSummary>;
  listSessionSummariesPage(request: AgentRemotePageRequest): Promise<AgentRemoteSessionPage>;
  listPersistedRecordsPage(request: AgentRemoteRecordsPageRequest): Promise<AgentRemoteRecordsPage>;
}

export interface CreateAgentRemoteReadOnlyClientOptions {
  readonly accountId: string;
  readonly targetId: string;
  readonly targetLabel?: string;
  readonly source: AgentRemoteReadOnlySource;
  readonly capabilities: unknown;
}

const AGENT_REMOTE_READ_ONLY_CLIENT_KEYS = [
  "binding",
  "capabilities",
  "getRuntimeStatus",
  "listSessionSummariesPage",
  "listPersistedRecordsPage"
] as const;

export function isClosedAgentRemoteReadOnlyClient(
  value: unknown
): value is AgentRemoteReadOnlyClient {
  if (
    !isRecord(value) ||
    !agentRemoteReadOnlyClients.has(value) ||
    !hasOnlyKeys(value, AGENT_REMOTE_READ_ONLY_CLIENT_KEYS)
  ) {
    return false;
  }
  if (
    typeof value.getRuntimeStatus !== "function" ||
    typeof value.listSessionSummariesPage !== "function" ||
    typeof value.listPersistedRecordsPage !== "function" ||
    !isAgentRemotePersistedTranscriptReady(value.capabilities) ||
    !isRecord(value.binding) ||
    !hasOnlyKeys(value.binding, ["accountId", "targetId", "targetLabel"])
  ) {
    return false;
  }
  return (
    typeof value.binding.accountId === "string" &&
    isBoundedOwnerId(value.binding.accountId) &&
    typeof value.binding.targetId === "string" &&
    isRemoteTargetId(value.binding.targetId) &&
    (value.binding.targetLabel === undefined ||
      (typeof value.binding.targetLabel === "string" &&
        isSafeDisplayText(value.binding.targetLabel, 256, false)))
  );
}

/**
 * Brand a provider-owned, account/target-bound source as the closed Phase 1
 * client. Every request and result is reconstructed through the sanitized DTO
 * boundary; source-owned objects and extension fields never escape.
 */
export function createAgentRemoteReadOnlyClient({
  accountId,
  targetId,
  targetLabel,
  source,
  capabilities
}: CreateAgentRemoteReadOnlyClientOptions): AgentRemoteReadOnlyClient {
  if (!isBoundedOwnerId(accountId)) {
    throw new Error("Remote Agent transcript access requires a bounded account binding");
  }
  if (!isRemoteTargetId(targetId)) {
    throw new Error("Remote Agent transcript access requires a bounded remote target binding");
  }
  if (targetLabel !== undefined && !isSafeDisplayText(targetLabel, 256, false)) {
    throw new Error("Remote Agent transcript target label is invalid");
  }
  if (
    !source ||
    typeof source.getRuntimeStatus !== "function" ||
    typeof source.listSessionSummariesPage !== "function" ||
    typeof source.listPersistedRecordsPage !== "function"
  ) {
    throw new Error("Remote Agent transcript source is incomplete");
  }
  if (!isAgentRemotePersistedTranscriptReady(capabilities)) {
    throw new Error("Remote Agent transcript capabilities are unavailable or too broad");
  }

  const decodedCapabilities = decodeAgentRemoteCapabilitySnapshot(capabilities)!;
  const binding: AgentRemoteAuthenticatedBinding = Object.freeze({
    accountId,
    targetId,
    ...(targetLabel ? { targetLabel } : {})
  });

  const client: AgentRemoteReadOnlyClient = Object.freeze({
    binding,
    capabilities: decodedCapabilities,
    getRuntimeStatus: async () => {
      const status = decodeAgentRemoteRuntimeSummary(await source.getRuntimeStatus());
      if (!status) throw invalidRemoteResult("runtime status");
      return status;
    },
    listSessionSummariesPage: async (request: AgentRemotePageRequest) => {
      const decodedRequest = decodeAgentRemotePageRequest(request);
      if (!decodedRequest) throw invalidRemoteRequest("task page");
      const page = decodeAgentRemoteSessionPage(
        await source.listSessionSummariesPage(decodedRequest),
        decodedRequest
      );
      if (!page) throw invalidRemoteResult("task page");
      return page;
    },
    listPersistedRecordsPage: async (request: AgentRemoteRecordsPageRequest) => {
      const decodedRequest = decodeAgentRemoteRecordsPageRequest(request);
      if (!decodedRequest) throw invalidRemoteRequest("history page");
      const page = decodeAgentRemoteRecordsPage(
        await source.listPersistedRecordsPage(decodedRequest),
        decodedRequest
      );
      if (!page) throw invalidRemoteResult("history page");
      return page;
    }
  });
  agentRemoteReadOnlyClients.add(client);
  return client;
}

export function decodeAgentRemoteRuntimeSummary(value: unknown): AgentRemoteRuntimeSummary | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["running", "activeRunCount"]) ||
    typeof value.running !== "boolean" ||
    !isNonnegativeSafeInteger(value.activeRunCount) ||
    value.activeRunCount > MAX_REMOTE_AGENT_ACTIVE_RUNS ||
    (!value.running && value.activeRunCount !== 0)
  ) {
    return null;
  }
  return Object.freeze({ running: value.running, activeRunCount: value.activeRunCount });
}

export function decodeAgentRemotePageRequest(value: unknown): AgentRemotePageRequest | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["cursor", "limit"]) ||
    !Number.isSafeInteger(value.limit) ||
    (value.limit as number) < 1 ||
    (value.limit as number) > MAX_REMOTE_AGENT_PAGE_SIZE ||
    !isNullableSafeToken(value.cursor, MAX_REMOTE_AGENT_CURSOR_BYTES)
  ) {
    return null;
  }
  return Object.freeze({
    ...(value.cursor === undefined ? {} : { cursor: value.cursor as string | null }),
    limit: value.limit as number
  });
}

export function decodeAgentRemoteRecordsPageRequest(
  value: unknown
): AgentRemoteRecordsPageRequest | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["sessionId", "cursor", "limit"]) ||
    !isSafeToken(value.sessionId, MAX_REMOTE_AGENT_ID_BYTES)
  ) {
    return null;
  }
  const page = decodeAgentRemotePageRequest({
    ...(value.cursor === undefined ? {} : { cursor: value.cursor }),
    limit: value.limit
  });
  if (!page) return null;
  return Object.freeze({ sessionId: value.sessionId, ...page });
}

export function decodeAgentRemoteSessionPage(
  value: unknown,
  request: AgentRemotePageRequest
): AgentRemoteSessionPage | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["items", "nextCursor"]) ||
    !Array.isArray(value.items) ||
    !isReturnedPageShape(value.items.length, value.nextCursor, request)
  ) {
    return null;
  }
  const items: AgentRemoteSessionSummary[] = [];
  const ids = new Set<string>();
  for (const item of value.items) {
    const decoded = decodeAgentRemoteSessionSummary(item);
    if (!decoded || ids.has(decoded.id)) return null;
    ids.add(decoded.id);
    items.push(decoded);
  }
  return Object.freeze({
    items,
    ...(value.nextCursor === undefined ? {} : { nextCursor: value.nextCursor as string | null })
  });
}

export function decodeAgentRemoteRecordsPage(
  value: unknown,
  request: AgentRemoteRecordsPageRequest
): AgentRemoteRecordsPage | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["records", "historyRevision", "nextCursor"]) ||
    !Array.isArray(value.records) ||
    !isSafeToken(value.historyRevision, MAX_REMOTE_AGENT_CURSOR_BYTES) ||
    !isReturnedPageShape(value.records.length, value.nextCursor, request)
  ) {
    return null;
  }

  const records: AgentRemoteHistoryRecord[] = [];
  const recordIds = new Set<string>();
  let pageBytes =
    1_024 +
    utf8ByteLength(value.historyRevision) +
    (typeof value.nextCursor === "string" ? utf8ByteLength(value.nextCursor) : 0);
  for (const record of value.records) {
    const decoded = decodeAgentRemoteHistoryRecord(record);
    if (!decoded || recordIds.has(decoded.recordId)) return null;
    recordIds.add(decoded.recordId);
    const encoded = JSON.stringify(decoded);
    const recordBytes = utf8ByteLength(encoded);
    if (recordBytes > MAX_REMOTE_AGENT_HISTORY_RECORD_BYTES) return null;
    pageBytes += recordBytes + 1;
    if (pageBytes > MAX_REMOTE_AGENT_HISTORY_PAGE_BYTES) return null;
    records.push(decoded);
  }
  return Object.freeze({
    records,
    historyRevision: value.historyRevision,
    ...(value.nextCursor === undefined ? {} : { nextCursor: value.nextCursor as string | null })
  });
}

function decodeAgentRemoteSessionSummary(value: unknown): AgentRemoteSessionSummary | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["id", "title", "createdMs", "updatedMs", "pageSortMs", "messageCount"]) ||
    !isSafeToken(value.id, MAX_REMOTE_AGENT_ID_BYTES) ||
    !isSafeDisplayText(value.title, MAX_REMOTE_AGENT_TITLE_BYTES, false) ||
    !isNonnegativeSafeInteger(value.createdMs) ||
    !isNonnegativeSafeInteger(value.updatedMs) ||
    !isNonnegativeSafeInteger(value.pageSortMs) ||
    !isNonnegativeSafeInteger(value.messageCount)
  ) {
    return null;
  }
  return Object.freeze({
    id: value.id,
    title: value.title,
    createdMs: value.createdMs,
    updatedMs: value.updatedMs,
    pageSortMs: value.pageSortMs,
    messageCount: value.messageCount
  });
}

function decodeAgentRemoteHistoryRecord(value: unknown): AgentRemoteHistoryRecord | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["recordId", "role", "createdMs", "items"]) ||
    !isSafeToken(value.recordId, MAX_REMOTE_AGENT_CURSOR_BYTES) ||
    typeof value.role !== "string" ||
    value.role.length < 1 ||
    value.role.length > MAX_REMOTE_AGENT_HISTORY_ROLE_BYTES ||
    !isPrintableAscii(value.role) ||
    !isNonnegativeSafeInteger(value.createdMs) ||
    !Array.isArray(value.items) ||
    value.items.length > MAX_REMOTE_AGENT_HISTORY_ITEMS_PER_RECORD
  ) {
    return null;
  }
  const items: AgentRemoteTimelineItem[] = [];
  for (const item of value.items) {
    const decoded = decodeAgentRemoteTimelineItem(item);
    if (!decoded) return null;
    items.push(decoded);
  }
  return Object.freeze({
    recordId: value.recordId,
    role: value.role,
    createdMs: value.createdMs,
    items
  });
}

function decodeAgentRemoteTimelineItem(value: unknown): AgentRemoteTimelineItem | null {
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
    !isSafeToken(value.id, MAX_REMOTE_AGENT_ID_BYTES) ||
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
    !isOptionalSafeDisplayText(value.title, MAX_REMOTE_AGENT_TITLE_BYTES) ||
    !isOptionalTimelineText(value.text) ||
    !isOptionalSafeDisplayText(value.status, MAX_REMOTE_AGENT_STATUS_BYTES) ||
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

  return Object.freeze({
    id: value.id,
    itemType: value.itemType,
    ...(value.role === undefined ? {} : { role: value.role }),
    ...(value.title === undefined ? {} : { title: value.title }),
    ...(value.text === undefined ? {} : { text: value.text }),
    ...(value.status === undefined ? {} : { status: value.status }),
    createdMs: value.createdMs,
    merge: value.merge
  }) as AgentRemoteTimelineItem;
}

function isReturnedPageShape(
  itemCount: number,
  nextCursor: unknown,
  request: AgentRemotePageRequest
): boolean {
  return (
    itemCount <= request.limit &&
    isNullableSafeToken(nextCursor, MAX_REMOTE_AGENT_CURSOR_BYTES) &&
    !(itemCount === 0 && nextCursor !== undefined && nextCursor !== null) &&
    !(typeof nextCursor === "string" && nextCursor === request.cursor)
  );
}

function invalidRemoteRequest(kind: string): Error {
  return new Error(`Remote Agent ${kind} request is invalid`);
}

function invalidRemoteResult(kind: string): Error {
  return new Error(`Remote Agent ${kind} response is invalid`);
}

function isBoundedOwnerId(value: string): boolean {
  return value.length > 0 && value.length <= 256 && !hasControlCharacter(value);
}

function isRemoteTargetId(value: string): boolean {
  return value !== "local" && isSafeToken(value, MAX_REMOTE_AGENT_ID_BYTES);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowedKeys: readonly string[]): boolean {
  const allowed = new Set(allowedKeys);
  return Reflect.ownKeys(value).every((key) => typeof key === "string" && allowed.has(key));
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isSafeToken(value: unknown, maxBytes: number): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= maxBytes &&
    SAFE_REMOTE_TOKEN_PATTERN.test(value)
  );
}

function isNullableSafeToken(value: unknown, maxBytes: number): boolean {
  return value === undefined || value === null || isSafeToken(value, maxBytes);
}

function isPrintableAscii(value: string): boolean {
  return [...value].every((character) => {
    const code = character.charCodeAt(0);
    return code >= 0x20 && code <= 0x7e;
  });
}

function hasControlCharacter(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x1f || code === 0x7f) return true;
  }
  return false;
}

function isOptionalSafeDisplayText(value: unknown, maxBytes: number): boolean {
  return value === undefined || isSafeDisplayText(value, maxBytes, true);
}

function isOptionalTimelineText(value: unknown): boolean {
  return (
    value === undefined ||
    (typeof value === "string" &&
      utf8ByteLength(value) <= MAX_REMOTE_AGENT_TEXT_BYTES &&
      !value.includes("\0") &&
      hasValidSurrogates(value))
  );
}

function isSafeDisplayText(value: unknown, maxBytes: number, allowEmpty: boolean): value is string {
  if (
    typeof value !== "string" ||
    (!allowEmpty && value.length === 0) ||
    utf8ByteLength(value) > maxBytes
  ) {
    return false;
  }
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (
      code <= 0x1f ||
      (code >= 0x7f && code <= 0x9f) ||
      code === 0x061c ||
      code === 0x200e ||
      code === 0x200f ||
      (code >= 0x202a && code <= 0x202e) ||
      (code >= 0x2066 && code <= 0x2069)
    ) {
      return false;
    }
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function hasValidSurrogates(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}
