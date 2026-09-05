import * as api from "./api";
import { snapshotPcrConfig, type PcrConfig } from "./pcr";
import { readTransportV2Credentials } from "./transportV2/auth";
import { canonicalizeTransportV2ApiUrl } from "./transportV2/auth";
import {
  transportV2Client,
  type TransportV2Authority,
  type TransportV2Client
} from "./transportV2/client";
import type { LogicalMethod, ResponseMode } from "./transportV2/envelope";

export interface CustomFetchOptions {
  /** Optional API key to use instead of a user-bound session. */
  apiKey?: string;
  /** Fixed API URL whose attestation policy governs every request. */
  apiUrl?: string;
  /** PCR0 trust policy enforced before non-loopback key exchange. */
  pcrConfig?: PcrConfig;
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export interface CustomFetchDependencies {
  client: Pick<TransportV2Client, "fetch">;
  getApiPcrConfig: typeof api.getApiPcrConfig;
  getApiUrl: typeof api.getApiUrl;
}

const defaultDependencies: CustomFetchDependencies = {
  client: transportV2Client,
  getApiPcrConfig: () => api.getApiPcrConfig(),
  getApiUrl: () => api.getApiUrl()
};

const FORBIDDEN_LOGICAL_HEADERS = new Set([
  "authorization",
  "user-agent",
  "proxy-authorization",
  "proxy-authenticate",
  "cookie",
  "set-cookie",
  "host",
  "content-length",
  "content-encoding",
  "content-md5",
  "digest",
  // Transport v2 selects unary versus streaming through the authenticated response mode.
  "accept",
  "accept-encoding",
  "connection",
  "keep-alive",
  "proxy-connection",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "x-session-id",
  "x-api-key",
  "api-key",
  "x-openai-api-key",
  "x-tinfoil-api-key",
  "x-goog-api-key",
  "x-anthropic-api-key",
  "openai-organization",
  "openai-project"
]);

function logicalMethod(value: string): LogicalMethod {
  const method = value.toUpperCase();
  if (!(["GET", "POST", "PUT", "PATCH", "DELETE"] as const).includes(method as LogicalMethod)) {
    throw new Error(`Transport v2 does not support the ${method} method.`);
  }
  return method as LogicalMethod;
}

function safeLogicalHeaders(input: Headers): Headers {
  const output = new Headers();
  input.forEach((value, name) => {
    const normalizedName = name.toLowerCase();
    if (
      !FORBIDDEN_LOGICAL_HEADERS.has(normalizedName) &&
      !normalizedName.startsWith("x-stainless-")
    ) {
      output.append(name, value);
    }
  });
  return output;
}

function rejectAutomaticOpenAiRetry(headers: Headers): void {
  const retryCount = headers.get("x-stainless-retry-count");
  if (retryCount !== null && retryCount !== "0") {
    throw new Error(
      "Transport v2 rejected an automatic OpenAI retry after a potentially sent request. Configure maxRetries: 0."
    );
  }
}

async function requestBody(normalized: Request): Promise<Uint8Array | null> {
  if (normalized.method === "GET" || normalized.method === "HEAD" || normalized.body === null) {
    return null;
  }
  return new Uint8Array(await normalized.arrayBuffer());
}

function logicalPath(apiUrl: string, requestUrl: string): string {
  const canonicalApiUrl = canonicalizeTransportV2ApiUrl(apiUrl);
  const base = new URL(canonicalApiUrl);
  const request = new URL(requestUrl);
  const basePath = base.pathname === "/" ? "" : base.pathname.replace(/\/+$/u, "");
  if (
    request.origin !== base.origin ||
    (basePath && request.pathname !== basePath && !request.pathname.startsWith(`${basePath}/`))
  ) {
    throw new Error("Transport v2 request escaped its attested API origin.");
  }
  return basePath ? request.pathname.slice(basePath.length) || "/" : request.pathname;
}

async function responseModeFor(
  apiUrl: string,
  request: Request,
  body: Uint8Array | null
): Promise<ResponseMode> {
  const path = logicalPath(apiUrl, request.url);
  if (path === "/v1/responses") return "stream";
  if (path !== "/v1/chat/completions" || body === null) return "unary";
  try {
    const value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body)) as {
      stream?: unknown;
    };
    return value.stream === true ? "stream" : "unary";
  } catch {
    return "unary";
  }
}

function authorityFor(apiUrl: string, requestUrl: string, apiKey?: string): TransportV2Authority {
  const path = logicalPath(apiUrl, requestUrl);
  if (apiKey !== undefined) return { kind: "api_key", value: apiKey };
  const credentials = readTransportV2Credentials(apiUrl, "user");
  if (credentials) {
    return {
      kind: "user",
      principalId: credentials.principalId,
      generation: credentials.generation
    };
  }
  if (path === "/v1/models") {
    return { kind: "anonymous", purpose: "public" };
  }
  throw new Error("A fresh transport v2 sign-in or API key is required.");
}

function decodeCanonicalBase64(value: string): Uint8Array {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    throw new Error("Invalid base64 audio data in TTS response");
  }
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  let canonical = "";
  for (const byte of bytes) canonical += String.fromCharCode(byte);
  if (btoa(canonical) !== value) {
    bytes.fill(0);
    throw new Error("Invalid base64 audio data in TTS response");
  }
  return bytes;
}

async function restoreTtsBinary(response: Response): Promise<Response> {
  if (!response.ok || response.headers.get("content-type")?.includes("text/event-stream")) {
    return response;
  }
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) return response;
  const bytes = new Uint8Array(await response.clone().arrayBuffer());
  if (bytes.length > 50 * 1024 * 1024) return response;
  try {
    const value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)) as {
      content_base64?: unknown;
      content_type?: unknown;
    };
    if (typeof value.content_base64 !== "string" || typeof value.content_type !== "string") {
      return response;
    }
    const audio = decodeCanonicalBase64(value.content_base64);
    const headers = new Headers(response.headers);
    headers.set("content-type", value.content_type);
    headers.delete("content-encoding");
    headers.delete("content-length");
    headers.delete("transfer-encoding");
    return new Response(audio, { status: response.status, headers });
  } catch {
    return response;
  } finally {
    bytes.fill(0);
  }
}

/**
 * Creates the attested Transport V2 fetch adapter. OpenAI clients should set
 * `maxRetries: 0`; nonzero Stainless retry attempts are rejected before any
 * second encrypted transport request can be sent.
 */
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
  return async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const normalized = new Request(input, init);
    normalized.signal.throwIfAborted();
    rejectAutomaticOpenAiRetry(normalized.headers);
    const apiUrl = options?.apiUrl || dependencies.getApiUrl();
    const pcrConfig = snapshotPcrConfig(options?.pcrConfig || dependencies.getApiPcrConfig());
    const authority = authorityFor(apiUrl, normalized.url, options?.apiKey);
    const body = await requestBody(normalized);
    normalized.signal.throwIfAborted();
    try {
      const response = await dependencies.client.fetch({
        apiUrl,
        pcrConfig,
        url: normalized.url,
        method: logicalMethod(normalized.method),
        headers: safeLogicalHeaders(normalized.headers),
        body,
        responseMode: await responseModeFor(apiUrl, normalized, body),
        authority,
        signal: normalized.signal
      });
      return restoreTtsBinary(response);
    } finally {
      body?.fill(0);
    }
  };
}
