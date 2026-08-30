import { getApiPcrConfig, getApiUrl } from "./api";
import { getPlatformApiUrl, getPlatformPcrConfig } from "./platformApi";
import { canonicalizeTransportV2ApiUrl, readTransportV2Credentials } from "./transportV2/auth";
import {
  transportV2Client,
  type TransportV2Authority,
  type TransportV2Client
} from "./transportV2/client";
import type { LogicalMethod } from "./transportV2/envelope";

interface ApiResponse<T> {
  status: number;
  hasData: boolean;
  data?: T;
  error?: string;
}

/** @internal Exported for deterministic transport tests, not from the package entry point. */
export interface EncryptedApiDependencies {
  client: Pick<TransportV2Client, "fetch">;
  getApiPcrConfig: typeof getApiPcrConfig;
  getApiUrl: typeof getApiUrl;
  getPlatformApiUrl: typeof getPlatformApiUrl;
  getPlatformPcrConfig: typeof getPlatformPcrConfig;
}

const defaultDependencies: EncryptedApiDependencies = {
  client: transportV2Client,
  getApiPcrConfig: () => getApiPcrConfig(),
  getApiUrl: () => getApiUrl(),
  getPlatformApiUrl: () => getPlatformApiUrl(),
  getPlatformPcrConfig: () => getPlatformPcrConfig()
};

function logicalMethod(method: string): LogicalMethod {
  const normalized = method.toUpperCase();
  if (!(["GET", "POST", "PUT", "PATCH", "DELETE"] as const).includes(normalized as LogicalMethod)) {
    throw new Error(`Transport v2 does not support the ${normalized} method.`);
  }
  return normalized as LogicalMethod;
}

function isPlatformUrl(url: string): boolean {
  return new URL(url).pathname.includes("/platform/");
}

function logicalPath(apiUrl: string, requestUrl: string): string {
  const base = new URL(canonicalizeTransportV2ApiUrl(apiUrl));
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

function isAnonymousPlatformPath(path: string): boolean {
  return (
    path === "/platform/login" ||
    path === "/platform/register" ||
    path === "/platform/password-reset/request" ||
    path === "/platform/password-reset/confirm"
  );
}

function isAnonymousUserPath(path: string): boolean {
  return (
    path === "/login" ||
    path === "/register" ||
    path.startsWith("/password-reset/") ||
    path === "/auth/github" ||
    path === "/auth/github/callback" ||
    path === "/auth/google" ||
    path === "/auth/google/callback" ||
    path === "/auth/apple" ||
    path === "/auth/apple/callback" ||
    path === "/auth/apple/native"
  );
}

function storedAuthority(
  apiUrl: string,
  kind: "user" | "platform"
): Extract<TransportV2Authority, { kind: "user" | "platform" }> {
  const credentials = readTransportV2Credentials(apiUrl, kind);
  if (!credentials) throw new Error("A fresh transport v2 sign-in is required.");
  return {
    kind,
    principalId: credentials.principalId,
    generation: credentials.generation
  };
}

function encryptedAuthority(url: string, apiUrl: string): TransportV2Authority {
  const path = logicalPath(apiUrl, url);
  if (isPlatformUrl(url)) {
    if (isAnonymousPlatformPath(path)) return { kind: "anonymous", purpose: "platform" };
    if (
      path.startsWith("/platform/verify-email/") &&
      !readTransportV2Credentials(apiUrl, "platform")
    ) {
      return { kind: "anonymous", purpose: "platform" };
    }
    return storedAuthority(apiUrl, "platform");
  }
  if (path === "/v1/models") return { kind: "anonymous", purpose: "public" };
  if (isAnonymousUserPath(path)) return { kind: "anonymous", purpose: "user" };
  if (path.startsWith("/verify-email/") && !readTransportV2Credentials(apiUrl, "user")) {
    return { kind: "anonymous", purpose: "user" };
  }
  return storedAuthority(apiUrl, "user");
}

function streamEventError(value: unknown, fallback: string): Error {
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, unknown>;
    if (typeof record.message === "string") return new Error(record.message);
    if (typeof record.error === "object" && record.error !== null) {
      const error = record.error as Record<string, unknown>;
      if (typeof error.message === "string") return new Error(error.message);
    }
  }
  return new Error(fallback);
}

