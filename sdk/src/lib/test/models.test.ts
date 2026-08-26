import { afterEach, beforeEach, expect, mock, test } from "bun:test";
import { encryptMessage } from "../encryption";
import { cacheAttestationSessionForTesting } from "../getAttestation";
import type { PcrConfig } from "../pcr";
import { fetchModelCatalog, fetchModels, getApiPcrConfig, getApiUrl, setApiUrl } from "../api";

const apiUrl = "https://models.example.com";
const sessionId = "models-session-id";
const sessionKey = new Uint8Array(32).fill(23);
const verifiedPcr0 =
  "eeddbb58f57c38894d6d5af5e575fbe791c5bf3bbcfb5df8da8cfcf0c2e1da1913108e6a762112444740b88c163d7f4b";
const pcrConfig: PcrConfig = { pcr0Values: [verifiedPcr0], remoteAttestation: false };
const modelsResponse = {
  object: "list" as const,
  data: [
    {
      id: "test-model",
      object: "model" as const,
      created: 0,
      owned_by: "opensecret"
    }
  ]
};

const originalFetch = globalThis.fetch;
const originalApiUrl = getApiUrl();
const originalApiPcrConfig = getApiPcrConfig();

beforeEach(async () => {
  window.localStorage.clear();
  window.sessionStorage.clear();
  setApiUrl(apiUrl, pcrConfig);
  await cacheAttestationSessionForTesting(
    apiUrl,
    pcrConfig,
    { sessionKey, sessionId },
    verifiedPcr0
  );
});

afterEach(() => {
  globalThis.fetch = originalFetch;
  setApiUrl(originalApiUrl, originalApiPcrConfig);
  window.localStorage.clear();
  window.sessionStorage.clear();
});

function encryptedModelsResponse() {
  return new Response(
    JSON.stringify({
      encrypted: encryptMessage(sessionKey, JSON.stringify(modelsResponse))
    }),
    { status: 200, headers: { "Content-Type": "application/json" } }
  );
}

test("fetchModels uses the encrypted session before sign-in", async () => {
  globalThis.fetch = mock(async (input: string | URL | Request, init?: RequestInit) => {
    expect(input.toString()).toBe(`${apiUrl}/v1/models`);
    expect(init?.method).toBe("GET");
    expect(init?.body).toBeUndefined();

    const headers = new Headers(init?.headers);
    expect(headers.get("x-session-id")).toBe(sessionId);
    expect(headers.has("Authorization")).toBe(false);

    return encryptedModelsResponse();
  }) as typeof fetch;

  await expect(fetchModels()).resolves.toEqual(modelsResponse.data);
});

test("fetchModels preserves stored JWT authentication", async () => {
  window.localStorage.setItem("access_token", "models-access-token");

  globalThis.fetch = mock(async (_input: string | URL | Request, init?: RequestInit) => {
    const headers = new Headers(init?.headers);
    expect(headers.get("Authorization")).toBe("Bearer models-access-token");
    expect(headers.get("x-session-id")).toBe(sessionId);
    return encryptedModelsResponse();
  }) as typeof fetch;

  await expect(fetchModels()).resolves.toEqual(modelsResponse.data);
});

test("fetchModels never downgrades a rejected stored JWT to anonymous access", async () => {
  let requestCount = 0;
  window.localStorage.setItem("access_token", "rejected-access-token");

  globalThis.fetch = mock(async (_input: string | URL | Request, init?: RequestInit) => {
    requestCount += 1;
    const headers = new Headers(init?.headers);
    expect(headers.get("Authorization")).toBe("Bearer rejected-access-token");

    return Response.json({ message: "Invalid JWT" }, { status: 401 });
  }) as typeof fetch;

  await expect(fetchModels()).rejects.toThrow("No refresh token available");
  expect(requestCount).toBe(1);
});

test("fetchModels never downgrades a rejected API key to anonymous access", async () => {
  let requestCount = 0;
  window.localStorage.setItem("access_token", "stored-jwt");

  globalThis.fetch = mock(async (_input: string | URL | Request, init?: RequestInit) => {
    requestCount += 1;
    const headers = new Headers(init?.headers);
    expect(headers.get("Authorization")).toBe("Bearer invalid-api-key");

    return Response.json({ message: "Invalid API key" }, { status: 401 });
  }) as typeof fetch;

  await expect(fetchModels("invalid-api-key")).rejects.toThrow("Invalid API key");
  expect(requestCount).toBe(1);
});

test("fetchModels does not interpret an explicitly empty API key as anonymous", async () => {
  const fetchMock = mock(async () => encryptedModelsResponse());
  globalThis.fetch = fetchMock as typeof fetch;

  await expect(fetchModels("")).rejects.toThrow("No access token available");
  expect(fetchMock).toHaveBeenCalledTimes(0);
});

test("fetchModelCatalog remains authentication-required", async () => {
  const fetchMock = mock(async () => encryptedModelsResponse());
  globalThis.fetch = fetchMock as typeof fetch;

  await expect(fetchModelCatalog()).rejects.toThrow("No access token available");
  expect(fetchMock).toHaveBeenCalledTimes(0);
});
