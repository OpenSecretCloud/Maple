import { encryptMessage, decryptMessage } from "./encryption";
import { getAttestation, type Attestation } from "./getAttestation";
import { getApiPcrConfig, getApiUrl, refreshToken } from "./api";
import { getPlatformApiUrl, getPlatformPcrConfig, platformRefreshToken } from "./platformApi";
import { apiConfig } from "./apiConfig";
import { serializePcrConfig, snapshotPcrConfig, type PcrConfig } from "./pcr";
import { classifyRecovery } from "./recovery";

interface EncryptedResponse {
  encrypted: string;
}

interface ApiResponse<T> {
  status: number;
  data?: T;
  error?: string;
}

interface RequestAuthentication {
  token?: string;
  refreshAccessToken?: () => Promise<string>;
}

interface ActiveAttestation {
  sessionKey: Uint8Array;
  sessionId: string;
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export interface EncryptedApiDependencies {
  decryptMessage: typeof decryptMessage;
  encryptMessage: typeof encryptMessage;
  fetch: typeof globalThis.fetch;
  getAttestation: typeof getAttestation;
  getApiPcrConfig: typeof getApiPcrConfig;
  getApiUrl: typeof getApiUrl;
  getPlatformApiUrl: typeof getPlatformApiUrl;
  getPlatformPcrConfig: typeof getPlatformPcrConfig;
  getAccessToken: () => string | null;
  refreshAccessToken: (url: string) => Promise<void>;
  resolveEndpoint: typeof apiConfig.resolveEndpoint;
}

const defaultDependencies: EncryptedApiDependencies = {
  decryptMessage,
  encryptMessage,
  fetch: (...args) => globalThis.fetch(...args),
  getAttestation,
  // Keep circular api.ts/platformApi.ts imports lazy until after module setup.
  getApiPcrConfig: () => getApiPcrConfig(),
  getApiUrl: () => getApiUrl(),
  getPlatformApiUrl: () => getPlatformApiUrl(),
  getPlatformPcrConfig: () => getPlatformPcrConfig(),
  getAccessToken: () => window.localStorage.getItem("access_token"),
  refreshAccessToken: async (url) => {
    console.log("Refreshing access token");
    const refreshFn = apiConfig.getRefreshFunction(url);
    console.log(`Using ${refreshFn}`);
    if (refreshFn === "platformRefreshToken") {
      await platformRefreshToken();
    } else {
      await refreshToken();
    }
  },
  resolveEndpoint: (url) => apiConfig.resolveEndpoint(url)
};

const attestationRenewals = new WeakMap<
  EncryptedApiDependencies["getAttestation"],
  Map<string, Promise<ActiveAttestation>>
>();

function requireActiveAttestation(attestation: Attestation): ActiveAttestation {
  if (!attestation.sessionKey || !attestation.sessionId) {
    throw new Error("Failed to make encrypted API call, no attestation available.");
  }
  return {
    sessionKey: attestation.sessionKey,
    sessionId: attestation.sessionId
  };
}

async function renewAttestation(
  failedSessionId: string,
  apiUrl: string,
  pcrConfig: PcrConfig,
  dependencies: EncryptedApiDependencies
): Promise<ActiveAttestation> {
  let renewals = attestationRenewals.get(dependencies.getAttestation);
  if (!renewals) {
    renewals = new Map();
    attestationRenewals.set(dependencies.getAttestation, renewals);
  }

  const scope = `${apiUrl}\n${serializePcrConfig(pcrConfig)}\n${failedSessionId}`;
  let renewal = renewals.get(scope);
  if (!renewal) {
    let resolveRenewal!: (attestation: ActiveAttestation) => void;
    let rejectRenewal!: (reason?: unknown) => void;
    renewal = new Promise<ActiveAttestation>((resolve, reject) => {
      resolveRenewal = resolve;
      rejectRenewal = reject;
    });
    renewals.set(scope, renewal);

    const registeredRenewal = renewal;
    const registeredRenewals = renewals;
    void (async () => {
      try {
        // Keep the cache comparison inside the already-registered leader. A
        // staggered stale response must join here even while the leader's
        // forced refresh has temporarily removed the cached session.
        const currentAttestation = requireActiveAttestation(
          await dependencies.getAttestation(false, apiUrl, pcrConfig)
        );
        const renewedAttestation =
          currentAttestation.sessionId === failedSessionId
            ? requireActiveAttestation(await dependencies.getAttestation(true, apiUrl, pcrConfig))
            : currentAttestation;
        resolveRenewal(renewedAttestation);
      } catch (error) {
        rejectRenewal(error);
      } finally {
        if (registeredRenewals.get(scope) === registeredRenewal) {
          registeredRenewals.delete(scope);
        }
        if (
          registeredRenewals.size === 0 &&
          attestationRenewals.get(dependencies.getAttestation) === registeredRenewals
        ) {
          attestationRenewals.delete(dependencies.getAttestation);
        }
      }
    })();
  }

  return renewal;
}

async function discardResponse(response: Response): Promise<void> {
  try {
    await response.body?.cancel();
  } catch {
    // A bounded recovery no longer needs this error response. It may already
    // be closed in some fetch implementations.
  }
}

async function performEncryptedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  authentication: RequestAuthentication,
  errorMessage: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<ApiResponse<U>> {
  try {
    // Snapshot every logical request value before the first transport send.
    // Only the token, session ID, and ciphertext may change on recovery.
    const plaintextBody = data ? JSON.stringify(data) : undefined;
    const endpoint = dependencies.resolveEndpoint(url);
    const explicitApiUrl =
      endpoint.context === "platform" ? dependencies.getPlatformApiUrl() : dependencies.getApiUrl();
    const pcrConfig = snapshotPcrConfig(
      endpoint.context === "platform"
        ? dependencies.getPlatformPcrConfig()
        : dependencies.getApiPcrConfig()
    );

    let token = authentication.token;
    let attestation = await dependencies.getAttestation(false, explicitApiUrl, pcrConfig);
    let replayed = false;

    const requireSession = async (forceRefresh: boolean) => {
      if (forceRefresh || !attestation.sessionKey || !attestation.sessionId) {
        attestation = await dependencies.getAttestation(true, explicitApiUrl, pcrConfig);
      }
      if (!attestation.sessionKey || !attestation.sessionId) {
        throw new Error("Failed to make encrypted API call, no attestation available.");
      }
      return {
        sessionKey: attestation.sessionKey,
        sessionId: attestation.sessionId
      };
    };

    while (true) {
      const session = await requireSession(false);
      const encryptedData = plaintextBody
        ? dependencies.encryptMessage(session.sessionKey, plaintextBody)
        : undefined;
      const headers: Record<string, string> = {
        "Content-Type": "application/json",
        "x-session-id": session.sessionId
      };
      if (token) headers.Authorization = `Bearer ${token}`;

      const response = await dependencies.fetch(url, {
        method,
        headers,
        body: encryptedData ? JSON.stringify({ encrypted: encryptedData }) : undefined
      });
      const recovery = classifyRecovery(response.status, response.headers);

      if (!replayed && recovery === "renew_session") {
        replayed = true;
        await discardResponse(response);
        console.log("Session not found, renewing attestation and retrying once");
        attestation = await renewAttestation(
          session.sessionId,
          explicitApiUrl,
          pcrConfig,
          dependencies
        );
        continue;
      }

      if (!replayed && recovery === "refresh_access_token" && authentication.refreshAccessToken) {
        replayed = true;
        await discardResponse(response);
        token = await authentication.refreshAccessToken();
        // The encrypted refresh request can repair a stale session with its
        // own replay budget, so always reload the current session afterward.
        attestation = await dependencies.getAttestation(false, explicitApiUrl, pcrConfig);
        continue;
      }

      const result: ApiResponse<U> = { status: response.status };
      if (!response.ok) {
        try {
          const errorBody = (await response.json()) as { message?: string };
          result.error =
            errorBody.message || errorMessage || `HTTP error! Status: ${response.status}`;
        } catch {
          result.error = errorMessage || `HTTP error! Status: ${response.status}`;
        }
        return result;
      }

      try {
        const encryptedResponse = (await response.json()) as EncryptedResponse;
        const decryptedResponse = dependencies.decryptMessage(
          session.sessionKey,
          encryptedResponse.encrypted
        );
        result.data = JSON.parse(decryptedResponse) as U;
      } catch (error) {
        console.error("Error decrypting or parsing response:", error);
        result.status = 500;
        result.error = "Failed to decrypt or parse the response";
      }
      return result;
    }
  } catch (error) {
    return {
      status: 500,
      error: error instanceof Error ? error.message : "Unknown error occurred"
    };
  }
}

function unwrapApiResponse<U>(response: ApiResponse<U>, missingDataMessage: string): U {
  if (response.error) throw new Error(response.error);
  if (!response.data) throw new Error(missingDataMessage);
  return response.data;
}

export async function authenticatedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage?: string
): Promise<U> {
  return authenticatedApiCallWithDependencies(url, method, data, errorMessage, defaultDependencies);
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export async function authenticatedApiCallWithDependencies<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<U> {
  try {
    const accessToken = dependencies.getAccessToken();
    if (!accessToken) throw new Error("No access token available");

    const response = await performEncryptedApiCall<T, U>(
      url,
      method,
      data,
      {
        token: accessToken,
        refreshAccessToken: async () => {
          await dependencies.refreshAccessToken(url);
          const refreshedToken = dependencies.getAccessToken();
          if (!refreshedToken) throw new Error("No access token available");
          return refreshedToken;
        }
      },
      errorMessage,
      dependencies
    );
    return unwrapApiResponse(response, "No data received from the server");
  } catch (error) {
    console.error(error);
    throw error;
  }
}

// Special version for OpenAI endpoints that supports API keys
export async function openAiAuthenticatedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage?: string,
  apiKey?: string
): Promise<U> {
  return openAiAuthenticatedApiCallWithDependencies(
    url,
    method,
    data,
    errorMessage,
    apiKey,
    defaultDependencies
  );
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export async function openAiAuthenticatedApiCallWithDependencies<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage: string | undefined,
  apiKey: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<U> {
  if (!apiKey) {
    return authenticatedApiCallWithDependencies(url, method, data, errorMessage, dependencies);
  }

  const response = await performEncryptedApiCall<T, U>(
    url,
    method,
    data,
    { token: apiKey },
    errorMessage,
    dependencies
  );
  return unwrapApiResponse(response, errorMessage || `Request to ${url} failed`);
}

export async function encryptedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  accessToken?: string,
  errorMessage?: string
): Promise<U> {
  return encryptedApiCallWithDependencies(
    url,
    method,
    data,
    accessToken,
    errorMessage,
    defaultDependencies
  );
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export async function encryptedApiCallWithDependencies<T, U>(
  url: string,
  method: string,
  data: T,
  accessToken: string | undefined,
  errorMessage: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<U> {
  const response = await performEncryptedApiCall<T, U>(
    url,
    method,
    data,
    { token: accessToken },
    errorMessage,
    dependencies
  );
  return unwrapApiResponse(response, "No data received from the server");
}
