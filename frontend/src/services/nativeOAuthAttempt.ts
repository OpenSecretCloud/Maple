import { invoke } from "@tauri-apps/api/core";

type InvokeCommand = (command: string, args?: Parameters<typeof invoke>[1]) => Promise<unknown>;

const PENDING_NATIVE_OAUTH_ATTEMPT_KEY = "maple_pending_native_oauth_attempt_v2";
export const PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS = 15 * 60 * 1000;

export interface NativeOAuthBeginResponse {
  nativeOAuthAttempt: string;
  sessionId: string;
}

export interface NativeOAuthRedeemResponse {
  userId: string;
  authBundle: string;
}

interface PendingNativeOAuthAttempt {
  attemptId: string;
  sessionId: string;
  startedAt: number;
}

export type NativeOAuthCallbackAuthorization =
  | "accepted"
  | "already_authenticated"
  | "missing_or_expired_attempt";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";

function isNonNilUuid(value: unknown): value is string {
  return typeof value === "string" && value !== NIL_UUID && UUID_PATTERN.test(value);
}

export function isNativeOAuthAttemptId(value: unknown): value is string {
  return isNonNilUuid(value);
}

export function isNativeOAuthSessionId(value: unknown): value is string {
  return isNonNilUuid(value);
}

function readPendingNativeOAuthAttempt(): PendingNativeOAuthAttempt | null {
  let encoded: string | null;
  try {
    encoded = localStorage.getItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY);
  } catch {
    return null;
  }
  if (!encoded) return null;

  try {
    const attempt = JSON.parse(encoded) as Partial<PendingNativeOAuthAttempt>;
    if (
      !isNativeOAuthAttemptId(attempt.attemptId) ||
      !isNativeOAuthSessionId(attempt.sessionId) ||
      typeof attempt.startedAt !== "number" ||
      !Number.isSafeInteger(attempt.startedAt) ||
      attempt.startedAt < 0
    ) {
      throw new Error("Invalid pending native OAuth attempt");
    }
    return attempt as PendingNativeOAuthAttempt;
  } catch {
    try {
      localStorage.removeItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY);
    } catch {
      // A later authorization check will continue to fail closed.
    }
    return null;
  }
}

function removePendingNativeOAuthAttempt(attemptId?: string): boolean {
  try {
    if (attemptId && readPendingNativeOAuthAttempt()?.attemptId !== attemptId) return false;
    localStorage.removeItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY);
    return localStorage.getItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY) === null;
  } catch {
    return false;
  }
}

export async function beginNativeOAuthAttempt(
  apiUrl: string,
  now = Date.now(),
  invokeCommand: InvokeCommand = invoke
): Promise<NativeOAuthBeginResponse> {
  if (!Number.isSafeInteger(now) || now < 0) {
    throw new Error("Cannot start native OAuth with an invalid timestamp");
  }
  const response = (await invokeCommand("native_oauth_begin", {
    request: { apiUrl }
  })) as NativeOAuthBeginResponse;
  if (
    !isNativeOAuthAttemptId(response.nativeOAuthAttempt) ||
    !isNativeOAuthSessionId(response.sessionId)
  ) {
    throw new Error("Native OAuth initiation returned invalid state");
  }
  try {
    localStorage.setItem(
      PENDING_NATIVE_OAUTH_ATTEMPT_KEY,
      JSON.stringify({
        attemptId: response.nativeOAuthAttempt,
        sessionId: response.sessionId,
        startedAt: now
      } satisfies PendingNativeOAuthAttempt)
    );
  } catch (error) {
    await invokeCommand("native_oauth_cancel", {
      request: { nativeOAuthAttempt: response.nativeOAuthAttempt }
    }).catch(() => undefined);
    throw error;
  }
  return response;
}

export async function cancelNativeOAuthAttempt(
  attemptId: string,
  invokeCommand: InvokeCommand = invoke
): Promise<void> {
  try {
    await invokeCommand("native_oauth_cancel", { request: { nativeOAuthAttempt: attemptId } });
  } finally {
    removePendingNativeOAuthAttempt(attemptId);
  }
}

export async function redeemNativeOAuthGrant(
  handoffGrant: string,
  nativeSessionId: string,
  invokeCommand: InvokeCommand = invoke
): Promise<NativeOAuthRedeemResponse> {
  return invokeCommand("native_oauth_redeem", {
    request: { handoffGrant, nativeSessionId }
  }) as Promise<NativeOAuthRedeemResponse>;
}

export function authorizeNativeOAuthCallback(
  isAuthenticated: boolean,
  now = Date.now()
): NativeOAuthCallbackAuthorization {
  if (isAuthenticated) {
    removePendingNativeOAuthAttempt();
    return "already_authenticated";
  }
  const attempt = readPendingNativeOAuthAttempt();
  if (!attempt) return "missing_or_expired_attempt";

  const age = now - attempt.startedAt;
  if (!Number.isSafeInteger(now) || age < 0 || age > PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS) {
    removePendingNativeOAuthAttempt(attempt.attemptId);
    return "missing_or_expired_attempt";
  }
  return "accepted";
}

export function readPendingNativeOAuthAttemptId(): string | null {
  return readPendingNativeOAuthAttempt()?.attemptId ?? null;
}

export function consumeNativeOAuthAttempt(attemptId: string): boolean {
  return removePendingNativeOAuthAttempt(attemptId);
}
