import { afterEach, beforeEach, expect, mock, spyOn, test } from "bun:test";
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
import type { PcrConfig } from "../../pcr";
import { clearTransportV2Credentials, installTransportV2Credentials } from "../../transportV2/auth";
import { transportV2Client, type TransportV2FetchInput } from "../../transportV2/client";

const apiUrl = "https://api.example.com";
const pcrConfig: PcrConfig = { environment: "production", remoteAttestation: false };
const originalApiUrl = getApiUrl();
const originalApiPcrConfig = getApiPcrConfig();

function userToken(kind: "access_descriptor" | "resumption"): string {
  const audience =
    kind === "access_descriptor"
      ? "urn:opensecret:internal:transport-v2:user:access-descriptor"
      : "urn:opensecret:internal:transport-v2:user:resumption";
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: kind,
      pk: "user",
      sub: "user-123",
      exp: 2_000_000_000
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

function expectWebRequest(
  input: TransportV2FetchInput,
  path: "/v1/web/search" | "/v1/web/extract"
): void {
  expect(input.url).toBe(`${apiUrl}${path}`);
  expect(input.method).toBe("POST");
  expect(input.authority).toMatchObject({ kind: "user", principalId: "user-123" });
  expect(input.authority).toHaveProperty("generation");
  expect(new Headers(input.headers).has("authorization")).toBe(false);
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  setApiUrl(apiUrl, pcrConfig);
  installTransportV2Credentials(
    apiUrl,
    "user",
    userToken("access_descriptor"),
    userToken("resumption")
  );
});

afterEach(() => {
  mock.restore();
  clearTransportV2Credentials(apiUrl);
  setApiUrl(originalApiUrl, originalApiPcrConfig);
  localStorage.clear();
  sessionStorage.clear();
});

test("webSearch sends a user-bound v2 request and reconstructs results", async () => {
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
    filters: { region: "US" }
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
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expectWebRequest(input, "/v1/web/search");
    expect(JSON.parse(new TextDecoder().decode(input.body!))).toEqual(request);
    return Response.json(response);
  });

  await expect(webSearch(request)).resolves.toEqual(response);
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("webExtract preserves ordered pages and typed partial failures", async () => {
  const request: WebExtractRequest = {
    urls: ["https://example.com/first", "https://example.com/second"],
    timeout: 4.5
  };
  const response: WebExtractResponse = {
    trace_id: "trace-extract-1",
    pages: [
      { url: request.urls[0], markdown: "# First\n\nExtracted text." },
      {
        url: request.urls[1],
        error: { code: "no_content", message: "No readable content was found." }
      }
    ]
  };
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expectWebRequest(input, "/v1/web/extract");
    expect(JSON.parse(new TextDecoder().decode(input.body!))).toEqual(request);
    return Response.json(response);
  });

  const result = await webExtract(request);
  expect(result).toEqual(response);
  expect(result.pages.map((page) => page.url)).toEqual(request.urls);
  expect(result.pages[1].error?.code).toBe("no_content");
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("web validation errors surface without a transport retry", async () => {
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expectWebRequest(input, "/v1/web/search");
    return Response.json(
      { status: 422, code: "invalid_request", message: "The web request is invalid." },
      { status: 422 }
    );
  });

  await expect(webSearch({ query: "maple privacy", limit: 51 })).rejects.toThrow(
    "The web request is invalid."
  );
  expect(fetch).toHaveBeenCalledTimes(1);
});