function completedResponseFromSse(text: string): unknown {
  for (const eventBlock of text.split(/\r?\n\r?\n/u)) {
    if (!eventBlock.trim()) continue;
    let eventName = "";
    const data: string[] = [];
    for (const line of eventBlock.split(/\r?\n/u)) {
      if (line.startsWith("event:")) eventName = line.slice(6).trim();
      else if (line.startsWith("data:")) data.push(line.slice(5).replace(/^ /u, ""));
    }
    if (data.length === 0 || data.join("\n") === "[DONE]") continue;
    let value: unknown;
    try {
      value = JSON.parse(data.join("\n"));
    } catch {
      throw new Error("Transport v2 Responses stream contained invalid JSON.");
    }
    const type =
      typeof value === "object" &&
      value !== null &&
      typeof (value as Record<string, unknown>).type === "string"
        ? ((value as Record<string, unknown>).type as string)
        : eventName;
    if (type === "response.completed") {
      const response = (value as Record<string, unknown>).response;
      if (typeof response !== "object" || response === null) {
        throw new Error("Transport v2 Responses completion contained no response.");
      }
      return response;
    }
    if (type === "response.error" || type === "response.failed" || type === "error") {
      throw streamEventError(value, "The response failed.");
    }
    if (type === "response.cancelled") {
      throw streamEventError(value, "The response was cancelled.");
    }
  }
  throw new Error("Transport v2 Responses stream ended without a completed response.");
}

function compatibilityResponseShape(url: string, value: unknown): unknown {
  const path = new URL(url).pathname;
  if (
    (path.endsWith("/auth/github") || path.endsWith("/auth/google")) &&
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).state === "string"
  ) {
    const { state, ...rest } = value as Record<string, unknown>;
    return { ...rest, csrf_token: state };
  }
  return value;
}

async function performTransportV2Call<T, U>(
  url: string,
  method: string,
  data: T,
  authority: TransportV2Authority,
  errorMessage: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<ApiResponse<U>> {
  try {
    const platform = isPlatformUrl(url);
    const apiUrl = platform ? dependencies.getPlatformApiUrl() : dependencies.getApiUrl();
    const pcrConfig = platform
      ? dependencies.getPlatformPcrConfig()
      : dependencies.getApiPcrConfig();
    const body = data === undefined ? null : new TextEncoder().encode(JSON.stringify(data));
    const requestMethod = logicalMethod(method);
    const responseMode =
      requestMethod === "POST" && logicalPath(apiUrl, url) === "/v1/responses" ? "stream" : "unary";
    let response: Response;
    try {
      response = await dependencies.client.fetch({
        apiUrl,
        pcrConfig,
        url,
        method: requestMethod,
        headers: body === null ? undefined : { "content-type": "application/json" },
        body,
        responseMode,
        authority
      });
    } finally {
      body?.fill(0);
    }

    const text = await response.text();
    if (!response.ok) {
      let message: string | undefined;
      try {
        const value = JSON.parse(text) as { message?: unknown; error?: unknown };
        if (typeof value.message === "string") message = value.message;
        else if (typeof value.error === "string") message = value.error;
      } catch {
        if (text.trim()) message = text;
      }
      return {
        status: response.status,
        hasData: false,
        error: message || errorMessage || `HTTP error! Status: ${response.status}`
      };
    }

    if (responseMode === "stream") {
      return {
        status: response.status,
        hasData: true,
        data: completedResponseFromSse(text) as U
      };
    }
    if (text.length === 0) {
      return { status: response.status, hasData: true, data: undefined as U };
    }
    try {
      return {
        status: response.status,
        hasData: true,
        data: compatibilityResponseShape(url, JSON.parse(text)) as U
      };
    } catch {
      return { status: 500, hasData: false, error: "Failed to parse the authenticated response" };
    }
  } catch (error) {
    return {
      status: 500,
      hasData: false,
      error: error instanceof Error ? error.message : "Unknown error occurred"
    };
  }
}

function unwrapApiResponse<U>(response: ApiResponse<U>, missingDataMessage: string): U {
  if (response.error) throw new Error(response.error);
  if (!response.hasData) throw new Error(missingDataMessage);
  return response.data as U;
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
  const platform = isPlatformUrl(url);
  const authority = storedAuthority(
    platform ? dependencies.getPlatformApiUrl() : dependencies.getApiUrl(),
    platform ? "platform" : "user"
  );
  const response = await performTransportV2Call<T, U>(
    url,
    method,
    data,
    authority,
    errorMessage,
    dependencies
  );
  return unwrapApiResponse(response, "No data received from the server");
}

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
  const authority: TransportV2Authority =
    apiKey !== undefined
      ? { kind: "api_key", value: apiKey }
      : storedAuthority(dependencies.getApiUrl(), "user");
  const response = await performTransportV2Call<T, U>(
    url,
    method,
    data,
    authority,
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
  _accessToken: string | undefined,
  errorMessage: string | undefined,
  dependencies: EncryptedApiDependencies
): Promise<U> {
  const apiUrl = isPlatformUrl(url) ? dependencies.getPlatformApiUrl() : dependencies.getApiUrl();
  const response = await performTransportV2Call<T, U>(
    url,
    method,
    data,
    encryptedAuthority(url, apiUrl),
    errorMessage,
    dependencies
  );
  return unwrapApiResponse(response, "No data received from the server");
}
