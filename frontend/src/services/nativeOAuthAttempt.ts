import { v4 as uuidv4 } from "uuid";

const PENDING_NATIVE_OAUTH_ATTEMPT_KEY = "maple_pending_native_oauth_attempt_v1";
export const PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS = 15 * 60 * 1000;

// This is a local freshness marker, not provider OAuth state and not a secret.
interface PendingNativeOAuthAttempt {
  attemptId: string;
  startedAt: number;
}

export type NativeOAuthCallbackAuthorization =
  | "accepted"
  | "already_authenticated"
  | "missing_or_expired_attempt";

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
      typeof attempt.attemptId !== "string" ||
      !attempt.attemptId ||
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

  if (!removePendingNativeOAuthAttempt(attempt.attemptId)) {
    return "missing_or_expired_attempt";
  }

  return "accepted";
}
