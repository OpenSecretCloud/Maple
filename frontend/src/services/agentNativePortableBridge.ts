import { isTauriMobile } from "@/utils/platform";
import {
  decodeAgentRemoteRecordsPage,
  decodeAgentRemoteRuntimeSummary,
  decodeAgentRemoteSessionPage,
  type AgentRemotePageRequest,
  type AgentRemoteReadOnlySource,
  type AgentRemoteRecordsPage,
  type AgentRemoteRecordsPageRequest,
  type AgentRemoteRuntimeSummary,
  type AgentRemoteSessionPage
} from "@/services/agentRemoteProviderBridge";
import {
  decodeAgentRemoteCapabilitySnapshot,
  isAgentRemotePersistedTranscriptReady,
  type AgentRemoteCapabilitySnapshot
} from "@/services/agentRemoteCapabilities";

const MAX_NATIVE_TARGETS = 64;
const MAX_TARGET_LABEL_BYTES = 256;
const MAX_TARGET_LABEL_CHARACTERS = 80;
const MAX_RUNTIME_ID_BYTES = 128;
const MAX_U64 = 18_446_744_073_709_551_615n;
const MAX_U64_DECIMAL_DIGITS = 20;
const NIL_ACCOUNT_ID = "00000000-0000-0000-0000-000000000000";
const ACCOUNT_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const RUNTIME_ID_PATTERN = /^runtime_[0-9a-f]{48}$/;
const TARGET_HANDLE_PATTERN = /^target_[0-9a-f]{48}$/;
const LEASE_HANDLE_PATTERN = /^lease_[0-9a-f]{48}$/;

export const AGENT_NATIVE_PORTABLE_COMMANDS = Object.freeze({
  refreshTargets: "agent_portable_refresh_targets",
  prepareTarget: "agent_portable_prepare_target",
  getRuntimeStatus: "agent_portable_get_runtime_status",
  listSessionsPage: "agent_portable_list_sessions_page",
  listRecordsPage: "agent_portable_list_records_page"
});

export type AgentNativePortableErrorCode =
  | "unavailable"
  | "unauthenticated"
  | "pairing_unavailable"
  | "unknown_target"
  | "busy"
  | "cancelled"
  | "stale_runtime"
  | "stale_lease"
  | "invalid_request"
  | "invalid_response"
  | "peer_unavailable"
  | "cleanup_failed";

const NATIVE_PORTABLE_ERROR_CODES = new Set<AgentNativePortableErrorCode>([
  "unavailable",
  "unauthenticated",
  "pairing_unavailable",
  "unknown_target",
  "busy",
  "cancelled",
  "stale_runtime",
  "stale_lease",
  "invalid_request",
  "invalid_response",
  "peer_unavailable",
  "cleanup_failed"
]);

export class AgentNativePortableError extends Error {
  constructor(readonly code: AgentNativePortableErrorCode) {
    super(`Native paired-host access failed (${code})`);
    this.name = "AgentNativePortableError";
  }
}

export interface AgentNativePortableTarget {
  readonly handle: string;
  readonly label: string;
}

export interface AgentNativePortableRefreshResult {
  readonly schemaVersion: 1;
  readonly runtimeId: string;
  readonly capabilities: AgentRemoteCapabilitySnapshot;
  readonly items: readonly AgentNativePortableTarget[];
}

export interface AgentNativePortableWireLease {
  readonly leaseHandle: string;
  readonly targetHandle: string;
  readonly hostEpoch: string;
  readonly connectionGeneration: number;
}

export interface AgentNativePortableReadBinding {
  readonly accountId: string;
  readonly runtimeId: string;
  readonly lease: AgentNativePortableWireLease;
}

/** Exact named native operations; there is deliberately no generic invoke. */
export interface AgentNativePortableBridge {
  refreshTargets(accountId: string): Promise<AgentNativePortableRefreshResult>;
  prepareTarget(
    accountId: string,
    runtimeId: string,
    targetHandle: string
  ): Promise<AgentNativePortableWireLease>;
  getRuntimeStatus(binding: AgentNativePortableReadBinding): Promise<AgentRemoteRuntimeSummary>;
  listSessionsPage(
    binding: AgentNativePortableReadBinding,
    page: AgentRemotePageRequest
  ): Promise<AgentRemoteSessionPage>;
  listRecordsPage(
    binding: AgentNativePortableReadBinding,
    page: AgentRemoteRecordsPageRequest
  ): Promise<AgentRemoteRecordsPage>;
}

