export const CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE = "chat_account_credential_mismatch";
const REQUEST_NOT_DISPATCHED_CODE = "opensecret_request_not_dispatched";

type AccountBoundFetch = (input: string | URL | Request, init?: RequestInit) => Promise<Response>;

function decodeBase64Url(value: string): string {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  return atob(padded);
}

export function chatAccessTokenSubject(accessToken: string | null): string | null {
  if (!accessToken) return null;
  try {
    const parts = accessToken.split(".");
    if (parts.length !== 3 || !parts[1]) return null;
    const payload = JSON.parse(decodeBase64Url(parts[1])) as { sub?: unknown };
    return typeof payload.sub === "string" && payload.sub ? payload.sub : null;
  } catch {
    return null;
  }
}

export class ChatAccountCredentialMismatchError extends Error {
  readonly code = CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE;

  constructor() {
    super("The authenticated Chat account changed before this request could start");
    this.name = "ChatAccountCredentialMismatchError";
  }
}

/**
 * Fails a user-scoped operation closed when another tab has replaced the
 * browser credentials since this React tree was rendered.
 */
export function assertChatAccountCredential(
  expectedUserId: string | undefined,
  getAccessToken: () => string | null = () => window.localStorage.getItem("access_token")
): void {
  if (!expectedUserId || chatAccessTokenSubject(getAccessToken()) !== expectedUserId) {
    throw new ChatAccountCredentialMismatchError();
  }
}

/**
 * Binds every Chat network request to the account that created its OpenAI
 * client. Access-token refreshes remain valid because the stable JWT subject,
 * rather than the token bytes, is compared. Cross-tab account replacement
 * fails before plaintext is handed to the encrypted transport.
 */
export function createAccountBoundChatFetch({
  expectedUserId,
  getAccessToken,
  fetch
}: {
  expectedUserId: string | undefined;
  getAccessToken: () => string | null;
  fetch: AccountBoundFetch;
}): AccountBoundFetch {
  return (input, init) => {
    try {
      assertChatAccountCredential(expectedUserId, getAccessToken);
    } catch (error) {
      if (typeof error === "object" && error !== null) {
        Object.assign(error, {
          requestDispatchCode: REQUEST_NOT_DISPATCHED_CODE,
          definitelyNotDispatched: true
        });
      }
      return Promise.reject(error);
    }
    return fetch(input, init);
  };
}

export function isChatAccountCredentialMismatchError(error: unknown): boolean {
  let current = error;
  for (let depth = 0; depth < 3 && current && typeof current === "object"; depth += 1) {
    if (
      current instanceof ChatAccountCredentialMismatchError ||
      ("code" in current && current.code === CHAT_ACCOUNT_CREDENTIAL_MISMATCH_CODE)
    ) {
      return true;
    }
    current = "cause" in current ? current.cause : undefined;
  }
  return false;
}
