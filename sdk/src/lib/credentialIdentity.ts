export const ACCOUNT_CREDENTIAL_MISMATCH_CODE = "chat_account_credential_mismatch";

export function accessTokenSubject(accessToken: string | null): string | null {
  if (!accessToken) return null;
  try {
    const parts = accessToken.split(".");
    if (parts.length !== 3 || !parts[1]) return null;
    const normalized = parts[1].replace(/-/g, "+").replace(/_/g, "/");
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
    const payload = JSON.parse(atob(padded)) as { sub?: unknown };
    return typeof payload.sub === "string" && payload.sub ? payload.sub : null;
  } catch {
    return null;
  }
}

export function accountCredentialMismatchError(): Error & { code: string } {
  return Object.assign(
    new Error("The authenticated account changed before this request could continue"),
    {
      name: "AccountCredentialMismatchError",
      code: ACCOUNT_CREDENTIAL_MISMATCH_CODE
    }
  );
}

export function isAccountCredentialMismatchError(
  error: unknown
): error is Error & { code: string } {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === ACCOUNT_CREDENTIAL_MISMATCH_CODE
  );
}

export type UserCredentialSnapshot = Readonly<{
  userId: string;
  accessToken: string;
  refreshToken: string | null;
}>;

export function assertExpectedAccessTokenSubject(
  expectedUserId: string,
  storage: Storage = window.localStorage
): void {
  if (accessTokenSubject(storage.getItem("access_token")) !== expectedUserId) {
    throw accountCredentialMismatchError();
  }
}

export function captureExpectedUserCredentials(
  expectedUserId: string,
  storage: Storage = window.localStorage
): UserCredentialSnapshot {
  const accessToken = storage.getItem("access_token");
  const refreshToken = storage.getItem("refresh_token");

  if (
    !accessToken ||
    accessTokenSubject(accessToken) !== expectedUserId ||
    storage.getItem("access_token") !== accessToken ||
    storage.getItem("refresh_token") !== refreshToken
  ) {
    throw accountCredentialMismatchError();
  }

  return {
    userId: expectedUserId,
    accessToken,
    refreshToken
  };
}

export function clearCapturedUserCredentials(
  snapshot: UserCredentialSnapshot,
  storage: Storage = window.localStorage
): void {
  if (
    accessTokenSubject(storage.getItem("access_token")) !== snapshot.userId ||
    storage.getItem("access_token") !== snapshot.accessToken ||
    storage.getItem("refresh_token") !== snapshot.refreshToken
  ) {
    throw accountCredentialMismatchError();
  }

  // Recheck each exact value immediately before removal. This prevents a logout
  // that crossed an asynchronous account transition from clearing the new
  // account's credentials.
  if (storage.getItem("access_token") !== snapshot.accessToken) {
    throw accountCredentialMismatchError();
  }
  storage.removeItem("access_token");

  if (storage.getItem("refresh_token") !== snapshot.refreshToken) {
    throw accountCredentialMismatchError();
  }
  storage.removeItem("refresh_token");
}

export async function revokeAndClearUserCredentials({
  expectedUserId,
  revokeRefreshToken,
  storage = window.localStorage
}: {
  expectedUserId?: string;
  revokeRefreshToken: (refreshToken: string) => Promise<void>;
  storage?: Storage;
}): Promise<void> {
  const snapshot = expectedUserId
    ? captureExpectedUserCredentials(expectedUserId, storage)
    : undefined;
  const refreshToken = snapshot ? snapshot.refreshToken : storage.getItem("refresh_token");

  if (refreshToken) {
    await revokeRefreshToken(refreshToken);
  }

  if (snapshot) {
    clearCapturedUserCredentials(snapshot, storage);
    return;
  }

  // An unauthenticated provider has no user identity to bind. Preserve its
  // existing best-effort credential cleanup behavior.
  storage.removeItem("access_token");
  storage.removeItem("refresh_token");
}

export function commitRefreshedUserTokensIfCurrent({
  initiatingRefreshToken,
  accessToken,
  refreshToken,
  storage = window.localStorage
}: {
  initiatingRefreshToken: string;
  accessToken: string;
  refreshToken: string;
  storage?: Storage;
}): void {
  if (storage.getItem("refresh_token") !== initiatingRefreshToken) {
    throw accountCredentialMismatchError();
  }
  storage.setItem("access_token", accessToken);
  storage.setItem("refresh_token", refreshToken);
}