export const tauriAgentNativePortableBridge: AgentNativePortableBridge = Object.freeze({
  async refreshTargets(accountId: string) {
    requireAccountId(accountId);
    const value = await invokePortable(AGENT_NATIVE_PORTABLE_COMMANDS.refreshTargets, {
      request: { accountId }
    });
    const decoded = decodeAgentNativePortableRefreshResult(value);
    if (!decoded) throw new AgentNativePortableError("invalid_response");
    return decoded;
  },

  async prepareTarget(accountId: string, runtimeId: string, targetHandle: string) {
    requireAccountId(accountId);
    requireRuntimeId(runtimeId);
    requireTargetHandle(targetHandle);
    const value = await invokePortable(AGENT_NATIVE_PORTABLE_COMMANDS.prepareTarget, {
      request: { accountId, runtimeId, targetHandle }
    });
    const decoded = decodeAgentNativePortableWireLease(value);
    if (!decoded || decoded.targetHandle !== targetHandle) {
      throw new AgentNativePortableError("invalid_response");
    }
    return decoded;
  },

  async getRuntimeStatus(binding: AgentNativePortableReadBinding) {
    const request = nativeReadRequest(binding);
    const value = await invokePortable(AGENT_NATIVE_PORTABLE_COMMANDS.getRuntimeStatus, {
      request
    });
    const decoded = decodeAgentRemoteRuntimeSummary(value);
    if (!decoded) throw new AgentNativePortableError("invalid_response");
    return decoded;
  },

  async listSessionsPage(binding: AgentNativePortableReadBinding, page: AgentRemotePageRequest) {
    const request = nativeReadRequest(binding);
    const normalizedPage = nativePageRequest(page);
    const value = await invokePortable(AGENT_NATIVE_PORTABLE_COMMANDS.listSessionsPage, {
      request: { ...request, page: normalizedPage }
    });
    const decoded = decodeAgentRemoteSessionPage(value, page);
    if (!decoded) throw new AgentNativePortableError("invalid_response");
    return decoded;
  },

  async listRecordsPage(
    binding: AgentNativePortableReadBinding,
    page: AgentRemoteRecordsPageRequest
  ) {
    const request = nativeReadRequest(binding);
    const normalizedPage = nativeRecordsPageRequest(page);
    const value = await invokePortable(AGENT_NATIVE_PORTABLE_COMMANDS.listRecordsPage, {
      request: { ...request, page: normalizedPage }
    });
    const decoded = decodeAgentNativePortableRecordsPage(value, page);
    if (!decoded) throw new AgentNativePortableError("invalid_response");
    return decoded;
  }
});

export function createAgentNativePortableReadOnlySource(
  bridge: AgentNativePortableBridge,
  binding: AgentNativePortableReadBinding,
  assertCurrent: () => void
): AgentRemoteReadOnlySource {
  const captured = freezeReadBinding(binding);
  return Object.freeze({
    getRuntimeStatus: async () => {
      assertCurrent();
      const result = await bridge.getRuntimeStatus(captured);
      assertCurrent();
      return result;
    },
    listSessionSummariesPage: async (page: AgentRemotePageRequest) => {
      assertCurrent();
      const result = await bridge.listSessionsPage(captured, page);
      assertCurrent();
      return result;
    },
    listPersistedRecordsPage: async (page: AgentRemoteRecordsPageRequest) => {
      assertCurrent();
      const result = await bridge.listRecordsPage(captured, page);
      assertCurrent();
      return result;
    }
  });
}

