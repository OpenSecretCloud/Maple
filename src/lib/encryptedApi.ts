import { encryptMessage, decryptMessage } from "./encryption";
import { getAttestation } from "./getAttestation";
import { getApiPcrConfig, getApiUrl, refreshToken } from "./api";
import { getPlatformApiUrl, getPlatformPcrConfig, platformRefreshToken } from "./platformApi";
import { apiConfig } from "./apiConfig";

interface EncryptedResponse {
  encrypted: string;
}

interface ApiResponse<T> {
  status: number;
  data?: T;
  error?: string;
}

export async function authenticatedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage?: string
): Promise<U> {
  const tryAuthenticatedRequest = async (forceRefresh: boolean = false): Promise<U> => {
    try {
      if (forceRefresh) {
        console.log("Refreshing access token");

        // Use the apiConfig to determine which refresh function to use
        const refreshFn = apiConfig.getRefreshFunction(url);
        console.log(`Using ${refreshFn}`);

        if (refreshFn === "platformRefreshToken") {
          await platformRefreshToken();
        } else {
          await refreshToken();
        }
      }

      // Always get the latest token from localStorage
      const accessToken = window.localStorage.getItem("access_token");
      if (!accessToken) {
        throw new Error("No access token available");
      }

      const response = await internalEncryptedApiCall<T, U>(
        url,
        method,
        data,
        accessToken,
        errorMessage
      );

      // Attempt to refresh token once if we get a 401
      if (response.status === 401 && !forceRefresh) {
        console.log(`Received 401 for URL ${url}, attempting to refresh token`);
        return tryAuthenticatedRequest(true);
      }

      // Throw an error if the response contains an error message
      if (response.error) {
        throw new Error(response.error);
      }

      // Throw an error if no data was received
      if (!response.data) {
        throw new Error("No data received from the server");
      }

      return response.data;
    } catch (error) {
      console.error(error);
      throw error;
    }
  };

  return tryAuthenticatedRequest();
}

// Special version for OpenAI endpoints that supports API keys
export async function openAiAuthenticatedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  errorMessage?: string,
  apiKey?: string
): Promise<U> {
  // If no API key provided, use regular authenticated call
  if (!apiKey) {
    return authenticatedApiCall<T, U>(url, method, data, errorMessage);
  }

  // For API key auth, call internal encrypted API directly (no refresh logic)
  const response = await internalEncryptedApiCall<T, U>(url, method, data, apiKey, errorMessage);

  if (response.error) {
    throw new Error(response.error);
  }

  if (!response.data) {
    throw new Error(errorMessage || `Request to ${url} failed`);
  }

  return response.data;
}

// This internal version can return a specific
async function internalEncryptedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  accessToken?: string,
  errorMessage?: string
): Promise<ApiResponse<U>> {
  // Use apiConfig to determine the context and get the appropriate API URL
  const endpoint = apiConfig.resolveEndpoint(url);
  const explicitApiUrl = endpoint.context === "platform" ? getPlatformApiUrl() : getApiUrl();
  const pcrConfig = endpoint.context === "platform" ? getPlatformPcrConfig() : getApiPcrConfig();

  let { sessionKey, sessionId } = await getAttestation(false, explicitApiUrl, pcrConfig);

  const makeRequest = async (token: string | undefined, forceNewAttestation: boolean = false) => {
    if (forceNewAttestation || !sessionKey || !sessionId) {
      const newAttestation = await getAttestation(true, explicitApiUrl, pcrConfig);
      sessionKey = newAttestation.sessionKey;
      sessionId = newAttestation.sessionId;
    }

    if (!sessionKey || !sessionId) {
      throw new Error("Failed to make encrypted API call, no attestation available.");
    }

    const jsonData = data ? JSON.stringify(data) : undefined;
    const encryptedData = jsonData ? encryptMessage(sessionKey, jsonData) : undefined;

    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      "x-session-id": sessionId
    };

    // Only add Authorization header if a token is provided
    if (token) {
      headers["Authorization"] = `Bearer ${token}`;
    }

    const response = await fetch(url, {
      method,
      headers,
      body: encryptedData ? JSON.stringify({ encrypted: encryptedData }) : undefined
    });

    const result: ApiResponse<U> = {
      status: response.status
    };

    if (!response.ok) {
      try {
        const errorBody = await response.json();
        result.error =
          errorBody.message || errorMessage || `HTTP error! Status: ${response.status}`;
      } catch {
        result.error = errorMessage || `HTTP error! Status: ${response.status}`;
      }
    } else {
      try {
        const encryptedResponse: EncryptedResponse = await response.json();
        const decryptedResponse = decryptMessage(sessionKey, encryptedResponse.encrypted);
        result.data = JSON.parse(decryptedResponse);
      } catch (error) {
        console.error("Error decrypting or parsing response:", error);
        result.status = 500;
        result.error = "Failed to decrypt or parse the response";
      }
    }

    return result;
  };

  const tryEncryptedRequest = async (
    token: string | undefined,
    forceNewAttestation: boolean = false
  ): Promise<ApiResponse<U>> => {
    try {
      const response = await makeRequest(token, forceNewAttestation);

      // Retry with new attestation if we get a 400 or encryption error, but only once
      if (response.status === 400 || response.error?.includes("Encryption error")) {
        if (!forceNewAttestation) {
          console.log("Encryption error or Bad Request, attempting to renew attestation");
          return tryEncryptedRequest(token, true);
        }
      }

      return response;
    } catch (error) {
      return {
        status: 500,
        error: error instanceof Error ? error.message : "Unknown error occurred"
      };
    }
  };

  return tryEncryptedRequest(accessToken);
}

export async function encryptedApiCall<T, U>(
  url: string,
  method: string,
  data: T,
  accessToken?: string,
  errorMessage?: string
): Promise<U> {
  const response = await internalEncryptedApiCall<T, U>(
    url,
    method,
    data,
    accessToken,
    errorMessage
  );

  // Throw an error if the response contains an error message
  if (response.error) {
    throw new Error(response.error);
  }

  // Throw an error if no data was received
  if (!response.data) {
    throw new Error("No data received from the server");
  }

  return response.data;
}
