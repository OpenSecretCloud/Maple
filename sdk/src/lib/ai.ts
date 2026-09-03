import { decryptMessage, encryptMessage } from "./encryption";
import { getAttestation, type Attestation } from "./getAttestation";
import * as api from "./api";
import { serializePcrConfig, snapshotPcrConfig, type PcrConfig } from "./pcr";
import { classifyRecovery, ERROR_CODE_HEADER, ERROR_CONTRACT_HEADER } from "./recovery";
import {
  ACCOUNT_CREDENTIAL_MISMATCH_CODE,
  accessTokenSubject,
  accountCredentialMismatchError,
  isAccountCredentialMismatchError
} from "./credentialIdentity";

export { ACCOUNT_CREDENTIAL_MISMATCH_CODE } from "./credentialIdentity";

/** Identifies a failure that occurred before the target fetch was invoked. */
export const REQUEST_NOT_DISPATCHED_CODE = "opensecret_request_not_dispatched";
const ERROR_CONTRACT_VERSION = "1";
const IMAGE_DESCRIPTION_UNAVAILABLE_ERROR_CODE = "image_description_unavailable";
const IMAGE_DESCRIPTION_UNAVAILABLE_STATUS = 503;

/** Orthogonal dispatch metadata that preserves the source error's code and name. */
export interface RequestNotDispatchedMarker {
  readonly requestDispatchCode: typeof REQUEST_NOT_DISPATCHED_CODE;
  readonly definitelyNotDispatched: true;
}

function markRequestNotDispatched(error: unknown): unknown & RequestNotDispatchedMarker {
  const marker: RequestNotDispatchedMarker = {
    requestDispatchCode: REQUEST_NOT_DISPATCHED_CODE,
    definitelyNotDispatched: true
  };

  if ((typeof error === "object" && error !== null) || typeof error === "function") {
    try {
      // Errors and DOMExceptions are normally extensible. Tagging the original
      // preserves credential codes, AbortError names, prototypes, and identity.
      return Object.assign(error, marker);
    } catch {
      // Fall through for frozen or host-provided exception objects.
    }
  }

  const wrapped = Object.assign(
    new Error(error instanceof Error ? error.message : "Request failed before transport dispatch"),
    { cause: error },
    marker
  ) as Error & { code?: unknown } & RequestNotDispatchedMarker;
  if (typeof error === "object" && error !== null) {
    if ("name" in error && typeof error.name === "string") wrapped.name = error.name;
    if ("code" in error) wrapped.code = error.code;
  }
  return wrapped;
}

export interface CustomFetchOptions {
  /** Optional API key to use instead of a JWT token. */
  apiKey?: string;
  /** API URL used for attestation; required outside OpenSecretProvider. */
  apiUrl?: string;
  /** PCR0 trust policy enforced before non-loopback session key exchange; defaults to production. */
  pcrConfig?: PcrConfig;
  /** Optional user ID that every JWT attempt and retry must retain. */
  expectedUserId?: string;
}