export function decodeAgentNativePortableRefreshResult(
  value: unknown
): AgentNativePortableRefreshResult | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["schemaVersion", "runtimeId", "capabilities", "items"]) ||
    value.schemaVersion !== 1 ||
    !isRuntimeId(value.runtimeId) ||
    !Array.isArray(value.items) ||
    value.items.length > MAX_NATIVE_TARGETS
  ) {
    return null;
  }
  const capabilities = decodeAgentRemoteCapabilitySnapshot(value.capabilities);
  if (!capabilities || !isAgentRemotePersistedTranscriptReady(capabilities)) return null;

  const handles = new Set<string>();
  const items: AgentNativePortableTarget[] = [];
  for (const item of value.items) {
    if (
      !isRecord(item) ||
      !hasOnlyKeys(item, ["handle", "label"]) ||
      !isTargetHandle(item.handle) ||
      !isSafeTargetLabel(item.label) ||
      handles.has(item.handle)
    ) {
      return null;
    }
    handles.add(item.handle);
    items.push(Object.freeze({ handle: item.handle, label: item.label }));
  }
  return Object.freeze({ schemaVersion: 1, runtimeId: value.runtimeId, capabilities, items });
}

export function decodeAgentNativePortableWireLease(
  value: unknown
): AgentNativePortableWireLease | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["leaseHandle", "targetHandle", "hostEpoch", "connectionGeneration"]) ||
    !isLeaseHandle(value.leaseHandle) ||
    !isTargetHandle(value.targetHandle) ||
    !isCanonicalPositiveU64(value.hostEpoch) ||
    !isPositiveSafeInteger(value.connectionGeneration)
  ) {
    return null;
  }
  return Object.freeze({
    leaseHandle: value.leaseHandle,
    targetHandle: value.targetHandle,
    hostEpoch: value.hostEpoch,
    connectionGeneration: value.connectionGeneration
  });
}

export function decodeAgentNativePortableRecordsPage(
  value: unknown,
  request: AgentRemoteRecordsPageRequest
): AgentRemoteRecordsPage | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["items", "historyRevision", "nextCursor"]) ||
    !Array.isArray(value.items)
  ) {
    return null;
  }
  // Rust deliberately names the outer collection `items`. Phase 1 names the
  // same bounded, decoded collection `records`; reconstruct instead of
  // retaining or mutating the provider-owned response.
  return decodeAgentRemoteRecordsPage(
    {
      records: value.items,
      historyRevision: value.historyRevision,
      ...(value.nextCursor === undefined ? {} : { nextCursor: value.nextCursor })
    },
    request
  );
}

export function decodeAgentNativePortableError(value: unknown): AgentNativePortableErrorCode {
  if (
    isRecord(value) &&
    hasOnlyKeys(value, ["code"]) &&
    typeof value.code === "string" &&
    NATIVE_PORTABLE_ERROR_CODES.has(value.code as AgentNativePortableErrorCode)
  ) {
    return value.code as AgentNativePortableErrorCode;
  }
  return "unavailable";
}

function freezeReadBinding(
  binding: AgentNativePortableReadBinding
): AgentNativePortableReadBinding {
  requireAccountId(binding.accountId);
  requireRuntimeId(binding.runtimeId);
  const lease = decodeAgentNativePortableWireLease(binding.lease);
  if (!lease) throw new AgentNativePortableError("invalid_request");
  return Object.freeze({ accountId: binding.accountId, runtimeId: binding.runtimeId, lease });
}

function nativeReadRequest(binding: AgentNativePortableReadBinding) {
  const captured = freezeReadBinding(binding);
  return {
    accountId: captured.accountId,
    runtimeId: captured.runtimeId,
    lease: captured.lease
  };
}

function nativePageRequest(page: AgentRemotePageRequest) {
  if (
    !isRecord(page) ||
    !hasOnlyKeys(page, ["cursor", "limit"]) ||
    !isPositiveSafeInteger(page.limit) ||
    page.limit > 50 ||
    !isNullableSafeToken(page.cursor)
  ) {
    throw new AgentNativePortableError("invalid_request");
  }
  return {
    ...(typeof page.cursor === "string" ? { cursor: page.cursor } : {}),
    limit: page.limit
  };
}

