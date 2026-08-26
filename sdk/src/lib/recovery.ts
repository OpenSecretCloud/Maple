export const ERROR_CONTRACT_HEADER = "x-opensecret-error-contract";
export const ERROR_CODE_HEADER = "x-opensecret-error-code";

const ERROR_CONTRACT_VERSION = "1";
const SESSION_NOT_FOUND_ERROR_CODE = "session_not_found";
const ACCESS_TOKEN_EXPIRED_ERROR_CODE = "access_token_expired";

export type RecoveryAction = "renew_session" | "refresh_access_token";

/**
 * Classify the only failures that are safe to replay before response handling.
 *
 * A missing contract marker means an older server, whose broad 400/401
 * behavior remains supported. Once a server advertises a contract, malformed,
 * missing, future, or status-mismatched codes fail closed.
 */
export function classifyRecovery(status: number, headers: Headers): RecoveryAction | undefined {
  const contractVersion = headers.get(ERROR_CONTRACT_HEADER);

  if (contractVersion === null) {
    if (status === 400) return "renew_session";
    if (status === 401) return "refresh_access_token";
    return undefined;
  }

  if (contractVersion !== ERROR_CONTRACT_VERSION) return undefined;

  const errorCode = headers.get(ERROR_CODE_HEADER);
  if (status === 400 && errorCode === SESSION_NOT_FOUND_ERROR_CODE) {
    return "renew_session";
  }
  if (status === 401 && errorCode === ACCESS_TOKEN_EXPIRED_ERROR_CODE) {
    return "refresh_access_token";
  }
  return undefined;
}
