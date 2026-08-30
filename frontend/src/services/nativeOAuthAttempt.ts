import { v4 as uuidv4 } from "uuid";

const PENDING_NATIVE_OAUTH_ATTEMPT_KEY = "maple_pending_native_oauth_attempt_v1";
export const PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS = 15 * 60 * 1000;

// This is native-to-hosted-browser handoff state, distinct from provider OAuth
// state and not a credential. Keep it out of application logs nonetheless.
interface PendingNativeOAuthAttempt {
  attemptId: string;
  startedAt: number;
}

export type NativeOAuthCallbackAuthorization =
  | "accepted"
  | "already_authenticated"
  | "attempt_mismatch"
  | "missing_or_expired_attempt";

const NATIVE_OAUTH_ATTEMPT_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function isNativeOAuthAttemptId(value: unknown): value is string {
  return typeof value === "string" && NATIVE_OAUTH_ATTEMPT_ID_PATTERN.test(value);
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
    if (attemptId) {
      const currentAttempt = readPendingNativeOAuthAttempt();
      if (currentAttempt?.attemptId !== attemptId) return false;
    }
    localStorage.removeItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY);
    return localStorage.getItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY) === null;
  } catch {
    return false;
  }
}

export function beginNativeOAuthAttempt(now = Date.now()): string {
  if (!Number.isSafeInteger(now) || now < 0) {
    throw new Error("Cannot start native OAuth with an invalid timestamp");
  }

  const attemptId = uuidv4();
  const attempt: PendingNativeOAuthAttempt = { attemptId, startedAt: now };
  localStorage.setItem(PENDING_NATIVE_OAUTH_ATTEMPT_KEY, JSON.stringify(attempt));
  return attemptId;
}

export function cancelNativeOAuthAttempt(attemptId: string): void {
  removePendingNativeOAuthAttempt(attemptId);
}

export function authorizeNativeOAuthCallback(
  isAuthenticated: boolean,
  callbackAttemptId: string,
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

  // Native handoff state values are compared exactly. A mismatch rejects this callback
  // without consuming the genuine pending attempt, so an unrelated deep link
  // cannot turn account confusion into a denial of the user's real callback.
  if (!isNativeOAuthAttemptId(callbackAttemptId) || callbackAttemptId !== attempt.attemptId) {
    return "attempt_mismatch";
  }

  if (!removePendingNativeOAuthAttempt(attempt.attemptId)) {
    return "missing_or_expired_attempt";
  }

  return "accepted";
}