interface ActiveAttestation {
  sessionKey: Uint8Array;
  sessionId: string;
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export interface CustomFetchDependencies {
  decryptMessage: typeof decryptMessage;
  encryptMessage: typeof encryptMessage;
  fetch: typeof globalThis.fetch;
  getAttestation: typeof getAttestation;
  refreshToken: typeof api.refreshToken;
}

interface RequestSnapshot {
  url: string;
  headers: Headers;
  options: RequestInit;
  plaintextBody?: string;
  signal?: AbortSignal | null;
}

const defaultDependencies: CustomFetchDependencies = {
  decryptMessage,
  encryptMessage,
  fetch: (...args) => globalThis.fetch(...args),
  getAttestation,
  refreshToken: api.refreshToken
};

function requireActiveAttestation(attestation: Attestation): ActiveAttestation {
  if (!attestation.sessionKey || !attestation.sessionId) {
    throw new Error("No session key or ID available");
  }

  return {
    sessionKey: attestation.sessionKey,
    sessionId: attestation.sessionId
  };
}

async function discardResponse(response: Response): Promise<void> {
  try {
    await response.body?.cancel();
  } catch {
    // The response is being discarded for a bounded retry. Some runtimes may
    // already have closed its body, which needs no further cleanup.
  }
}

function throwIfAborted(signal?: AbortSignal | null): void {
  signal?.throwIfAborted();
}

function allowsRequestBody(method: string): boolean {
  return method !== "GET" && method !== "HEAD";
}

async function snapshotPlaintextBody(
  normalized: Request,
  init?: RequestInit
): Promise<string | undefined> {
  if (!allowsRequestBody(normalized.method)) return undefined;

  const bodyKnownPresent = init?.body != null || normalized.body != null;
  const plaintextBody = await normalized.text();
  return bodyKnownPresent || normalized.bodyUsed || plaintextBody !== ""
    ? plaintextBody
    : undefined;
}

async function snapshotRequest(
  input: string | URL | Request,
  init?: RequestInit
): Promise<RequestSnapshot> {
  const normalized = new Request(input, init);
  const signal =
    init?.signal === null
      ? null
      : (init?.signal ?? (input instanceof Request ? input.signal : undefined));
  const url = normalized.url;
  const headers = new Headers(normalized.headers);
  const options: RequestInit = {
    ...init,
    method: normalized.method,
    cache: init?.cache ?? normalized.cache,
    credentials: init?.credentials ?? normalized.credentials,
    integrity: init?.integrity ?? normalized.integrity,
    keepalive: init?.keepalive ?? normalized.keepalive,
    mode: init?.mode ?? normalized.mode,
    redirect: init?.redirect ?? normalized.redirect,
    referrer: init?.referrer ?? normalized.referrer,
    referrerPolicy: init?.referrerPolicy ?? normalized.referrerPolicy,
    signal
  };
  delete options.body;
  delete options.headers;
  // Firefox does not expose Request.body. Gate on the normalized method first,
  // then use the plaintext and explicit init body as fallback presence signals.
  const plaintextBody = await snapshotPlaintextBody(normalized, init);

  return {
    url,
    headers,
    options,
    plaintextBody,
    signal
  };
}

export function createCustomFetch(
  options?: CustomFetchOptions
): (input: string | URL | Request, init?: RequestInit) => Promise<Response> {
  return createCustomFetchWithDependencies(options, defaultDependencies);
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export function createCustomFetchWithDependencies(
  options: CustomFetchOptions | undefined,
  dependencies: CustomFetchDependencies
): (input: string | URL | Request, init?: RequestInit) => Promise<Response> {
  const attestationRefreshes = new Map<string, Promise<ActiveAttestation>>();

  const resolveAttestationIdentity = () => {
    const apiUrl = options?.apiUrl || api.getApiUrl() || undefined;
    const pcrConfig = snapshotPcrConfig(options?.pcrConfig || api.getApiPcrConfig());
    return {
      apiUrl,
      pcrConfig,
      scope: `${apiUrl || ""}\n${serializePcrConfig(pcrConfig)}`
    };
  };

  const renewAttestation = async (
    failedSessionId: string,
    identity: ReturnType<typeof resolveAttestationIdentity>
  ): Promise<ActiveAttestation> => {
    const renewalScope = `${identity.scope}\n${failedSessionId}`;
    let attestationRefresh = attestationRefreshes.get(renewalScope);
    if (!attestationRefresh) {
      let resolveRefresh!: (attestation: ActiveAttestation) => void;
      let rejectRefresh!: (reason?: unknown) => void;
      attestationRefresh = new Promise<ActiveAttestation>((resolve, reject) => {
        resolveRefresh = resolve;
        rejectRefresh = reject;
      });
      attestationRefreshes.set(renewalScope, attestationRefresh);

      const registeredRefresh = attestationRefresh;
      void (async () => {
        try {
          // A concurrent request or token refresh may already have replaced
          // the failed generation. This lookup belongs inside the registered
          // leader so a late caller cannot miss the in-flight renewal after
          // its forced refresh evicts the cache.
          const currentAttestation = requireActiveAttestation(
            await dependencies.getAttestation(false, identity.apiUrl, identity.pcrConfig)
          );
          const renewedAttestation =
            currentAttestation.sessionId === failedSessionId
              ? requireActiveAttestation(
                  await dependencies.getAttestation(true, identity.apiUrl, identity.pcrConfig)
                )
              : currentAttestation;
          resolveRefresh(renewedAttestation);
        } catch (error) {
          rejectRefresh(error);
        } finally {
          if (attestationRefreshes.get(renewalScope) === registeredRefresh) {
            attestationRefreshes.delete(renewalScope);
          }
        }
      })();
    }

    return attestationRefresh;
  };

  return async (requestUrl: string | URL | Request, init?: RequestInit): Promise<Response> => {
    // Authentication mode is part of the logical request snapshot. A caller
    // may retain and mutate the options object while this request is in
    // flight; recovery must not switch between API-key and JWT credentials.
    const apiKey = options?.apiKey;
    const usesApiKey = Boolean(apiKey);
    const expectedUserId = options?.expectedUserId;
    let requestAcceptanceAmbiguous = false;
    const assertExpectedAccount = () => {
      if (!expectedUserId) return;
      if (accessTokenSubject(window.localStorage.getItem("access_token")) !== expectedUserId) {
        throw accountCredentialMismatchError();
      }
    };
    const getAuthHeader = () => {
      assertExpectedAccount();
      // If an API key is provided, use it instead of JWT token
      if (apiKey) {
        return `Bearer ${apiKey}`;
      }

      // Otherwise, use the standard JWT token
      const currentAccessToken = window.localStorage.getItem("access_token");
      if (!currentAccessToken) {
        throw new Error("No access token or API key available");
      }
      return `Bearer ${currentAccessToken}`;
    };

    try {
      // Capture endpoint and trust policy together so retries cannot cross a
      // provider reconfiguration that happens while this request is in flight.
      const attestationIdentity = resolveAttestationIdentity();
      // Keep this operation bound to the identity that initiated it. An
      // unrelated account change during attestation must not send the
      // already-prepared plaintext request under a different token.
      let authHeader = getAuthHeader();
      const request = await snapshotRequest(requestUrl, init);
      throwIfAborted(request.signal);

      const makeRequest = async (attestation: ActiveAttestation) => {
        // Attestation and request snapshots can yield. Re-check immediately
        // before each network attempt so an account replacement cannot replay
        // retained plaintext under another user's credential.
        assertExpectedAccount();
        const headers = new Headers(request.headers);
        headers.set("Authorization", authHeader);
        headers.set("x-session-id", attestation.sessionId);

        const requestOptions: RequestInit = { ...request.options, headers };

        // Encrypt the original plaintext again for every attempt. Reusing an
        // old request body with a new session ID would make recovery fail.
        if (
          request.plaintextBody !== undefined &&
          allowsRequestBody(request.options.method ?? "GET")
        ) {
          const encryptedBody = dependencies.encryptMessage(
            attestation.sessionKey,
            request.plaintextBody
          );
          requestOptions.body = JSON.stringify({ encrypted: encryptedBody });
          headers.set("Content-Type", "application/json");
        }

        // Encryption can be substantial for image-bearing requests, and another
        // tab can replace browser credentials while this synchronous work runs.
        // Close that preparation window before handing the request to fetch.
        assertExpectedAccount();

        // Flip this immediately before invocation: synchronous throws and
        // rejected fetch promises are both ambiguous because the transport was
        // asked to dispatch. Only earlier failures are safe to replay.
        requestAcceptanceAmbiguous = true;
        const response = await dependencies.fetch(request.url, requestOptions);
        return { attestation, response };
      };

      let attestation = requireActiveAttestation(
        await dependencies.getAttestation(
          false,
          attestationIdentity.apiUrl,
          attestationIdentity.pcrConfig
        )
      );
      throwIfAborted(request.signal);
      let replayed = false;
      let finalAttempt: Awaited<ReturnType<typeof makeRequest>>;

      while (true) {
        const attempt = await makeRequest(attestation);
        assertExpectedAccount();
        const recovery = classifyRecovery(attempt.response.status, attempt.response.headers);

        if (recovery === "refresh_access_token" && !usesApiKey && !replayed) {
          replayed = true;
          // This HTTP response definitively rejected the outer request. Any
          // failure while discarding it, refreshing credentials, or preparing
          // the retry remains safe for the caller to restore. The next fetch
          // invocation makes acceptance ambiguous again.
          requestAcceptanceAmbiguous = false;
          await discardResponse(attempt.response);
          throwIfAborted(request.signal);
          // Do not consume or rotate a replacement account's refresh token.
          assertExpectedAccount();
          console.warn("Unauthorized, refreshing access token");
          await dependencies.refreshToken();
          throwIfAborted(request.signal);
          authHeader = getAuthHeader();

          // The encrypted refresh call may itself have replaced a stale
          // attestation. Always rebuild the outer request from current state.
          attestation = requireActiveAttestation(
            await dependencies.getAttestation(
              false,
              attestationIdentity.apiUrl,
              attestationIdentity.pcrConfig
            )
          );
          continue;
        }

        if (recovery === "renew_session" && !replayed) {
          replayed = true;
          requestAcceptanceAmbiguous = false;
          await discardResponse(attempt.response);
          throwIfAborted(request.signal);
          console.warn("Bad Request, renewing attestation and retrying once");
          attestation = await renewAttestation(attempt.attestation.sessionId, attestationIdentity);
          throwIfAborted(request.signal);
          continue;
        }

        finalAttempt = attempt;
        break;
      }

      const { response } = finalAttempt;
      const { sessionKey } = finalAttempt.attestation;

      if (!response.ok) {
        // OpenSecret's non-timeout 4xx responses and explicit image-description
        // failure reject a Responses turn before persistence. Record that fact
        // before reading a possibly truncated error body so callers never wait
        // for response ownership that cannot exist. The generic contract header
        // only versions the error schema; it is not proof that a 5xx happened
        // before persistence.
        const isImageDescriptionPreAcceptanceError =
          response.status === IMAGE_DESCRIPTION_UNAVAILABLE_STATUS &&
          response.headers.get(ERROR_CONTRACT_HEADER) === ERROR_CONTRACT_VERSION &&
          response.headers.get(ERROR_CODE_HEADER) === IMAGE_DESCRIPTION_UNAVAILABLE_ERROR_CODE;
        if (
          response.status !== 408 &&
          ((response.status >= 400 && response.status < 500) ||
            isImageDescriptionPreAcceptanceError)
        ) {
          requestAcceptanceAmbiguous = false;
        }
        const errorText = await response.text();
        assertExpectedAccount();
        console.error(
          "Request failed with response status:",
          response.status,
          " and message:",
          errorText
        );
        throw Object.assign(
          new Error(`Request failed with status ${response.status}: ${errorText}`),
          {
            status: response.status,
            headers: new Headers(response.headers)
          }
        );
      }

      // Decrypt SSE events
      if (response.headers.get("content-type")?.includes("text/event-stream")) {
        const reader = response.body?.getReader();
        const decoder = new TextDecoder();

        let buffer = "";
        const stream = new ReadableStream({
          async start(controller) {
            try {
              while (true) {
                const { done, value } = await reader!.read();
                assertExpectedAccount();
                if (done) break;

                const chunk = decoder.decode(value);
                buffer += chunk;

                let event;
                while ((event = extractEvent(buffer))) {
                  buffer = buffer.slice(event.length);

                  // Split the event into individual lines
                  const lines = event.split("\n");

                  for (const line of lines) {
                    // Handle event: lines - pass them through as-is
                    if (line.trim().startsWith("event: ")) {
                      assertExpectedAccount();
                      controller.enqueue(line + "\n");
                    }
                    // Handle data: lines - decrypt them
                    else if (line.trim().startsWith("data: ")) {
                      const data = line.slice(6).trim();
                      if (data === "[DONE]") {
                        assertExpectedAccount();
                        controller.enqueue(`data: [DONE]\n\n`);
                      } else {
                        try {
                          const decrypted = dependencies.decryptMessage(sessionKey, data);

                          // Always enqueue the decrypted data
                          // Note: We don't add \n\n here because the empty line will be added separately
                          assertExpectedAccount();
                          controller.enqueue(`data: ${decrypted}\n`);
                        } catch (error) {
                          if (isAccountCredentialMismatchError(error)) throw error;
                          console.error("Decryption error:", error, "Data:", data);
                          // Instead of sending the encrypted data, we'll skip this chunk
                          console.log("Skipping corrupted chunk");
                        }
                      }
                    }
                    // Pass through empty lines
                    else if (line === "") {
                      assertExpectedAccount();
                      controller.enqueue("\n");
                    }
                  }
                }
              }
              assertExpectedAccount();
              controller.close();
            } catch (error) {
              try {
                await reader?.cancel(error);
              } catch {
                // The upstream body may already be closed or errored.
              }
              controller.error(error);
            }
          },
          async cancel(reason) {
            await reader?.cancel(reason);
          }
        });

        assertExpectedAccount();
        return new Response(stream, {
          headers: response.headers,
          status: response.status,
          statusText: response.statusText
        });
      }

      // Decrypt regular JSON responses
      const responseText = await response.text();
      assertExpectedAccount();
      try {
        const responseData = JSON.parse(responseText);

        // Check if the response has an encrypted field
        if (responseData.encrypted) {
          const decrypted = dependencies.decryptMessage(sessionKey, responseData.encrypted);

          // Try to parse as JSON to check for TTS response format
          try {
            const decryptedData = JSON.parse(decrypted);

            // Check if this is a TTS response with content_base64 and content_type
            if (decryptedData.content_base64 && decryptedData.content_type) {
              console.log("TTS response detected with content_type:", decryptedData.content_type);

              // Decode base64 audio data to binary
              let bytes: Uint8Array;
              try {
                const binaryString = atob(decryptedData.content_base64);
                bytes = new Uint8Array(binaryString.length);
                for (let i = 0; i < binaryString.length; i++) {
                  bytes[i] = binaryString.charCodeAt(i);
                }
              } catch (e) {
                console.error("Failed to decode base64 audio data:", e);
                throw new Error("Invalid base64 audio data in TTS response");
              }

              console.log("Decoded audio bytes length:", bytes.length);

              // Return as a binary response with the proper content type
              const headersOut = new Headers(response.headers);
              headersOut.set("content-type", decryptedData.content_type);
              // Remove headers that are no longer valid for the decoded response
              headersOut.delete("content-encoding");
              headersOut.delete("content-length");
              headersOut.delete("transfer-encoding");

              assertExpectedAccount();
              return new Response(bytes, {
                headers: headersOut,
                status: response.status,
                statusText: response.statusText
              });
            }
          } catch (error) {
            if (isAccountCredentialMismatchError(error)) throw error;
            // Not JSON, continue with regular text response
          }
          // Return a new Response with the decrypted data
          assertExpectedAccount();
          return new Response(decrypted, {
            headers: response.headers,
            status: response.status,
            statusText: response.statusText
          });
        }
      } catch (error) {
        if (isAccountCredentialMismatchError(error)) throw error;
        // If it's not JSON or doesn't have encrypted field, return original response
        console.log("Response is not encrypted JSON, returning as-is");
      }

      // Return the original response text as a new Response
      assertExpectedAccount();
      return new Response(responseText, {
        headers: response.headers,
        status: response.status,
        statusText: response.statusText
      });
    } catch (error) {
      // Keep the original error code/name intact and add orthogonal dispatch
      // metadata only when no transport invocation occurred in this logical
      // call. Once fetch is invoked, failure remains deliberately ambiguous.
      const reportedError = !requestAcceptanceAmbiguous ? markRequestNotDispatched(error) : error;
      console.error("Error during fetch process:", reportedError);
      throw reportedError;
    }
  };
}

function extractEvent(buffer: string): string | null {
  const eventEnd = buffer.indexOf("\n\n");
  if (eventEnd === -1) return null;
  return buffer.slice(0, eventEnd + 2);
}
