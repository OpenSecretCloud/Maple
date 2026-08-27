import { afterEach, beforeEach, expect, mock, test } from "bun:test";
import { decryptMessage, encryptMessage } from "../../encryption";
import { cacheAttestationSessionForTesting } from "../../getAttestation";
import type { PcrConfig } from "../../pcr";
import {
  getApiPcrConfig,
  getApiUrl,
  setApiUrl,
  webExtract,
  webSearch,
  type WebExtractRequest,
  type WebExtractResponse,
  type WebSearchRequest,
  type WebSearchResponse
} from "../../api";

const apiUrl = "https://api.example.com";
const accessToken = "web-access-token";
const sessionId = "web-session-id";
const sessionKey = new Uint8Array(32).fill(19);
const verifiedPcr0 =
  "eeddbb58f57c38894d6d5af5e575fbe791c5bf3bbcfb5df8da8cfcf0c2e1da1913108e6a762112444740b88c163d7f4b";
const pcrConfig: PcrConfig = { environment: "prod" };
const originalFetch = globalThis.fetch;
const originalApiUrl = getApiUrl();
const originalApiPcrConfig = getApiPcrConfig();

beforeEach(async () => {
  window.localStorage.clear();
  window.sessionStorage.clear();
  window.localStorage.setItem("access_token", accessToken);
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

test("webSearch sends an authenticated encrypted request and decrypts results", async () => {
  const request: WebSearchRequest = {
    query: "rust confidential computing",
    workflow: "news",
    page: 2,
    limit: 25,
    safe_search: false,
    timeout: 2.5,
    lens: {
      sites_included: ["example.com"],
      keywords_included: ["enclave"],
      time_relative: "week",
      search_region: "US"
    },
    filters: {
      region: "US"
    }
  };
  const response: WebSearchResponse = {
    trace_id: "trace-search-1",
    results: [
      {
        category: "news",
        url: "https://example.com/enclave",
        title: "Enclave update",
        snippet: "A short description.",
        published_at: "2026-07-16T12:00:00Z"
      }
    ]
  };

  globalThis.fetch = mock(async (input: string | URL | Request, init?: RequestInit) => {
    expect(input.toString()).toBe(`${apiUrl}/v1/web/search`);
    expect(init?.method).toBe("POST");
    expect(init?.headers).toMatchObject({
      Authorization: `Bearer ${accessToken}`,
      "x-session-id": sessionId
    });

    const body = JSON.parse(String(init?.body)) as { encrypted: string };
    expect(JSON.parse(decryptMessage(sessionKey, body.encrypted))).toEqual(request);

    return new Response(
      JSON.stringify({ encrypted: encryptMessage(sessionKey, JSON.stringify(response)) }),
      { status: 200, headers: { "Content-Type": "application/json" } }
    );
  }) as typeof fetch;

  await expect(webSearch(request)).resolves.toEqual(response);
});

test("webExtract preserves ordered pages and typed partial failures", async () => {
  const request: WebExtractRequest = {
    urls: ["https://example.com/first", "https://example.com/second"],
    timeout: 4.5
  };
  const response: WebExtractResponse = {
    trace_id: "trace-extract-1",
    pages: [
      {
        url: request.urls[0],
        markdown: "# First\n\nExtracted text."
      },
      {
        url: request.urls[1],
        error: {
          code: "no_content",
          message: "No readable content was found."
        }
      }
    ]
  };

  globalThis.fetch = mock(async (input: string | URL | Request, init?: RequestInit) => {
    expect(input.toString()).toBe(`${apiUrl}/v1/web/extract`);
    expect(init?.method).toBe("POST");
    expect(init?.headers).toMatchObject({
      Authorization: `Bearer ${accessToken}`,
      "x-session-id": sessionId
    });

    const body = JSON.parse(String(init?.body)) as { encrypted: string };
    expect(JSON.parse(decryptMessage(sessionKey, body.encrypted))).toEqual(request);

    return new Response(
      JSON.stringify({ encrypted: encryptMessage(sessionKey, JSON.stringify(response)) }),
      { status: 200, headers: { "Content-Type": "application/json" } }
    );
  }) as typeof fetch;

  const result = await webExtract(request);

  expect(result).toEqual(response);
  expect(result.pages.map((page) => page.url)).toEqual(request.urls);
  expect(result.pages[1].error?.code).toBe("no_content");
});

test("web validation errors surface without an attestation retry", async () => {
  let requestCount = 0;

  globalThis.fetch = mock(async (input: string | URL | Request) => {
    requestCount += 1;
    expect(input.toString()).toBe(`${apiUrl}/v1/web/search`);

    return new Response(
      JSON.stringify({
        status: 422,
        code: "invalid_request",
        message: "The web request is invalid."
      }),
      { status: 422, headers: { "Content-Type": "application/json" } }
    );
  }) as typeof fetch;

  try {
    await webSearch({ query: "maple privacy", limit: 51 });
    throw new Error("expected webSearch to reject invalid input");
  } catch (error) {
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe("The web request is invalid.");
  }

  expect(requestCount).toBe(1);
});
