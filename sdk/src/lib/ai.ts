import { decryptMessage, encryptMessage } from "./encryption";
import { getAttestation, type Attestation } from "./getAttestation";
import * as api from "./api";
import { serializePcrConfig, snapshotPcrConfig, type PcrConfig } from "./pcr";
import { classifyRecovery } from "./recovery";

export interface CustomFetchOptions {
  /** Optional API key to use instead of a JWT token. */
  apiKey?: string;
  /** API URL used for attestation; required outside OpenSecretProvider. */
  apiUrl?: string;
  /** PCR0 trust policy enforced before non-loopback session key exchange; defaults to production. */
  pcrConfig?: PcrConfig;
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
    const getAuthHeader = () => {
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

        return {
          attestation,
          response: await dependencies.fetch(request.url, requestOptions)
        };
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
        const recovery = classifyRecovery(attempt.response.status, attempt.response.headers);

        if (recovery === "refresh_access_token" && !usesApiKey && !replayed) {
          replayed = true;
          await discardResponse(attempt.response);
          throwIfAborted(request.signal);
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
        const errorText = await response.text();
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
            while (true) {
              const { done, value } = await reader!.read();
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
                    controller.enqueue(line + "\n");
                  }
                  // Handle data: lines - decrypt them
                  else if (line.trim().startsWith("data: ")) {
                    const data = line.slice(6).trim();
                    if (data === "[DONE]") {
                      controller.enqueue(`data: [DONE]\n\n`);
                    } else {
                      try {
                        const decrypted = dependencies.decryptMessage(sessionKey, data);

                        // Always enqueue the decrypted data
                        // Note: We don't add \n\n here because the empty line will be added separately
                        controller.enqueue(`data: ${decrypted}\n`);
                      } catch (error) {
                        console.error("Decryption error:", error, "Data:", data);
                        // Instead of sending the encrypted data, we'll skip this chunk
                        console.log("Skipping corrupted chunk");
                      }
                    }
                  }
                  // Pass through empty lines
                  else if (line === "") {
                    controller.enqueue("\n");
                  }
                }
              }
            }
            controller.close();
          }
        });

        return new Response(stream, {
          headers: response.headers,
          status: response.status,
          statusText: response.statusText
        });
      }

      // Decrypt regular JSON responses
      const responseText = await response.text();
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

              return new Response(bytes, {
                headers: headersOut,
                status: response.status,
                statusText: response.statusText
              });
            }
          } catch {
            // Not JSON, continue with regular text response
          }
          // Return a new Response with the decrypted data
          return new Response(decrypted, {
            headers: response.headers,
            status: response.status,
            statusText: response.statusText
          });
        }
      } catch {
        // If it's not JSON or doesn't have encrypted field, return original response
        console.log("Response is not encrypted JSON, returning as-is");
      }

      // Return the original response text as a new Response
      return new Response(responseText, {
        headers: response.headers,
        status: response.status,
        statusText: response.statusText
      });
    } catch (error) {
      console.error("Error during fetch process:", error);
      throw error;
    }
  };
}

function extractEvent(buffer: string): string | null {
  const eventEnd = buffer.indexOf("\n\n");
  if (eventEnd === -1) return null;
  return buffer.slice(0, eventEnd + 2);
}
