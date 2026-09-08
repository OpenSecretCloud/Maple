const ERROR_CONTRACT_HEADER = "x-opensecret-error-contract";
const ERROR_CODE_HEADER = "x-opensecret-error-code";
const ERROR_CONTRACT_VERSION = "1";
const IMAGE_DESCRIPTION_UNAVAILABLE_ERROR_CODE = "image_description_unavailable";
const IMAGE_DESCRIPTION_UNAVAILABLE_STATUS = 503;
const MAX_ERROR_CAUSE_DEPTH = 4;

type ErrorResponseMetadata = {
  status?: unknown;
  headers?: unknown;
  cause?: unknown;
};

export function isImageDescriptionUnavailableError(error: unknown): boolean {
  const seen = new Set<object>();
  let current = error;

  for (let depth = 0; depth < MAX_ERROR_CAUSE_DEPTH; depth += 1) {
    if (typeof current !== "object" || current === null || seen.has(current)) return false;
    seen.add(current);

    const metadata = current as ErrorResponseMetadata;
    if (
      metadata.status === IMAGE_DESCRIPTION_UNAVAILABLE_STATUS &&
      metadata.headers instanceof Headers &&
      metadata.headers.get(ERROR_CONTRACT_HEADER) === ERROR_CONTRACT_VERSION &&
      metadata.headers.get(ERROR_CODE_HEADER) === IMAGE_DESCRIPTION_UNAVAILABLE_ERROR_CODE
    ) {
      return true;
    }

    current = metadata.cause;
  }

  return false;
}