function nativeRecordsPageRequest(page: AgentRemoteRecordsPageRequest) {
  if (
    !isRecord(page) ||
    !hasOnlyKeys(page, ["sessionId", "cursor", "limit"]) ||
    !isSafeToken(page.sessionId, 128)
  ) {
    throw new AgentNativePortableError("invalid_request");
  }
  return {
    sessionId: page.sessionId,
    ...nativePageRequest({
      ...(page.cursor === undefined ? {} : { cursor: page.cursor }),
      limit: page.limit
    })
  };
}

async function invokePortable(command: string, args: Record<string, unknown>): Promise<unknown> {
  try {
    if (!isTauriMobile()) throw new AgentNativePortableError("unavailable");
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new AgentNativePortableError(decodeAgentNativePortableError(error));
  }
}

function requireAccountId(accountId: string): void {
  if (!isAgentNativePortableAccountId(accountId)) {
    throw new AgentNativePortableError("invalid_request");
  }
}

export function isAgentNativePortableAccountId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length === 36 &&
    value !== NIL_ACCOUNT_ID &&
    ACCOUNT_ID_PATTERN.test(value)
  );
}

function requireRuntimeId(runtimeId: string): void {
  if (!isRuntimeId(runtimeId)) throw new AgentNativePortableError("invalid_request");
}

function requireTargetHandle(handle: string): void {
  if (!isTargetHandle(handle)) throw new AgentNativePortableError("invalid_request");
}

function isRuntimeId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= MAX_RUNTIME_ID_BYTES &&
    RUNTIME_ID_PATTERN.test(value)
  );
}

function isTargetHandle(value: unknown): value is string {
  return typeof value === "string" && TARGET_HANDLE_PATTERN.test(value);
}

function isLeaseHandle(value: unknown): value is string {
  return typeof value === "string" && LEASE_HANDLE_PATTERN.test(value);
}

function isCanonicalPositiveU64(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    value.length < 1 ||
    value.length > MAX_U64_DECIMAL_DIGITS ||
    !/^[1-9][0-9]*$/.test(value)
  ) {
    return false;
  }
  return BigInt(value) <= MAX_U64;
}

function isPositiveSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function isSafeTargetLabel(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    value.length < 1 ||
    hasRustWhitespaceEdge(value) ||
    [...value].length > MAX_TARGET_LABEL_CHARACTERS ||
    new TextEncoder().encode(value).byteLength > MAX_TARGET_LABEL_BYTES
  ) {
    return false;
  }
  for (const character of value) {
    const code = character.codePointAt(0)!;
    if (
      code <= 0x1f ||
      (code >= 0x7f && code <= 0x9f) ||
      code === 0x061c ||
      code === 0x200e ||
      code === 0x200f ||
      (code >= 0xd800 && code <= 0xdfff) ||
      (code >= 0x202a && code <= 0x202e) ||
      (code >= 0x2066 && code <= 0x2069)
    ) {
      return false;
    }
  }
  return true;
}

function hasRustWhitespaceEdge(value: string): boolean {
  return (
    isRustWhitespaceCodeUnit(value.charCodeAt(0)) ||
    isRustWhitespaceCodeUnit(value.charCodeAt(value.length - 1))
  );
}

// Rust str::trim follows Unicode White_Space. Keep this explicit because
// ECMAScript trim also removes U+FEFF, which native deliberately admits.
function isRustWhitespaceCodeUnit(code: number): boolean {
  return (
    (code >= 0x0009 && code <= 0x000d) ||
    code === 0x0020 ||
    code === 0x0085 ||
    code === 0x00a0 ||
    code === 0x1680 ||
    (code >= 0x2000 && code <= 0x200a) ||
    code === 0x2028 ||
    code === 0x2029 ||
    code === 0x202f ||
    code === 0x205f ||
    code === 0x3000
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, allowedKeys: readonly string[]): boolean {
  const allowed = new Set(allowedKeys);
  return Reflect.ownKeys(value).every((key) => typeof key === "string" && allowed.has(key));
}

function isSafeToken(value: unknown, maxBytes: number): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= maxBytes &&
    /^[A-Za-z0-9._:-]+$/.test(value)
  );
}

function isNullableSafeToken(value: unknown): boolean {
  return value === undefined || value === null || isSafeToken(value, 512);
}
